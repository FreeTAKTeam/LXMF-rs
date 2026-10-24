use lxmf_core::WireMessage;
use rand_core::OsRng;
use rns_core::destination::{DestinationName, SingleOutputDestination};
use rns_core::identity::{Identity, PrivateIdentity};

use crate::Error;

/// Signed, recipient-encrypted local output. Fields are private so remote ciphertext
/// cannot be admitted by asserting that it belongs to the local sender.
pub struct PreparedMessage {
    pub(crate) owner: [u8; 16],
    pub(crate) destination: String,
    pub(crate) message_id: [u8; 32],
    pub(crate) transient_id: String,
    pub(crate) payload: Vec<u8>,
    pub(crate) recovery: Vec<u8>,
    pub(crate) application_key: Option<String>,
}

impl PreparedMessage {
    pub fn new(
        wire: &WireMessage,
        owner: &PrivateIdentity,
        recipient: Option<&Identity>,
    ) -> Result<Self, Error> {
        let recipient = recipient.ok_or(Error::UnresolvedRecipient)?;
        if wire.source != delivery_address(owner.as_identity())
            || !wire.verify(owner.as_identity())?
        {
            return Err(Error::ForeignMessage);
        }
        if wire.destination != delivery_address(recipient) {
            return Err(Error::UnresolvedRecipient);
        }
        let (payload, transient) = wire.pack_propagation_transient_with_rng(recipient, OsRng)?;
        // Keep the signed wire recoverable by the sender using existing identity encryption.
        let recovery = wire.pack_paper_with_rng(owner.as_identity(), OsRng)?;
        Ok(Self {
            owner: delivery_address(owner.as_identity()),
            destination: hex::encode(wire.destination),
            message_id: wire.try_message_id()?,
            transient_id: hex::encode(transient),
            payload,
            recovery,
            application_key: None,
        })
    }

    pub fn with_application_key(mut self, key: Option<&str>) -> Result<Self, Error> {
        if key.is_some_and(|key| key.is_empty() || key.len() > 128) {
            return Err(Error::InvalidState("invalid application correlation key"));
        }
        self.application_key = key.map(ToOwned::to_owned);
        Ok(self)
    }

    pub fn message_id(&self) -> &[u8; 32] {
        &self.message_id
    }
    pub fn transient_id(&self) -> &str {
        &self.transient_id
    }
    pub fn encrypted_bytes(&self) -> usize {
        self.payload.len() + self.recovery.len()
    }
}

pub fn delivery_address(identity: &Identity) -> [u8; 16] {
    let dest = SingleOutputDestination::new(*identity, DestinationName::new("lxmf", "delivery"));
    let mut bytes = [0; 16];
    bytes.copy_from_slice(dest.desc.address_hash.as_slice());
    bytes
}

pub(crate) fn recover_wire(
    bytes: &[u8],
    owner: &PrivateIdentity,
    message_id: &str,
    destination: &str,
) -> Result<Vec<u8>, Error> {
    let wire = WireMessage::unpack_paper(bytes, owner)?;
    if wire.source != delivery_address(owner.as_identity())
        || !wire.verify(owner.as_identity())?
        || hex::encode(wire.destination) != destination
        || hex::encode(wire.try_message_id()?) != message_id
    {
        return Err(Error::ForeignMessage);
    }
    Ok(wire.pack()?)
}
