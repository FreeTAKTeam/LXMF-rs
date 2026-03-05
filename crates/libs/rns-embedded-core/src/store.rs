use crate::{
    EmbeddedError, EmbeddedResult, attachment::ChunkCursor, hash::digest32,
};
use alloc::collections::BTreeMap;
use alloc::vec::Vec;

const STORE_MAGIC: &[u8; 4] = b"RES1";
const SCHEMA_V1: u8 = 1;
const SCHEMA_V2: u8 = 2;

pub trait EmbeddedStore {
    fn load_replay_floor(&self, identity: &[u8; 32]) -> EmbeddedResult<u64>;
    fn save_replay_floor(&mut self, identity: &[u8; 32], floor: u64) -> EmbeddedResult<()>;
    fn load_chunk_cursor(&self, transfer_id: u32) -> EmbeddedResult<Option<ChunkCursor>>;
    fn save_chunk_cursor(&mut self, cursor: &ChunkCursor) -> EmbeddedResult<()>;
    fn clear_chunk_cursor(&mut self, transfer_id: u32) -> EmbeddedResult<()>;
}

#[derive(Debug, Clone)]
pub struct JournaledEmbeddedStore {
    replay_floors: BTreeMap<[u8; 32], u64>,
    cursors: BTreeMap<u32, ChunkCursor>,
    schema_version: u8,
}

impl JournaledEmbeddedStore {
    pub fn new() -> Self {
        Self {
            replay_floors: BTreeMap::new(),
            cursors: BTreeMap::new(),
            schema_version: SCHEMA_V2,
        }
    }

    pub fn schema_version(&self) -> u8 {
        self.schema_version
    }

    pub fn snapshot_bytes(&self) -> EmbeddedResult<Vec<u8>> {
        let mut body = Vec::new();
        body.push(self.schema_version);

        let replay_len = u16::try_from(self.replay_floors.len()).map_err(|_| EmbeddedError::InvalidState)?;
        body.extend_from_slice(&replay_len.to_le_bytes());
        for (identity, floor) in &self.replay_floors {
            body.extend_from_slice(identity);
            body.extend_from_slice(&floor.to_le_bytes());
        }

        if self.schema_version >= SCHEMA_V2 {
            let cursors_len =
                u16::try_from(self.cursors.len()).map_err(|_| EmbeddedError::InvalidState)?;
            body.extend_from_slice(&cursors_len.to_le_bytes());
            for (transfer_id, cursor) in &self.cursors {
                body.extend_from_slice(&transfer_id.to_le_bytes());
                body.extend_from_slice(&cursor.total_size.to_le_bytes());
                body.extend_from_slice(&cursor.next_offset.to_le_bytes());
                body.extend_from_slice(&cursor.expected_sequence.to_le_bytes());
                body.extend_from_slice(&cursor.chunk_size.to_le_bytes());
            }
        }

        let checksum = digest32(&body);

        let mut out = Vec::with_capacity(4 + 4 + body.len() + 32);
        out.extend_from_slice(STORE_MAGIC);
        let body_len_u32 = u32::try_from(body.len()).map_err(|_| EmbeddedError::InvalidState)?;
        out.extend_from_slice(&body_len_u32.to_le_bytes());
        out.extend_from_slice(&body);
        out.extend_from_slice(&checksum);
        Ok(out)
    }

    pub fn from_snapshot_bytes(bytes: &[u8]) -> EmbeddedResult<Self> {
        if bytes.len() < 8 + 32 {
            return Err(EmbeddedError::StorageCorruption);
        }
        if &bytes[0..4] != STORE_MAGIC {
            return Err(EmbeddedError::StorageCorruption);
        }
        let body_len = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);
        let body_len = usize::try_from(body_len).map_err(|_| EmbeddedError::StorageCorruption)?;
        if bytes.len() != 8 + body_len + 32 {
            return Err(EmbeddedError::StorageCorruption);
        }

        let body = &bytes[8..8 + body_len];
        let mut expected = [0_u8; 32];
        expected.copy_from_slice(&bytes[8 + body_len..]);
        if digest32(body) != expected {
            return Err(EmbeddedError::StorageCorruption);
        }

        Self::decode_body(body)
    }

    fn decode_body(body: &[u8]) -> EmbeddedResult<Self> {
        if body.is_empty() {
            return Err(EmbeddedError::StorageCorruption);
        }

        let mut idx = 0usize;
        let schema = body[idx];
        idx += 1;

        if schema != SCHEMA_V1 && schema != SCHEMA_V2 {
            return Err(EmbeddedError::Unsupported);
        }

        let replay_count = read_u16(body, &mut idx)? as usize;
        let mut replay_floors = BTreeMap::new();
        for _ in 0..replay_count {
            let identity = read_identity(body, &mut idx)?;
            let floor = read_u64(body, &mut idx)?;
            replay_floors.insert(identity, floor);
        }

        let mut cursors = BTreeMap::new();
        if schema >= SCHEMA_V2 {
            let cursor_count = read_u16(body, &mut idx)? as usize;
            for _ in 0..cursor_count {
                let transfer_id = read_u32(body, &mut idx)?;
                let total_size = read_u32(body, &mut idx)?;
                let next_offset = read_u32(body, &mut idx)?;
                let expected_sequence = read_u16(body, &mut idx)?;
                let chunk_size = read_u16(body, &mut idx)?;
                let cursor = ChunkCursor {
                    transfer_id,
                    total_size,
                    next_offset,
                    expected_sequence,
                    chunk_size,
                };
                cursor.validate()?;
                cursors.insert(transfer_id, cursor);
            }
        }

        if idx != body.len() {
            return Err(EmbeddedError::StorageCorruption);
        }

        Ok(Self {
            replay_floors,
            cursors,
            schema_version: SCHEMA_V2,
        })
    }
}

impl Default for JournaledEmbeddedStore {
    fn default() -> Self {
        Self::new()
    }
}

impl EmbeddedStore for JournaledEmbeddedStore {
    fn load_replay_floor(&self, identity: &[u8; 32]) -> EmbeddedResult<u64> {
        Ok(*self.replay_floors.get(identity).unwrap_or(&0))
    }

    fn save_replay_floor(&mut self, identity: &[u8; 32], floor: u64) -> EmbeddedResult<()> {
        self.replay_floors.insert(*identity, floor);
        Ok(())
    }

    fn load_chunk_cursor(&self, transfer_id: u32) -> EmbeddedResult<Option<ChunkCursor>> {
        Ok(self.cursors.get(&transfer_id).cloned())
    }

    fn save_chunk_cursor(&mut self, cursor: &ChunkCursor) -> EmbeddedResult<()> {
        cursor.validate()?;
        self.cursors.insert(cursor.transfer_id, cursor.clone());
        Ok(())
    }

    fn clear_chunk_cursor(&mut self, transfer_id: u32) -> EmbeddedResult<()> {
        self.cursors.remove(&transfer_id);
        Ok(())
    }
}

fn read_u16(bytes: &[u8], idx: &mut usize) -> EmbeddedResult<u16> {
    if *idx + 2 > bytes.len() {
        return Err(EmbeddedError::StorageCorruption);
    }
    let out = u16::from_le_bytes([bytes[*idx], bytes[*idx + 1]]);
    *idx += 2;
    Ok(out)
}

fn read_u32(bytes: &[u8], idx: &mut usize) -> EmbeddedResult<u32> {
    if *idx + 4 > bytes.len() {
        return Err(EmbeddedError::StorageCorruption);
    }
    let out = u32::from_le_bytes([bytes[*idx], bytes[*idx + 1], bytes[*idx + 2], bytes[*idx + 3]]);
    *idx += 4;
    Ok(out)
}

fn read_u64(bytes: &[u8], idx: &mut usize) -> EmbeddedResult<u64> {
    if *idx + 8 > bytes.len() {
        return Err(EmbeddedError::StorageCorruption);
    }
    let out = u64::from_le_bytes([
        bytes[*idx],
        bytes[*idx + 1],
        bytes[*idx + 2],
        bytes[*idx + 3],
        bytes[*idx + 4],
        bytes[*idx + 5],
        bytes[*idx + 6],
        bytes[*idx + 7],
    ]);
    *idx += 8;
    Ok(out)
}

fn read_identity(bytes: &[u8], idx: &mut usize) -> EmbeddedResult<[u8; 32]> {
    if *idx + 32 > bytes.len() {
        return Err(EmbeddedError::StorageCorruption);
    }
    let mut out = [0_u8; 32];
    out.copy_from_slice(&bytes[*idx..*idx + 32]);
    *idx += 32;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::{EmbeddedStore, JournaledEmbeddedStore, SCHEMA_V1};
    use crate::{EmbeddedError, attachment::ChunkCursor};

    #[test]
    fn snapshot_roundtrip_preserves_state() {
        let mut store = JournaledEmbeddedStore::new();
        let identity = [7_u8; 32];
        store.save_replay_floor(&identity, 42).expect("save replay");
        store
            .save_chunk_cursor(&ChunkCursor {
                transfer_id: 99,
                total_size: 1024,
                next_offset: 256,
                expected_sequence: 4,
                chunk_size: 64,
            })
            .expect("save cursor");

        let snapshot = store.snapshot_bytes().expect("snapshot");
        let restored = JournaledEmbeddedStore::from_snapshot_bytes(&snapshot).expect("restore");

        assert_eq!(restored.load_replay_floor(&identity).expect("load replay"), 42);
        let cursor = restored.load_chunk_cursor(99).expect("load cursor").expect("cursor exists");
        assert_eq!(cursor.next_offset, 256);
        assert_eq!(cursor.expected_sequence, 4);
    }

    #[test]
    fn detects_corrupted_snapshot() {
        let mut store = JournaledEmbeddedStore::new();
        store.save_replay_floor(&[1_u8; 32], 10).expect("save");
        let mut snapshot = store.snapshot_bytes().expect("snapshot");
        snapshot[12] ^= 0x55;

        let err = JournaledEmbeddedStore::from_snapshot_bytes(&snapshot).expect_err("corruption detected");
        assert_eq!(err, EmbeddedError::StorageCorruption);
    }

    #[test]
    fn migrates_v1_snapshot_without_cursors() {
        let identity = [2_u8; 32];

        // Build a valid v1 body: schema + replay_count + identity + floor.
        let mut body = Vec::new();
        body.push(SCHEMA_V1);
        body.extend_from_slice(&1_u16.to_le_bytes());
        body.extend_from_slice(&identity);
        body.extend_from_slice(&123_u64.to_le_bytes());

        let checksum = crate::hash::digest32(&body);
        let mut snapshot = Vec::new();
        snapshot.extend_from_slice(b"RES1");
        snapshot.extend_from_slice(&(u32::try_from(body.len()).expect("len")).to_le_bytes());
        snapshot.extend_from_slice(&body);
        snapshot.extend_from_slice(&checksum);

        let restored = JournaledEmbeddedStore::from_snapshot_bytes(&snapshot).expect("restore");
        assert_eq!(restored.schema_version(), 2, "v1 migrates to v2 runtime schema");
        assert_eq!(restored.load_replay_floor(&identity).expect("replay"), 123);
        assert!(restored.load_chunk_cursor(1).expect("cursor read").is_none());
    }

    #[test]
    fn clear_cursor_persists_after_restore() {
        let mut store = JournaledEmbeddedStore::new();
        store
            .save_chunk_cursor(&ChunkCursor {
                transfer_id: 5,
                total_size: 100,
                next_offset: 60,
                expected_sequence: 3,
                chunk_size: 20,
            })
            .expect("save cursor");
        store.clear_chunk_cursor(5).expect("clear cursor");

        let snapshot = store.snapshot_bytes().expect("snapshot");
        let restored = JournaledEmbeddedStore::from_snapshot_bytes(&snapshot).expect("restore");
        assert!(restored.load_chunk_cursor(5).expect("load cursor").is_none());
    }
}
