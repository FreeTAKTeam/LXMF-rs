use heapless::{Deque, Vec};

use crate::{
    wire::WIRE_HEADER_LENGTH, LinkState, MiniError, MiniMessage, MiniResult, MiniStore,
    MiniTransport, HASH_LENGTH,
};

const RECENT_REPLAY_SOURCES: usize = 8;

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct MiniRuntimeConfig {
    pub local_identity: [u8; HASH_LENGTH],
}

#[derive(Debug, Clone, Copy, Eq, PartialEq, Default)]
pub struct MiniRuntimeStats {
    pub queued: u32,
    pub sent: u32,
    pub deferred: u32,
    pub received: u32,
    pub replay_rejected: u32,
    pub event_overflows: u32,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct RuntimeStatus {
    pub outbound_len: usize,
    pub event_len: usize,
    pub replay_floor: u64,
    pub stats: MiniRuntimeStats,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum MiniEvent {
    Bootstrapped { replay_floor: u64 },
    MessageQueued { sequence: u64, bytes: usize },
    FrameSent { sequence: u64, bytes: usize },
    FrameDeferred { sequence: u64, error: MiniError },
    MessageReceived { sequence: u64, source: [u8; HASH_LENGTH], bytes: usize },
    FrameRejected { sequence: u64, error: MiniError },
}

#[derive(Debug, Clone, Eq, PartialEq)]
struct OutboundFrame<const FRAME: usize> {
    sequence: u64,
    bytes: Vec<u8, FRAME>,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
struct ReplaySourceFloor {
    source: [u8; HASH_LENGTH],
    floor: u64,
}

#[derive(Debug, Clone)]
pub struct MiniNodeRuntime<
    const TITLE: usize,
    const CONTENT: usize,
    const FRAME: usize,
    const OUTBOUND: usize,
    const EVENTS: usize,
> {
    config: MiniRuntimeConfig,
    replay_floor: u64,
    next_sequence: u64,
    bootstrapped: bool,
    outbound: Deque<OutboundFrame<FRAME>, OUTBOUND>,
    events: Deque<MiniEvent, EVENTS>,
    replay_source_floors: Vec<ReplaySourceFloor, RECENT_REPLAY_SOURCES>,
    stats: MiniRuntimeStats,
}

impl<
        const TITLE: usize,
        const CONTENT: usize,
        const FRAME: usize,
        const OUTBOUND: usize,
        const EVENTS: usize,
    > MiniNodeRuntime<TITLE, CONTENT, FRAME, OUTBOUND, EVENTS>
{
    pub fn new(config: MiniRuntimeConfig) -> MiniResult<Self> {
        if config.local_identity == [0; HASH_LENGTH] || OUTBOUND == 0 || EVENTS == 0 {
            return Err(MiniError::InvalidInput);
        }
        Ok(Self {
            config,
            replay_floor: 0,
            next_sequence: 1,
            bootstrapped: false,
            outbound: Deque::new(),
            events: Deque::new(),
            replay_source_floors: Vec::new(),
            stats: MiniRuntimeStats::default(),
        })
    }

    pub fn queue_message(
        &mut self,
        destination: [u8; HASH_LENGTH],
        title: &[u8],
        content: &[u8],
        signature: [u8; 64],
        timestamp: f64,
    ) -> MiniResult<u64> {
        if self.outbound.len() >= OUTBOUND {
            return Err(MiniError::Backpressure);
        }
        self.ensure_event_capacity()?;

        let sequence = self.next_sequence;
        let message = MiniMessage::<TITLE, CONTENT>::new(
            destination,
            self.config.local_identity,
            signature,
            timestamp,
            title,
            content,
        )?;
        let mut bytes = Vec::<u8, FRAME>::new();
        bytes.resize_default(message.encoded_len()?).map_err(|_| MiniError::CapacityExceeded)?;
        let written = message.encode(&mut bytes)?;
        bytes.truncate(written);
        let queued_bytes = bytes.len();
        self.outbound
            .push_back(OutboundFrame { sequence, bytes })
            .map_err(|_| MiniError::Backpressure)?;
        self.next_sequence = self.next_sequence.saturating_add(1);
        self.stats.queued = self.stats.queued.saturating_add(1);
        self.push_event(MiniEvent::MessageQueued { sequence, bytes: queued_bytes })?;
        Ok(sequence)
    }

    pub fn tick<T: MiniTransport, S: MiniStore>(
        &mut self,
        transport: &mut T,
        store: &mut S,
        scratch: &mut [u8],
    ) -> MiniResult<()> {
        if !self.bootstrapped {
            self.replay_floor = store.load_replay_floor(&self.config.local_identity)?;
            self.bootstrapped = true;
            self.push_event(MiniEvent::Bootstrapped { replay_floor: self.replay_floor })?;
        }

        self.poll_inbound(transport, store, scratch)?;
        self.flush_outbound(transport)?;
        Ok(())
    }

    pub fn poll_event(&mut self) -> Option<MiniEvent> {
        self.events.pop_front()
    }

    pub fn status(&self) -> RuntimeStatus {
        RuntimeStatus {
            outbound_len: self.outbound.len(),
            event_len: self.events.len(),
            replay_floor: self.replay_floor,
            stats: self.stats,
        }
    }

    fn poll_inbound<T: MiniTransport, S: MiniStore>(
        &mut self,
        transport: &mut T,
        store: &mut S,
        scratch: &mut [u8],
    ) -> MiniResult<()> {
        while let Some(len) = transport.poll_frame(scratch)? {
            let message = MiniMessage::<TITLE, CONTENT>::decode(&scratch[..len])?;
            let sequence = message_sequence(&message);
            if sequence <= self.replay_floor_for(&message.source) {
                self.stats.replay_rejected = self.stats.replay_rejected.saturating_add(1);
                self.push_event(MiniEvent::FrameRejected {
                    sequence,
                    error: MiniError::ReplayRejected,
                })?;
                continue;
            }

            self.note_replay_floor(message.source, sequence);
            if sequence > self.replay_floor {
                self.replay_floor = sequence;
                // Persist a coarse high-water mark for compatibility, while
                // replay rejection in the live runtime is scoped per source.
                store.save_replay_floor(&self.config.local_identity, self.replay_floor)?;
            }
            self.stats.received = self.stats.received.saturating_add(1);
            self.push_event(MiniEvent::MessageReceived {
                sequence,
                source: message.source,
                bytes: message.content.len(),
            })?;
        }
        Ok(())
    }

    fn flush_outbound<T: MiniTransport>(&mut self, transport: &mut T) -> MiniResult<()> {
        if transport.link_state() != LinkState::Up {
            return Ok(());
        }

        while let Some(frame) = self.outbound.front() {
            let sequence = frame.sequence;
            match transport.send_frame(&frame.bytes) {
                Ok(()) => {
                    let sent = self.outbound.pop_front().ok_or(MiniError::InvalidInput)?;
                    self.stats.sent = self.stats.sent.saturating_add(1);
                    self.push_event(MiniEvent::FrameSent {
                        sequence: sent.sequence,
                        bytes: sent.bytes.len(),
                    })?;
                }
                Err(error @ (MiniError::Backpressure | MiniError::Disconnected)) => {
                    self.stats.deferred = self.stats.deferred.saturating_add(1);
                    self.push_event(MiniEvent::FrameDeferred { sequence, error })?;
                    break;
                }
                Err(error) => {
                    let dropped = self.outbound.pop_front().ok_or(MiniError::InvalidInput)?;
                    self.stats.deferred = self.stats.deferred.saturating_add(1);
                    self.push_event(MiniEvent::FrameDeferred {
                        sequence: dropped.sequence,
                        error,
                    })?;
                }
            }
        }
        Ok(())
    }

    fn ensure_event_capacity(&mut self) -> MiniResult<()> {
        if self.events.len() >= EVENTS {
            self.stats.event_overflows = self.stats.event_overflows.saturating_add(1);
            return Err(MiniError::EventOverflow);
        }
        Ok(())
    }

    fn push_event(&mut self, event: MiniEvent) -> MiniResult<()> {
        self.ensure_event_capacity()?;
        self.events.push_back(event).map_err(|_| MiniError::EventOverflow)
    }

    fn replay_floor_for(&self, source: &[u8; HASH_LENGTH]) -> u64 {
        self.replay_source_floors
            .iter()
            .find(|entry| &entry.source == source)
            .map(|entry| entry.floor)
            .unwrap_or(0)
    }

    fn note_replay_floor(&mut self, source: [u8; HASH_LENGTH], floor: u64) {
        if let Some(entry) =
            self.replay_source_floors.iter_mut().find(|entry| entry.source == source)
        {
            entry.floor = floor;
            return;
        }

        if self.replay_source_floors.len() >= RECENT_REPLAY_SOURCES {
            self.replay_source_floors.remove(0);
        }
        self.replay_source_floors
            .push(ReplaySourceFloor { source, floor })
            .expect("replay source capacity enforced before insertion");
    }
}

fn message_sequence<const TITLE: usize, const CONTENT: usize>(
    message: &MiniMessage<TITLE, CONTENT>,
) -> u64 {
    message.timestamp
}

pub const fn minimum_frame_capacity(title: usize, content: usize) -> usize {
    WIRE_HEADER_LENGTH + 1 + 9 + 5 + title + 5 + content + 1
}
