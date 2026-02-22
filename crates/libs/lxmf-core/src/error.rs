use alloc::string::String;
use core::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LxmfError {
    Decode(String),
    Encode(String),
    Io(String),
    Verify(String),
}

impl fmt::Display for LxmfError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Decode(err) => write!(f, "decode error: {err}"),
            Self::Encode(err) => write!(f, "encode error: {err}"),
            Self::Io(err) => write!(f, "io error: {err}"),
            Self::Verify(err) => write!(f, "verify error: {err}"),
        }
    }
}

#[cfg(feature = "std")]
impl std::error::Error for LxmfError {}
