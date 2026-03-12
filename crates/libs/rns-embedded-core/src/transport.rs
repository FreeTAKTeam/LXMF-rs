use crate::{packet::PacketFrame, EmbeddedError, EmbeddedResult};
use alloc::{collections::VecDeque, vec::Vec};

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum LinkState {
    Down,
    Connecting,
    Up,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct TransportCaps {
    pub mtu_hint: u16,
    pub ordered_delivery: bool,
}

pub trait EmbeddedTransport {
    fn link_state(&self) -> LinkState;
    fn capabilities(&self) -> TransportCaps;
    fn send_frame(&mut self, frame: &PacketFrame) -> EmbeddedResult<()>;
    fn poll_frame(&mut self) -> EmbeddedResult<Option<PacketFrame>>;
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum FaultMode {
    None,
    DropEvery(usize),
    DuplicateEvery(usize),
    ReorderPairEvery(usize),
    BackpressureEvery(usize),
}

pub struct FaultInjectingMockTransport {
    caps: TransportCaps,
    state: LinkState,
    sent_count: usize,
    recv_count: usize,
    outbound: VecDeque<PacketFrame>,
    inbound: VecDeque<PacketFrame>,
    faults: Vec<FaultMode>,
}

impl FaultInjectingMockTransport {
    pub fn new(caps: TransportCaps) -> Self {
        Self {
            caps,
            state: LinkState::Up,
            sent_count: 0,
            recv_count: 0,
            outbound: VecDeque::new(),
            inbound: VecDeque::new(),
            faults: Vec::new(),
        }
    }

    pub fn with_faults(mut self, faults: Vec<FaultMode>) -> Self {
        self.faults = faults;
        self
    }

    pub fn set_state(&mut self, state: LinkState) {
        self.state = state;
    }

    pub fn enqueue_inbound(&mut self, frames: impl IntoIterator<Item = PacketFrame>) {
        self.inbound.extend(frames);
    }

    pub fn drain_outbound(&mut self) -> Vec<PacketFrame> {
        self.outbound.drain(..).collect()
    }

    fn should_trigger(step: usize, n: usize) -> bool {
        n > 0 && step > 0 && step % n == 0
    }
}

impl EmbeddedTransport for FaultInjectingMockTransport {
    fn link_state(&self) -> LinkState {
        self.state
    }

    fn capabilities(&self) -> TransportCaps {
        self.caps
    }

    fn send_frame(&mut self, frame: &PacketFrame) -> EmbeddedResult<()> {
        if self.state != LinkState::Up {
            return Err(EmbeddedError::Disconnected);
        }
        if frame.payload.len() > usize::from(self.caps.mtu_hint) {
            return Err(EmbeddedError::InvalidArgument);
        }

        self.sent_count = self.sent_count.saturating_add(1);
        for mode in &self.faults {
            match *mode {
                FaultMode::BackpressureEvery(n) if Self::should_trigger(self.sent_count, n) => {
                    return Err(EmbeddedError::Backpressure);
                }
                FaultMode::DropEvery(n) if Self::should_trigger(self.sent_count, n) => {
                    return Ok(());
                }
                FaultMode::DuplicateEvery(n) if Self::should_trigger(self.sent_count, n) => {
                    self.outbound.push_back(frame.clone());
                    self.outbound.push_back(frame.clone());
                    return Ok(());
                }
                FaultMode::ReorderPairEvery(n) if Self::should_trigger(self.sent_count, n) => {
                    if let Some(prev) = self.outbound.pop_back() {
                        self.outbound.push_back(frame.clone());
                        self.outbound.push_back(prev);
                    } else {
                        self.outbound.push_back(frame.clone());
                    }
                    return Ok(());
                }
                _ => {}
            }
        }

        self.outbound.push_back(frame.clone());
        Ok(())
    }

    fn poll_frame(&mut self) -> EmbeddedResult<Option<PacketFrame>> {
        if self.state == LinkState::Down {
            return Err(EmbeddedError::Disconnected);
        }
        self.recv_count = self.recv_count.saturating_add(1);
        Ok(self.inbound.pop_front())
    }
}

#[cfg(test)]
mod tests {
    use super::{
        EmbeddedTransport, FaultInjectingMockTransport, FaultMode, LinkState, TransportCaps,
    };
    use crate::{packet::PacketFrame, EmbeddedError};

    fn frame(kind: u8, seq: u32, payload: &[u8]) -> PacketFrame {
        PacketFrame::new(kind, seq, payload.to_vec()).expect("frame")
    }

    #[test]
    fn drop_every_n_discards_target_frame() {
        let caps = TransportCaps { mtu_hint: 64, ordered_delivery: false };
        let mut tx =
            FaultInjectingMockTransport::new(caps).with_faults(vec![FaultMode::DropEvery(2)]);
        tx.send_frame(&frame(1, 1, b"aa")).expect("send1");
        tx.send_frame(&frame(1, 2, b"bb")).expect("send2");
        tx.send_frame(&frame(1, 3, b"cc")).expect("send3");

        let out = tx.drain_outbound();
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].sequence, 1);
        assert_eq!(out[1].sequence, 3);
    }

    #[test]
    fn duplicate_every_n_emits_duplicate_frame() {
        let caps = TransportCaps { mtu_hint: 64, ordered_delivery: false };
        let mut tx =
            FaultInjectingMockTransport::new(caps).with_faults(vec![FaultMode::DuplicateEvery(2)]);
        tx.send_frame(&frame(1, 1, b"aa")).expect("send1");
        tx.send_frame(&frame(1, 2, b"bb")).expect("send2");

        let out = tx.drain_outbound();
        assert_eq!(out.len(), 3);
        assert_eq!(out[0].sequence, 1);
        assert_eq!(out[1].sequence, 2);
        assert_eq!(out[2].sequence, 2);
    }

    #[test]
    fn reorder_pair_swaps_adjacent_frames_on_trigger() {
        let caps = TransportCaps { mtu_hint: 64, ordered_delivery: false };
        let mut tx = FaultInjectingMockTransport::new(caps)
            .with_faults(vec![FaultMode::ReorderPairEvery(2)]);
        tx.send_frame(&frame(1, 1, b"aa")).expect("send1");
        tx.send_frame(&frame(1, 2, b"bb")).expect("send2");

        let out = tx.drain_outbound();
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].sequence, 2);
        assert_eq!(out[1].sequence, 1);
    }

    #[test]
    fn backpressure_every_n_maps_to_error() {
        let caps = TransportCaps { mtu_hint: 64, ordered_delivery: false };
        let mut tx = FaultInjectingMockTransport::new(caps)
            .with_faults(vec![FaultMode::BackpressureEvery(2)]);
        tx.send_frame(&frame(1, 1, b"aa")).expect("send1");
        let err = tx.send_frame(&frame(1, 2, b"bb")).expect_err("backpressure");
        assert_eq!(err, EmbeddedError::Backpressure);
    }

    #[test]
    fn disconnected_state_rejects_send_and_poll() {
        let caps = TransportCaps { mtu_hint: 64, ordered_delivery: false };
        let mut tx = FaultInjectingMockTransport::new(caps);
        tx.set_state(LinkState::Down);
        let err = tx.send_frame(&frame(1, 1, b"aa")).expect_err("send disconnected");
        assert_eq!(err, EmbeddedError::Disconnected);

        let err = tx.poll_frame().expect_err("poll disconnected");
        assert_eq!(err, EmbeddedError::Disconnected);
    }

    #[test]
    fn mtu_violation_maps_to_invalid_argument() {
        let caps = TransportCaps { mtu_hint: 4, ordered_delivery: false };
        let mut tx = FaultInjectingMockTransport::new(caps);
        let err = tx.send_frame(&frame(1, 1, b"payload-too-large")).expect_err("mtu violation");
        assert_eq!(err, EmbeddedError::InvalidArgument);
    }
}
