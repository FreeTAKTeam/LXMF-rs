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
        let state = guard.clone();
        drop(guard);
        self.update_daemon_status_snapshot(|snapshot| {
            snapshot.propagation = state;
        });
    }
}
