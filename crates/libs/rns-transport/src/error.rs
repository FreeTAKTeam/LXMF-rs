#[derive(Debug, thiserror::Error)]
pub enum RnsError {
    #[error("allocation failed")]
    OutOfMemory,
    #[error("invalid argument")]
    InvalidArgument,
    #[error("signature verification failed")]
    IncorrectSignature,
    #[error("hash verification failed")]
    IncorrectHash,
    #[error("cryptographic operation failed")]
    CryptoError,
    #[error("invalid packet")]
    PacketError,
    #[error("connection failed")]
    ConnectionError,
}
