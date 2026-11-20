//! RNS adapter (feature `rns`): resolve an object from its Destination alone,
//! and serve or call owner operations on `/rdo/1/op` over identified Links.

use std::collections::HashMap;
use std::io::Cursor;
use std::sync::{Arc, Mutex as StdMutex, MutexGuard};
use std::time::Duration;

use rns_transport::destination::link::{unpack_response_envelope, Link, LinkEvent, LinkStatus};
use rns_transport::destination::{
    DestinationAnnounce, DestinationDesc, DestinationName, SingleInputDestination,
};
use rns_transport::hash::{address_hash, AddressHash};
use rns_transport::identity::{Identity, PrivateIdentity};
use rns_transport::packet::PacketContext;
use rns_transport::transport::{SendPacketOutcome, Transport};
use tokio::sync::{broadcast, Mutex};

use crate::error::{RdoError, Result};
use crate::identity::{Destination, PublicIdentity};
use crate::lookup::{self, ObjectBinding, Token};
use crate::owner::{OpResult, Owner, Policy, Request, REQUEST_PATH};

/// Clock in UTC Unix seconds.
pub type Clock = fn() -> u64;

pub fn system_clock() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs())
}

/// `rdo` / `object.<hex token>`, the name an owner registers.
pub fn object_name(token: &Token) -> DestinationName {
    DestinationName::new(lookup::APP_NAME, &lookup::object_aspect(token))
}

pub fn public_identity(identity: &Identity) -> PublicIdentity {
    let mut bytes = [0u8; 64];
    bytes[..32].copy_from_slice(identity.public_key_bytes());
    bytes[32..].copy_from_slice(identity.verifying_key_bytes());
    PublicIdentity(bytes)
}

fn transport_error(context: &str, error: impl std::fmt::Debug) -> RdoError {
    RdoError::Transport(format!("{context}: {error:?}"))
}

fn destination_bytes(hash: &AddressHash) -> Destination {
    hash.as_slice().try_into().unwrap_or([0; 16])
}

fn lock<T>(mutex: &StdMutex<T>) -> MutexGuard<'_, T> {
    // Owner state is only replaced whole; a poisoned guard holds valid state.
    mutex.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Registers an owned object on the transport, so path responses carry the
/// signed `CBOR([1, token])` app data. Announce the returned Destination with
/// [`lookup::announce_app_data`] as well.
pub async fn register_object(
    transport: &Transport,
    owner: &PrivateIdentity,
    token: &Token,
) -> Arc<Mutex<SingleInputDestination>> {
    let destination = transport.add_destination(owner.clone(), object_name(token)).await;
    transport
        .set_destination_announce_app_data(&destination, Some(lookup::announce_app_data(token)))
        .await;
    destination
}

/// Serves `/rdo/1/op` for every registered owned object.
pub struct OwnerService {
    transport: Arc<Transport>,
    owners: Arc<StdMutex<HashMap<Destination, Owner>>>,
    policy: Arc<dyn Policy + Send + Sync>,
    clock: Clock,
}

impl OwnerService {
    pub fn new(
        transport: Arc<Transport>,
        policy: Arc<dyn Policy + Send + Sync>,
        clock: Clock,
    ) -> Self {
        Self { transport, owners: Arc::default(), policy, clock }
    }

    pub fn add(&self, owner: Owner) {
        lock(&self.owners).insert(owner.object(), owner);
    }

    /// Shared owner state, e.g. to inspect committed history.
    pub fn owners(&self) -> Arc<StdMutex<HashMap<Destination, Owner>>> {
        self.owners.clone()
    }

    /// Runs until the transport's data events close.
    pub async fn run(&self) -> Result<()> {
        let mut data = self.transport.received_data_events();
        loop {
            match data.recv().await {
                Ok(event) if event.context == Some(PacketContext::Request) => {
                    if let Some(id) = event.request_id {
                        self.handle(event.destination, id.to_vec(), event.data.as_slice()).await;
                    }
                }
                Ok(_) => {}
                Err(broadcast::error::RecvError::Lagged(missed)) => {
                    log::warn!("rdo: data events lagged by {missed}; clients will retry");
                }
                Err(broadcast::error::RecvError::Closed) => {
                    return Err(RdoError::Transport("data events closed".into()));
                }
            }
        }
    }

    async fn handle(&self, link_id: AddressHash, request_id: Vec<u8>, packed: &[u8]) {
        let Some(request) = request_data(packed) else { return };
        let Some(link) = self.transport.find_in_link(&link_id).await else { return };
        let (destination, peer) = {
            let guard = link.lock().await;
            let destination = destination_bytes(&guard.destination().address_hash);
            (destination, guard.identified_peer_identity().map(public_identity))
        };
        let result = {
            let mut owners = lock(&self.owners);
            let Some(owner) = owners.get_mut(&destination) else { return };
            owner.handle(peer.as_ref(), &destination, &request, (self.clock)(), &*self.policy)
        };
        let Some(result) = result else { return };
        let envelope = rmpv::Value::Array(vec![
            rmpv::Value::Binary(request_id.clone()),
            rmpv::Value::Binary(result),
        ]);
        let mut encoded = Vec::new();
        if let Err(error) = rmpv::encode::write_value(&mut encoded, &envelope) {
            log::warn!("rdo: response envelope on link {link_id} failed: {error}");
            return;
        }
        if let Err(error) = self.transport.send_response(&link_id, request_id, encoded, None).await
        {
            log::warn!("rdo: response on link {link_id} failed: {error:?}");
        }
    }
}

/// Unpacks `[time, path_hash, data]` and returns `data` for `/rdo/1/op` only.
fn request_data(packed: &[u8]) -> Option<Vec<u8>> {
    let mut cursor = Cursor::new(packed);
    let value = rmpv::decode::read_value(&mut cursor).ok()?;
    if cursor.position() != packed.len() as u64 {
        return None;
    }
    match value.as_array()?.as_slice() {
        [_, path, rmpv::Value::Binary(data)]
            if path.as_slice() == Some(address_hash(REQUEST_PATH.as_bytes()).as_slice()) =>
        {
            Some(data.clone())
        }
        _ => None,
    }
}

/// Starting from only the wanted Destination: requests its path and binds
/// the first signed announce for it whose token and Identity reconstruct it.
pub async fn resolve_object(
    transport: &Transport,
    wanted: &Destination,
    timeout: Duration,
) -> Result<(ObjectBinding, DestinationDesc)> {
    let mut announces = transport.recv_announces().await;
    // `new_from_slice` hashes its input; the Destination is used as-is.
    let hash = AddressHash::new(*wanted);
    // The announce may have arrived before the caller learned the
    // Destination (it races the reply that names it); use it if cached.
    if let Some(packet) = transport.cached_announce(&hash).await {
        if let Ok(info) = DestinationAnnounce::validate(&packet) {
            let desc = info.destination.desc;
            match lookup::bind(public_identity(&desc.identity), wanted, info.app_data, wanted) {
                Ok(binding) => return Ok((binding, desc)),
                Err(error) => log::warn!("rdo: cached announce for {hash} refused: {error}"),
            }
        }
    }
    tokio::time::timeout(timeout, async {
        transport.request_path(&hash, None, None).await;
        let mut retry = tokio::time::interval(Duration::from_secs(2));
        loop {
            tokio::select! {
                event = announces.recv() => {
                    let event = match event {
                        Ok(event) => event,
                        Err(broadcast::error::RecvError::Lagged(_)) => continue,
                        Err(broadcast::error::RecvError::Closed) => {
                            return Err(RdoError::Transport("announce events closed".into()));
                        }
                    };
                    let desc = event.destination.lock().await.desc;
                    if desc.address_hash != hash {
                        continue;
                    }
                    let owner = public_identity(&desc.identity);
                    match lookup::bind(owner, wanted, event.app_data.as_slice(), wanted) {
                        Ok(binding) => return Ok((binding, desc)),
                        Err(error) => log::warn!("rdo: announce for {hash} refused: {error}"),
                    }
                }
                _ = retry.tick() => {
                    transport.request_path(&hash, None, None).await;
                }
            }
        }
    })
    .await
    .map_err(|_| RdoError::Transport("path or announce timed out".into()))?
}

/// Opens a Link to a resolved object and identifies with `me`.
pub async fn open_object_link(
    transport: &Transport,
    me: &PrivateIdentity,
    object: DestinationDesc,
    timeout: Duration,
) -> Result<Arc<Mutex<Link>>> {
    let mut events = transport.out_link_events();
    let link = transport.link(object).await;
    let (link_id, active) = {
        let guard = link.lock().await;
        (*guard.id(), guard.status() == LinkStatus::Active)
    };
    // `link` reuses an already active Link to the same Destination.
    if !active {
        wait_active(&mut events, link_id, timeout).await?;
    }
    let identify = {
        let guard = link.lock().await;
        guard
            .identify_packet(&guard.identify_payload(me))
            .map_err(|e| transport_error("identify", e))?
    };
    let outcome = transport.send_link_packet_on_bound_iface(&link, identify).await;
    if !matches!(outcome, SendPacketOutcome::SentDirect | SendPacketOutcome::SentBroadcast) {
        return Err(RdoError::Transport("could not identify on link".into()));
    }
    Ok(link)
}

async fn wait_active(
    events: &mut broadcast::Receiver<rns_transport::destination::link::LinkEventData>,
    link_id: AddressHash,
    timeout: Duration,
) -> Result<()> {
    tokio::time::timeout(timeout, async {
        loop {
            match events.recv().await {
                Ok(event) if event.id == link_id && matches!(event.event, LinkEvent::Activated) => {
                    return Ok(());
                }
                Ok(_) | Err(broadcast::error::RecvError::Lagged(_)) => {}
                Err(broadcast::error::RecvError::Closed) => {
                    return Err(RdoError::Transport("link events closed".into()));
                }
            }
        }
    })
    .await
    .map_err(|_| RdoError::Transport("link establishment timed out".into()))?
}

/// Sends one owner operation on an identified Link and decodes the result.
pub async fn call(
    transport: &Transport,
    link: &Arc<Mutex<Link>>,
    request: &Request,
    timeout: Duration,
) -> Result<OpResult> {
    let payload = Link::request_payload(REQUEST_PATH, rmpv::Value::Binary(request.encode()))
        .map_err(|e| transport_error("request payload", e))?;
    let mut receipt = transport
        .send_request_on_link(link, payload, timeout, None)
        .await
        .map_err(|e| transport_error("send request", e))?;
    let response = tokio::time::timeout(timeout, receipt.wait())
        .await
        .map_err(|_| RdoError::Transport("response timed out".into()))?
        .map_err(|e| transport_error("request", e))?;
    let (_, value) =
        unpack_response_envelope(&response.data).map_err(|e| transport_error("response", e))?;
    let rmpv::Value::Binary(bytes) = value else {
        return Err(RdoError::Malformed("response is not a byte string"));
    };
    let result = OpResult::decode(&bytes)?;
    if result.id != request.id {
        return Err(RdoError::Malformed("result does not echo the request ID"));
    }
    Ok(result)
}
