use super::*;

#[derive(Debug, PartialEq, Eq)]
pub(in crate::bridge) enum LinkAttemptOutcome {
    Packet,
    /// The Resource event consumer owns completion, including dispatch failure.
    /// This does not imply successful transmission.
    ResourceOwned,
    Stopped,
}

#[derive(Default)]
pub(super) struct DispatchObservation {
    pub(super) packet: Option<String>,
    pub(super) resource: Option<rns_transport::hash::Hash>,
}

impl DispatchObservation {
    pub(super) fn failed(
        self,
        task: &DeliveryTask,
        stage: &str,
        error: std::io::Error,
    ) -> Result<LinkAttemptOutcome, std::io::Error> {
        if let Some(hash) = self.resource {
            // Transport publishes OutboundFailed before returning its dispatch
            // error. Retrying here would race that registered lifecycle.
            log::warn!(
                "[daemon] Resource dispatch failed message_id={} stage={stage} resource={hash}: {error}; Resource lifecycle owns completion",
                task.message_id
            );
            return Ok(LinkAttemptOutcome::ResourceOwned);
        }
        if let Some(hash) = self.packet {
            // send_on_link_observed returns an error after its packet observer
            // only when transport reports that the packet was not sent.
            match task.receipt_map.lock() {
                Ok(mut map) => {
                    if map.get(&hash) == Some(&task.message_id) {
                        map.remove(&hash);
                    }
                }
                Err(err) => log::error!(
                    "[daemon] failed to remove dropped packet correlation message_id={} packet={hash}: {err}",
                    task.message_id
                ),
            }
        }
        Err(error)
    }
}
