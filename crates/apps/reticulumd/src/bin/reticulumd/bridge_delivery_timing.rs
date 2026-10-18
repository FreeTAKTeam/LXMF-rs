use lxmf::message::{DeliveryTiming, InvalidDeliveryTiming};
use rns_transport::transport::PathTimingSnapshot;

pub(super) fn measured_delivery_timing(
    measurements: PathTimingSnapshot,
) -> Result<DeliveryTiming, InvalidDeliveryTiming> {
    const DEFAULT_PER_HOP_SECONDS: f64 = 6.0;
    const RETICULUM_MTU: f64 = 500.0;
    let medium = measurements.medium_timeout_seconds();
    let round_trip = if !measurements.path_known {
        medium
    } else if measurements.next_hop.is_some_and(|next| next.bitrate == 0) {
        // Python's first-hop division raises for zero bitrate; the router
        // catches that measurement error and uses zero destination RTT.
        0.0
    } else {
        let first_hop = measurements
            .next_hop
            .and_then(|next| next.per_byte_latency())
            .map_or(DEFAULT_PER_HOP_SECONDS, |latency| {
                RETICULUM_MTU * latency + DEFAULT_PER_HOP_SECONDS
            });
        // Preserve Python's operation order and f64 precision until the policy
        // has rounded attempt limits. The interface hardware MTU is irrelevant.
        2.0 * (first_hop - DEFAULT_PER_HOP_SECONDS).max(0.0) + DEFAULT_PER_HOP_SECONDS
    };
    DeliveryTiming::from_round_trip(medium, measurements.lowest_interface_bitrate, round_trip)
}

#[cfg(test)]
#[path = "bridge_delivery_timing_tests.rs"]
mod tests;

impl super::delivery_task::DeliveryTask {
    pub(super) fn delivery_timing(
        &self,
        measurements: PathTimingSnapshot,
    ) -> Result<DeliveryTiming, InvalidDeliveryTiming> {
        #[cfg(test)]
        if let Some(control) = self.wakeup.trace() {
            return Ok(control.timing(measurements.path_known));
        }
        measured_delivery_timing(measurements)
    }
}
