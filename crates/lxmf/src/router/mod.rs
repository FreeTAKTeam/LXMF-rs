use crate::constants::{
    PEERING_COST, PN_META_NAME, PROPAGATION_COST, PROPAGATION_COST_FLEX, PROPAGATION_COST_MIN,
    PROPAGATION_LIMIT, SYNC_LIMIT,
};
use crate::error::LxmfError;
use crate::message::WireMessage;
use crate::peer::Peer;
use crate::propagation::PropagationService;
use crate::reticulum::Adapter;
use crate::storage::PropagationStore;
use crate::ticket::Ticket;
use serde::{Deserialize, Serialize};
use serde_bytes::ByteBuf;
use std::collections::{btree_map::Entry, BTreeMap, BTreeSet, VecDeque};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

// Router internals are split by responsibility to keep the public surface stable and auditable.
// announce: propagation-node app-data and background housekeeping hooks.
mod announce;
// auth: identity registration and destination policy gates.
mod auth;
// outbound: queueing, send attempts, and stamp/ticket caches.
mod outbound;
// peer: peer state, paper ingest, and peer sync workflows.
mod peer;
// propagation: propagation service integration and transfer state machine.
mod propagation;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RouterConfig {
    pub propagation_per_transfer_limit: u32,
    pub propagation_per_sync_limit: u32,
    pub propagation_stamp_cost: u32,
    pub propagation_stamp_cost_flexibility: u32,
    pub peering_cost: u32,
    pub auth_required: bool,
    pub transfer_state_ttl_secs: u64,
}

impl Default for RouterConfig {
    fn default() -> Self {
        let propagation_stamp_cost = PROPAGATION_COST.max(PROPAGATION_COST_MIN);
        let propagation_per_transfer_limit = PROPAGATION_LIMIT;
        let propagation_per_sync_limit = SYNC_LIMIT.max(propagation_per_transfer_limit);

        Self {
            propagation_per_transfer_limit,
            propagation_per_sync_limit,
            propagation_stamp_cost,
            propagation_stamp_cost_flexibility: PROPAGATION_COST_FLEX,
            peering_cost: PEERING_COST,
            auth_required: false,
            transfer_state_ttl_secs: 600,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct RouterStats {
    pub outbound_enqueued_total: usize,
    pub outbound_processed_total: usize,
    pub outbound_cancelled_total: usize,
    pub outbound_adapter_errors_total: usize,
    pub outbound_rejected_auth_total: usize,
    pub outbound_ignored_total: usize,
    pub propagation_ingested_total: usize,
    pub propagation_requests_total: usize,
    pub propagation_completed_total: usize,
    pub propagation_cancelled_total: usize,
    pub peer_sync_runs_total: usize,
    pub peer_sync_items_total: usize,
    pub peer_sync_rejected_total: usize,
    pub paper_uri_ingested_total: usize,
    pub paper_uri_duplicate_total: usize,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum TransferPhase {
    Requested,
    InProgress,
    Completed,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PropagationTransferState {
    pub transient_id: Vec<u8>,
    pub phase: TransferPhase,
    pub progress: u8,
    pub reason: Option<String>,
    pub updated_at: u64,
}

impl PropagationTransferState {
    fn requested(transient_id: Vec<u8>, now: u64) -> Self {
        Self {
            transient_id,
            phase: TransferPhase::Requested,
            progress: 0,
            reason: None,
            updated_at: now,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutboundStatus {
    Sent,
    DeferredNoAdapter,
    DeferredAdapterError,
    RejectedAuth,
    Ignored,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutboundProcessResult {
    pub message_id: Vec<u8>,
    pub destination: [u8; 16],
    pub status: OutboundStatus,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PaperIngestResult {
    pub destination: [u8; 16],
    pub transient_id: Vec<u8>,
    pub bytes_len: usize,
    pub duplicate: bool,
}

type DeliveryCallback = Box<dyn FnMut(&WireMessage) + Send + Sync + 'static>;
type OutboundProgressCallback = Box<dyn FnMut(&[u8], u8) + Send + Sync + 'static>;

#[derive(Default)]
pub struct Router {
    config: RouterConfig,
    stats: RouterStats,
    outbound_queue: VecDeque<Vec<u8>>,
    outbound_messages: BTreeMap<Vec<u8>, WireMessage>,
    outbound_progress: BTreeMap<Vec<u8>, u8>,
    propagation_service: Option<PropagationService>,
    last_ingest_count: usize,
    name: Option<String>,
    propagation_node: bool,
    from_static_only: bool,
    adapter: Option<Adapter>,
    registered_identities: BTreeMap<[u8; 16], Option<String>>,
    allowed_destinations: BTreeSet<[u8; 16]>,
    denied_destinations: BTreeSet<[u8; 16]>,
    ignored_destinations: BTreeSet<[u8; 16]>,
    prioritised_destinations: BTreeSet<[u8; 16]>,
    stamp_cache: BTreeMap<Vec<u8>, Vec<u8>>,
    ticket_cache: BTreeMap<[u8; 16], Ticket>,
    propagation_transfers: BTreeMap<Vec<u8>, PropagationTransferState>,
    peers: BTreeMap<[u8; 16], Peer>,
    paper_messages: BTreeMap<Vec<u8>, Vec<u8>>,
    delivery_callbacks: Vec<DeliveryCallback>,
    outbound_progress_callbacks: Vec<OutboundProgressCallback>,
}

impl Router {
    pub fn with_adapter(adapter: Adapter) -> Self {
        Self { adapter: Some(adapter), ..Self::default() }
    }

    pub fn has_adapter(&self) -> bool {
        self.adapter.is_some()
    }

    pub fn config(&self) -> &RouterConfig {
        &self.config
    }

    pub fn set_config(&mut self, config: RouterConfig) {
        self.config = config;
    }

    pub fn stats(&self) -> &RouterStats {
        &self.stats
    }
}

fn unix_now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs()
}
