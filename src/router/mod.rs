use crate::constants::{
    PEERING_COST, PN_META_NAME, PROPAGATION_COST, PROPAGATION_COST_FLEX, PROPAGATION_COST_MIN,
    PROPAGATION_LIMIT, SYNC_LIMIT,
};
use crate::message::WireMessage;
use crate::propagation::PropagationService;
use crate::reticulum::Adapter;
use crate::storage::PropagationStore;
use std::path::Path;
use std::collections::BTreeMap;
use std::time::{SystemTime, UNIX_EPOCH};
use serde_bytes::ByteBuf;

pub struct Router {
    outbound: Vec<WireMessage>,
    propagation_service: Option<PropagationService>,
    propagation_ingested: usize,
    last_ingest_count: usize,
    name: Option<String>,
    propagation_node: bool,
    from_static_only: bool,
    propagation_per_transfer_limit: u32,
    propagation_per_sync_limit: u32,
    propagation_stamp_cost: u32,
    propagation_stamp_cost_flexibility: u32,
    peering_cost: u32,
}

impl Default for Router {
    fn default() -> Self {
        let propagation_stamp_cost = PROPAGATION_COST.max(PROPAGATION_COST_MIN);
        let mut propagation_per_sync_limit = SYNC_LIMIT;
        if propagation_per_sync_limit < PROPAGATION_LIMIT {
            propagation_per_sync_limit = PROPAGATION_LIMIT;
        }

        Self {
            outbound: Vec::new(),
            propagation_service: None,
            propagation_ingested: 0,
            last_ingest_count: 0,
            name: None,
            propagation_node: false,
            from_static_only: false,
            propagation_per_transfer_limit: PROPAGATION_LIMIT,
            propagation_per_sync_limit,
            propagation_stamp_cost,
            propagation_stamp_cost_flexibility: PROPAGATION_COST_FLEX,
            peering_cost: PEERING_COST,
        }
    }
}

impl Router {
    pub fn with_adapter(_adapter: Adapter) -> Self {
        Self::default()
    }

    pub fn enqueue_outbound(&mut self, msg: WireMessage) {
        self.outbound.push(msg);
    }

    pub fn outbound_len(&self) -> usize {
        self.outbound.len()
    }

    pub fn dequeue_outbound(&mut self) -> Option<WireMessage> {
        if self.outbound.is_empty() {
            None
        } else {
            Some(self.outbound.remove(0))
        }
    }

    pub fn enable_propagation(&mut self, store_root: &Path, target_cost: u32) {
        let store = PropagationStore::new(store_root);
        self.propagation_service = Some(PropagationService::new(store, target_cost));
    }

    pub fn propagation_enabled(&self) -> bool {
        self.propagation_service.is_some()
    }

    pub fn ingest_propagation(&mut self, bytes: &[u8]) -> Result<usize, crate::error::LxmfError> {
        let Some(service) = &self.propagation_service else {
            return Ok(0);
        };

        let count = service.ingest(bytes)?;
        self.propagation_ingested += count;
        self.last_ingest_count = count;
        Ok(count)
    }

    pub fn fetch_propagated(&self, transient_id: &[u8]) -> Result<Vec<u8>, crate::error::LxmfError> {
        let Some(service) = &self.propagation_service else {
            return Err(crate::error::LxmfError::Io("propagation disabled".into()));
        };

        service.fetch(transient_id)
    }

    pub fn propagation_ingested_total(&self) -> usize {
        self.propagation_ingested
    }

    pub fn last_ingest_count(&self) -> usize {
        self.last_ingest_count
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
        self.propagation_per_transfer_limit = per_transfer;
        self.propagation_per_sync_limit = per_sync.max(per_transfer);
    }

    pub fn set_propagation_stamp_cost(&mut self, cost: u32, flexibility: u32) {
        self.propagation_stamp_cost = cost.max(PROPAGATION_COST_MIN);
        self.propagation_stamp_cost_flexibility = flexibility;
    }

    pub fn set_peering_cost(&mut self, cost: u32) {
        self.peering_cost = cost;
    }

    fn propagation_node_announce_metadata(&self) -> BTreeMap<u8, ByteBuf> {
        let mut metadata = BTreeMap::new();
        if let Some(name) = &self.name {
            metadata.insert(PN_META_NAME, ByteBuf::from(name.as_bytes().to_vec()));
        }
        metadata
    }

    pub fn get_propagation_node_app_data(&self) -> Vec<u8> {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        self.get_propagation_node_app_data_at(timestamp)
    }

    pub fn get_propagation_node_app_data_at(&self, timestamp: u64) -> Vec<u8> {
        let metadata = self.propagation_node_announce_metadata();
        let node_state = self.propagation_node && !self.from_static_only;
        let stamp_cost = [
            self.propagation_stamp_cost,
            self.propagation_stamp_cost_flexibility,
            self.peering_cost,
        ];
        let announce_data = (
            false,
            timestamp,
            node_state,
            self.propagation_per_transfer_limit,
            self.propagation_per_sync_limit,
            stamp_cost,
            metadata,
        );

        rmp_serde::to_vec(&announce_data).expect("propagation node app data msgpack")
    }
}
