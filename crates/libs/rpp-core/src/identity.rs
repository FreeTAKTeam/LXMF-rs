//! RNS Identity arithmetic, shared with RDO, plus the RPP inbox Destination.

pub use rdo_core::identity::*;

/// RPP-specific derivation on an RNS public Identity.
pub trait ReceiveDestination {
    /// The identity's `rpp.receive` inbox Destination.
    fn receive_destination(&self) -> Destination;
}

impl ReceiveDestination for PublicIdentity {
    fn receive_destination(&self) -> Destination {
        destination_hash(&self.0, "rpp.receive")
    }
}
