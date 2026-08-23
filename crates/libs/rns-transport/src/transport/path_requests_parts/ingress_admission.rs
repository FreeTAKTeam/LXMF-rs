impl PathRequests {
    pub fn register_discovery_before_queue(
        &mut self,
        destination: &AddressHash,
        on_iface: AddressHash,
    ) -> bool {
        let now = Instant::now();
        self.prune_discovery(now);
        if let Some(inflight) = self.discovery.get_mut(destination) {
            if !inflight.requesting_ifaces.contains(&on_iface) {
                inflight.requesting_ifaces.push(on_iface);
            }
            return false;
        }
        let expiry = now + self.request_timeout;
        self.discovery.insert(
            *destination,
            InflightPathRequest {
                expires_at: expiry,
                outbound_iface: Some(on_iface),
                requesting_ifaces: vec![on_iface],
                engaged: false,
            },
        );
        self.increment_pending_recursive_count(Some(on_iface));
        self.queue.push_back((*destination, expiry));
        true
    }

    pub fn rollback_admission(&mut self, request: &PathRequest, on_iface: AddressHash) {
        let key = (request.destination, request.tag_bytes.clone());
        self.cache.remove(&key);
        let should_remove = self
            .discovery
            .get_mut(&request.destination)
            .is_some_and(|inflight| {
                if inflight.engaged {
                    return false;
                }
                inflight.requesting_ifaces.retain(|iface| *iface != on_iface);
                inflight.requesting_ifaces.is_empty()
            });
        if should_remove {
            if let Some(inflight) = self.discovery.remove(&request.destination) {
                self.decrement_pending_recursive_count(inflight.outbound_iface);
            }
        }
    }
}
