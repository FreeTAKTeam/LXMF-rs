//! RNS adapter (feature `rns`): runs the sans-IO engines over public
//! `reticulum-rs-transport` Links, Link requests and native Resources.
//!
//! The caller still owns RDO resolution: it registers Package Destinations
//! (see [`rdo_object_name`]) and supplies the resolved `DestinationDesc`.

use std::collections::VecDeque;
use std::io::Cursor;
use std::sync::{Arc, Mutex as StdMutex, MutexGuard};
use std::time::Duration;

use rns_transport::destination::link::{unpack_response_envelope, Link, LinkEvent};
use rns_transport::destination::{DestinationDesc, DestinationName};
use rns_transport::hash::{address_hash, AddressHash};
use rns_transport::identity::{Identity, PrivateIdentity};
use rns_transport::packet::PacketContext;
use rns_transport::resource::{LinkResourceAdmission, ResourceEvent, ResourceEventKind};
use rns_transport::transport::{SendPacketOutcome, Transport};
use tokio::sync::{broadcast, Mutex};

use crate::control::MAX_WINDOW;
use crate::error::{Result, RppError};
use crate::identity::PublicIdentity;
use crate::manifest::MAX_SIGNED_MANIFEST;
use crate::publisher::{LinkId, Publisher};
use crate::receiver::{Progress, Pull};
use crate::store::{ChunkStore, PackageSource};
use crate::{pack_resource_metadata, unpack_resource_metadata, REQUEST_PATH};

/// Native framing allowance on top of an RPP payload: msgpack metadata
/// (18 bytes) plus its 3-byte length prefix, rounded up.
const METADATA_ALLOWANCE: u64 = 64;

/// Clock in UTC Unix seconds.
pub type Clock = fn() -> u64;

pub fn system_clock() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs())
}

/// The RDO owner binding v1 destination name for a 16-byte object token.
pub fn rdo_object_name(token: &[u8; 16]) -> DestinationName {
    DestinationName::new("rdo", &format!("object.{}", crate::store::hex(token)))
}

pub fn public_identity(identity: &Identity) -> PublicIdentity {
    let mut bytes = [0u8; 64];
    bytes[..32].copy_from_slice(identity.public_key_bytes());
    bytes[32..].copy_from_slice(identity.verifying_key_bytes());
    PublicIdentity(bytes)
}

fn transport_error(context: &str, error: impl std::fmt::Debug) -> RppError {
    RppError::Transport(format!("{context}: {error:?}"))
}

fn link_bytes(id: &AddressHash) -> LinkId {
    id.as_slice().try_into().unwrap_or([0; 16])
}

/// Unpacks `[time, path_hash, data]` and returns `data` for `/rpp/3` only.
fn rpp_request_data(packed: &[u8]) -> Option<Vec<u8>> {
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

fn lock<T>(mutex: &StdMutex<T>) -> MutexGuard<'_, T> {
    // A panic inside the engine leaves no half-applied state worth refusing.
    mutex.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Serves `/rpp/3` for every Package the engine publishes. Register each
/// Package Destination on the transport (and announce it) before `run`.
pub struct PublisherService<S: PackageSource + Send + 'static> {
    transport: Arc<Transport>,
    engine: Arc<StdMutex<Publisher<S>>>,
    clock: Clock,
}

impl<S: PackageSource + Send + 'static> PublisherService<S> {
    pub fn new(transport: Arc<Transport>, engine: Publisher<S>, clock: Clock) -> Self {
        Self { transport, engine: Arc::new(StdMutex::new(engine)), clock }
    }

    pub fn engine(&self) -> Arc<StdMutex<Publisher<S>>> {
        self.engine.clone()
    }

    /// Runs until the transport's event channels close.
    pub async fn run(&self) -> Result<()> {
        let mut data = self.transport.received_data_events();
        let mut resources = self.transport.resource_events();
        let mut links = self.transport.in_link_events();
        loop {
            tokio::select! {
                event = data.recv() => match event {
                    Ok(event) if event.context == Some(PacketContext::Request) => {
                        if let Some(id) = event.request_id {
                            self.handle(event.destination, id.to_vec(), event.data.as_slice()).await;
                        }
                    }
                    Ok(_) => {}
                    Err(error) => lagged_or_closed("received data", error)?,
                },
                event = resources.recv() => match event {
                    Ok(ResourceEvent { link_id, kind: ResourceEventKind::Complete(complete), .. })
                        if complete.is_request =>
                    {
                        if let Some(id) = complete.request_id {
                            self.handle(link_id, id, &complete.data).await;
                        }
                    }
                    Ok(_) => {}
                    Err(error) => lagged_or_closed("resource", error)?,
                },
                event = links.recv() => match event {
                    Ok(event) if matches!(event.event, LinkEvent::Closed) => {
                        lock(&self.engine).link_closed(link_bytes(&event.id));
                    }
                    Ok(_) => {}
                    Err(error) => lagged_or_closed("link", error)?,
                },
            }
        }
    }

    async fn handle(&self, link_id: AddressHash, request_id: Vec<u8>, packed: &[u8]) {
        let Some(request) = rpp_request_data(packed) else { return };
        let Some(link) = self.transport.find_in_link(&link_id).await else { return };
        let (destination, peer) = {
            let guard = link.lock().await;
            (
                guard.destination().address_hash,
                guard.identified_peer_identity().map(public_identity),
            )
        };
        let served = lock(&self.engine).handle(
            link_bytes(&link_id),
            link_bytes(&destination),
            peer.as_ref(),
            &request,
            (self.clock)(),
        );
        let Some(served) = served else { return };
        let envelope = rmpv::Value::Array(vec![
            rmpv::Value::Binary(request_id.clone()),
            rmpv::Value::Binary(served.response),
        ]);
        let mut encoded = Vec::new();
        if rmpv::encode::write_value(&mut encoded, &envelope).is_err() {
            return;
        }
        if let Err(error) = self.transport.send_response(&link_id, request_id, encoded, None).await
        {
            log::warn!("rpp: response on link {link_id} failed: {error:?}");
            return;
        }
        let Some(outbound) = served.outbound else { return };
        let (transport, engine) = (self.transport.clone(), self.engine.clone());
        tokio::spawn(async move {
            let size = outbound.payload.len() as u64;
            let mut completion = None;
            let sent = transport
                .send_resource_from_reader_with_completion(
                    &link_id,
                    Cursor::new(outbound.payload),
                    size,
                    Some(pack_resource_metadata(&outbound.metadata)),
                    false,
                    |_, observed| completion = Some(observed),
                )
                .await;
            if let Err(error) = sent {
                log::warn!("rpp: Resource on link {link_id} failed to start: {error:?}");
            }
            if let Some(mut completion) = completion {
                let outcome = completion.wait().await;
                log::debug!("rpp: Resource on link {link_id} concluded: {outcome:?}");
            }
            lock(&engine).resource_concluded(link_bytes(&link_id));
        });
    }
}

fn lagged_or_closed(stream: &str, error: broadcast::error::RecvError) -> Result<()> {
    match error {
        broadcast::error::RecvError::Lagged(missed) => {
            log::warn!(
                "rpp: {stream} events lagged by {missed}; affected requests will be retried"
            );
            Ok(())
        }
        broadcast::error::RecvError::Closed => {
            Err(RppError::Transport(format!("{stream} events closed")))
        }
    }
}

/// Opens and identifies a Link to a Package Destination and bounds its
/// inbound Resources to RPP's limits.
pub async fn open_package_link(
    transport: &Transport,
    me: &PrivateIdentity,
    package: DestinationDesc,
    timeout: Duration,
) -> Result<Arc<Mutex<Link>>> {
    let mut events = transport.out_link_events();
    let link = transport.link(package).await;
    let link_id = *link.lock().await.id();
    tokio::time::timeout(timeout, async {
        loop {
            match events.recv().await {
                Ok(event) if event.id == link_id && matches!(event.event, LinkEvent::Activated) => {
                    return Ok::<(), RppError>(());
                }
                Ok(_) => {}
                Err(error) => lagged_or_closed("link", error)?,
            }
        }
    })
    .await
    .map_err(|_| RppError::Transport("link establishment timed out".into()))??;
    let identify = {
        let guard = link.lock().await;
        guard
            .identify_packet(&guard.identify_payload(me))
            .map_err(|e| transport_error("identify", e))?
    };
    let outcome = transport.send_link_packet_on_bound_iface(&link, identify).await;
    if !matches!(outcome, SendPacketOutcome::SentDirect | SendPacketOutcome::SentBroadcast) {
        return Err(RppError::Transport("could not identify on link".into()));
    }
    let max_data_size =
        (MAX_SIGNED_MANIFEST as u64).max(crate::control::MAX_BUNDLE_BYTES) + METADATA_ALLOWANCE;
    let admission = LinkResourceAdmission { max_data_size, max_concurrent: MAX_WINDOW as usize };
    transport.set_link_resource_admission(&link_id, Some(admission)).await;
    Ok(link)
}

/// Drives `pull` over one Package Link until the publisher acknowledges the
/// TransferReceipt. On error the Link-local state is dropped; verified chunks
/// stay, so calling again resumes with only the missing chunks.
pub async fn run_pull<S: ChunkStore>(
    transport: &Transport,
    me: &PrivateIdentity,
    package: DestinationDesc,
    pull: &mut Pull<S>,
    timeout: Duration,
    clock: Clock,
) -> Result<Progress> {
    // Subscribe before the Link exists so no Resource completion is missed.
    let mut resources = transport.resource_events();
    let link = open_package_link(transport, me, package, timeout).await?;
    let result = drive(transport, &link, &mut resources, pull, timeout, clock).await;
    if result.is_err() {
        pull.link_closed();
    }
    if let Err(error) = transport.teardown_link_observed(&link, || {}).await {
        log::debug!("rpp: link teardown handoff failed: {error:?}");
    }
    result
}

async fn drive<S: ChunkStore>(
    transport: &Transport,
    link: &Arc<Mutex<Link>>,
    resources: &mut broadcast::Receiver<ResourceEvent>,
    pull: &mut Pull<S>,
    timeout: Duration,
    clock: Clock,
) -> Result<Progress> {
    let link_id = *link.lock().await.id();
    let mut queue = VecDeque::new();
    loop {
        if pull.progress().receipt_acknowledged {
            return Ok(pull.progress());
        }
        if queue.is_empty() {
            queue.extend(pull.next_requests(clock())?);
        }
        let Some(request) = queue.pop_front() else {
            // Waiting only for accepted Resources.
            let event = tokio::time::timeout(timeout, resources.recv())
                .await
                .map_err(|_| RppError::Transport("Resource wait timed out".into()))?;
            match event {
                Ok(event) => on_resource_event(pull, &link_id, event),
                Err(error) => lagged_or_closed("resource", error)?,
            }
            continue;
        };
        let payload = Link::request_payload(REQUEST_PATH, rmpv::Value::Binary(request))
            .map_err(|e| transport_error("request payload", e))?;
        let mut receipt = transport
            .send_request_on_link(link, payload, timeout, None)
            .await
            .map_err(|e| transport_error("send request", e))?;
        let response = {
            let wait = receipt.wait();
            tokio::pin!(wait);
            let deadline = tokio::time::sleep(timeout);
            tokio::pin!(deadline);
            loop {
                tokio::select! {
                    response = &mut wait => break response.map_err(|e| transport_error("request", e))?,
                    event = resources.recv() => match event {
                        Ok(event) => on_resource_event(pull, &link_id, event),
                        Err(error) => lagged_or_closed("resource", error)?,
                    },
                    _ = &mut deadline => return Err(RppError::Transport("response timed out".into())),
                }
            }
        };
        let (_, value) =
            unpack_response_envelope(&response.data).map_err(|e| transport_error("response", e))?;
        let rmpv::Value::Binary(bytes) = value else {
            return Err(RppError::Malformed("response is not a byte string"));
        };
        queue.extend(pull.on_response(&bytes, clock())?);
    }
}

fn on_resource_event<S: ChunkStore>(
    pull: &mut Pull<S>,
    link_id: &AddressHash,
    event: ResourceEvent,
) {
    if event.link_id != *link_id {
        return;
    }
    match event.kind {
        ResourceEventKind::Complete(complete) if !complete.is_request && !complete.is_response => {
            let accepted =
                unpack_resource_metadata(complete.metadata.as_deref().unwrap_or_default())
                    .and_then(|id| pull.on_resource(&id, &complete.data));
            if let Err(error) = accepted {
                // Discarded; verified data is kept and the gap is re-requested.
                log::warn!("rpp: discarded Resource on link {link_id}: {error}");
            }
        }
        ResourceEventKind::InboundFailed(failure) => {
            log::warn!("rpp: inbound Resource on link {link_id} failed: {}", failure.reason);
        }
        _ => {}
    }
}
