#[cfg(test)]
pub(super) use super::native_shutdown::stop_and_join;
pub(super) use super::native_shutdown::stop_owned_native;
use super::*;
use rand_core::{OsRng, RngCore};
use reticulum_daemon::config::{InterfaceConfig, ReticulumRuntimePolicy};
use rns_transport::discovery::lifecycle::{
    endpoint_hash, plan_autoconnect_with_policy, AutoconnectInterfacePolicy,
    InterfaceDiscoveryLifecycle, RuntimeInterfaceState,
};
use rns_transport::discovery::{DiscoveredInterface, DiscoveryListFilter, InterfaceDiscoveryStore};
use std::collections::HashSet;
use tokio::time::{Duration, Instant};

#[path = "discovery_monitor.rs"]
mod monitor;

#[derive(Clone)]
pub(crate) struct DiscoveryObservationSender(
    tokio::sync::mpsc::WeakSender<InterfaceHotApplyCommand>,
);

impl DiscoveryObservationSender {
    pub(super) fn new(sender: tokio::sync::mpsc::WeakSender<InterfaceHotApplyCommand>) -> Self {
        Self(sender)
    }

    pub(crate) async fn observed(&self, info: DiscoveredInterface) -> io::Result<()> {
        let sender = self.0.upgrade().ok_or_else(|| {
            io::Error::new(io::ErrorKind::BrokenPipe, "interface mutation worker is closed")
        })?;
        sender.send(InterfaceHotApplyCommand::DiscoveryObserved(Box::new(info))).await.map_err(
            |_| io::Error::new(io::ErrorKind::BrokenPipe, "interface mutation worker is closed"),
        )
    }
}

pub(crate) struct NativeDiscoveryConfig {
    storage_path: PathBuf,
    allowed_network_ids: Vec<String>,
    maximum: usize,
    mode: Option<String>,
    gravity: i64,
    announces_to_internal: bool,
    allow_unverified: bool,
    bootstrap: Vec<(usize, InterfaceConfig)>,
    announce_defaults: (u64, u64, u64),
}

impl NativeDiscoveryConfig {
    pub(crate) fn new(
        storage_path: PathBuf,
        policy: &ReticulumRuntimePolicy,
        allow_unverified: bool,
        bootstrap: &[InterfaceConfig],
        announce_defaults: (u64, u64, u64),
    ) -> Option<Self> {
        (policy.discover_interfaces && policy.max_autoconnected_interfaces > 0).then(|| Self {
            storage_path,
            allowed_network_ids: policy.interface_discovery_sources.clone(),
            maximum: policy.max_autoconnected_interfaces as usize,
            mode: policy.autoconnect_interface_mode.clone(),
            gravity: policy.autoconnect_interface_gravity.unwrap_or(0),
            announces_to_internal: policy.autoconnect_announces_to_internal == Some(true),
            allow_unverified,
            // Recipes come from the immutable startup configuration, never RPC status.
            bootstrap: bootstrap
                .iter()
                .enumerate()
                .filter(|(_, iface)| iface.enabled() && iface.bootstrap_only == Some(true))
                .map(|(index, iface)| (index, iface.clone()))
                .collect(),
            announce_defaults,
        })
    }
}

#[derive(Clone)]
pub(super) enum ManagedInterfaceOrigin {
    Configured,
    StartupOnly,
    Discovered { endpoint: [u8; 32], source: String, down_since: Option<Instant> },
}

impl ManagedInterfaceOrigin {
    pub(super) fn configured(record: &InterfaceRecord) -> Self {
        if record.kind == "backbone_client" {
            Self::StartupOnly
        } else {
            Self::Configured
        }
    }

    pub(super) fn endpoint_hash(&self) -> Option<[u8; 32]> {
        match self {
            Self::Discovered { endpoint, .. } => Some(*endpoint),
            _ => None,
        }
    }

    pub(super) fn configuration_owned(&self) -> bool {
        matches!(self, Self::Configured)
    }
}

pub(super) struct NativeDiscovery {
    config: NativeDiscoveryConfig,
    lifecycle: InterfaceDiscoveryLifecycle,
    suppressed: HashSet<[u8; 32]>,
    started: Instant,
}

impl NativeDiscovery {
    pub(super) fn new(config: NativeDiscoveryConfig) -> Self {
        let mut lifecycle = InterfaceDiscoveryLifecycle::default();
        lifecycle.start();
        Self { config, lifecycle, suppressed: HashSet::new(), started: Instant::now() }
    }

    pub(super) fn suppress(&mut self, endpoint: [u8; 32], key: &ManagedInterfaceKey) {
        self.suppressed.insert(endpoint);
        self.lifecycle.teardown_interface(&key.monitor_id());
    }

    pub(super) fn allow(&mut self, endpoint: [u8; 32]) {
        self.suppressed.remove(&endpoint);
    }

    fn candidates(&self, only_available: bool) -> Vec<DiscoveredInterface> {
        match InterfaceDiscoveryStore::new(&self.config.storage_path).list(
            rns_transport::time::now_epoch_secs_u64() as f64,
            &self.config.allowed_network_ids,
            DiscoveryListFilter { only_transport: true, only_available },
        ) {
            Ok(candidates) => candidates,
            Err(error) => {
                log::warn!("[daemon] discovery store scan failed: {error}");
                Vec::new()
            }
        }
    }

    pub(super) async fn initial(
        &mut self,
        manager: &Arc<tokio::sync::Mutex<InterfaceManager>>,
        managed: &mut HashMap<ManagedInterfaceKey, ManagedHotApplyInterface>,
        transport: &Arc<Transport>,
        daemon: &Arc<RpcDaemon>,
    ) {
        if transport.connected_to_shared_instance().await {
            return;
        }
        for info in self.candidates(false) {
            self.admit(&info, manager, managed, transport, daemon).await;
        }
        // Install serially so a rejected install does not spend the next candidate's slot.
        self.lifecycle.initial_autoconnect(&[], &[], self.config.maximum);
    }

    pub(super) async fn admit(
        &mut self,
        info: &DiscoveredInterface,
        manager: &Arc<tokio::sync::Mutex<InterfaceManager>>,
        managed: &mut HashMap<ManagedInterfaceKey, ManagedHotApplyInterface>,
        transport: &Arc<Transport>,
        daemon: &Arc<RpcDaemon>,
    ) {
        if transport.connected_to_shared_instance().await
            || self.suppressed.contains(&endpoint_hash(info))
            || (!self.config.allowed_network_ids.is_empty()
                && !self.config.allowed_network_ids.contains(&info.network_id))
            || managed.values().filter(|active| active.origin.endpoint_hash().is_some()).count()
                >= self.config.maximum
        {
            return;
        }
        for (field, identity) in
            [("network_id", &info.network_id), ("transport_id", &info.transport_id)]
        {
            let Ok(identity) =
                rns_transport::destination_hash::parse_destination_hash_required(identity)
            else {
                log::warn!("[daemon] discovered interface has malformed identity field={field}");
                return;
            };
            if transport.is_identity_blackholed(&AddressHash::new(identity)).await {
                return;
            }
        }
        let transport_enabled = transport.transport_enabled().await;
        let existing = admission_states(runtime_states(manager, managed).await, managed, daemon);
        let policy = AutoconnectInterfacePolicy {
            mode: self.config.mode.as_deref().or(transport_enabled.then_some("gateway")),
            gravity: self.config.gravity,
            announces_to_internal: self.config.announces_to_internal,
            allow_unverified: self.config.allow_unverified,
        };
        let Some(plan) = plan_autoconnect_with_policy(info, &existing, policy) else {
            return;
        };
        let iface = InterfaceConfig {
            kind: discovered_carrier_kind().to_owned(),
            enabled: Some(true),
            name: Some(plan.name.clone()),
            host: Some(plan.target_host),
            port: Some(plan.target_port),
            interface_mode: plan.interface_mode,
            gravity: Some(plan.gravity),
            bitrate: Some(5_000_000),
            network_name: plan.ifac_netname,
            passphrase: plan.ifac_netkey,
            announces_to_internal: plan.announces_to_internal.then_some(true),
            announce_rate_target: transport_enabled.then_some(self.config.announce_defaults.0),
            announce_rate_grace: transport_enabled.then_some(self.config.announce_defaults.1),
            announce_rate_penalty: transport_enabled.then_some(self.config.announce_defaults.2),
            ..InterfaceConfig::default()
        };
        let record = crate::bootstrap::interface_record_from_config(&iface);
        let Some((address, runtime_status)) =
            spawn_interface::spawn_trusted_stream(manager, &iface).await
        else {
            return;
        };
        let key = ManagedInterfaceKey::Discovered(plan.endpoint_hash);
        managed.insert(
            key.clone(),
            ManagedHotApplyInterface::active(
                record,
                address,
                runtime_status,
                ManagedInterfaceOrigin::Discovered {
                    endpoint: plan.endpoint_hash,
                    source: plan.network_id,
                    down_since: None,
                },
            ),
        );
        self.lifecycle.monitor_interface(key.monitor_id());
        publish_live_runtime_records(managed, daemon);
        log::info!(
            "[daemon] discovery interface installed name={} address={address} source={}",
            plan.name,
            info.network_id
        );
    }
}

fn discovered_carrier_kind() -> &'static str {
    if cfg!(any(target_os = "windows", target_os = "macos")) {
        "tcp_client"
    } else {
        "backbone_client"
    }
}

pub(super) fn record_endpoint_hash(record: &InterfaceRecord) -> Option<[u8; 32]> {
    use sha2::{Digest, Sha256};
    if !matches!(record.kind.as_str(), "tcp_client" | "backbone_client") {
        return None;
    }
    let host = record
        .settings
        .as_ref()
        .and_then(|settings| settings["target_host"].as_str())
        .or(record.host.as_deref())
        .unwrap_or("");
    let port = record
        .settings
        .as_ref()
        .and_then(|settings| settings["target_port"].as_u64())
        .and_then(|port| u16::try_from(port).ok())
        .or(record.port);
    let endpoint = match port {
        Some(port) => format!("{host}:{port}"),
        None => host.to_string(),
    };
    Some(Sha256::digest(endpoint.as_bytes()).into())
}

struct NativeInterfaceSnapshot {
    keys: HashMap<String, ManagedInterfaceKey>,
    observed_at: Instant,
    interfaces: Vec<RuntimeInterfaceState>,
    names: Vec<String>,
}

async fn runtime_states(
    manager: &Arc<tokio::sync::Mutex<InterfaceManager>>,
    managed: &HashMap<ManagedInterfaceKey, ManagedHotApplyInterface>,
) -> NativeInterfaceSnapshot {
    let manager = manager.lock().await;
    let keys = managed.keys().map(|key| (key.monitor_id(), key.clone())).collect();
    let interfaces = managed
        .iter()
        .map(|(key, active)| {
            let record = &active.record;
            let address = active.native.as_ref().map(|native| native.address);
            let registered = address.is_some_and(|address| manager.online(&address).is_some());
            let (hash, source) = match &active.origin {
                ManagedInterfaceOrigin::Discovered { endpoint, source, .. } => {
                    (Some(*endpoint), Some(source.clone()))
                }
                _ => (None, None),
            };
            let stream =
                registered && matches!(record.kind.as_str(), "tcp_client" | "backbone_client");
            RuntimeInterfaceState {
                id: key.monitor_id(),
                autoconnect_hash: hash,
                autoconnect_source: source,
                target_host: stream.then(|| record.host.clone()).flatten(),
                target_port: stream.then_some(record.port).flatten(),
                i2p_b32: None,
                bootstrap_only: registered
                    && record
                        .settings
                        .as_ref()
                        .and_then(|settings| settings["bootstrap_only"].as_bool())
                        == Some(true),
                online: address.and_then(|address| manager.online(&address)).unwrap_or(false),
                down_since: None,
            }
        })
        .collect();
    let names = manager
        .interface_hashes()
        .iter()
        .filter_map(|hash| manager.display_name(hash).map(str::to_owned))
        .collect();
    // Timestamp after the native reads: lock contention cannot backdate observed downtime.
    NativeInterfaceSnapshot { observed_at: Instant::now(), interfaces, names, keys }
}

pub(super) fn same_record(left: &InterfaceRecord, right: &InterfaceRecord) -> bool {
    left.kind == right.kind
        && left.name == right.name
        && left.host == right.host
        && left.port == right.port
}

fn admission_states(
    snapshot: NativeInterfaceSnapshot,
    managed: &HashMap<ManagedInterfaceKey, ManagedHotApplyInterface>,
    daemon: &RpcDaemon,
) -> Vec<RuntimeInterfaceState> {
    let records = daemon.interface_records();
    let mut states = snapshot.interfaces;
    for state in &mut states {
        if let Some(active) = snapshot.keys.get(&state.id).and_then(|key| managed.get(key)) {
            state.id = active.record.name.clone().unwrap_or_else(|| {
                records.iter().position(|record| same_record(record, &active.record)).map_or_else(
                    || state.id.clone(),
                    |index| format!("{}[{index}]", active.record.kind),
                )
            });
        }
    }
    let names =
        snapshot.names.into_iter().chain(records.into_iter().enumerate().map(|(index, record)| {
            record.name.unwrap_or_else(|| format!("{}[{index}]", record.kind))
        }));
    for name in names {
        if !states.iter().any(|state| state.id == name) {
            states.push(RuntimeInterfaceState {
                id: name,
                autoconnect_hash: None,
                autoconnect_source: None,
                target_host: None,
                target_port: None,
                i2p_b32: None,
                bootstrap_only: false,
                online: false,
                down_since: None,
            });
        }
    }
    states
}

pub(super) fn publish_live_runtime_records(
    managed: &HashMap<ManagedInterfaceKey, ManagedHotApplyInterface>,
    daemon: &RpcDaemon,
) {
    let mut records = daemon.interface_records();
    for active in managed.values() {
        let position = match &active.configured {
            Some(binding) => Some(binding.position),
            None => records.iter().position(|record| same_record(record, &active.record)),
        };
        if let Some(record) = position.and_then(|position| records.get_mut(position)) {
            record_publication::project_native_record(record, active);
        } else {
            let mut record = active.record.clone();
            record_publication::project_native_record(&mut record, active);
            records.push(record);
        }
    }
    daemon.replace_interfaces(records);
}

pub(super) fn key_for_discovered_name(
    managed: &HashMap<ManagedInterfaceKey, ManagedHotApplyInterface>,
    name: &str,
) -> Option<ManagedInterfaceKey> {
    managed.iter().find_map(|(key, active)| {
        (active.origin.endpoint_hash().is_some() && active.record.name.as_deref() == Some(name))
            .then(|| key.clone())
    })
}

pub(super) async fn detach_discovered(
    manager: &Arc<tokio::sync::Mutex<InterfaceManager>>,
    managed: &mut HashMap<ManagedInterfaceKey, ManagedHotApplyInterface>,
    name: &str,
    transport: Option<&Arc<Transport>>,
    daemon: Option<&Weak<RpcDaemon>>,
) -> io::Result<serde_json::Value> {
    let key = key_for_discovered_name(managed, name)
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "discovered interface is absent"))?;
    let active = managed
        .remove(&key)
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "discovered interface is absent"))?;
    let stopped = match active.native {
        Some(native) => stop_owned_native(manager, transport, native).await,
        None => Ok(()),
    };
    if let Some(daemon) = daemon.and_then(Weak::upgrade) {
        let mut records = daemon.interface_records();
        if let Some(position) =
            records.iter().position(|record| same_record(record, &active.record))
        {
            records.remove(position);
            for entry in managed.values_mut() {
                if let Some(binding) = &mut entry.configured {
                    if binding.position > position {
                        binding.position -= 1;
                    }
                }
            }
        }
        daemon.replace_interfaces(records);
    }
    stopped?;
    Ok(
        serde_json::json!({ "name": name, "operation": "detach", "complete": true, "state": "detached" }),
    )
}

#[cfg(test)]
#[path = "../tests/interface_hot_apply/discovery_owner.rs"]
mod owner_tests;
#[cfg(test)]
#[path = "../tests/interface_hot_apply/discovery_runtime.rs"]
mod tests;

#[cfg(test)]
#[path = "../tests/interface_hot_apply/configured_root_owner.rs"]
mod configured_root_owner_tests;
