use heapless::{Deque, Vec};
use lxmf_core::{Payload, WireMessage};

use crate::{
    LinkState, MiniError, MiniMessage, MiniNodeRuntime, MiniRuntimeConfig, MiniStore,
    MiniTransport, RamReplayStore,
};

const TITLE: usize = 32;
const CONTENT: usize = 128;
const FRAME: usize = 256;

type DefaultRuntime = MiniNodeRuntime<TITLE, CONTENT, FRAME, 4, 8>;
type TinyEventRuntime = MiniNodeRuntime<TITLE, CONTENT, FRAME, 4, 1>;

#[derive(Debug)]
struct MockTransport<const IN: usize, const OUT: usize> {
    state: LinkState,
    inbound: Deque<Vec<u8, FRAME>, IN>,
    sent: Deque<Vec<u8, FRAME>, OUT>,
    fail_send: Option<MiniError>,
}

impl<const IN: usize, const OUT: usize> MockTransport<IN, OUT> {
    fn new() -> Self {
        Self { state: LinkState::Up, inbound: Deque::new(), sent: Deque::new(), fail_send: None }
    }

    fn enqueue_inbound(&mut self, bytes: &[u8]) {
        let mut frame = Vec::<u8, FRAME>::new();
        frame.extend_from_slice(bytes).expect("test frame fits");
        self.inbound.push_back(frame).expect("test inbound capacity");
    }
}

impl<const IN: usize, const OUT: usize> MiniTransport for MockTransport<IN, OUT> {
    fn link_state(&self) -> LinkState {
        self.state
    }

    fn send_frame(&mut self, frame: &[u8]) -> crate::MiniResult<()> {
        if self.state != LinkState::Up {
            return Err(MiniError::Disconnected);
        }
        if let Some(error) = self.fail_send.take() {
            return Err(error);
        }
        let mut out = Vec::<u8, FRAME>::new();
        out.extend_from_slice(frame).map_err(|_| MiniError::CapacityExceeded)?;
        self.sent.push_back(out).map_err(|_| MiniError::Backpressure)
    }

    fn poll_frame(&mut self, out: &mut [u8]) -> crate::MiniResult<Option<usize>> {
        let Some(frame) = self.inbound.pop_front() else {
            return Ok(None);
        };
        if out.len() < frame.len() {
            return Err(MiniError::BufferTooSmall);
        }
        out[..frame.len()].copy_from_slice(&frame);
        Ok(Some(frame.len()))
    }
}

fn config() -> MiniRuntimeConfig {
    MiniRuntimeConfig { local_identity: [0x22; 16] }
}

fn signature() -> [u8; 64] {
    [0xA5; 64]
}

#[test]
fn mini_wire_roundtrips_without_alloc() {
    let message = MiniMessage::<TITLE, CONTENT>::new(
        [0x11; 16],
        [0x22; 16],
        signature(),
        1234.5,
        b"status",
        b"hello",
    )
    .expect("message");
    let mut out = [0u8; FRAME];
    let len = message.encode(&mut out).expect("encode");
    let decoded = MiniMessage::<TITLE, CONTENT>::decode(&out[..len]).expect("decode");

    assert_eq!(decoded.destination, [0x11; 16]);
    assert_eq!(decoded.source, [0x22; 16]);
    assert_eq!(decoded.signature, signature());
    assert_eq!(decoded.timestamp(), 1234.5);
    assert_eq!(decoded.title.as_slice(), b"status");
    assert_eq!(decoded.content.as_slice(), b"hello");
}

#[test]
fn mini_decode_accepts_lxmf_core_basic_wire_subset() {
    let payload =
        Payload::new(42.0, Some(std::vec![0xCA, 0xFE]), Some(std::vec![0xBA, 0xBE]), None, None);
    let mut wire = WireMessage::new([0x11; 16], [0x22; 16], payload);
    wire.signature = Some(signature());
    let packed = wire.pack().expect("pack");

    let decoded = MiniMessage::<TITLE, CONTENT>::decode(&packed).expect("mini decode");
    assert_eq!(decoded.destination, [0x11; 16]);
    assert_eq!(decoded.source, [0x22; 16]);
    assert_eq!(decoded.title.as_slice(), &[0xBA, 0xBE]);
    assert_eq!(decoded.content.as_slice(), &[0xCA, 0xFE]);
}

#[test]
fn lxmf_core_decodes_mini_basic_wire_subset() {
    let message = MiniMessage::<TITLE, CONTENT>::new(
        [0x11; 16],
        [0x22; 16],
        signature(),
        7.0,
        b"title",
        b"content",
    )
    .expect("message");
    let mut out = [0u8; FRAME];
    let len = message.encode(&mut out).expect("encode");

    let decoded = WireMessage::unpack(&out[..len]).expect("lxmf-core decode");
    assert_eq!(decoded.destination, [0x11; 16]);
    assert_eq!(decoded.source, [0x22; 16]);
    assert_eq!(decoded.signature, Some(signature()));
    assert_eq!(decoded.payload.title.expect("title").as_ref(), b"title");
    assert_eq!(decoded.payload.content.expect("content").as_ref(), b"content");
    assert!(decoded.payload.fields.is_none());
}

#[test]
fn runtime_queues_and_flushes_when_link_is_up() {
    let mut runtime = DefaultRuntime::new(config()).expect("runtime");
    let mut transport = MockTransport::<4, 4>::new();
    let mut store = RamReplayStore::new();
    let mut scratch = [0u8; FRAME];

    let sequence =
        runtime.queue_message([0x11; 16], b"title", b"content", signature(), 1.0).expect("queue");
    runtime.tick(&mut transport, &mut store, &mut scratch).expect("tick");

    assert_eq!(sequence, 1);
    assert_eq!(runtime.status().outbound_len, 0);
    assert_eq!(runtime.status().stats.sent, 1);
    assert_eq!(transport.sent.len(), 1);
}

#[test]
fn runtime_defers_on_transport_backpressure() {
    let mut runtime = DefaultRuntime::new(config()).expect("runtime");
    let mut transport = MockTransport::<4, 4>::new();
    let mut store = RamReplayStore::new();
    let mut scratch = [0u8; FRAME];

    runtime.queue_message([0x11; 16], b"title", b"content", signature(), 1.0).expect("queue");
    transport.fail_send = Some(MiniError::Backpressure);
    runtime.tick(&mut transport, &mut store, &mut scratch).expect("tick");

    assert_eq!(runtime.status().outbound_len, 1);
    assert_eq!(runtime.status().stats.deferred, 1);
}

#[test]
fn runtime_rejects_replayed_inbound_message() {
    let mut inbound = MiniMessage::<TITLE, CONTENT>::new(
        [0x22; 16],
        [0x33; 16],
        signature(),
        9.0,
        b"title",
        b"content",
    )
    .expect("message");
    let mut frame = [0u8; FRAME];
    let len = inbound.encode(&mut frame).expect("encode");

    let mut runtime = DefaultRuntime::new(config()).expect("runtime");
    let mut transport = MockTransport::<4, 4>::new();
    transport.enqueue_inbound(&frame[..len]);
    inbound.timestamp = 9.0f64.to_bits();
    let duplicate_len = inbound.encode(&mut frame).expect("encode duplicate");
    transport.enqueue_inbound(&frame[..duplicate_len]);
    let mut store = RamReplayStore::new();
    let mut scratch = [0u8; FRAME];

    runtime.tick(&mut transport, &mut store, &mut scratch).expect("tick");

    assert_eq!(runtime.status().stats.received, 1);
    assert_eq!(runtime.status().stats.replay_rejected, 1);
    assert_eq!(store.load_replay_floor(&[0x22; 16]).expect("floor"), 9.0f64.to_bits());
}

#[test]
fn runtime_accepts_out_of_order_messages_from_different_peers() {
    let first = MiniMessage::<TITLE, CONTENT>::new(
        [0x22; 16],
        [0x33; 16],
        signature(),
        100.0,
        b"title",
        b"content-a",
    )
    .expect("first message");
    let second = MiniMessage::<TITLE, CONTENT>::new(
        [0x22; 16],
        [0x44; 16],
        signature(),
        90.0,
        b"title",
        b"content-b",
    )
    .expect("second message");

    let mut first_frame = [0u8; FRAME];
    let first_len = first.encode(&mut first_frame).expect("encode first");
    let mut second_frame = [0u8; FRAME];
    let second_len = second.encode(&mut second_frame).expect("encode second");

    let mut runtime = DefaultRuntime::new(config()).expect("runtime");
    let mut transport = MockTransport::<4, 4>::new();
    transport.enqueue_inbound(&first_frame[..first_len]);
    transport.enqueue_inbound(&second_frame[..second_len]);
    let mut store = RamReplayStore::new();
    let mut scratch = [0u8; FRAME];

    runtime.tick(&mut transport, &mut store, &mut scratch).expect("tick");

    assert_eq!(runtime.status().stats.received, 2);
    assert_eq!(runtime.status().stats.replay_rejected, 0);
}

#[test]
fn runtime_reports_event_overflow() {
    let mut runtime = TinyEventRuntime::new(config()).expect("runtime");
    let mut transport = MockTransport::<4, 4>::new();
    let mut store = RamReplayStore::new();
    let mut scratch = [0u8; FRAME];

    runtime.tick(&mut transport, &mut store, &mut scratch).expect("bootstrap event fits");
    let err = runtime
        .queue_message([0x11; 16], b"title", b"content", signature(), 1.0)
        .expect_err("event overflow");

    assert_eq!(err, MiniError::EventOverflow);
    assert_eq!(runtime.status().stats.event_overflows, 1);
    assert_eq!(runtime.status().outbound_len, 0);
    assert_eq!(runtime.status().stats.queued, 0);

    runtime.poll_event().expect("drain bootstrap event");
    let sequence =
        runtime.queue_message([0x11; 16], b"title", b"content", signature(), 1.0).expect("queue");

    assert_eq!(sequence, 1);
    assert_eq!(runtime.status().outbound_len, 1);
    assert_eq!(runtime.status().stats.queued, 1);
}
