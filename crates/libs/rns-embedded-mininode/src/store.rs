use alloc::vec::Vec;
use rns_core::identity::{Identity, PRIVATE_KEY_LENGTH};

use crate::{
    error::MiniNodeError,
    telemetry::{PositionFix, TelemetryPoint},
};

#[derive(Clone)]
pub struct NeighborSnapshot {
    pub destination_hash: [u8; 16],
    pub identity: Identity,
    pub last_seen_ms: u64,
    pub app_data: Vec<u8>,
}

#[derive(Clone, Default)]
pub struct NodeSnapshot {
    pub last_announce_ms: Option<u64>,
    pub neighbors: Vec<NeighborSnapshot>,
    pub recent_message_ids: Vec<[u8; 32]>,
    pub telemetry: Vec<TelemetryPoint>,
    pub latest_position: Option<PositionFix>,
}

pub trait MiniNodeStore {
    fn load_identity(&self) -> Result<Option<[u8; PRIVATE_KEY_LENGTH]>, MiniNodeError>;
    fn save_identity(
        &mut self,
        identity_bytes: &[u8; PRIVATE_KEY_LENGTH],
    ) -> Result<(), MiniNodeError>;
    fn load_snapshot(&self) -> Result<Option<NodeSnapshot>, MiniNodeError>;
    fn save_snapshot(&mut self, snapshot: &NodeSnapshot) -> Result<(), MiniNodeError>;
}

#[derive(Clone, Default)]
pub struct MemoryStore {
    identity: Option<[u8; PRIVATE_KEY_LENGTH]>,
    snapshot: Option<NodeSnapshot>,
}

impl MemoryStore {
    pub fn new() -> Self {
        Self::default()
    }
}

impl MiniNodeStore for MemoryStore {
    fn load_identity(&self) -> Result<Option<[u8; PRIVATE_KEY_LENGTH]>, MiniNodeError> {
        Ok(self.identity)
    }

    fn save_identity(
        &mut self,
        identity_bytes: &[u8; PRIVATE_KEY_LENGTH],
    ) -> Result<(), MiniNodeError> {
        self.identity = Some(*identity_bytes);
        Ok(())
    }

    fn load_snapshot(&self) -> Result<Option<NodeSnapshot>, MiniNodeError> {
        Ok(self.snapshot.clone())
    }

    fn save_snapshot(&mut self, snapshot: &NodeSnapshot) -> Result<(), MiniNodeError> {
        self.snapshot = Some(snapshot.clone());
        Ok(())
    }
}
