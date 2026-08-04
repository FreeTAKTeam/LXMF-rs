impl RpcDaemon {
    #[allow(clippy::too_many_arguments)]
    pub fn configure_propagation_node(
        &self,
        enabled: bool,
        peer_announce_at_start: bool,
        peer_announce_interval_secs: Option<u64>,
        node_announce_at_start: bool,
        node_announce_interval_secs: Option<u64>,
        transfer_limit_kb: u32,
        sync_limit_kb: u32,
        stamp_cost: u32,
        stamp_cost_flexibility: u32,
        peering_cost: u32,
        control_allowed: Vec<String>,
        message_storage_limit_mb: Option<u64>,
        peer_entry_limit: u64,
        peer_entry_limit_per_peer: u64,
        peer_entry_ttl_secs: u64,
        completed_peer_entry_ttl_secs: u64,
        max_propagation_peers: u32,
        storage_maintenance_interval_secs: u64,
    ) {
        let mut guard = self.propagation_state.lock().expect("propagation mutex poisoned");
        guard.propagation_node_enabled = enabled;
        guard.peer_announce_at_start = peer_announce_at_start;
        guard.peer_announce_interval_secs = peer_announce_interval_secs;
        guard.node_announce_at_start = node_announce_at_start;
        guard.node_announce_interval_secs = node_announce_interval_secs;
        guard.propagation_limit = transfer_limit_kb;
        guard.sync_limit = sync_limit_kb.max(transfer_limit_kb);
        guard.target_cost = stamp_cost;
        guard.stamp_cost_flexibility = stamp_cost_flexibility;
        guard.peering_cost = Some(peering_cost);
        guard.control_allowed = control_allowed;
        guard.message_storage_limit_mb = message_storage_limit_mb;
        guard.peer_entry_limit = peer_entry_limit.max(1);
        guard.peer_entry_limit_per_peer = peer_entry_limit_per_peer.max(1);
        guard.peer_entry_ttl_secs = peer_entry_ttl_secs.max(1);
        guard.completed_peer_entry_ttl_secs = completed_peer_entry_ttl_secs.max(1);
        guard.max_propagation_peers = max_propagation_peers.max(1);
        guard.storage_maintenance_interval_secs = storage_maintenance_interval_secs.max(1);
        let state = guard.clone();
        drop(guard);
        self.update_daemon_status_snapshot(|snapshot| {
            snapshot.propagation = state;
        });
    }
}
