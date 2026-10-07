use super::dispatch_legacy_messages::{peer_sync_backoff_active, LocalUnpeerCleanup};

use super::*;

pub(super) const LXMF_PEER_SYNC_BACKOFF_STEP_SECS: u32 = 12 * 60;

pub(super) const LXMF_PEER_MAX_UNREACHABLE_SECS: i64 = 14 * 24 * 60 * 60;

const LXMF_PEER_FASTEST_RANDOM_POOL: usize = 2;

const LXMF_PEER_ROTATION_HEADROOM_PCT: usize = 10;

const LXMF_PEER_ROTATION_ACCEPTANCE_RATE_MAX: f64 = 0.5;

// Maintenance ranks peers by metadata; queue inventories remain owned by the
// live records and must not be copied for each selection/rotation pass.
struct PeerMaintenanceCandidate {
    peer: String,
    last_seen: i64,
    last_sync_attempt: i64,
    next_sync_attempt: i64,
    alive: bool,
    offered: u64,
    sync_transfer_rate: f64,
    rotation_acceptance_rate: f64,
}

impl From<&PeerRecord> for PeerMaintenanceCandidate {
    fn from(peer: &PeerRecord) -> Self {
        Self {
            peer: peer.peer.clone(),
            last_seen: peer.last_seen,
            last_sync_attempt: peer.last_sync_attempt,
            next_sync_attempt: peer.next_sync_attempt,
            alive: peer.alive,
            offered: peer.offered,
            sync_transfer_rate: peer.sync_transfer_rate,
            rotation_acceptance_rate: peer_rotation_acceptance_rate(peer),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub(super) struct PeerPropagationState {
    pub(super) transfer_limit: Option<u32>,
    pub(super) sync_limit: Option<u32>,
    pub(super) stamp_cost: Option<u32>,
    pub(super) stamp_cost_flexibility: Option<u32>,
    pub(super) peering_cost: Option<u32>,
    pub(super) network_distance: Option<u32>,
    pub(super) peering_timebase: Option<i64>,
}

pub(super) struct PeerUpsertRequest {
    pub(super) peer: String,
    pub(super) timestamp: i64,
    pub(super) capabilities: Vec<String>,
    pub(super) name: Option<String>,
    pub(super) name_source: Option<String>,
    pub(super) metadata: Option<JsonValue>,
    pub(super) peer_type: Option<String>,
}
