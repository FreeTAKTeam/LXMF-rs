use alloc::{collections::VecDeque, vec::Vec};
use rns_embedded_core::{
    EmbeddedError, EmbeddedResult,
    packet::{PacketFrame, decode_frame, encode_frame},
    transport::{EmbeddedTransport, LinkState, TransportCaps},
};

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub struct BleShimConfig {
    pub mtu_hint: u16,
    pub max_inbound_frames: usize,
    pub max_outbound_frames: usize,
    pub ordered_delivery: bool,
}

impl Default for BleShimConfig {
    fn default() -> Self {
        Self {
            mtu_hint: 244,
            max_inbound_frames: 16,
            max_outbound_frames: 16,
            ordered_delivery: true,
        }
    }
}

pub struct BleShimTransport {
    state: LinkState,
    caps: TransportCaps,
    max_inbound_frames: usize,
    max_outbound_frames: usize,
    inbound_frames: VecDeque<PacketFrame>,
    outbound_wire: VecDeque<Vec<u8>>,
}

impl BleShimTransport {
    pub fn new(config: BleShimConfig) -> EmbeddedResult<Self> {
        if config.mtu_hint == 0
            || config.max_inbound_frames == 0
            || config.max_outbound_frames == 0
        {
            return Err(EmbeddedError::InvalidArgument);
        }
        Ok(Self {
            state: LinkState::Down,
            caps: TransportCaps {
                mtu_hint: config.mtu_hint,
                ordered_delivery: config.ordered_delivery,
            },
            max_inbound_frames: config.max_inbound_frames,
            max_outbound_frames: config.max_outbound_frames,
            inbound_frames: VecDeque::new(),
            outbound_wire: VecDeque::new(),
        })
    }

    pub fn set_link_state(&mut self, state: LinkState) {
        self.state = state;
    }

    pub fn push_inbound_wire(&mut self, bytes: &[u8]) -> EmbeddedResult<()> {
        if self.inbound_frames.len() >= self.max_inbound_frames {
            return Err(EmbeddedError::Backpressure);
        }
        let frame = decode_frame(bytes)?;
        self.inbound_frames.push_back(frame);
        Ok(())
    }

    pub fn drain_outbound_wire(&mut self) -> Vec<Vec<u8>> {
        self.outbound_wire.drain(..).collect()
    }

    pub fn take_outbound_wire(&mut self) -> Option<Vec<u8>> {
        self.outbound_wire.pop_front()
    }

    pub fn pending_inbound_len(&self) -> usize {
        self.inbound_frames.len()
    }

    pub fn pending_outbound_len(&self) -> usize {
        self.outbound_wire.len()
    }
}

impl EmbeddedTransport for BleShimTransport {
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
        if self.outbound_wire.len() >= self.max_outbound_frames {
            return Err(EmbeddedError::Backpressure);
        }
        if frame.payload.len() > usize::from(self.caps.mtu_hint) {
            return Err(EmbeddedError::InvalidArgument);
        }
        self.outbound_wire.push_back(encode_frame(frame)?);
        Ok(())
    }

    fn poll_frame(&mut self) -> EmbeddedResult<Option<PacketFrame>> {
        if self.state != LinkState::Up {
            return Ok(None);
        }
        Ok(self.inbound_frames.pop_front())
    }
}

#[cfg(test)]
mod tests {
    use super::{BleShimConfig, BleShimTransport};
    use crate::{EmbeddedNodeRuntime, FRAME_KIND_ANNOUNCE, RuntimeConfig};
    use rns_embedded_core::{
        EmbeddedError,
        packet::{PacketFrame, decode_frame, encode_frame},
        store::{EmbeddedStore, JournaledEmbeddedStore},
        transport::{EmbeddedTransport, LinkState},
    };

    fn runtime_config() -> RuntimeConfig {
        RuntimeConfig {
            store_identity: [0x5A; 32],
            lxmf_address: [0xC3; 16],
            announce_interval_ms: 1_000,
            max_outbound_queue: 8,
            max_events: 16,
        }
    }

    fn wire_frame(kind: u8, sequence: u32, payload: &[u8]) -> Vec<u8> {
        let frame = PacketFrame::new(kind, sequence, payload.to_vec()).expect("frame");
        encode_frame(&frame).expect("encode")
    }

    #[test]
    fn shim_round_trips_wire_bytes() {
        let mut shim = BleShimTransport::new(BleShimConfig::default()).expect("shim");
        shim.set_link_state(LinkState::Up);
        let input = wire_frame(0x41, 7, b"hello-ble");
        shim.push_inbound_wire(&input).expect("push inbound");

        let frame = shim.poll_frame().expect("poll").expect("frame");
        assert_eq!(frame.kind, 0x41);
        assert_eq!(frame.sequence, 7);
        assert_eq!(frame.payload, b"hello-ble");

        shim.send_frame(&frame).expect("send frame");
        let outbound = shim.drain_outbound_wire();
        assert_eq!(outbound, vec![input]);
    }

    #[test]
    fn shim_enforces_link_and_queue_limits() {
        let mut shim = BleShimTransport::new(BleShimConfig {
            mtu_hint: 32,
            max_inbound_frames: 1,
            max_outbound_frames: 1,
            ordered_delivery: true,
        })
        .expect("shim");

        let err = shim
            .send_frame(&PacketFrame::new(0x11, 1, b"abc".to_vec()).expect("frame"))
            .expect_err("disconnected");
        assert_eq!(err, EmbeddedError::Disconnected);

        assert_eq!(shim.poll_frame().expect("poll while down"), None);

        shim.set_link_state(LinkState::Up);
        shim.push_inbound_wire(&wire_frame(0x01, 1, b"x")).expect("first inbound");
        let err = shim
            .push_inbound_wire(&wire_frame(0x01, 2, b"y"))
            .expect_err("inbound backpressure");
        assert_eq!(err, EmbeddedError::Backpressure);

        shim.send_frame(&PacketFrame::new(0x11, 1, b"abc".to_vec()).expect("frame"))
            .expect("first outbound");
        let err = shim
            .send_frame(&PacketFrame::new(0x11, 2, b"def".to_vec()).expect("frame"))
            .expect_err("outbound backpressure");
        assert_eq!(err, EmbeddedError::Backpressure);
    }

    #[test]
    fn runtime_tick_drains_announce_into_ble_wire() {
        let mut runtime = EmbeddedNodeRuntime::new(runtime_config()).expect("runtime");
        let mut store = JournaledEmbeddedStore::new();
        let mut shim = BleShimTransport::new(BleShimConfig::default()).expect("shim");
        shim.set_link_state(LinkState::Up);

        runtime.tick(0, &mut shim, &mut store).expect("tick");

        let outbound = shim.drain_outbound_wire();
        assert_eq!(outbound.len(), 1);
        let frame = decode_frame(&outbound[0]).expect("decode outbound");
        assert_eq!(frame.kind, FRAME_KIND_ANNOUNCE);
        assert_eq!(frame.sequence, 1);
        assert_eq!(frame.payload, vec![0x5A; 32]);
    }

    #[test]
    fn runtime_accepts_ble_wire_and_updates_replay_floor() {
        let mut runtime = EmbeddedNodeRuntime::new(runtime_config()).expect("runtime");
        let mut store = JournaledEmbeddedStore::new();
        let mut shim = BleShimTransport::new(BleShimConfig::default()).expect("shim");
        shim.set_link_state(LinkState::Up);

        shim.push_inbound_wire(&wire_frame(0x45, 9, b"ping"))
            .expect("push inbound");
        runtime.tick(0, &mut shim, &mut store).expect("tick");

        assert_eq!(
            store
                .load_replay_floor(&runtime_config().store_identity)
                .expect("load replay"),
            9
        );
    }
}
