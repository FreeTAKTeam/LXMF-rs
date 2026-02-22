use thiserror::Error;

#[derive(Debug, Error)]
pub enum LxmfError {
    #[error("decode error: {0}")]
    Decode(String),
    #[error("encode error: {0}")]
    Encode(String),
    #[error("io error: {0}")]
    Io(String),
    #[error("verify error: {0}")]
    Verify(String),
}
