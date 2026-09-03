// Kept separate from `close_channel.rs` so that section stays below the
// repository's 500-line limit. `include!`d into the same module.

impl Transport {
    /// How long an outbound link to `destination` may stay unestablished,
    /// sized as `RNS.Link.__init__` sizes it: the first hop's own timeout,
    /// then [`crate::destination::link::ESTABLISHMENT_TIMEOUT_PER_HOP`] for
    /// every hop to the destination.
    async fn establishment_timeout_for(
        &self,
        destination: &AddressHash,
        next_hop_mtu: Option<usize>,
        hops: u8,
    ) -> Duration {
        let per_hop = crate::destination::link::ESTABLISHMENT_TIMEOUT_PER_HOP;
        let first_hop_timeout = match self.next_hop_metrics(destination).await {
            Some(metrics) => {
                metrics.first_hop_timeout(next_hop_mtu.unwrap_or(crate::packet::PACKET_MDU), per_hop)
            }
            None => per_hop,
        };
        first_hop_timeout + per_hop * u32::from(hops.max(1))
    }
}
