use lxmf::WireMessage;
use rand_core::OsRng;
use reticulum_daemon::lxmf_stamps::generate_propagation_stamp_with_value_until_cancelled;
use rns_core::identity::Identity as CoreIdentity;
use rns_transport::destination::{link::Link, link::LinkStatus, DestinationDesc};
use rns_transport::identity::Identity;
use rns_transport::transport::Transport;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

pub(super) const PROPAGATION_INVALID_STAMP_SIGNAL: u8 = 0xF5;
/// `LXMRouter.PROPAGATION_COST`: the cost to mine when the relay's announced
/// cost is unknown. A default relay accepts three less
/// (`PROPAGATION_COST_FLEX`), but that minimum is the relay's leniency, not
/// the sender's target, and a relay run with no flexibility accepts nothing
/// below its own cost.
pub(crate) const DEFAULT_PROPAGATION_STAMP_COST: u32 = lxmf::stamp::DEFAULT_PROPAGATION_STAMP_COST;

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

/// A selection hint, not a lifetime lease. Native Link state governs reuse;
/// callers must still observe that exact Link before dispatch.
pub(super) async fn cached_propagation_link(
    state: &Arc<tokio::sync::Mutex<Option<CachedPropagationLink>>>,
    node_hex: &str,
) -> Option<Arc<tokio::sync::Mutex<Link>>> {
    let link = propagation_link_context(state, node_hex).await?;
    if link.lock().await.status() == LinkStatus::Closed {
        None
    } else {
        Some(link)
    }
}

/// Consume a Closed hint once so the retained message can apply the closure
/// schedule before replacement. Active/Pending hints remain available to peers.
pub(super) async fn propagation_link_context(
    state: &Arc<tokio::sync::Mutex<Option<CachedPropagationLink>>>,
    node_hex: &str,
) -> Option<Arc<tokio::sync::Mutex<Link>>> {
    let mut guard = state.lock().await;
    let cached = guard.clone()?;

    if cached.node_hex != node_hex {
        *guard = None;
        return None;
    }

    let status = cached.link.lock().await.status();
    if status != LinkStatus::Closed {
        return Some(cached.link);
    }

    *guard = None;
    Some(cached.link)
}

/// Retire only the hint for the exact context whose closure was processed.
/// A concurrently published replacement remains available to its other owners.
pub(super) async fn forget_propagation_link_context(
    state: &Arc<tokio::sync::Mutex<Option<CachedPropagationLink>>>,
    link: &Arc<tokio::sync::Mutex<Link>>,
) {
    let mut state = state.lock().await;
    if state.as_ref().is_some_and(|cached| Arc::ptr_eq(&cached.link, link)) {
        *state = None;
    }
}

pub(super) async fn propagation_link_for_node(
    transport: &Transport,
    state: &Arc<tokio::sync::Mutex<Option<CachedPropagationLink>>>,
    node_hex: &str,
    destination: DestinationDesc,
    observe: impl FnOnce(&mut Link),
) -> Arc<tokio::sync::Mutex<Link>> {
    if let Some(link) = cached_propagation_link(state, node_hex).await {
        return link;
    }

    // A cache miss must not tear down Transport-owned Links or their Resources.
    let link = transport.link_observed(destination, observe).await;
    let mut guard = state.lock().await;
    *guard = Some(CachedPropagationLink { node_hex: node_hex.to_string(), link: link.clone() });
    link
}

fn now_secs_f64() -> f64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs_f64()
}

#[cfg(test)]
#[path = "bridge_propagation_tests.rs"]
mod tests;
