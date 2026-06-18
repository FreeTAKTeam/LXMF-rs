impl RpcDaemon {
    pub fn set_propagation_destination_hash(&self, hash: Option<String>) {
        let mut guard = self
            .propagation_destination_hash
            .lock()
            .expect("propagation_destination_hash mutex poisoned");
        *guard = hash;
    }

    pub fn local_propagation_hash(&self) -> Option<String> {
        self.propagation_destination_hash
            .lock()
            .expect("propagation_destination_hash mutex poisoned")
            .clone()
    }

    pub fn outbound_propagation_cost_lookup(
        &self,
        peer: Option<&str>,
    ) -> (Option<String>, Option<u32>, &'static str) {
        let selected = peer
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned)
            .or_else(|| self.outbound_propagation_node());
        let Some(peer) = selected else {
            return (None, None, "unavailable");
        };
        let cost = self
            .peers
            .lock()
            .ok()
            .and_then(|guard| {
                guard
                    .values()
                    .find(|record| record.peer.eq_ignore_ascii_case(peer.as_str()))
                    .and_then(|record| record.propagation_stamp_cost)
            })
            .or_else(|| self.store.latest_announce_stamp_cost_for(peer.as_str()).ok().flatten());
        let source = if cost.is_some() { "cached_announce" } else { "unavailable" };
        (Some(peer), cost, source)
    }
}
