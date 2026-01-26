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

impl PropagationNode {
    pub fn new(store: Box<dyn Store + Send + Sync>) -> Self {
        Self {
            store,
            mode: VerificationMode::Permissive,
            verifier: None,
        }
    }

    pub fn new_strict(
        store: Box<dyn Store + Send + Sync>,
        verifier: Box<dyn Verifier + Send + Sync>,
    ) -> Self {
        Self {
            store,
            mode: VerificationMode::Strict,
            verifier: Some(verifier),
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
