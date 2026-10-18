use super::link_send::LinkAttemptOutcome;
use rns_transport::hash::AddressHash;

/// One borrowed attempt's disposition. The scheduler owns the task and bytes;
/// these variants do not themselves authorize another transmission.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum DeliveryAttemptOutcome {
    AwaitPath {
        destination: AddressHash,
    },
    FallbackPropagation,
    AwaitPacket,
    OpportunisticPacket,
    /// A status read failed; retry observation without publishing a competing terminal.
    AwaitStatus,
    ResourceOwned,
    /// Native custody is established; publish and await its commit outside execution permits.
    PropagationCustody(Box<reticulum_daemon::receipt_bridge::ReceiptEvent>),
    /// The attempt owns terminal publication; its caller must not publish a second receipt.
    Terminal,
    Stopped,
    DeferredStorage,
}

impl From<LinkAttemptOutcome> for DeliveryAttemptOutcome {
    fn from(outcome: LinkAttemptOutcome) -> Self {
        match outcome {
            LinkAttemptOutcome::Packet => Self::AwaitPacket,
            LinkAttemptOutcome::ResourceOwned => Self::ResourceOwned,
            LinkAttemptOutcome::Stopped => Self::Stopped,
        }
    }
}
