// Kept separate from `new.rs` so that module stays below the repository's
// 500-line limit. `include!`d into the same module, so this is a file
// boundary and not a privacy one.

/// `Link.ESTABLISHMENT_TIMEOUT_PER_HOP` (`Reticulum.DEFAULT_PER_HOP_TIMEOUT`):
/// how long each hop is given for a link request to come back proven.
pub const ESTABLISHMENT_TIMEOUT_PER_HOP: Duration = Duration::from_secs(6);

/// What a link is given before the transport has sized it from the path:
/// the first hop plus one more, as for an unknown or single-hop path.
const DEFAULT_ESTABLISHMENT_TIMEOUT: Duration = Duration::from_secs(12);

impl Link {
    /// Bounds how long this link may stay unestablished. `RNS.Link.__init__`
    /// sizes it as the first hop's timeout plus
    /// [`ESTABLISHMENT_TIMEOUT_PER_HOP`] per hop to the destination, and the
    /// transport does the same when it creates an outbound link.
    pub fn set_establishment_timeout(&mut self, timeout: Duration) {
        self.establishment_timeout = timeout;
    }

    /// Whether a link that is still `Pending` or in `Handshake` has outlived
    /// its establishment timeout. `RNS.Link`'s watchdog closes it then with
    /// `teardown_reason = TIMEOUT`, and a non-transport instance expires the
    /// path it was made on.
    ///
    /// Measured from the start of the current attempt, which is not the same
    /// as the latest request packet: repeating a still-pending request keeps
    /// the clock running, since that is the retransmit this timeout exists to
    /// bound. Only a request that starts over from a state the link had
    /// already left, or an explicit [`Link::restart`], begins a new attempt.
    pub fn establishment_timed_out(&self, now: Instant) -> bool {
        matches!(self.status, LinkStatus::Pending | LinkStatus::Handshake)
            && now.duration_since(self.establishment_started_at) >= self.establishment_timeout
    }

    /// Begins a new establishment attempt, restarting the timeout above.
    fn start_establishment(&mut self) {
        self.establishment_started_at = Instant::now();
    }

    #[cfg(test)]
    pub(crate) fn set_establishment_start_for_test(&mut self, started_at: Instant) {
        self.establishment_started_at = started_at;
    }
}
