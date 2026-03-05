use crate::{EmbeddedError, EmbeddedResult, hash::digest32};
use alloc::vec::Vec;

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct ChunkCursor {
    pub transfer_id: u32,
    pub total_size: u32,
    pub next_offset: u32,
    pub expected_sequence: u16,
    pub chunk_size: u16,
}

impl ChunkCursor {
    pub fn validate(&self) -> EmbeddedResult<()> {
        if self.transfer_id == 0 || self.chunk_size == 0 {
            return Err(EmbeddedError::InvalidInput);
        }
        if self.next_offset > self.total_size {
            return Err(EmbeddedError::InvalidCursor);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Eq, PartialEq)]
pub struct AttachmentChunk {
    pub transfer_id: u32,
    pub sequence: u16,
    pub payload: Vec<u8>,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum ChunkApply {
    Appended,
    DuplicateAccepted,
}

#[derive(Debug, Clone)]
pub struct AttachmentReceiver {
    transfer_id: u32,
    total_size: u32,
    chunk_size: u16,
    expected_sequence: u16,
    next_offset: u32,
    bytes: Vec<u8>,
}

impl AttachmentReceiver {
    pub fn start(transfer_id: u32, total_size: u32, chunk_size: u16) -> EmbeddedResult<Self> {
        if transfer_id == 0 || chunk_size == 0 || total_size == 0 {
            return Err(EmbeddedError::InvalidArgument);
        }
        let cap = usize::try_from(total_size).map_err(|_| EmbeddedError::InvalidArgument)?;
        Ok(Self {
            transfer_id,
            total_size,
            chunk_size,
            expected_sequence: 0,
            next_offset: 0,
            bytes: Vec::with_capacity(cap.min(128 * 1024)),
        })
    }

    pub fn resume(
        transfer_id: u32,
        total_size: u32,
        chunk_size: u16,
        existing_bytes: Vec<u8>,
    ) -> EmbeddedResult<Self> {
        if transfer_id == 0 || chunk_size == 0 || total_size == 0 {
            return Err(EmbeddedError::InvalidArgument);
        }
        let existing_len = u32::try_from(existing_bytes.len()).map_err(|_| EmbeddedError::InvalidArgument)?;
        if existing_len > total_size {
            return Err(EmbeddedError::InvalidCursor);
        }
        let chunk_size_u32 = u32::from(chunk_size);
        let expected_sequence = (existing_len / chunk_size_u32)
            .try_into()
            .map_err(|_| EmbeddedError::InvalidCursor)?;
        Ok(Self {
            transfer_id,
            total_size,
            chunk_size,
            expected_sequence,
            next_offset: existing_len,
            bytes: existing_bytes,
        })
    }

    pub fn cursor(&self) -> ChunkCursor {
        ChunkCursor {
            transfer_id: self.transfer_id,
            total_size: self.total_size,
            next_offset: self.next_offset,
            expected_sequence: self.expected_sequence,
            chunk_size: self.chunk_size,
        }
    }

    pub fn apply_chunk(&mut self, offset: u32, chunk: &AttachmentChunk) -> EmbeddedResult<ChunkApply> {
        if chunk.transfer_id != self.transfer_id {
            return Err(EmbeddedError::NotFound);
        }
        if chunk.payload.is_empty() {
            return Err(EmbeddedError::InvalidArgument);
        }
        if chunk.payload.len() > usize::from(self.chunk_size) {
            return Err(EmbeddedError::InvalidArgument);
        }

        if offset != self.next_offset {
            if offset < self.next_offset {
                return self.handle_duplicate(offset, chunk);
            }
            return Err(EmbeddedError::InvalidCursor);
        }

        if chunk.sequence != self.expected_sequence {
            return Err(EmbeddedError::SeqGap);
        }

        let payload_len_u32 = u32::try_from(chunk.payload.len()).map_err(|_| EmbeddedError::InvalidArgument)?;
        let new_offset = self
            .next_offset
            .checked_add(payload_len_u32)
            .ok_or(EmbeddedError::InvalidArgument)?;
        if new_offset > self.total_size {
            return Err(EmbeddedError::InvalidArgument);
        }

        self.bytes.extend_from_slice(&chunk.payload);
        self.next_offset = new_offset;
        self.expected_sequence = self.expected_sequence.saturating_add(1);
        Ok(ChunkApply::Appended)
    }

    pub fn commit(self, expected_sha256: Option<[u8; 32]>) -> EmbeddedResult<Vec<u8>> {
        if self.next_offset != self.total_size {
            return Err(EmbeddedError::InvalidArgument);
        }
        if let Some(expected) = expected_sha256 {
            let digest = digest32(&self.bytes);
            if digest != expected {
                return Err(EmbeddedError::ChecksumMismatch);
            }
        }
        Ok(self.bytes)
    }

    fn handle_duplicate(&self, offset: u32, chunk: &AttachmentChunk) -> EmbeddedResult<ChunkApply> {
        let start = usize::try_from(offset).map_err(|_| EmbeddedError::InvalidCursor)?;
        let end = start
            .checked_add(chunk.payload.len())
            .ok_or(EmbeddedError::InvalidCursor)?;
        if end > self.bytes.len() {
            return Err(EmbeddedError::InvalidCursor);
        }
        if self.bytes[start..end] == chunk.payload {
            return Ok(ChunkApply::DuplicateAccepted);
        }
        Err(EmbeddedError::IdempotencyConflict)
    }
}

pub struct AttachmentChunker<'a> {
    transfer_id: u32,
    chunk_size: u16,
    data: &'a [u8],
    next_offset: usize,
    next_sequence: u16,
}

impl<'a> AttachmentChunker<'a> {
    pub fn new(transfer_id: u32, chunk_size: u16, data: &'a [u8]) -> EmbeddedResult<Self> {
        if transfer_id == 0 || chunk_size == 0 || data.is_empty() {
            return Err(EmbeddedError::InvalidArgument);
        }
        Ok(Self {
            transfer_id,
            chunk_size,
            data,
            next_offset: 0,
            next_sequence: 0,
        })
    }

    pub fn resume_from_offset(
        transfer_id: u32,
        chunk_size: u16,
        data: &'a [u8],
        offset: usize,
    ) -> EmbeddedResult<Self> {
        if offset > data.len() {
            return Err(EmbeddedError::InvalidCursor);
        }
        let mut chunker = Self::new(transfer_id, chunk_size, data)?;
        chunker.next_offset = offset;
        chunker.next_sequence = u16::try_from(offset / usize::from(chunk_size))
            .map_err(|_| EmbeddedError::InvalidCursor)?;
        Ok(chunker)
    }

    pub fn next_chunk(&mut self) -> Option<(u32, AttachmentChunk)> {
        if self.next_offset >= self.data.len() {
            return None;
        }
        let end = (self.next_offset + usize::from(self.chunk_size)).min(self.data.len());
        let payload = self.data[self.next_offset..end].to_vec();
        let offset = u32::try_from(self.next_offset).ok()?;
        let sequence = self.next_sequence;
        self.next_offset = end;
        self.next_sequence = self.next_sequence.saturating_add(1);
        Some((
            offset,
            AttachmentChunk {
                transfer_id: self.transfer_id,
                sequence,
                payload,
            },
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::{AttachmentChunk, AttachmentChunker, AttachmentReceiver, ChunkApply};
    use crate::{EmbeddedError, hash::digest32};

    #[test]
    fn roundtrip_append_duplicate_and_commit() {
        let payload = b"abcdefghijklmnopqrstuvwxyz";
        let mut chunker = AttachmentChunker::new(9, 8, payload).expect("chunker");
        let mut receiver = AttachmentReceiver::start(9, payload.len() as u32, 8).expect("receiver");

        let (offset0, chunk0) = chunker.next_chunk().expect("chunk0");
        assert_eq!(receiver.apply_chunk(offset0, &chunk0).expect("apply chunk0"), ChunkApply::Appended);
        assert_eq!(
            receiver.apply_chunk(offset0, &chunk0).expect("apply duplicate"),
            ChunkApply::DuplicateAccepted
        );

        while let Some((offset, chunk)) = chunker.next_chunk() {
            assert_eq!(receiver.apply_chunk(offset, &chunk).expect("append"), ChunkApply::Appended);
        }

        let checksum = digest32(payload);
        let out = receiver.commit(Some(checksum)).expect("commit");
        assert_eq!(out, payload);
    }

    #[test]
    fn rejects_seq_gap_and_offset_mismatch() {
        let mut receiver = AttachmentReceiver::start(77, 6, 3).expect("receiver");
        let bad_seq = AttachmentChunk {
            transfer_id: 77,
            sequence: 1,
            payload: b"abc".to_vec(),
        };
        let err = receiver.apply_chunk(0, &bad_seq).expect_err("seq gap");
        assert_eq!(err, EmbeddedError::SeqGap);

        let good = AttachmentChunk {
            transfer_id: 77,
            sequence: 0,
            payload: b"abc".to_vec(),
        };
        receiver.apply_chunk(0, &good).expect("first chunk");

        let next = AttachmentChunk {
            transfer_id: 77,
            sequence: 1,
            payload: b"def".to_vec(),
        };
        let err = receiver.apply_chunk(4, &next).expect_err("offset mismatch");
        assert_eq!(err, EmbeddedError::InvalidCursor);
    }

    #[test]
    fn duplicate_conflict_maps_to_idempotency_conflict() {
        let mut receiver = AttachmentReceiver::start(21, 4, 2).expect("receiver");
        let first = AttachmentChunk {
            transfer_id: 21,
            sequence: 0,
            payload: b"ab".to_vec(),
        };
        receiver.apply_chunk(0, &first).expect("first");
        let conflicting = AttachmentChunk {
            transfer_id: 21,
            sequence: 0,
            payload: b"zz".to_vec(),
        };
        let err = receiver.apply_chunk(0, &conflicting).expect_err("conflict");
        assert_eq!(err, EmbeddedError::IdempotencyConflict);
    }

    #[test]
    fn commit_checks_completeness_and_checksum() {
        let mut receiver = AttachmentReceiver::start(33, 3, 3).expect("receiver");
        let chunk = AttachmentChunk {
            transfer_id: 33,
            sequence: 0,
            payload: b"abc".to_vec(),
        };
        receiver.apply_chunk(0, &chunk).expect("chunk");
        let err = receiver.clone().commit(Some([0_u8; 32])).expect_err("bad checksum");
        assert_eq!(err, EmbeddedError::ChecksumMismatch);

        let mut incomplete = AttachmentReceiver::start(34, 4, 4).expect("incomplete");
        let c = AttachmentChunk {
            transfer_id: 34,
            sequence: 0,
            payload: b"abc".to_vec(),
        };
        incomplete.apply_chunk(0, &c).expect("partial");
        let err = incomplete.commit(None).expect_err("incomplete commit");
        assert_eq!(err, EmbeddedError::InvalidArgument);
    }

    #[test]
    fn resume_cursor_semantics() {
        let existing = b"abcdef".to_vec();
        let receiver = AttachmentReceiver::resume(55, 10, 3, existing).expect("resume");
        let cursor = receiver.cursor();
        assert_eq!(cursor.transfer_id, 55);
        assert_eq!(cursor.next_offset, 6);
        assert_eq!(cursor.expected_sequence, 2);
        assert_eq!(cursor.chunk_size, 3);
    }

    #[test]
    fn chunker_resume_from_offset() {
        let payload = b"0123456789";
        let mut chunker = AttachmentChunker::resume_from_offset(9, 4, payload, 4).expect("resume");
        let (offset, chunk) = chunker.next_chunk().expect("next");
        assert_eq!(offset, 4);
        assert_eq!(chunk.sequence, 1);
        assert_eq!(chunk.payload, b"4567");
    }
}
