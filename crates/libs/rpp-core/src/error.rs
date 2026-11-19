use crate::control::Status;

/// Every way RPP input can be refused. Variants follow the RC1 negative
/// fixture classes so conformance runners can compare them directly.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RppError {
    #[error("malformed CBOR or structure: {0}")]
    Malformed(&'static str),
    #[error("non-deterministic CBOR encoding: {0}")]
    Noncanonical(&'static str),
    #[error("unsupported RPP version {0}")]
    UnsupportedVersion(u64),
    #[error("unsupported critical extension {0}")]
    UnsupportedExtension(u64),
    #[error("invalid manifest: {0}")]
    InvalidManifest(&'static str),
    #[error("invalid signature")]
    InvalidSignature,
    #[error("invalid publication reference: {0}")]
    InvalidPublication(&'static str),
    #[error("not authorized")]
    NotAuthorized,
    #[error("request expired or outside its validity window")]
    Expired,
    #[error("invalid chunk selection")]
    InvalidSelection,
    #[error("size or count limit exceeded")]
    LimitExceeded,
    #[error("chunk or bundle digest mismatch")]
    ChunkMismatch,
    #[error("conflicting duplicate Resource")]
    ConflictingDuplicate,
    #[error("Resource has no accepted plan")]
    NoAcceptedPlan,
    #[error("transfer incomplete")]
    Incomplete,
    #[error("request ID reused with different bytes")]
    ReplayConflict,
    #[error("package not found")]
    NotFound,
    #[error("storage failure: {0}")]
    Storage(String),
    #[error("transport failure: {0}")]
    Transport(String),
}

impl RppError {
    /// The RDO v1 outer status a responder returns for this refusal.
    pub fn status(&self) -> Status {
        match self {
            Self::NotAuthorized => Status::NotAuthorized,
            Self::NotFound => Status::NotFound,
            Self::Storage(_) | Self::Transport(_) => Status::Error,
            _ => Status::Rejected,
        }
    }
}

impl From<std::io::Error> for RppError {
    fn from(error: std::io::Error) -> Self {
        Self::Storage(error.to_string())
    }
}

pub type Result<T> = std::result::Result<T, RppError>;
