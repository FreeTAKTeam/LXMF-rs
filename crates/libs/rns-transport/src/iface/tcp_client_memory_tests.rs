use std::collections::VecDeque;
use std::io;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};
use std::time::Duration;

use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
use tokio_util::sync::CancellationToken;

use super::{
    run_hdlc_stream_with_runtime, tcp_read_buffer_len, tcp_wire_buffer_capacity, HdlcStreamEvent,
    HdlcStreamRuntime, HdlcStreamWatchdog, TcpClient, TcpRxBuffers, TcpTxBuffers,
    HDLC_STREAM_EVENT_CHANNEL_CAPACITY,
};
use crate::buffer::{InputBuffer, OutputBuffer};
use crate::hash::AddressHash;
use crate::iface::hdlc::Hdlc;
use crate::iface::{RxMessage, TxMessage, TxMessageType};
use crate::packet::{Packet, PacketDataBuffer};

struct ChunkedReader {
    chunks: VecDeque<Vec<u8>>,
}

impl ChunkedReader {
    fn new(chunks: impl IntoIterator<Item = Vec<u8>>) -> Self {
        Self { chunks: chunks.into_iter().collect() }
    }
}

impl AsyncRead for ChunkedReader {
    fn poll_read(
        mut self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        if let Some(chunk) = self.chunks.pop_front() {
            buffer.put_slice(&chunk);
        }
        Poll::Ready(Ok(()))
    }
}

struct PendingReader;

impl AsyncRead for PendingReader {
    fn poll_read(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        _buffer: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Poll::Pending
    }
}

struct BrokenWriter;

impl AsyncWrite for BrokenWriter {
    fn poll_write(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        _buffer: &[u8],
    ) -> Poll<io::Result<usize>> {
        Poll::Ready(Err(io::Error::new(io::ErrorKind::BrokenPipe, "simulated disconnect")))
    }

    fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }

    fn poll_shutdown(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}

fn frame_for_packet(packet: &Packet) -> Vec<u8> {
    let raw = packet.to_bytes().expect("serialize packet");
    let mut wire = vec![0_u8; tcp_wire_buffer_capacity(raw.len())];
    let used = {
        let mut output = OutputBuffer::new(wire.as_mut_slice());
        Hdlc::encode(&raw, &mut output).expect("encode packet");
        output.offset()
    };
    wire.truncate(used);
    wire
}

async fn decode_chunks(chunks: impl IntoIterator<Item = Vec<u8>>, mtu: usize) -> Vec<RxMessage> {
    let cancel = CancellationToken::new();
    let iface_stop = CancellationToken::new();
    let (rx_sender, mut rx_receiver) = tokio::sync::mpsc::channel(8);
    let (_tx_sender, tx_receiver) = tokio::sync::mpsc::channel(1);
    run_hdlc_stream_with_runtime(
        "memory-test".to_string(),
        AddressHash::new([0x51; 16]),
        mtu,
        cancel,
        iface_stop,
        rx_sender,
        Arc::new(tokio::sync::Mutex::new(tx_receiver)),
        ChunkedReader::new(chunks),
        tokio::io::sink(),
        HdlcStreamRuntime::new(),
    )
    .await;

    let mut messages = Vec::new();
    while let Ok(message) = rx_receiver.try_recv() {
        messages.push(message);
    }
    messages
}

#[test]
fn idle_stream_buffers_are_small_and_tx_is_lazy() {
    let mtu = TcpClient::DEFAULT_MTU;
    let rx = TcpRxBuffers::new(mtu);
    let tx = TcpTxBuffers::default();

    assert_eq!(tcp_read_buffer_len(mtu), 32 * 1024);
    assert_eq!(rx.tcp.len(), 32 * 1024);
    assert!(rx.frame.capacity() <= 4 * 1024);
    assert_eq!(rx.decoded.capacity(), 0);
    assert_eq!(tx.raw.capacity(), 0);
    assert_eq!(tx.wire.capacity(), 0);
    assert!(
        rx.tcp.capacity() + rx.frame.capacity() < 64 * 1024,
        "idle connection must not reserve memory proportional to mtu * 16"
    );
}

#[test]
fn tx_buffers_are_reused_and_stale_bytes_are_not_exposed() {
    let mtu = 4096;
    let mut buffers = TcpTxBuffers::default();
    let first =
        Packet { data: PacketDataBuffer::new_from_slice(&vec![0x7e; 512]), ..Default::default() };

    let (first_raw_len, first_wire_len, raw_ptr, wire_ptr, raw_capacity, wire_capacity) = {
        let (raw_len, wire) = buffers.encode_packet(&first, mtu).expect("encode first packet");
        (
            raw_len,
            wire.len(),
            buffers.raw.as_ptr(),
            buffers.wire.as_ptr(),
            buffers.raw.capacity(),
            buffers.wire.capacity(),
        )
    };
    assert!(first_wire_len > first_raw_len);

    let second =
        Packet { data: PacketDataBuffer::new_from_slice(&vec![0x42; 512]), ..Default::default() };
    let second_wire = {
        let (_, wire) = buffers.encode_packet(&second, mtu).expect("encode second packet");
        wire.to_vec()
    };
    assert_eq!(buffers.raw.as_ptr(), raw_ptr);
    assert_eq!(buffers.wire.as_ptr(), wire_ptr);
    assert_eq!(buffers.raw.capacity(), raw_capacity);
    assert_eq!(buffers.wire.capacity(), wire_capacity);

    let mut decoded = vec![0_u8; mtu];
    let decoded_len = {
        let mut output = OutputBuffer::new(decoded.as_mut_slice());
        Hdlc::decode(&second_wire, &mut output).expect("decode second packet");
        output.offset()
    };
    let expected = second.to_bytes().expect("serialize second packet");
    assert_eq!(&decoded[..decoded_len], expected.as_slice());
}

#[tokio::test]
async fn valid_frame_split_across_single_byte_reads_is_reassembled() {
    let packet = Packet {
        data: PacketDataBuffer::new_from_slice(b"split-across-many-reads"),
        ..Default::default()
    };
    let frame = frame_for_packet(&packet);
    let messages = decode_chunks(frame.into_iter().map(|byte| vec![byte]), 4096).await;

    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].packet, packet);
}

#[tokio::test]
async fn multiple_frames_in_one_read_are_all_delivered() {
    let first = Packet { data: PacketDataBuffer::new_from_slice(b"first"), ..Default::default() };
    let second = Packet { data: PacketDataBuffer::new_from_slice(b"second"), ..Default::default() };
    let mut combined = frame_for_packet(&first);
    combined.extend_from_slice(&frame_for_packet(&second));

    let messages = decode_chunks([combined], 4096).await;
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[0].packet, first);
    assert_eq!(messages[1].packet, second);
}

#[tokio::test]
async fn maximum_mtu_frame_is_delivered_across_fixed_size_reads() {
    let mtu = TcpClient::DEFAULT_MTU;
    let base_len = Packet::default().serialized_len().expect("default packet length");
    let packet = Packet {
        data: PacketDataBuffer::new_from_slice(&vec![0x55; mtu - base_len]),
        ..Default::default()
    };
    assert_eq!(packet.serialized_len().expect("maximum packet length"), mtu);
    let frame = frame_for_packet(&packet);
    let chunks = frame.chunks(997).map(<[u8]>::to_vec).collect::<Vec<_>>();

    let messages = decode_chunks(chunks, mtu).await;
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].packet.data.len(), mtu - base_len);
    assert_eq!(messages[0].packet, packet);
}

#[tokio::test]
async fn maximum_escaping_frame_is_delivered() {
    let mtu = 4096;
    let base_len = Packet::default().serialized_len().expect("default packet length");
    let payload = (0..mtu - base_len)
        .map(|index| if index % 2 == 0 { 0x7e } else { 0x7d })
        .collect::<Vec<_>>();
    let packet = Packet { data: PacketDataBuffer::new_from_slice(&payload), ..Default::default() };
    let frame = frame_for_packet(&packet);
    assert!(frame.len() > mtu * 2 - 64, "test must exercise near-maximum expansion");

    let messages = decode_chunks(frame.chunks(31).map(<[u8]>::to_vec), mtu).await;
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].packet, packet);
}

#[tokio::test]
async fn tx_disconnect_stops_both_stream_tasks() {
    let cancel = CancellationToken::new();
    let iface_stop = CancellationToken::new();
    let (rx_sender, _rx_receiver) = tokio::sync::mpsc::channel(1);
    let (tx_sender, tx_receiver) = tokio::sync::mpsc::channel(1);
    let (event_sender, mut event_receiver) =
        tokio::sync::mpsc::channel(HDLC_STREAM_EVENT_CHANNEL_CAPACITY);
    tx_sender
        .send(TxMessage { tx_type: TxMessageType::Broadcast(None), packet: Packet::default() })
        .await
        .expect("queue packet");

    tokio::time::timeout(
        Duration::from_secs(1),
        run_hdlc_stream_with_runtime(
            "broken-writer".to_string(),
            AddressHash::new([0x52; 16]),
            4096,
            cancel,
            iface_stop,
            rx_sender,
            Arc::new(tokio::sync::Mutex::new(tx_receiver)),
            PendingReader,
            BrokenWriter,
            HdlcStreamRuntime::new().with_events(event_sender),
        ),
    )
    .await
    .expect("stream must stop after TX disconnect");

    assert!(
        std::iter::from_fn(|| event_receiver.try_recv().ok())
            .any(|event| matches!(event, HdlcStreamEvent::Error { .. })),
        "TX disconnect must emit an error event"
    );
}

#[tokio::test]
async fn watchdog_stream_cancels_promptly_while_tx_is_idle() {
    let cancel = CancellationToken::new();
    let iface_stop = CancellationToken::new();
    let (rx_sender, _rx_receiver) = tokio::sync::mpsc::channel(1);
    let (_tx_sender, tx_receiver) = tokio::sync::mpsc::channel(1);
    let task = tokio::spawn(run_hdlc_stream_with_runtime(
        "idle-watchdog".to_string(),
        AddressHash::new([0x53; 16]),
        TcpClient::DEFAULT_MTU,
        cancel.clone(),
        iface_stop,
        rx_sender,
        Arc::new(tokio::sync::Mutex::new(tx_receiver)),
        PendingReader,
        tokio::io::sink(),
        HdlcStreamRuntime::new().with_watchdog(HdlcStreamWatchdog {
            keepalive_after: Duration::from_secs(60),
            stale_after: Duration::from_secs(120),
            read_timeout: Duration::from_secs(180),
        }),
    ));

    tokio::task::yield_now().await;
    cancel.cancel();
    tokio::time::timeout(Duration::from_secs(1), task)
        .await
        .expect("idle watchdog stream must observe cancellation")
        .expect("stream task must not panic");
}

#[test]
fn second_tx_frame_decodes_without_bytes_from_the_first() {
    let mut buffers = TcpTxBuffers::default();
    let large =
        Packet { data: PacketDataBuffer::new_from_slice(&vec![0x7e; 2048]), ..Default::default() };
    let _ = buffers.encode_packet(&large, 4096).expect("encode large frame");

    let small = Packet { data: PacketDataBuffer::new_from_slice(b"small"), ..Default::default() };
    let (_, wire) = buffers.encode_packet(&small, 4096).expect("encode small frame");
    let mut raw = vec![0_u8; 4096];
    let raw_len = {
        let mut output = OutputBuffer::new(raw.as_mut_slice());
        Hdlc::decode(wire, &mut output).expect("decode reused frame");
        output.offset()
    };
    let decoded = Packet::deserialize(&mut InputBuffer::new(&raw[..raw_len])).expect("packet");
    assert_eq!(decoded, small);
}
