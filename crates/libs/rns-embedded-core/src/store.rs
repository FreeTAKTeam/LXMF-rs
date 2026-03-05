use crate::{EmbeddedResult, attachment::ChunkCursor};

pub trait EmbeddedStore {
    fn load_replay_floor(&self, identity: &[u8; 32]) -> EmbeddedResult<u64>;
    fn save_replay_floor(&mut self, identity: &[u8; 32], floor: u64) -> EmbeddedResult<()>;
    fn load_chunk_cursor(&self, transfer_id: u32) -> EmbeddedResult<Option<ChunkCursor>>;
    fn save_chunk_cursor(&mut self, cursor: &ChunkCursor) -> EmbeddedResult<()>;
    fn clear_chunk_cursor(&mut self, transfer_id: u32) -> EmbeddedResult<()>;
}
