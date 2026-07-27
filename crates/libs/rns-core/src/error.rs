#[derive(Debug)]
pub enum RnsError {
    OutOfMemory,
    InvalidArgument,
    IncorrectSignature,
    IncorrectHash,
    CryptoError,
    PacketError,
    ConnectionError,
    /// No wall-clock time source is available. Returned by
    /// timestamp-dependent operations (announce timestamps, ratchet
    /// rotation) in `no_std` builds before the embedding application has
    /// installed one via `ratchets::set_time_override`.
    TimeSourceUnavailable,
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
            Self::TimeSourceUnavailable => {
                "no wall-clock time source available (install one via \
                 ratchets::set_time_override in no_std builds)"
            }
        })
    }
}

impl core::error::Error for RnsError {}
