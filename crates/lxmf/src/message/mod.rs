mod container;
mod delivery;
mod payload;
mod state;
mod types;
mod wire;

pub use container::MessageContainer;
pub use delivery::{decide_delivery, DeliveryDecision};
pub use payload::Payload;
pub use state::State;
pub use types::{MessageMethod, MessageState, TransportMethod, UnverifiedReason};
pub use wire::WireMessage;

use crate::error::LxmfError;
use reticulum::identity::PrivateIdentity;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone)]
pub struct Message {
    pub destination_hash: Option<[u8; 16]>,
    pub source_hash: Option<[u8; 16]>,
    pub signature: Option<[u8; wire::SIGNATURE_LENGTH]>,
    pub content: Vec<u8>,
    pub title: Vec<u8>,
    pub fields: Option<rmpv::Value>,
    pub stamp: Option<Vec<u8>>,
    pub timestamp: Option<f64>,
    state: State,
}

impl Message {
    pub fn new() -> Self {
        Self {
            destination_hash: None,
            source_hash: None,
            signature: None,
            content: Vec::new(),
            title: Vec::new(),
            fields: None,
            stamp: None,
            timestamp: None,
            state: State::Generating,
        }
    }

    pub fn set_state(&mut self, state: State) {
        self.state = state;
    }

    pub fn is_outbound(&self) -> bool {
        self.state == State::Outbound
    }

    pub fn set_title_from_string(&mut self, title: &str) {
        self.title = title.as_bytes().to_vec();
    }

    pub fn set_title_from_bytes(&mut self, title: &[u8]) {
        self.title = title.to_vec();
    }

    pub fn title_as_string(&self) -> Option<String> {
        String::from_utf8(self.title.clone()).ok()
    }

    pub fn set_content_from_string(&mut self, content: &str) {
        self.content = content.as_bytes().to_vec();
    }

    pub fn set_content_from_bytes(&mut self, content: &[u8]) {
        self.content = content.to_vec();
    }

    pub fn set_stamp_from_bytes(&mut self, stamp: &[u8]) {
        self.stamp = Some(stamp.to_vec());
    }

    pub fn stamp_bytes(&self) -> Option<Vec<u8>> {
        self.stamp.clone()
    }

    pub fn content_as_string(&self) -> Option<String> {
        String::from_utf8(self.content.clone()).ok()
    }

    pub fn from_wire(bytes: &[u8]) -> Result<Self, LxmfError> {
        let wire = WireMessage::unpack(bytes)?;
        let payload = wire.payload;
        Ok(Self {
            destination_hash: Some(wire.destination),
            source_hash: Some(wire.source),
            signature: wire.signature,
            content: payload
                .content
                .as_ref()
                .map(|c| c.to_vec())
                .unwrap_or_default(),
            title: payload
                .title
                .as_ref()
                .map(|t| t.to_vec())
                .unwrap_or_default(),
            fields: payload.fields,
            stamp: payload.stamp.as_ref().map(|s| s.to_vec()),
            timestamp: Some(payload.timestamp),
            state: State::Generating,
        })
    }

    pub fn to_wire(&self, signer: Option<&PrivateIdentity>) -> Result<Vec<u8>, LxmfError> {
        let destination =
            self.destination_hash.ok_or_else(|| LxmfError::Encode("missing destination".into()))?;
        let source = self.source_hash.ok_or_else(|| LxmfError::Encode("missing source".into()))?;

        let timestamp = self.timestamp.unwrap_or_else(|| {
            let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default();
            now.as_secs_f64()
        });

        let payload = Payload::new(
            timestamp,
            Some(self.content.clone()),
            Some(self.title.clone()),
            self.fields.clone(),
            self.stamp.clone(),
        );

        let mut wire = WireMessage::new(destination, source, payload);
        if let Some(signature) = self.signature {
            wire.signature = Some(signature);
        } else if let Some(signer) = signer {
            wire.sign(signer)?;
        } else {
            return Err(LxmfError::Encode("missing signature".into()));
        }

        wire.pack()
    }
}

impl Default for Message {
    fn default() -> Self {
        Self::new()
    }
}
