use super::delivery_attempt::DeliveryAttemptOutcome;
use super::delivery_task::DeliveryTask;
use super::delivery_timing::measured_delivery_timing;
use lxmf::message::DeliveryTiming;
use rns_transport::hash::AddressHash;
use std::collections::HashMap;
use std::sync::Mutex;
use tokio::time::Instant;

/// LXMF discovery history shared by admitted deliveries, independent of RNS routing.
/// Entries expire on the next admitted request; this is time-window retention,
/// not an entry-count limit. Pending reads never discard debounce history.
#[derive(Default)]
pub(super) struct DeliveryPathRequests {
    times: Mutex<HashMap<AddressHash, Instant>>,
}

impl DeliveryPathRequests {
    /// Forced rediscovery uses pending lifetime, not ordinary request debounce.
    /// Claim atomically before expiring the route; do not prune other history.
    pub(super) fn claim_rediscovery(
        &self,
        destination: AddressHash,
        timing: DeliveryTiming,
    ) -> std::io::Result<bool> {
        let mut times = self.times.lock().map_err(|e| std::io::Error::other(e.to_string()))?;
        let now = Instant::now();
        if times
            .get(&destination)
            .is_some_and(|sent| timing.path_request_pending(now.duration_since(*sent)))
        {
            return Ok(false);
        }
        times.insert(destination, now);
        Ok(true)
    }

    fn claim(&self, destination: AddressHash, timing: DeliveryTiming) -> std::io::Result<bool> {
        let mut times =
            self.times.lock().map_err(|error| std::io::Error::other(error.to_string()))?;
        let now = Instant::now();
        if times
            .get(&destination)
            .is_some_and(|sent| timing.request_debounced(now.duration_since(*sent)))
        {
            return Ok(false);
        }
        times.retain(|_, sent| timing.request_debounced(now.duration_since(*sent)));
        times.insert(destination, now);
        Ok(true)
    }

    pub(super) fn pending(
        &self,
        destination: AddressHash,
        timing: DeliveryTiming,
    ) -> std::io::Result<bool> {
        let times = self.times.lock().map_err(|error| std::io::Error::other(error.to_string()))?;
        let now = Instant::now();
        Ok(times
            .get(&destination)
            .is_some_and(|sent| timing.path_request_pending(now.duration_since(*sent))))
    }
}

impl DeliveryTask {
    pub(super) async fn request_delivery_path(
        &self,
        destination: AddressHash,
    ) -> Result<DeliveryTiming, DeliveryAttemptOutcome> {
        let result = async {
            let timing = measured_delivery_timing(self.transport.path_timing(&destination).await)
                .map_err(std::io::Error::other)?;
            if self.path_requests.claim(destination, timing)? {
                // Keep the timestamp even on cancellation or failed dispatch, as
                // the reference router records before handing the request to RNS.
                self.transport.request_path(&destination, None, None).await;
            } else {
                let pending = self.path_requests.pending(destination, timing)?;
                log::debug!("delivery path request suppressed message_id={} destination={destination} pending={pending}", self.message_id);
            }
            // Scheduling follows handoff in the reference router; measurements
            // may have changed while the transport waited for its locks.
            measured_delivery_timing(self.transport.path_timing(&destination).await)
                .map_err(std::io::Error::other)
        }.await;
        result.map_err(|error| {
            log::error!(
                "delivery path request failed message_id={} destination={destination}: {error}",
                self.message_id
            );
            self.fail_payload_build(error);
            DeliveryAttemptOutcome::Terminal
        })
    }
}

#[cfg(test)]
#[path = "bridge_delivery_paths_tests.rs"]
mod tests;
