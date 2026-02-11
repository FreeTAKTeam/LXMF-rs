use crate::error::LxmfError;
use crate::message::WireMessage;
use ed25519_dalek::Signature;
use reticulum::hash::AddressHash;
use reticulum::identity::{Identity, PrivateIdentity};
use std::sync::Arc;

type OutboundSender = Arc<dyn Fn(&WireMessage) -> Result<(), LxmfError> + Send + Sync + 'static>;

pub struct Adapter {
    outbound_sender: Option<OutboundSender>,
}

impl Adapter {
    pub const DEST_HASH_LEN: usize = 16;

    pub fn new() -> Self {
        Self {
            outbound_sender: None,
        }
    }

    pub fn with_outbound_sender<F>(sender: F) -> Self
    where
        F: Fn(&WireMessage) -> Result<(), LxmfError> + Send + Sync + 'static,
    {
        Self {
            outbound_sender: Some(Arc::new(sender)),
        }
    }

    pub fn has_outbound_sender(&self) -> bool {
        self.outbound_sender.is_some()
    }

    pub fn send_outbound(&self, message: &WireMessage) -> Result<(), LxmfError> {
        match &self.outbound_sender {
            Some(sender) => sender(message),
            None => Err(LxmfError::Io(
                "no outbound sender configured for reticulum adapter".into(),
            )),
        }
    }

    pub fn address_hash(identity: &Identity) -> [u8; Self::DEST_HASH_LEN] {
        let mut out = [0u8; Self::DEST_HASH_LEN];
        out.copy_from_slice(identity.address_hash.as_slice());
        out
    }

    pub fn address_hash_from_dest(dest: &AddressHash) -> [u8; Self::DEST_HASH_LEN] {
        let mut out = [0u8; Self::DEST_HASH_LEN];
        out.copy_from_slice(dest.as_slice());
        out
    }

    pub fn sign(identity: &PrivateIdentity, data: &[u8]) -> [u8; ed25519_dalek::SIGNATURE_LENGTH] {
        identity.sign(data).to_bytes()
    }

    pub fn verify(identity: &Identity, data: &[u8], signature: &[u8]) -> bool {
        let signature = match Signature::from_slice(signature) {
            Ok(sig) => sig,
            Err(_) => return false,
        };
        identity.verify(data, &signature).is_ok()
    }
}

impl Default for Adapter {
    fn default() -> Self {
        Self::new()
    }
}
