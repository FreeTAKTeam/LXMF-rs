use crate::error::LxmfError;
use crate::message::WireMessage;
use crate::storage::Store;

#[derive(Debug, Clone, Copy)]
pub enum VerificationMode {
    Strict,
    Permissive,
}

pub trait Verifier: Send + Sync {
    fn verify(&self, message: &WireMessage) -> Result<bool, LxmfError>;
}

pub struct PropagationNode {
    store: Box<dyn Store + Send + Sync>,
    mode: VerificationMode,
    verifier: Option<Box<dyn Verifier + Send + Sync>>,
}

impl PropagationNode {
    pub fn new(store: Box<dyn Store + Send + Sync>) -> Self {
        Self {
            store,
            mode: VerificationMode::Strict,
            verifier: None,
        }
    }

    pub fn with_verifier(
        store: Box<dyn Store + Send + Sync>,
        mode: VerificationMode,
        verifier: Box<dyn Verifier + Send + Sync>,
    ) -> Self {
        Self {
            store,
            mode,
            verifier: Some(verifier),
        }
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
