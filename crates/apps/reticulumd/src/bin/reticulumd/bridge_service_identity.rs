use super::*;
use rand_core::OsRng;
use reticulum_daemon::announce_names::encode_delivery_announce_app_data_with_capabilities;
use reticulum_daemon::identity_store::save_identity;
use rns_rpc::{ServiceIdentityBridge, ServiceIdentityRecord, ServiceIdentitySpec};
use serde::{Deserialize, Serialize};
use serde_json::Map as JsonMap;
use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

const SERVICE_IDENTITY_MANIFEST_VERSION: u8 = 1;
const SERVICE_IDENTITY_OPERATION_TIMEOUT: Duration = Duration::from_secs(15);

#[derive(Clone)]
pub(super) struct RegisteredServiceIdentity {
    pub(super) identity: PrivateIdentity,
    pub(super) destination: Arc<tokio::sync::Mutex<SingleInputDestination>>,
    pub(super) identity_hash: String,
    pub(super) delivery_destination: String,
    pub(super) display_name: Option<String>,
    pub(super) capabilities: Vec<String>,
    pub(super) metadata: JsonMap<String, JsonValue>,
    pub(super) persisted: bool,
}

#[derive(Default)]
pub(super) struct ServiceIdentityRegistry {
    by_identity: HashMap<String, RegisteredServiceIdentity>,
    identity_by_destination: HashMap<String, String>,
}

impl ServiceIdentityRegistry {
    pub(super) fn insert(&mut self, record: RegisteredServiceIdentity) {
        self.identity_by_destination
            .insert(record.delivery_destination.clone(), record.identity_hash.clone());
        self.by_identity.insert(record.identity_hash.clone(), record);
    }

    pub(super) fn by_identity(&self, identity: &str) -> Option<RegisteredServiceIdentity> {
        self.by_identity.get(&identity.to_ascii_lowercase()).cloned()
    }

    pub(super) fn by_destination(&self, destination: &str) -> Option<RegisteredServiceIdentity> {
        let identity = self.identity_by_destination.get(&destination.to_ascii_lowercase())?;
        self.by_identity.get(identity).cloned()
    }

    fn records(&self) -> Vec<RegisteredServiceIdentity> {
        self.by_identity.values().cloned().collect()
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct ServiceIdentityManifest {
    version: u8,
    identities: Vec<ServiceIdentityManifestEntry>,
}

#[derive(Debug, Serialize, Deserialize)]
struct ServiceIdentityManifestEntry {
    identity: String,
    key_file: String,
    display_name: Option<String>,
    #[serde(default)]
    capabilities: Vec<String>,
    #[serde(default)]
    metadata: JsonMap<String, JsonValue>,
}

impl TransportBridge {
    #[cfg(test)]
    pub(crate) fn service_identity_storage_dir(&self) -> PathBuf {
        self.service_identity_dir.clone()
    }

    pub(super) fn initialize_default_service_identity(&self, display_name: Option<String>) {
        let identity_hash = hex::encode(self.signer.address_hash().as_slice());
        let delivery_destination = hex::encode(self.delivery_source_hash);
        let record = RegisteredServiceIdentity {
            identity: self.signer.clone(),
            destination: self.announce_destination.clone(),
            identity_hash,
            delivery_destination,
            display_name,
            capabilities: self.announce_capabilities.clone(),
            metadata: JsonMap::new(),
            persisted: false,
        };
        self.service_identities
            .write()
            .expect("service identity registry rwlock poisoned")
            .insert(record);
    }

    pub(crate) async fn load_persisted_service_identities(&self) -> io::Result<usize> {
        let manifest_path = self.service_identity_manifest_path();
        let raw = match fs::read(&manifest_path) {
            Ok(raw) => raw,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(0),
            Err(error) => return Err(error),
        };
        let manifest: ServiceIdentityManifest = serde_json::from_slice(raw.as_slice())
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
        if manifest.version != SERVICE_IDENTITY_MANIFEST_VERSION {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "unsupported service identity manifest version",
            ));
        }
        let mut loaded = 0usize;
        for entry in manifest.identities {
            let key_path = self.service_identity_dir.join(entry.key_file.as_str());
            let key_bytes = fs::read(&key_path)?;
            let identity =
                PrivateIdentity::from_private_key_bytes(key_bytes.as_slice()).map_err(|error| {
                    io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("invalid service identity {}: {error:?}", key_path.display()),
                    )
                })?;
            let derived_identity = hex::encode(identity.address_hash().as_slice());
            if derived_identity != entry.identity.to_ascii_lowercase() {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("service identity hash mismatch for {}", key_path.display()),
                ));
            }
            let record = self
                .register_service_identity_async(
                    identity,
                    ServiceIdentitySpec {
                        display_name: entry.display_name,
                        capabilities: entry.capabilities,
                        metadata: entry.metadata,
                    },
                    true,
                )
                .await?;
            self.service_identities
                .write()
                .expect("service identity registry rwlock poisoned")
                .insert(record);
            loaded = loaded.saturating_add(1);
        }
        Ok(loaded)
    }

    pub(super) fn service_identity_for_destination(
        &self,
        destination: &str,
    ) -> Option<RegisteredServiceIdentity> {
        let registry =
            self.service_identities.read().expect("service identity registry rwlock poisoned");
        registry.by_destination(destination).or_else(|| {
            let records = registry.records();
            (records.len() == 1).then(|| records[0].clone())
        })
    }

    pub(super) fn announce_registered_service_destination(
        &self,
        delivery_destination: &str,
    ) -> io::Result<()> {
        let record = self
            .service_identities
            .read()
            .expect("service identity registry rwlock poisoned")
            .by_destination(delivery_destination)
            .ok_or_else(|| {
                io::Error::new(io::ErrorKind::NotFound, "service delivery destination not found")
            })?;
        let transport = self.transport.clone();
        let destination = record.destination;
        let app_data = service_announce_app_data(&ServiceIdentitySpec {
            display_name: record.display_name,
            capabilities: record.capabilities,
            metadata: record.metadata,
        });
        self.runtime_handle.spawn(async move {
            transport.set_destination_announce_app_data(&destination, app_data.clone()).await;
            transport.send_announce(&destination, app_data.as_deref()).await;
        });
        Ok(())
    }

    fn service_identity_manifest_path(&self) -> PathBuf {
        self.service_identity_dir.join("registry.json")
    }

    async fn register_service_identity_async(
        &self,
        identity: PrivateIdentity,
        spec: ServiceIdentitySpec,
        persisted: bool,
    ) -> io::Result<RegisteredServiceIdentity> {
        let identity_hash = hex::encode(identity.address_hash().as_slice());
        let existing = {
            self.service_identities
                .read()
                .expect("service identity registry rwlock poisoned")
                .by_identity(identity_hash.as_str())
        };
        if let Some(existing) = existing {
            return Ok(RegisteredServiceIdentity {
                display_name: spec.display_name,
                capabilities: spec.capabilities,
                metadata: spec.metadata,
                persisted: existing.persisted || persisted,
                ..existing
            });
        }
        let transport_identity = rns_transport::identity::PrivateIdentity::from_private_key_bytes(
            &identity.to_private_key_bytes(),
        )
        .map_err(|error| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("invalid transport service identity: {error:?}"),
            )
        })?;
        let destination = self
            .transport
            .add_destination(transport_identity, DestinationName::new("lxmf", "delivery"))
            .await;
        let delivery_destination = {
            let destination = destination.lock().await;
            hex::encode(destination.desc.address_hash.as_slice())
        };
        let app_data = service_announce_app_data(&spec);
        self.transport.set_destination_announce_app_data(&destination, app_data).await;
        Ok(RegisteredServiceIdentity {
            identity,
            destination,
            identity_hash,
            delivery_destination,
            display_name: spec.display_name,
            capabilities: spec.capabilities,
            metadata: spec.metadata,
            persisted,
        })
    }

    fn register_service_identity_blocking(
        &self,
        identity: PrivateIdentity,
        spec: ServiceIdentitySpec,
    ) -> io::Result<RegisteredServiceIdentity> {
        let identity_hash = hex::encode(identity.address_hash().as_slice());
        let existing = {
            self.service_identities
                .read()
                .expect("service identity registry rwlock poisoned")
                .by_identity(identity_hash.as_str())
        };
        if let Some(existing) = existing {
            let updated = RegisteredServiceIdentity {
                display_name: spec.display_name,
                capabilities: spec.capabilities,
                metadata: spec.metadata,
                persisted: true,
                ..existing
            };
            self.service_identities
                .write()
                .expect("service identity registry rwlock poisoned")
                .insert(updated.clone());
            self.persist_service_identity(&updated)?;
            return Ok(updated);
        }

        let transport = self.transport.clone();
        let runtime_handle = self.runtime_handle.clone();
        let identity_for_task = identity.clone();
        let spec_for_task = spec.clone();
        let (destination, delivery_destination) = tokio::task::block_in_place(|| {
            runtime_handle.block_on(async move {
                tokio::time::timeout(SERVICE_IDENTITY_OPERATION_TIMEOUT, async move {
                    let transport_identity =
                        rns_transport::identity::PrivateIdentity::from_private_key_bytes(
                            &identity_for_task.to_private_key_bytes(),
                        )
                        .map_err(|error| {
                            io::Error::new(
                                io::ErrorKind::InvalidData,
                                format!(
                                    "failed to convert service identity for transport: {error:?}"
                                ),
                            )
                        })?;
                    let destination = transport
                        .add_destination(
                            transport_identity,
                            DestinationName::new("lxmf", "delivery"),
                        )
                        .await;
                    let delivery_destination = {
                        let destination = destination.lock().await;
                        hex::encode(destination.desc.address_hash.as_slice())
                    };
                    transport
                        .set_destination_announce_app_data(
                            &destination,
                            service_announce_app_data(&spec_for_task),
                        )
                        .await;
                    Ok::<_, io::Error>((destination, delivery_destination))
                })
                .await
                .map_err(|error| io::Error::new(io::ErrorKind::TimedOut, error))?
            })
        })?;
        let record = RegisteredServiceIdentity {
            identity,
            destination,
            identity_hash,
            delivery_destination,
            display_name: spec.display_name,
            capabilities: spec.capabilities,
            metadata: spec.metadata,
            persisted: true,
        };
        self.service_identities
            .write()
            .expect("service identity registry rwlock poisoned")
            .insert(record.clone());
        self.persist_service_identity(&record)?;
        Ok(record)
    }

    fn persist_service_identity(&self, record: &RegisteredServiceIdentity) -> io::Result<()> {
        fs::create_dir_all(&self.service_identity_dir)?;
        let key_file = format!("{}.identity", record.identity_hash);
        save_identity(self.service_identity_dir.join(key_file).as_path(), &record.identity)?;
        self.persist_service_identity_manifest()
    }

    fn persist_service_identity_manifest(&self) -> io::Result<()> {
        let registry =
            self.service_identities.read().expect("service identity registry rwlock poisoned");
        let mut identities = registry
            .records()
            .into_iter()
            .filter(|record| record.persisted)
            .map(|record| ServiceIdentityManifestEntry {
                identity: record.identity_hash.clone(),
                key_file: format!("{}.identity", record.identity_hash),
                display_name: record.display_name,
                capabilities: record.capabilities,
                metadata: record.metadata,
            })
            .collect::<Vec<_>>();
        identities.sort_by(|left, right| left.identity.cmp(&right.identity));
        drop(registry);
        write_json_atomic(
            self.service_identity_manifest_path().as_path(),
            &ServiceIdentityManifest { version: SERVICE_IDENTITY_MANIFEST_VERSION, identities },
        )
    }
}

impl ServiceIdentityBridge for TransportBridge {
    fn list_service_identities(&self) -> io::Result<Vec<ServiceIdentityRecord>> {
        let mut records = self
            .service_identities
            .read()
            .expect("service identity registry rwlock poisoned")
            .records()
            .iter()
            .map(service_identity_record)
            .collect::<Vec<_>>();
        records.sort_by(|left, right| left.identity.cmp(&right.identity));
        Ok(records)
    }

    fn create_service_identity(
        &self,
        spec: ServiceIdentitySpec,
    ) -> io::Result<ServiceIdentityRecord> {
        let identity = PrivateIdentity::new_from_rand(OsRng);
        self.register_service_identity_blocking(identity, spec)
            .map(|record| service_identity_record(&record))
    }

    fn import_service_identity(
        &self,
        private_key: &[u8],
        spec: ServiceIdentitySpec,
    ) -> io::Result<ServiceIdentityRecord> {
        let identity = PrivateIdentity::from_private_key_bytes(private_key).map_err(|error| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("invalid Reticulum private identity: {error:?}"),
            )
        })?;
        self.register_service_identity_blocking(identity, spec)
            .map(|record| service_identity_record(&record))
    }

    fn export_service_identity(&self, identity: &str) -> io::Result<Vec<u8>> {
        let record = self
            .service_identities
            .read()
            .expect("service identity registry rwlock poisoned")
            .by_identity(identity)
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "service identity not found"))?;
        Ok(record.identity.to_private_key_bytes().to_vec())
    }

    fn announce_service_identity(
        &self,
        identity: &str,
        spec: ServiceIdentitySpec,
    ) -> io::Result<ServiceIdentityRecord> {
        let existing = self
            .service_identities
            .read()
            .expect("service identity registry rwlock poisoned")
            .by_identity(identity)
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "service identity not found"))?;
        let updated = RegisteredServiceIdentity {
            display_name: spec.display_name,
            capabilities: spec.capabilities,
            metadata: spec.metadata,
            ..existing
        };
        self.service_identities
            .write()
            .expect("service identity registry rwlock poisoned")
            .insert(updated.clone());
        if updated.persisted {
            self.persist_service_identity(&updated)?;
        }
        let transport = self.transport.clone();
        let destination = updated.destination.clone();
        let app_data = service_announce_app_data(&ServiceIdentitySpec {
            display_name: updated.display_name.clone(),
            capabilities: updated.capabilities.clone(),
            metadata: updated.metadata.clone(),
        });
        self.runtime_handle.spawn(async move {
            transport.set_destination_announce_app_data(&destination, app_data.clone()).await;
            transport.send_announce(&destination, app_data.as_deref()).await;
        });
        Ok(service_identity_record(&updated))
    }
}

fn service_announce_app_data(spec: &ServiceIdentitySpec) -> Option<Vec<u8>> {
    spec.display_name.as_deref().and_then(|display_name| {
        encode_delivery_announce_app_data_with_capabilities(
            display_name,
            None,
            spec.capabilities.as_slice(),
        )
        .inspect_err(|error| {
            log::warn!("[daemon] failed to encode service identity announce: {error}")
        })
        .ok()
    })
}

fn service_identity_record(record: &RegisteredServiceIdentity) -> ServiceIdentityRecord {
    let public_identity = record.identity.as_identity();
    ServiceIdentityRecord {
        identity: record.identity_hash.clone(),
        delivery_destination: record.delivery_destination.clone(),
        public_key: format!(
            "{}{}",
            hex::encode(public_identity.public_key_bytes()),
            hex::encode(public_identity.verifying_key_bytes())
        ),
        display_name: record.display_name.clone(),
        capabilities: record.capabilities.clone(),
        metadata: record.metadata.clone(),
    }
}

fn write_json_atomic(path: &Path, value: &impl Serialize) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let raw = serde_json::to_vec_pretty(value).map_err(io::Error::other)?;
    let unique = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos();
    let temporary = path.with_extension(format!("tmp-{unique}"));
    fs::write(&temporary, raw)?;
    #[cfg(windows)]
    if path.exists() {
        fs::remove_file(path)?;
    }
    fs::rename(temporary, path)
}
