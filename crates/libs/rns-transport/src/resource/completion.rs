/// Terminal outcome of one logical outbound Resource, including every segment.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResourceOutcome {
    Complete,
    Failed,
    Rejected,
    Cancelled,
}

/// A retained native outcome, independent of the lossy Resource event broadcast.
/// This observes transport completion, not application persistence or custody.
#[derive(Debug)]
pub struct ResourceCompletion {
    receiver: tokio::sync::watch::Receiver<Option<ResourceOutcome>>,
}

impl ResourceCompletion {
    pub fn current(&self) -> Option<ResourceOutcome> {
        *self.receiver.borrow()
    }

    /// Returns the final result even after the native producer has been removed.
    /// Channel closure without a result means observation became unavailable.
    /// Dropping this wait does not cancel the Resource or consume its result.
    pub async fn wait(&mut self) -> Result<ResourceOutcome, tokio::sync::watch::error::RecvError> {
        loop {
            if let Some(outcome) = self.current() {
                return Ok(outcome);
            }
            self.receiver.changed().await?;
        }
    }
}

impl ResourceManager {
    // Called only while registering a newly prepared outbound Resource, before
    // any packet handoff. Its first hash is also the logical/original hash.
    pub(crate) fn observe_outbound(&mut self, hash: Hash) -> ResourceCompletion {
        let sender = self
            .outbound_completions
            .entry(hash)
            .or_insert_with(|| tokio::sync::watch::channel(None).0);
        ResourceCompletion { receiver: sender.subscribe() }
    }

    fn publish_outbound_completions(&mut self, events: &[ResourceEvent]) {
        for event in events {
            let outcome = match event.kind {
                ResourceEventKind::OutboundComplete => ResourceOutcome::Complete,
                ResourceEventKind::OutboundFailed => ResourceOutcome::Failed,
                ResourceEventKind::OutboundRejected => ResourceOutcome::Rejected,
                ResourceEventKind::OutboundCancelled => ResourceOutcome::Cancelled,
                _ => continue,
            };
            if let Some(sender) = self.outbound_completions.remove(&event.hash) {
                sender.send_replace(Some(outcome));
            }
        }
    }
}
