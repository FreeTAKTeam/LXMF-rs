#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("recipient encryption identity is unresolved")]
    UnresolvedRecipient,
    #[error("message is not signed by this store's owner")]
    ForeignMessage,
    #[error("encrypted payload exceeds local storage capacity")]
    PayloadTooLarge,
    #[error("local propagation store is full")]
    StoreFull,
    #[error("message has expired")]
    Expired,
    #[error("invalid local propagation configuration or state: {0}")]
    InvalidState(&'static str),
    #[error("local propagation service is stopped")]
    Stopped,
    #[error(transparent)]
    Storage(#[from] rusqlite::Error),
    #[error(transparent)]
    Wire(#[from] lxmf_core::LxmfError),
}
