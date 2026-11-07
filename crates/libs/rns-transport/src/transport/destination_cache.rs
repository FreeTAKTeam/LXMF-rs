use super::*;
use std::collections::HashSet;

impl TransportHandler {
    pub(super) async fn prune_destination_cache(&mut self) {
        // Prepared SINGLE proofs and live links can outlast their learned route.
        let mut referenced: HashSet<_> =
            self.packet_cache.lock().await.tracked_destinations().collect();
        referenced.extend(self.link_table.destinations());
        referenced.extend(self.out_links.keys().copied());
        for link in self.in_links.values() {
            let link = link.lock().await;
            if let Some(peer) = link.identified_peer_identity() {
                referenced.insert(
                    SingleOutputDestination::new(*peer, link.destination().name).desc.address_hash,
                );
            }
        }
        let before = self.single_out_destinations.len();
        self.single_out_destinations.retain(|address, _| {
            self.path_table.get(address).is_some()
                || self.single_in_destinations.contains_key(address)
                || referenced.contains(address)
        });
        self.single_out_destinations_removed_total +=
            (before - self.single_out_destinations.len()) as u64;
    }
}

impl Transport {
    /// Nonblocking cache-lifetime query for daemon identity-cache cleanup.
    /// A busy handler returns `None`; callers must retain the entry and retry.
    pub fn destination_is_cached_now(&self, destination: &AddressHash) -> Option<bool> {
        let handler = self.handler.try_lock().ok()?;
        Some(
            handler.single_out_destinations.contains_key(destination)
                || handler.single_in_destinations.contains_key(destination),
        )
    }
}
