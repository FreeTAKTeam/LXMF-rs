use super::*;

impl Router {
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

    pub fn get_propagation_node_app_data(&self) -> Result<Vec<u8>, LxmfError> {
        self.get_propagation_node_app_data_at(unix_now())
    }

    pub fn get_propagation_node_app_data_at(&self, timestamp: u64) -> Result<Vec<u8>, LxmfError> {
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

        rmp_serde::to_vec(&announce_data)
            .map_err(|err| LxmfError::Encode(format!("propagation node app data msgpack: {err}")))
    }

    pub fn jobs(&mut self) {
        self.jobs_at(unix_now());
    }

    pub fn jobs_at(&mut self, now: u64) {
        self.expire_tickets(now as f64);
        self.prune_transfer_state(now);
    }

    fn expire_tickets(&mut self, now: f64) {
        self.ticket_cache.retain(|_, ticket| ticket.is_valid_with_grace(now));
    }

    fn prune_transfer_state(&mut self, now: u64) {
        let ttl = self.config.transfer_state_ttl_secs;
        self.propagation_transfers.retain(|_, state| now.saturating_sub(state.updated_at) <= ttl);
    }
}
