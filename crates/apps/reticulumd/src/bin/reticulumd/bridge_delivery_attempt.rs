use rns_transport::hash::AddressHash;

/// One borrowed attempt's disposition. The scheduler owns the task and bytes;
/// these variants do not themselves authorize another transmission.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum DeliveryAttemptOutcome {
    AwaitPath {
        destination: AddressHash,
    },
    /// A SINGLE packet cannot carry these bytes; the same owner advances to Link.
    LinkRequired,
    OpportunisticPacket,
    /// A status read failed; retry observation without publishing a competing terminal.
    AwaitStatus,
    /// The attempt owns terminal publication; its caller must not publish a second receipt.
    Terminal,
    Stopped,
    DeferredStorage,
}
