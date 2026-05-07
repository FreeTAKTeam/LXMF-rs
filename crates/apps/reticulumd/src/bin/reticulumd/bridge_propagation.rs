use lxmf::WireMessage;
use rand_core::OsRng;
use reticulum_daemon::lxmf_stamps::generate_propagation_stamp_with_value_until_cancelled;
use rns_core::identity::Identity as CoreIdentity;
use rns_transport::destination::{link::Link, link::LinkStatus, DestinationDesc};
use rns_transport::hash::AddressHash;
use rns_transport::identity::Identity;
use rns_transport::packet::PacketContext;
use rns_transport::transport::Transport;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub(super) const PROPAGATION_INVALID_STAMP_SIGNAL: u8 = 0xF5;
pub(super) const DEFAULT_PROPAGATION_STAMP_COST: u32 = 13;

pub(super) struct PropagationPayload {
    pub(super) bytes: Vec<u8>,
    pub(super) transient_id: [u8; 32],
    pub(super) stamp_value: u32,
}

#[derive(Clone)]
pub(super) struct CachedPropagationLink {
    pub(super) node_hex: String,
    pub(super) link: Arc<tokio::sync::Mutex<Link>>,
}

pub(super) fn build_propagation_payload_until_cancelled(
    payload: &[u8],
    destination_identity: &Identity,
    propagation_stamp_cost: u32,
    cancelled: impl FnMut() -> bool,
) -> Result<PropagationPayload, std::io::Error> {
    let wire = WireMessage::unpack(payload).map_err(std::io::Error::other)?;
    let core_identity = CoreIdentity::new_from_slices(
        destination_identity.public_key_bytes(),
        destination_identity.verifying_key_bytes(),
    );
    let (lxmf_data, transient_id) = wire
        .pack_propagation_transient_with_rng(&core_identity, OsRng)
        .map_err(std::io::Error::other)?;
    let (propagation_stamp, stamp_value) = generate_propagation_stamp_with_value_until_cancelled(
        &transient_id,
        propagation_stamp_cost,
        cancelled,
    )
    .ok_or_else(|| std::io::Error::other("failed to generate propagation stamp"))?;
    let bytes = WireMessage::pack_propagation_envelope(
        now_secs_f64(),
        &lxmf_data,
        Some(propagation_stamp.as_slice()),
    )
    .map_err(std::io::Error::other)?;
    Ok(PropagationPayload { bytes, transient_id, stamp_value })
}

pub(super) async fn cached_propagation_link(
    state: &Arc<tokio::sync::Mutex<Option<CachedPropagationLink>>>,
    node_hex: &str,
) -> Option<Arc<tokio::sync::Mutex<Link>>> {
    let mut guard = state.lock().await;
    let cached = guard.clone()?;

    if cached.node_hex != node_hex {
        *guard = None;
        return None;
    }

    if cached.link.lock().await.status() == LinkStatus::Closed {
        *guard = None;
        return None;
    }

    Some(cached.link)
}

pub(super) async fn propagation_link_for_node(
    transport: &Transport,
    state: &Arc<tokio::sync::Mutex<Option<CachedPropagationLink>>>,
    node_hex: &str,
    destination: DestinationDesc,
) -> Arc<tokio::sync::Mutex<Link>> {
    if let Some(link) = cached_propagation_link(state, node_hex).await {
        return link;
    }

    let link = transport.link(destination).await;
    let mut guard = state.lock().await;
    *guard = Some(CachedPropagationLink { node_hex: node_hex.to_string(), link: link.clone() });
    link
}

pub(super) async fn wait_for_propagation_signal(
    rx: &mut tokio::sync::broadcast::Receiver<rns_transport::transport::ReceivedData>,
    link_id: AddressHash,
    timeout: Duration,
) -> Option<u8> {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            return None;
        }
        let Ok(result) = tokio::time::timeout(remaining, rx.recv()).await else {
            return None;
        };
        let Ok(event) = result else {
            continue;
        };
        if event.destination != link_id {
            continue;
        }
        if !matches!(event.context, Some(PacketContext::None | PacketContext::LinkClose)) {
            continue;
        }
        let Ok(value) = rmp_serde::from_slice::<rmpv::Value>(event.data.as_slice()) else {
            continue;
        };
        let rmpv::Value::Array(items) = value else {
            continue;
        };
        let Some(signal) = items.first().and_then(|entry| entry.as_u64()) else {
            continue;
        };
        return u8::try_from(signal).ok();
    }
}

fn now_secs_f64() -> f64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs_f64()
}
