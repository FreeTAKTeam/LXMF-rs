//! LXMF retry timing, independent of transport I/O and the owner's clock.
use std::time::Duration;

const PATH_REQUEST_WAIT: Duration = Duration::from_secs(7);
const DELIVERY_RETRY_WAIT: Duration = Duration::from_secs(10);
const PATH_REQUEST_DEBOUNCE: Duration = Duration::from_secs(60);
const MAX_DELIVERY_ATTEMPTS: u32 = 5;

/// A snapshot of medium and destination measurements for one scheduling decision.
#[derive(Clone, Copy, Debug)]
pub struct DeliveryTiming {
    medium_timeout: Duration,
    destination_round_trip: Duration,
    lowest_bitrate: Option<u64>,
    path_attempt_limit: u32,
    delivery_attempt_limit: u32,
}

impl DeliveryTiming {
    /// Measurements are in seconds. Preserve their floating-point precision for
    /// Python-compatible retry rounding; only timer waits become `Duration`s.
    /// `first_hop_timeout` is absent when no path is known. For a known path it
    /// includes one MTU serialization time plus `default_per_hop_timeout`.
    pub fn new(
        medium_timeout: f64,
        lowest_bitrate: Option<u64>,
        first_hop_timeout: Option<f64>,
        default_per_hop_timeout: f64,
    ) -> Result<Self, InvalidDeliveryTiming> {
        let duration =
            |seconds| Duration::try_from_secs_f64(seconds).map_err(|_| InvalidDeliveryTiming);
        duration(default_per_hop_timeout)?;
        if let Some(first_hop) = first_hop_timeout {
            duration(first_hop)?;
        }
        let destination_round_trip = first_hop_timeout.map_or(medium_timeout, |first_hop| {
            2.0 * (first_hop - default_per_hop_timeout).max(0.0) + default_per_hop_timeout
        });
        Self::from_round_trip(medium_timeout, lowest_bitrate, destination_round_trip)
    }

    /// Use an already measured destination round trip, in unquantized seconds.
    /// This also represents the reference router's zero-RTT fallback when a
    /// known path's first-hop measurement fails.
    pub fn from_round_trip(
        medium_timeout: f64,
        lowest_bitrate: Option<u64>,
        destination_round_trip: f64,
    ) -> Result<Self, InvalidDeliveryTiming> {
        let duration =
            |seconds| Duration::try_from_secs_f64(seconds).map_err(|_| InvalidDeliveryTiming);
        Ok(Self {
            medium_timeout: duration(medium_timeout)?,
            destination_round_trip: duration(destination_round_trip)?,
            lowest_bitrate,
            path_attempt_limit: attempt_limit(medium_timeout),
            delivery_attempt_limit: attempt_limit(destination_round_trip),
        })
    }

    pub fn path_request_wait(self) -> Duration {
        PATH_REQUEST_WAIT.max(self.medium_timeout)
    }

    /// Request suppression and pending-path lifetime are deliberately different
    /// on slow interfaces. Checking pending work must not discard debounce history.
    pub fn request_debounce(self) -> Duration {
        if self.lowest_bitrate.is_some_and(|bitrate| bitrate < 2000) {
            PATH_REQUEST_DEBOUNCE
        } else {
            self.path_request_wait()
        }
    }

    pub fn destination_round_trip(self) -> Duration {
        self.destination_round_trip
    }
}

/// A negative, non-finite or out-of-range transport timing measurement.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct InvalidDeliveryTiming;

impl std::fmt::Display for InvalidDeliveryTiming {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("delivery timing must be finite, non-negative and fit a duration")
    }
}

impl std::error::Error for InvalidDeliveryTiming {}

/// Scheduling state for one admitted message. The delivery owner retains the
/// attempt count, prepared bytes, receipt/resource state and terminal decision.
/// All supplied times must use the same monotonic origin.
#[derive(Clone, Copy, Debug)]
pub struct DeliverySchedule {
    next_attempt: Option<Duration>,
    awaiting_path: bool,
    max_attempts: u32,
}

impl Default for DeliverySchedule {
    fn default() -> Self {
        Self { next_attempt: None, awaiting_path: false, max_attempts: MAX_DELIVERY_ATTEMPTS }
    }
}

impl DeliverySchedule {
    pub fn due(self, now: Duration, path_known: bool) -> bool {
        self.next_attempt.is_none_or(|deadline| now > deadline)
            || (self.awaiting_path && path_known)
    }

    pub fn begin_attempt(&mut self) {
        self.awaiting_path = false;
    }

    pub fn schedule_path(&mut self, now: Duration, timing: DeliveryTiming) {
        self.next_attempt = Some(now.saturating_add(timing.path_request_wait()));
        self.awaiting_path = true;
        self.max_attempts = timing.path_attempt_limit;
    }

    /// Like the reference router, scheduling does not begin an attempt or clear
    /// path waiting. The owner calls `begin_attempt` after claiming a due attempt.
    pub fn schedule_delivery(&mut self, now: Duration, timing: DeliveryTiming) {
        self.next_attempt =
            Some(now.saturating_add(DELIVERY_RETRY_WAIT.max(timing.destination_round_trip)));
        self.max_attempts = timing.delivery_attempt_limit;
    }

    pub fn deadline(self) -> Option<Duration> {
        self.next_attempt
    }

    pub fn awaiting_path(self) -> bool {
        self.awaiting_path
    }

    pub fn max_attempts(self) -> u32 {
        self.max_attempts
    }
}

fn attempt_limit(round_trip_seconds: f64) -> u32 {
    if round_trip_seconds > DELIVERY_RETRY_WAIT.as_secs_f64() {
        // Python round() uses ties-to-even, unlike Rust's round().
        (50.0 / round_trip_seconds).round_ties_even().max(3.0) as u32
    } else {
        MAX_DELIVERY_ATTEMPTS
    }
}

#[cfg(test)]
mod tests {
    use super::attempt_limit;

    #[test]
    fn attempt_limit_rounds_python_ties_to_even() {
        assert_eq!(attempt_limit(50.0 / 4.5), 4);
        assert_eq!(attempt_limit(50.0 / 3.5), 4);
        assert_eq!(attempt_limit(50.0 / 2.5), 3);
        assert_eq!(attempt_limit(10.0), 5);
    }
}
