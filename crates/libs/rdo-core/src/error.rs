/// Every way RDO owner-binding v1 input can be refused.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RdoError {
    #[error("malformed CBOR or structure: {0}")]
    Malformed(&'static str),
    #[error("non-deterministic CBOR encoding: {0}")]
    Noncanonical(&'static str),
    #[error("size or count limit exceeded")]
    LimitExceeded,
    #[error("unsupported version {0}")]
    UnsupportedVersion(u64),
    #[error("invalid signature")]
    InvalidSignature,
    #[error("invalid revision: {0}")]
    InvalidRevision(&'static str),
    #[error("announce does not bind the requested object: {0}")]
    InvalidBinding(&'static str),
    #[error("transport failure: {0}")]
    Transport(String),
}

pub type Result<T> = std::result::Result<T, RdoError>;
