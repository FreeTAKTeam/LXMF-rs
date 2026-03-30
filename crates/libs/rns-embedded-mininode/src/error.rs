use lxmf_core::LxmfError;
use rns_core::RnsError;

#[derive(Debug)]
pub enum MiniNodeError {
    QueueFull,
    LinkDown,
    MtuExceeded { frame_len: usize, mtu: usize },
    StorageError,
    InvalidPacket,
    MissingIdentity,
    Rns(RnsError),
    Lxmf(LxmfError),
}

impl From<RnsError> for MiniNodeError {
    fn from(value: RnsError) -> Self {
        Self::Rns(value)
    }
}

impl From<LxmfError> for MiniNodeError {
    fn from(value: LxmfError) -> Self {
        Self::Lxmf(value)
    }
}
