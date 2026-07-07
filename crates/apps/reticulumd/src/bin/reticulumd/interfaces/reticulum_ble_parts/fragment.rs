use std::collections::BTreeMap;
use std::time::{Duration, Instant};

const TYPE_START: u8 = 0x01;
const TYPE_CONTINUE: u8 = 0x02;
const TYPE_END: u8 = 0x03;
const HEADER_SIZE: usize = 5;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum FragmentError {
    EmptyPacket,
    MtuTooSmall(usize),
    TooManyFragments(usize),
    TooShort(usize),
    Oversize(usize),
    InvalidType(u8),
    InvalidSequence { sequence: u16, total: u16 },
    DuplicateMismatch(u16),
}

impl std::fmt::Display for FragmentError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for FragmentError {}

pub(crate) fn fragment_packet(packet: &[u8], mtu: usize) -> Result<Vec<Vec<u8>>, FragmentError> {
    if packet.is_empty() {
        return Err(FragmentError::EmptyPacket);
    }
    if mtu <= HEADER_SIZE {
        return Err(FragmentError::MtuTooSmall(mtu));
    }
    let payload_size = mtu - HEADER_SIZE;
    let total = packet.len().div_ceil(payload_size);
    let total_u16 = u16::try_from(total).map_err(|_| FragmentError::TooManyFragments(total))?;
    let mut fragments = Vec::with_capacity(total);
    for (index, chunk) in packet.chunks(payload_size).enumerate() {
        let kind = if index == 0 {
            TYPE_START
        } else if index + 1 == total {
            TYPE_END
        } else {
            TYPE_CONTINUE
        };
        let sequence = u16::try_from(index).expect("fragment index already bounded by u16");
        let mut fragment = Vec::with_capacity(HEADER_SIZE + chunk.len());
        fragment.push(kind);
        fragment.extend_from_slice(&sequence.to_be_bytes());
        fragment.extend_from_slice(&total_u16.to_be_bytes());
        fragment.extend_from_slice(chunk);
        fragments.push(fragment);
    }
    Ok(fragments)
}

#[derive(Debug, Clone)]
struct ReassemblyState {
    total: u16,
    fragments: BTreeMap<u16, Vec<u8>>,
    start_time: Instant,
}

#[derive(Debug)]
pub(super) struct BleReassembler {
    timeout: Duration,
    buffers: BTreeMap<[u8; 16], ReassemblyState>,
}

impl BleReassembler {
    pub(super) fn new(timeout: Duration) -> Self {
        Self { timeout, buffers: BTreeMap::new() }
    }

    pub(super) fn receive_fragment(
        &mut self,
        sender: [u8; 16],
        fragment: &[u8],
        now: Instant,
    ) -> Result<Option<Vec<u8>>, FragmentError> {
        let parsed = ParsedFragment::parse(fragment)?;
        let state = self.buffers.entry(sender).or_insert_with(|| ReassemblyState {
            total: parsed.total,
            fragments: BTreeMap::new(),
            start_time: now,
        });
        if now.duration_since(state.start_time) > self.timeout || parsed.sequence == 0 {
            state.fragments.clear();
            state.total = parsed.total;
            state.start_time = now;
        }
        if state.total != parsed.total {
            self.buffers.remove(&sender);
            return Err(FragmentError::InvalidSequence {
                sequence: parsed.sequence,
                total: parsed.total,
            });
        }
        if let Some(existing) = state.fragments.get(&parsed.sequence) {
            if existing == parsed.payload {
                return Ok(None);
            }
            self.buffers.remove(&sender);
            return Err(FragmentError::DuplicateMismatch(parsed.sequence));
        }
        state.fragments.insert(parsed.sequence, parsed.payload.to_vec());
        if state.fragments.len() != parsed.total as usize {
            return Ok(None);
        }
        let mut packet = Vec::new();
        for sequence in 0..parsed.total {
            let Some(payload) = state.fragments.get(&sequence) else {
                return Ok(None);
            };
            packet.extend_from_slice(payload);
        }
        self.buffers.remove(&sender);
        Ok(Some(packet))
    }

    pub(super) fn drop_stale(&mut self, now: Instant) -> u64 {
        let before = self.buffers.len();
        self.buffers.retain(|_, state| now.duration_since(state.start_time) <= self.timeout);
        before.saturating_sub(self.buffers.len()) as u64
    }
}

struct ParsedFragment<'a> {
    sequence: u16,
    total: u16,
    payload: &'a [u8],
}

impl<'a> ParsedFragment<'a> {
    fn parse(fragment: &'a [u8]) -> Result<Self, FragmentError> {
        if fragment.len() < HEADER_SIZE {
            return Err(FragmentError::TooShort(fragment.len()));
        }
        let kind = fragment[0];
        if !matches!(kind, TYPE_START | TYPE_CONTINUE | TYPE_END) {
            return Err(FragmentError::InvalidType(kind));
        }
        let sequence = u16::from_be_bytes([fragment[1], fragment[2]]);
        let total = u16::from_be_bytes([fragment[3], fragment[4]]);
        if total == 0 || sequence >= total {
            return Err(FragmentError::InvalidSequence { sequence, total });
        }
        Ok(Self { sequence, total, payload: &fragment[HEADER_SIZE..] })
    }
}
