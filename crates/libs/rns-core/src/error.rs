#[derive(Debug)]
pub enum RnsError {
    OutOfMemory,
    InvalidArgument,
    IncorrectSignature,
    IncorrectHash,
    CryptoError,
    PacketError,
    ConnectionError,
}

impl core::fmt::Display for RnsError {
    fn fmt(&self, formatter: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        formatter.write_str(match self {
            Self::OutOfMemory => "allocation failed",
            Self::InvalidArgument => "invalid argument",
            Self::IncorrectSignature => "signature verification failed",
            Self::IncorrectHash => "hash verification failed",
            Self::CryptoError => "cryptographic operation failed",
            Self::PacketError => "invalid packet",
            Self::ConnectionError => "connection failed",
        })
    }
}

impl core::error::Error for RnsError {}
