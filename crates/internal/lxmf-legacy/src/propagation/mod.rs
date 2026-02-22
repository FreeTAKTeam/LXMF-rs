use crate::error::LxmfError;
use crate::message::WireMessage;
use crate::storage::{PropagationStore, Store};
use serde::Deserialize;
use serde_bytes::ByteBuf;

#[derive(Debug, Clone, Copy)]
pub enum VerificationMode {
    Strict,
    Permissive,
}

pub trait Verifier: Send + Sync {
    fn verify(&self, message: &WireMessage) -> Result<bool, LxmfError>;
}

pub struct NoopVerifier;

impl Verifier for NoopVerifier {
    fn verify(&self, _message: &WireMessage) -> Result<bool, LxmfError> {
        Ok(true)
    }
}

pub struct PropagationNode {
    store: Box<dyn Store + Send + Sync>,
    mode: VerificationMode,
    verifier: Option<Box<dyn Verifier + Send + Sync>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PropagationEnvelope {
    pub timestamp: f64,
    pub messages: Vec<Vec<u8>>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PropagationStamp {
    pub transient_id: Vec<u8>,
    pub lxmf_data: Vec<u8>,
    pub stamp_value: u32,
    pub stamp: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct IngestedMessage {
    pub transient_id: Vec<u8>,
    pub lxmf_data: Vec<u8>,
    pub stamp_value: Option<u32>,
    pub stamp: Option<Vec<u8>>,
}

pub fn unpack_envelope(bytes: &[u8]) -> Result<PropagationEnvelope, LxmfError> {
    #[derive(Deserialize)]
    struct Envelope(f64, Vec<ByteBuf>);

    let Envelope(timestamp, messages) =
        rmp_serde::from_slice::<Envelope>(bytes).map_err(|e| LxmfError::Decode(e.to_string()))?;
    Ok(PropagationEnvelope {
        timestamp,
        messages: messages.into_iter().map(|b| b.into_vec()).collect(),
    })
}

pub fn validate_stamp(transient_data: &[u8], target_cost: u32) -> Option<PropagationStamp> {
    let (transient_id, lxmf_data, stamp_value, stamp) =
        crate::stamper::validate_pn_stamp(transient_data, target_cost)?;
    Some(PropagationStamp { transient_id, lxmf_data, stamp_value, stamp })
}

pub fn ingest_envelope(bytes: &[u8], target_cost: u32) -> Result<Vec<IngestedMessage>, LxmfError> {
    let envelope = unpack_envelope(bytes)?;
    let mut out = Vec::new();

    for data in envelope.messages {
        let maybe_stamped = if data.len() > reticulum::hash::HASH_SIZE {
            validate_stamp(&data, target_cost)
        } else {
            None
        };

        if let Some(stamped) = maybe_stamped {
            out.push(IngestedMessage {
                transient_id: stamped.transient_id,
                lxmf_data: stamped.lxmf_data,
                stamp_value: Some(stamped.stamp_value),
                stamp: Some(stamped.stamp),
            });
        } else if target_cost == 0 {
            let transient_id = reticulum::hash::Hash::new_from_slice(&data).to_bytes().to_vec();
            out.push(IngestedMessage {
                transient_id,
                lxmf_data: data,
                stamp_value: None,
                stamp: None,
            });
        }
    }

    Ok(out)
}

pub struct PropagationService {
    store: PropagationStore,
    target_cost: u32,
}

impl PropagationService {
    pub fn new(store: PropagationStore, target_cost: u32) -> Self {
        Self { store, target_cost }
    }

    pub fn ingest(&self, bytes: &[u8]) -> Result<usize, LxmfError> {
        let messages = ingest_envelope(bytes, self.target_cost)?;
        for msg in &messages {
            self.store.save(&msg.transient_id, &msg.lxmf_data)?;
        }
        Ok(messages.len())
    }

    pub fn fetch(&self, transient_id: &[u8]) -> Result<Vec<u8>, LxmfError> {
        self.store.get(transient_id)
    }
}

impl PropagationNode {
    pub fn new(store: Box<dyn Store + Send + Sync>) -> Self {
        Self { store, mode: VerificationMode::Permissive, verifier: None }
    }

    pub fn new_strict(
        store: Box<dyn Store + Send + Sync>,
        verifier: Box<dyn Verifier + Send + Sync>,
    ) -> Self {
        Self { store, mode: VerificationMode::Strict, verifier: Some(verifier) }
    }

    pub fn with_verifier(
        store: Box<dyn Store + Send + Sync>,
        mode: VerificationMode,
        verifier: Box<dyn Verifier + Send + Sync>,
    ) -> Self {
        Self { store, mode, verifier: Some(verifier) }
    }

    pub fn store(&mut self, msg: WireMessage) -> Result<(), LxmfError> {
        self.enforce_verification(&msg)?;
        self.store.save(&msg)
    }

    pub fn fetch(&self, id: &[u8; 32]) -> Result<WireMessage, LxmfError> {
        self.store.get(id)
    }

    fn enforce_verification(&self, msg: &WireMessage) -> Result<(), LxmfError> {
        if let VerificationMode::Strict = self.mode {
            if msg.signature.is_none() {
                return Err(LxmfError::Verify("missing signature".into()));
            }
            if self.verifier.is_none() {
                return Err(LxmfError::Verify("missing verifier".into()));
            }
        }

        if let Some(verifier) = &self.verifier {
            let ok = verifier.verify(msg)?;
            if !ok && matches!(self.mode, VerificationMode::Strict) {
                return Err(LxmfError::Verify("invalid signature".into()));
            }
        }

        Ok(())
    }
}
