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
        Self {
            adapter: Some(adapter),
            ..Self::default()
        }
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

    pub fn register_identity(&mut self, destination: [u8; 16], name: Option<String>) -> bool {
        self.registered_identities
            .insert(destination, name)
            .is_none()
    }

    pub fn unregister_identity(&mut self, destination: &[u8; 16]) -> bool {
        self.registered_identities.remove(destination).is_some()
    }

    pub fn is_identity_registered(&self, destination: &[u8; 16]) -> bool {
        self.registered_identities.contains_key(destination)
    }

    pub fn identity_name(&self, destination: &[u8; 16]) -> Option<&str> {
        self.registered_identities
            .get(destination)
            .and_then(|n| n.as_deref())
    }

    pub fn register_delivery_callback(&mut self, callback: DeliveryCallback) {
        self.delivery_callbacks.push(callback);
    }

    pub fn register_outbound_progress_callback(&mut self, callback: OutboundProgressCallback) {
        self.outbound_progress_callbacks.push(callback);
    }

    pub fn set_auth_required(&mut self, enabled: bool) {
        self.config.auth_required = enabled;
    }

    pub fn auth_required(&self) -> bool {
        self.config.auth_required
    }

    pub fn allow_destination(&mut self, destination: [u8; 16]) {
        self.allowed_destinations.insert(destination);
        self.denied_destinations.remove(&destination);
    }

    pub fn deny_destination(&mut self, destination: [u8; 16]) {
        self.denied_destinations.insert(destination);
        self.allowed_destinations.remove(&destination);
    }

    pub fn clear_destination_policy(&mut self, destination: &[u8; 16]) {
        self.allowed_destinations.remove(destination);
        self.denied_destinations.remove(destination);
    }

    pub fn is_destination_allowed(&self, destination: &[u8; 16]) -> bool {
        if self.denied_destinations.contains(destination) {
            return false;
        }

        if !self.config.auth_required {
            return true;
        }

        if self.allowed_destinations.contains(destination) {
            return true;
        }

        self.registered_identities.contains_key(destination)
    }

    pub fn ignore_destination(&mut self, destination: [u8; 16]) {
        self.ignored_destinations.insert(destination);
    }

    pub fn unignore_destination(&mut self, destination: &[u8; 16]) {
        self.ignored_destinations.remove(destination);
    }

    pub fn is_destination_ignored(&self, destination: &[u8; 16]) -> bool {
        self.ignored_destinations.contains(destination)
    }

    pub fn prioritise_destination(&mut self, destination: [u8; 16]) {
        self.prioritised_destinations.insert(destination);
    }

    pub fn deprioritise_destination(&mut self, destination: &[u8; 16]) {
        self.prioritised_destinations.remove(destination);
    }

    pub fn is_destination_prioritised(&self, destination: &[u8; 16]) -> bool {
        self.prioritised_destinations.contains(destination)
    }

    pub fn enqueue_outbound(&mut self, msg: WireMessage) {
        let message_id = msg.message_id().to_vec();
        let destination = msg.destination;
        let is_new = self
            .outbound_messages
            .insert(message_id.clone(), msg)
            .is_none();
        self.outbound_progress
            .entry(message_id.clone())
            .or_insert(0);

        if is_new {
            if self.prioritised_destinations.contains(&destination) {
                self.outbound_queue.push_front(message_id);
            } else {
                self.outbound_queue.push_back(message_id);
            }
            self.stats.outbound_enqueued_total += 1;
        }
    }

    pub fn outbound_len(&self) -> usize {
        self.outbound_messages.len()
    }

    pub fn dequeue_outbound(&mut self) -> Option<WireMessage> {
        while let Some(message_id) = self.outbound_queue.pop_front() {
            if let Some(msg) = self.outbound_messages.remove(&message_id) {
                self.outbound_progress.remove(&message_id);
                return Some(msg);
            }
        }

        None
    }

    pub fn handle_outbound(
        &mut self,
        max_items: usize,
    ) -> Result<Vec<OutboundProcessResult>, LxmfError> {
        let mut results = Vec::new();
        let items_to_process = max_items.min(self.outbound_queue.len());

        for _ in 0..items_to_process {
            let Some(message_id) = self.outbound_queue.pop_front() else {
                break;
            };
            let Some(msg) = self.outbound_messages.remove(&message_id) else {
                self.outbound_progress.remove(&message_id);
                continue;
            };

            let destination = msg.destination;
            let status = if self.is_destination_ignored(&destination) {
                self.stats.outbound_ignored_total += 1;
                OutboundStatus::Ignored
            } else if !self.is_destination_allowed(&destination) {
                self.stats.outbound_rejected_auth_total += 1;
                OutboundStatus::RejectedAuth
            } else if let Some(adapter) = self.adapter.as_ref() {
                if !adapter.has_outbound_sender() {
                    self.outbound_messages.insert(message_id.clone(), msg);
                    self.outbound_queue.push_back(message_id.clone());
                    OutboundStatus::DeferredNoAdapter
                } else {
                    let send_result = adapter.send_outbound(&msg);
                    if let Err(_error) = send_result {
                        self.outbound_messages.insert(message_id.clone(), msg);
                        self.outbound_queue.push_back(message_id.clone());
                        self.stats.outbound_adapter_errors_total += 1;
                        OutboundStatus::DeferredAdapterError
                    } else {
                        for callback in &mut self.delivery_callbacks {
                            callback(&msg);
                        }
                        self.outbound_progress.insert(message_id.clone(), 100);
                        for callback in &mut self.outbound_progress_callbacks {
                            callback(&message_id, 100);
                        }
                        self.stats.outbound_processed_total += 1;
                        OutboundStatus::Sent
                    }
                }
            } else {
                self.outbound_messages.insert(message_id.clone(), msg);
                self.outbound_queue.push_back(message_id.clone());
                OutboundStatus::DeferredNoAdapter
            };

            results.push(OutboundProcessResult {
                message_id,
                destination,
                status,
            });
        }

        Ok(results)
    }

    pub fn cancel_outbound(&mut self, message_id: &[u8]) -> bool {
        let removed_message = self.outbound_messages.remove(message_id);
        let removed_progress = self.outbound_progress.remove(message_id);
        let mut removed_from_queue = false;
        self.outbound_queue.retain(|id| {
            let keep = id.as_slice() != message_id;
            if !keep {
                removed_from_queue = true;
            }
            keep
        });

        let cancelled =
            removed_message.is_some() || removed_progress.is_some() || removed_from_queue;
        if cancelled {
            self.stats.outbound_cancelled_total += 1;
        }
        cancelled
    }

    pub fn set_outbound_progress(&mut self, message_id: &[u8], progress: u8) -> bool {
        let clamped = progress.min(100);
        match self.outbound_progress.get_mut(message_id) {
            Some(current) => {
                *current = clamped;
                for callback in &mut self.outbound_progress_callbacks {
                    callback(message_id, clamped);
                }
                true
            }
            None => false,
        }
    }

    pub fn outbound_progress(&self, message_id: &[u8]) -> Option<u8> {
        self.outbound_progress.get(message_id).copied()
    }

    pub fn cache_stamp(&mut self, material: &[u8], stamp: &[u8]) {
        self.stamp_cache.insert(material.to_vec(), stamp.to_vec());
    }

    pub fn cached_stamp(&self, material: &[u8]) -> Option<&[u8]> {
        self.stamp_cache.get(material).map(|v| v.as_slice())
    }

    pub fn remove_cached_stamp(&mut self, material: &[u8]) -> Option<Vec<u8>> {
        self.stamp_cache.remove(material)
    }

    pub fn cache_ticket(&mut self, destination: [u8; 16], ticket: Ticket) {
        self.ticket_cache.insert(destination, ticket);
    }

    pub fn ticket_for(&self, destination: &[u8; 16]) -> Option<&Ticket> {
        self.ticket_cache.get(destination)
    }

    pub fn remove_ticket(&mut self, destination: &[u8; 16]) -> Option<Ticket> {
        self.ticket_cache.remove(destination)
    }

    pub fn register_peer(&mut self, destination: [u8; 16]) -> bool {
        match self.peers.entry(destination) {
            Entry::Vacant(entry) => {
                entry.insert(Peer::new(destination));
                true
            }
            Entry::Occupied(_) => false,
        }
    }

    pub fn remove_peer(&mut self, destination: &[u8; 16]) -> Option<Peer> {
        self.peers.remove(destination)
    }

    pub fn peer(&self, destination: &[u8; 16]) -> Option<&Peer> {
        self.peers.get(destination)
    }

    pub fn peer_mut(&mut self, destination: &[u8; 16]) -> Option<&mut Peer> {
        self.peers.get_mut(destination)
    }

    pub fn peer_count(&self) -> usize {
        self.peers.len()
    }

    pub fn queue_peer_unhandled(&mut self, destination: [u8; 16], transient_id: &[u8]) {
        let peer = self
            .peers
            .entry(destination)
            .or_insert_with(|| Peer::new(destination));
        peer.queue_unhandled_message(transient_id);
    }

    pub fn queue_peer_handled(&mut self, destination: [u8; 16], transient_id: &[u8]) {
        let peer = self
            .peers
            .entry(destination)
            .or_insert_with(|| Peer::new(destination));
        peer.queue_handled_message(transient_id);
    }

    pub fn process_peer_queues(&mut self, destination: &[u8; 16]) -> bool {
        let Some(peer) = self.peers.get_mut(destination) else {
            return false;
        };
        peer.process_queues();
        true
    }

    pub fn ingest_lxm_uri(&mut self, uri: &str) -> Result<PaperIngestResult, LxmfError> {
        let paper = WireMessage::decode_lxm_uri(uri)?;
        self.ingest_paper_message_bytes(&paper)
    }

    pub fn ingest_paper_message_bytes(
        &mut self,
        paper: &[u8],
    ) -> Result<PaperIngestResult, LxmfError> {
        if paper.len() <= 16 {
            return Err(LxmfError::Decode("paper message too short".into()));
        }

        let mut destination = [0u8; 16];
        destination.copy_from_slice(&paper[..16]);
        let transient_id = reticulum::hash::Hash::new_from_slice(paper)
            .to_bytes()
            .to_vec();
        let duplicate = self.paper_messages.contains_key(&transient_id);

        if duplicate {
            self.stats.paper_uri_duplicate_total += 1;
        } else {
            self.paper_messages
                .insert(transient_id.clone(), paper.to_vec());
            self.stats.paper_uri_ingested_total += 1;
            self.register_peer(destination);
            self.queue_peer_unhandled(destination, &transient_id);
        }

        Ok(PaperIngestResult {
            destination,
            transient_id,
            bytes_len: paper.len(),
            duplicate,
        })
    }

    pub fn paper_message(&self, transient_id: &[u8]) -> Option<&[u8]> {
        self.paper_messages
            .get(transient_id)
            .map(std::vec::Vec::as_slice)
    }

    pub fn paper_message_count(&self) -> usize {
        self.paper_messages.len()
    }

    pub fn build_peer_sync_batch(
        &mut self,
        destination: &[u8; 16],
        requested: usize,
    ) -> Vec<Vec<u8>> {
        let max_items = requested
            .min(self.config.propagation_per_sync_limit as usize)
            .max(1);
        let Some(batch) = self.peers.get_mut(destination).map(|peer| {
            peer.process_queues();
            peer.unhandled_messages()
                .into_iter()
                .take(max_items)
                .collect::<Vec<Vec<u8>>>()
        }) else {
            return Vec::new();
        };

        for transient_id in &batch {
            if self.propagation_transfer_state(transient_id).is_none() {
                self.request_propagation_transfer(transient_id.clone());
            }
        }

        if !batch.is_empty() {
            self.stats.peer_sync_runs_total += 1;
            self.stats.peer_sync_items_total += batch.len();
        }

        batch
    }

    pub fn apply_peer_sync_result(
        &mut self,
        destination: &[u8; 16],
        delivered: &[Vec<u8>],
        rejected: &[Vec<u8>],
    ) -> bool {
        {
            let Some(peer) = self.peers.get_mut(destination) else {
                return false;
            };

            for transient_id in delivered {
                peer.add_handled_message(transient_id);
            }

            for transient_id in rejected {
                peer.add_unhandled_message(transient_id);
            }

            if rejected.is_empty() {
                peer.set_sync_backoff(0);
            } else {
                let next_backoff = peer.sync_backoff().saturating_add(5).min(300);
                peer.set_sync_backoff(next_backoff);
                self.stats.peer_sync_rejected_total += rejected.len();
            }
        }

        for transient_id in delivered {
            self.complete_propagation_transfer(transient_id);
        }

        for transient_id in rejected {
            self.cancel_propagation_transfer(transient_id, "peer rejected");
        }

        true
    }

    pub fn enable_propagation(&mut self, store_root: &Path, target_cost: u32) {
        let store = PropagationStore::new(store_root);
        self.propagation_service = Some(PropagationService::new(store, target_cost));
    }

    pub fn propagation_enabled(&self) -> bool {
        self.propagation_service.is_some()
    }

    pub fn ingest_propagation(&mut self, bytes: &[u8]) -> Result<usize, LxmfError> {
        let Some(service) = &self.propagation_service else {
            return Ok(0);
        };

        let count = service.ingest(bytes)?;
        self.stats.propagation_ingested_total += count;
        self.last_ingest_count = count;
        Ok(count)
    }

    pub fn fetch_propagated(&self, transient_id: &[u8]) -> Result<Vec<u8>, LxmfError> {
        let Some(service) = &self.propagation_service else {
            return Err(LxmfError::Io("propagation disabled".into()));
        };

        service.fetch(transient_id)
    }

    pub fn propagation_ingested_total(&self) -> usize {
        self.stats.propagation_ingested_total
    }

    pub fn last_ingest_count(&self) -> usize {
        self.last_ingest_count
    }

    pub fn request_propagation_transfer(
        &mut self,
        transient_id: impl Into<Vec<u8>>,
    ) -> PropagationTransferState {
        let now = unix_now();
        let state = PropagationTransferState::requested(transient_id.into(), now);
        self.propagation_transfers
            .insert(state.transient_id.clone(), state.clone());
        self.stats.propagation_requests_total += 1;
        state
    }

    pub fn update_propagation_transfer_progress(
        &mut self,
        transient_id: &[u8],
        progress: u8,
    ) -> bool {
        let Some(state) = self.propagation_transfers.get_mut(transient_id) else {
            return false;
        };
        state.phase = TransferPhase::InProgress;
        state.progress = progress.min(100);
        state.updated_at = unix_now();
        true
    }

    pub fn complete_propagation_transfer(&mut self, transient_id: &[u8]) -> bool {
        let Some(state) = self.propagation_transfers.get_mut(transient_id) else {
            return false;
        };
        state.phase = TransferPhase::Completed;
        state.progress = 100;
        state.reason = None;
        state.updated_at = unix_now();
        self.stats.propagation_completed_total += 1;
        true
    }

    pub fn cancel_propagation_transfer(
        &mut self,
        transient_id: &[u8],
        reason: impl Into<String>,
    ) -> bool {
        let Some(state) = self.propagation_transfers.get_mut(transient_id) else {
            return false;
        };
        state.phase = TransferPhase::Cancelled;
        state.reason = Some(reason.into());
        state.updated_at = unix_now();
        self.stats.propagation_cancelled_total += 1;
        true
    }

    pub fn propagation_transfer_state(
        &self,
        transient_id: &[u8],
    ) -> Option<&PropagationTransferState> {
        self.propagation_transfers.get(transient_id)
    }

    pub fn set_name(&mut self, name: impl Into<String>) {
        self.name = Some(name.into());
    }

    pub fn set_propagation_node(&mut self, enabled: bool) {
        self.propagation_node = enabled;
    }

    pub fn set_from_static_only(&mut self, enabled: bool) {
        self.from_static_only = enabled;
    }

    pub fn set_propagation_limits(&mut self, per_transfer: u32, per_sync: u32) {
        self.config.propagation_per_transfer_limit = per_transfer;
        self.config.propagation_per_sync_limit = per_sync.max(per_transfer);
    }

    pub fn set_propagation_stamp_cost(&mut self, cost: u32, flexibility: u32) {
        self.config.propagation_stamp_cost = cost.max(PROPAGATION_COST_MIN);
        self.config.propagation_stamp_cost_flexibility = flexibility;
    }

    pub fn set_peering_cost(&mut self, cost: u32) {
        self.config.peering_cost = cost;
    }

    fn propagation_node_announce_metadata(&self) -> BTreeMap<u8, ByteBuf> {
        let mut metadata = BTreeMap::new();
        if let Some(name) = &self.name {
            metadata.insert(PN_META_NAME, ByteBuf::from(name.as_bytes().to_vec()));
        }
        metadata
    }

    pub fn get_propagation_node_app_data(&self) -> Vec<u8> {
        self.get_propagation_node_app_data_at(unix_now())
    }

    pub fn get_propagation_node_app_data_at(&self, timestamp: u64) -> Vec<u8> {
        let metadata = self.propagation_node_announce_metadata();
        let node_state = self.propagation_node && !self.from_static_only;
        let stamp_cost = [
            self.config.propagation_stamp_cost,
            self.config.propagation_stamp_cost_flexibility,
            self.config.peering_cost,
        ];
        let announce_data = (
            false,
            timestamp,
            node_state,
            self.config.propagation_per_transfer_limit,
            self.config.propagation_per_sync_limit,
            stamp_cost,
            metadata,
        );

        rmp_serde::to_vec(&announce_data).expect("propagation node app data msgpack")
    }

    pub fn jobs(&mut self) {
        self.jobs_at(unix_now());
    }

    pub fn jobs_at(&mut self, now: u64) {
        self.expire_tickets(now as f64);
        self.prune_transfer_state(now);
    }

    fn expire_tickets(&mut self, now: f64) {
        self.ticket_cache
            .retain(|_, ticket| ticket.is_valid_with_grace(now));
    }

    fn prune_transfer_state(&mut self, now: u64) {
        let ttl = self.config.transfer_state_ttl_secs;
        self.propagation_transfers
            .retain(|_, state| now.saturating_sub(state.updated_at) <= ttl);
    }
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
