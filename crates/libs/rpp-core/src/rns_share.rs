//! One-file sharing over RNS (feature `rns`) for applications such as REM
//! and RCH: publish a file as a signed Package, answer RDO READ of its
//! publication record, serve pulls only to chosen recipients, send and
//! answer Offers on `rpp.receive`, and fetch an offered Package to a store.

use std::collections::{HashMap, HashSet};
use std::io::{Read as _, Seek as _, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex as StdMutex};
use std::time::Duration;

use rand_core::{OsRng, RngCore};
use rdo_core::rns::{call, open_object_link, register_object, resolve_object, OwnerService};
use rdo_core::{lookup, Owner, Revision};
use rns_transport::destination::link::{unpack_response_envelope, Link};
use rns_transport::destination::{DestinationDesc, DestinationName};
use rns_transport::hash::{address_hash, AddressHash};
use rns_transport::identity::PrivateIdentity;
use rns_transport::packet::PacketContext;
use rns_transport::transport::Transport;
use tokio::sync::broadcast;

use crate::cbor::{self, Value};
use crate::control::{Body, Control, OfferDecision};
use crate::error::{Result, RppError};
use crate::identity::{Destination, PublicIdentity, SigningIdentity};
use crate::inbox::{answer_offer, build_offer};
use crate::manifest::VerifiedManifest;
use crate::manifest_build::{FileDraft, ManifestDraft};
use crate::publication::Publication;
use crate::publisher::{Publisher, PublisherLimits};
use crate::receiver::{Pull, PullConfig};
use crate::replay::ReplayCache;
use crate::rns::{public_identity, run_pull, Clock, PublisherService};
use crate::store::{FsStore, PackageSource};
use crate::REQUEST_PATH;

const PACKAGE_TYPE: &str = "https://freetakteam.github.io/rpp/ontology#Package";
/// Chunk size for shared files: small enough for constrained links.
pub const SHARE_CHUNK_BYTES: usize = 64 * 1024;

/// Reads one shared file regardless of its manifest path.
pub struct FileSource(pub PathBuf);

impl PackageSource for FileSource {
    fn read_chunk(&self, verified: &VerifiedManifest, index: u32) -> Result<Vec<u8>> {
        let manifest = &verified.manifest;
        let chunk = manifest.chunks.get(index as usize).ok_or(RppError::InvalidSelection)?;
        let mut file = std::fs::File::open(&self.0)?;
        file.seek(SeekFrom::Start(manifest.chunk_offsets[index as usize]))?;
        let mut bytes = vec![0u8; chunk.len as usize];
        file.read_exact(&mut bytes)?;
        Ok(bytes)
    }
}

/// What an application persists to republish a shared file after restart.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SharedPackage {
    pub package_token: [u8; 16],
    /// Exact signed manifest bytes.
    pub manifest: Vec<u8>,
    /// Signed RDO Package publication record.
    pub publication: Vec<u8>,
}

/// An Offer that arrived on this node's inbox from the verified owner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReceivedOffer {
    pub sender: PublicIdentity,
    pub package_destination: Destination,
    pub manifest_hash: [u8; 32],
}

/// A completed, verified fetch.
#[derive(Debug, Clone)]
pub struct FetchedFile {
    pub path: PathBuf,
    pub name: String,
    pub media_type: Option<String>,
    pub manifest: VerifiedManifest,
}

type Allowed = Arc<StdMutex<HashMap<Destination, HashSet<Destination>>>>;

fn lock<T>(mutex: &StdMutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn random_16() -> [u8; 16] {
    let mut bytes = [0u8; 16];
    OsRng.fill_bytes(&mut bytes);
    bytes
}

/// Publishing, serving and fetching for one node identity on one transport.
pub struct ShareNode {
    transport: Arc<Transport>,
    identity: PrivateIdentity,
    signer: SigningIdentity,
    publisher: Arc<PublisherService<FileSource>>,
    owners: Arc<OwnerService>,
    allowed: Allowed,
    clock: Clock,
}

struct AllowedReaders(Allowed);

impl rdo_core::Policy for AllowedReaders {
    fn may_read(&self, who: &PublicIdentity) -> bool {
        lock(&self.0).values().any(|readers| readers.contains(&who.hash()))
    }
    fn may_propose(&self, _: &PublicIdentity) -> bool {
        false
    }
}

impl ShareNode {
    /// Registers the `rpp.receive` inbox and starts the publisher, owner and
    /// inbox services. `on_offer` is called for each verified Offer, which is
    /// answered `defer`: the operator decides later whether to pull.
    pub async fn start(
        transport: Arc<Transport>,
        identity: PrivateIdentity,
        clock: Clock,
        on_offer: impl Fn(ReceivedOffer) + Send + Sync + 'static,
    ) -> Arc<Self> {
        let signer = SigningIdentity::from_private_bytes(&identity.to_private_key_bytes());
        let allowed: Allowed = Arc::default();
        let policy = {
            let allowed = allowed.clone();
            Box::new(move |request: &crate::publisher::AuthRequest<'_>| {
                let package = request.manifest.manifest.package_destination;
                lock(&allowed)
                    .get(&package)
                    .is_some_and(|set| set.contains(&request.requester.hash()))
            })
        };
        let engine = Publisher::new(policy, PublisherLimits::default(), clock());
        let publisher = Arc::new(PublisherService::new(transport.clone(), engine, clock));
        let owners = Arc::new(OwnerService::new(
            transport.clone(),
            Arc::new(AllowedReaders(allowed.clone())),
            clock,
        ));
        let node =
            Arc::new(Self { transport, identity, signer, publisher, owners, allowed, clock });
        let inbox = node
            .transport
            .add_destination(node.identity.clone(), DestinationName::new("rpp", "receive"))
            .await;
        let inbox_destination = inbox.lock().await.desc.address_hash;
        let publisher = node.publisher.clone();
        tokio::spawn(async move { publisher.run().await });
        let owners = node.owners.clone();
        tokio::spawn(async move { owners.run().await });
        let this = node.clone();
        tokio::spawn(async move { this.run_inbox(inbox_destination, on_offer).await });
        node
    }

    pub fn me(&self) -> PublicIdentity {
        self.signer.public()
    }

    /// Publishes `path` as a new one-file Package named `name`.
    pub async fn publish_file(
        &self,
        path: &Path,
        name: &str,
        media_type: Option<&str>,
    ) -> Result<SharedPackage> {
        let package_token = random_16();
        let me = self.signer.public();
        let package_destination = lookup::object_destination(&me, &package_token);
        let manifest_destination = lookup::object_destination(&me, &random_16());
        let file_destination = lookup::object_destination(&me, &random_16());
        let mut file = FileDraft::from_reader(
            name,
            file_destination,
            std::fs::File::open(path)?,
            SHARE_CHUNK_BYTES,
        )?;
        file.media_type = media_type.map(str::to_string);
        let manifest = ManifestDraft {
            package_destination,
            manifest_destination,
            files: vec![file],
            created: Some((self.clock)()),
            display_name: Some(name.to_string()),
            ..ManifestDraft::default()
        }
        .sign(&self.signer)?;
        let body = cbor::map([
            (0, Some(Value::Bytes(package_destination.to_vec()))),
            (1, Some(Value::Bytes(manifest_destination.to_vec()))),
            (2, Some(Value::Bytes(manifest.manifest.package_id.to_vec()))),
            (3, Some(Value::Bytes(manifest.hash.to_vec()))),
        ])
        .into_map()?;
        let publication =
            Revision::sign(&self.signer, package_destination, None, PACKAGE_TYPE, body)?.bytes;
        let shared = SharedPackage { package_token, manifest: manifest.bytes.clone(), publication };
        self.serve(&shared, path.to_path_buf(), []).await?;
        Ok(shared)
    }

    /// Serves a published Package from `path` (also after restart) and
    /// allows the given recipients' identity hashes to pull it.
    pub async fn serve(
        &self,
        shared: &SharedPackage,
        path: PathBuf,
        recipients: impl IntoIterator<Item = Destination>,
    ) -> Result<VerifiedManifest> {
        let me = self.signer.public();
        let package_destination = lookup::object_destination(&me, &shared.package_token);
        let revision = Revision::verify(&shared.publication, &me)?;
        let publication = Publication::verify(&shared.publication, &me)?;
        let binding = crate::manifest::OwnerBinding {
            owner: me,
            package_destination,
            manifest_destination: publication.manifest_destination,
        };
        let manifest = VerifiedManifest::verify(&shared.manifest, &binding)?;
        publication.check_manifest(&manifest)?;
        lock(&self.allowed).entry(package_destination).or_default().extend(recipients);
        let signer = SigningIdentity::from_private_bytes(&self.identity.to_private_key_bytes());
        self.owners.add(Owner::new(signer, vec![revision])?);
        lock(&self.publisher.engine()).publish(manifest.clone(), FileSource(path));
        let destination =
            register_object(&self.transport, &self.identity, &shared.package_token).await;
        self.transport
            .send_announce(&destination, Some(&lookup::announce_app_data(&shared.package_token)))
            .await;
        Ok(manifest)
    }

    /// Lets `recipient` pull the Package and sends it an Offer. Returns the
    /// recipient's decision.
    pub async fn offer(
        &self,
        shared: &SharedPackage,
        recipient: PublicIdentity,
        recipient_desc: DestinationDesc,
        timeout: Duration,
    ) -> Result<OfferDecision> {
        let me = self.signer.public();
        let package_destination = lookup::object_destination(&me, &shared.package_token);
        let publication = Publication::verify(&shared.publication, &me)?;
        let binding = crate::manifest::OwnerBinding {
            owner: me,
            package_destination,
            manifest_destination: publication.manifest_destination,
        };
        let manifest = VerifiedManifest::verify(&shared.manifest, &binding)?;
        lock(&self.allowed).entry(package_destination).or_default().insert(recipient.hash());
        let link = open_object_link(&self.transport, &self.identity, recipient_desc, timeout)
            .await
            .map_err(RppError::from)?;
        let request = build_offer(&manifest, &recipient, (self.clock)(), 120);
        let response = request_bytes(&self.transport, &link, request, timeout).await?;
        let result = crate::control::RppResult::decode(&response)?;
        match result.value.as_deref().map(Control::decode) {
            Some(Ok(Control { body: Body::OfferAck { decision }, .. })) => Ok(decision),
            _ if result.status == crate::control::Status::NotAuthorized => {
                Err(RppError::NotAuthorized)
            }
            _ => Err(RppError::Malformed("Offer was not acknowledged")),
        }
    }

    /// The `rpp.receive` DestinationDesc of a peer identity, for [`Self::offer`].
    pub fn inbox_of(identity: rns_transport::identity::Identity) -> DestinationDesc {
        rns_transport::destination::new_out(identity, "rpp", "receive").desc
    }

    /// Pulls an offered Package into `store_dir` and returns the verified file.
    pub async fn fetch(
        &self,
        package_destination: Destination,
        store_dir: &Path,
        timeout: Duration,
    ) -> Result<FetchedFile> {
        let (owner, desc) = resolve_object(&self.transport, &package_destination, timeout)
            .await
            .map_err(RppError::from)?;
        let link = open_object_link(&self.transport, &self.identity, desc, timeout)
            .await
            .map_err(RppError::from)?;
        let mut id = [0u8; 16];
        OsRng.fill_bytes(&mut id);
        let read = rdo_core::Request::read(id, package_destination, (self.clock)() + 60);
        let answer = call(&self.transport, &link, &read, timeout).await.map_err(RppError::from)?;
        if answer.status != rdo_core::Status::Applied {
            return Err(match answer.status {
                rdo_core::Status::NotAuthorized => RppError::NotAuthorized,
                _ => RppError::NotFound,
            });
        }
        let record = answer.value.ok_or(RppError::NotFound)?;
        let publication = Publication::verify(&record, &owner.owner)?;
        let binding = crate::manifest::OwnerBinding {
            owner: owner.owner,
            package_destination,
            manifest_destination: publication.manifest_destination,
        };
        let me = self.signer.public();
        let mut pull = Pull::new(
            FsStore::open(store_dir)?,
            &me,
            binding,
            publication.clone(),
            PullConfig { max_bundle: 256 * 1024, window: 2, ..PullConfig::default() },
        )?;
        let progress =
            run_pull(&self.transport, &self.identity, desc, &mut pull, timeout, self.clock).await?;
        if !progress.complete_verified {
            return Err(RppError::Incomplete);
        }
        let manifest = pull.manifest().cloned().ok_or(RppError::Incomplete)?;
        let file = manifest.manifest.files.first().ok_or(RppError::Incomplete)?;
        let path = FsStore::open(store_dir)?.package_dir(&manifest.hash).join(&file.path);
        Ok(FetchedFile {
            path,
            name: file.path.clone(),
            media_type: file.media_type.clone(),
            manifest,
        })
    }

    async fn run_inbox(
        self: Arc<Self>,
        inbox: AddressHash,
        on_offer: impl Fn(ReceivedOffer) + Send + Sync + 'static,
    ) {
        let me = self.signer.public();
        let mut replay = ReplayCache::new((self.clock)(), 256);
        let mut data = self.transport.received_data_events();
        loop {
            let event = match data.recv().await {
                Ok(event) => event,
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(broadcast::error::RecvError::Closed) => return,
            };
            if event.context != Some(PacketContext::Request) {
                continue;
            }
            let (Some(request_id), Some(request)) =
                (event.request_id, rpp_request(event.data.as_slice()))
            else {
                continue;
            };
            let Some(link) = self.transport.find_in_link(&event.destination).await else {
                continue;
            };
            let (destination, sender) = {
                let guard = link.lock().await;
                (
                    guard.destination().address_hash,
                    guard.identified_peer_identity().map(public_identity),
                )
            };
            if destination != inbox {
                continue;
            }
            // Resolve the claimed Package's owner independently before answering.
            let claimed = Control::decode(&request).ok().map(|c| c.header.package_destination);
            let owner = match claimed {
                Some(package) => resolve_object(&self.transport, &package, Duration::from_secs(20))
                    .await
                    .ok()
                    .map(|(binding, _)| binding.owner),
                None => None,
            };
            let mut offered = None;
            let response = answer_offer(
                &mut replay,
                &me,
                sender.as_ref(),
                &request,
                (self.clock)(),
                |package| owner.filter(|_| Some(*package) == claimed),
                |control| {
                    offered = Some(control.header);
                    OfferDecision::Defer
                },
            );
            let Some(response) = response else { continue };
            if let (Some(header), Some(sender)) = (offered, sender) {
                on_offer(ReceivedOffer {
                    sender,
                    package_destination: header.package_destination,
                    manifest_hash: header.manifest_hash,
                });
            }
            let envelope = rmpv::Value::Array(vec![
                rmpv::Value::Binary(request_id.to_vec()),
                rmpv::Value::Binary(response),
            ]);
            let mut encoded = Vec::new();
            if rmpv::encode::write_value(&mut encoded, &envelope).is_ok() {
                let sent = self
                    .transport
                    .send_response(&event.destination, request_id.to_vec(), encoded, None)
                    .await;
                if let Err(error) = sent {
                    log::warn!("rpp: inbox response failed: {error:?}");
                }
            }
        }
    }
}

fn rpp_request(packed: &[u8]) -> Option<Vec<u8>> {
    let mut cursor = std::io::Cursor::new(packed);
    let value = rmpv::decode::read_value(&mut cursor).ok()?;
    match value.as_array()?.as_slice() {
        [_, path, rmpv::Value::Binary(data)]
            if path.as_slice() == Some(address_hash(REQUEST_PATH.as_bytes()).as_slice()) =>
        {
            Some(data.clone())
        }
        _ => None,
    }
}

async fn request_bytes(
    transport: &Transport,
    link: &Arc<tokio::sync::Mutex<Link>>,
    request: Vec<u8>,
    timeout: Duration,
) -> Result<Vec<u8>> {
    let payload = Link::request_payload(REQUEST_PATH, rmpv::Value::Binary(request))
        .map_err(|e| RppError::Transport(format!("request payload: {e:?}")))?;
    let mut receipt = transport
        .send_request_on_link(link, payload, timeout, None)
        .await
        .map_err(|e| RppError::Transport(format!("send request: {e:?}")))?;
    let response = tokio::time::timeout(timeout, receipt.wait())
        .await
        .map_err(|_| RppError::Transport("response timed out".into()))?
        .map_err(|e| RppError::Transport(format!("request: {e:?}")))?;
    let (_, value) = unpack_response_envelope(&response.data)
        .map_err(|e| RppError::Transport(format!("response: {e:?}")))?;
    match value {
        rmpv::Value::Binary(bytes) => Ok(bytes),
        _ => Err(RppError::Malformed("response is not a byte string")),
    }
}
