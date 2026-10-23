use super::*;
use crate::hash::AddressHash;
use crate::iface::{TxMessage, TxMessageType};
use crate::packet::PacketDataBuffer;
use std::pin::Pin;
use std::task::{Context, Poll};
use tokio::sync::{mpsc, oneshot};
use tokio::time::timeout;

struct ObservedWriter<W> {
    inner: W,
    barrier: Option<oneshot::Sender<()>>,
    pending_flush: bool,
}

impl<W: AsyncWrite + Unpin> AsyncWrite for ObservedWriter<W> {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<io::Result<usize>> {
        let this = self.get_mut();
        let result = Pin::new(&mut this.inner).poll_write(cx, bytes);
        if !this.pending_flush && matches!(result, Poll::Ready(Ok(n)) if n > 0) {
            if let Some(barrier) = this.barrier.take() {
                barrier.send(()).expect("write-progress observer remains owned");
            }
        }
        result
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        if this.pending_flush {
            if let Some(barrier) = this.barrier.take() {
                barrier.send(()).expect("flush-entry observer remains owned");
            }
            // Only cancellation can release this test-owned flush.
            Poll::Pending
        } else {
            Pin::new(&mut this.inner).poll_flush(cx)
        }
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().inner).poll_shutdown(cx)
    }
}

async fn writer_shutdown(keepalive: bool, pending_flush: bool) {
    let (stream, mut peer) = tokio::io::duplex(if pending_flush { 1024 } else { 1 });
    let (reader, writer) = tokio::io::split(stream);
    let (barrier_tx, barrier_rx) = oneshot::channel();
    let writer = ObservedWriter { inner: writer, barrier: Some(barrier_tx), pending_flush };
    let cancel = CancellationToken::new();
    let iface_stop = CancellationToken::new();
    let (rx_channel, _rx_messages) = mpsc::channel(1);
    let (tx_sender, tx_receiver) = mpsc::channel(1);
    let (events_tx, mut events_rx) = mpsc::channel(8);
    let mut runtime = HdlcStreamRuntime::new().with_events(events_tx);
    if keepalive {
        runtime = runtime.with_watchdog(HdlcStreamWatchdog {
            keepalive_after: Duration::ZERO,
            stale_after: Duration::from_secs(60),
            read_timeout: Duration::from_secs(120),
        });
    }
    let mut task = tokio::spawn(run_hdlc_stream_with_runtime_and_ifac(
        "blocked-output".into(),
        AddressHash::new([0x51; 16]),
        512,
        cancel.clone(),
        iface_stop.clone(),
        rx_channel,
        Arc::new(tokio::sync::Mutex::new(tx_receiver)),
        Arc::new(std::sync::RwLock::new(None)),
        Arc::new(std::sync::atomic::AtomicU64::new(0)),
        reader,
        writer,
        runtime,
    ));
    if !keepalive {
        tx_sender
            .send(TxMessage {
                tx_type: TxMessageType::Broadcast(None),
                packet: Packet {
                    data: PacketDataBuffer::new_from_slice(b"blocked-output"),
                    ..Default::default()
                },
            })
            .await
            .expect("one native packet before cancellation");
    }
    timeout(Duration::from_secs(2), barrier_rx)
        .await
        .expect("native blocked-output barrier deadline")
        .expect("native blocked-output barrier");
    assert!(!task.is_finished());
    if keepalive && !pending_flush {
        // Half-close input; keep the peer's blocked output buffer alive.
        peer.shutdown().await.expect("peer input EOF triggers internal stream stop");
    } else if pending_flush && !keepalive {
        iface_stop.cancel();
    } else {
        cancel.cancel();
    }
    timeout(Duration::from_secs(2), &mut task)
        .await
        .expect("blocked HDLC output must join without draining or dropping peer")
        .expect("both native HDLC tasks joined");
    let mut wire = Vec::new();
    timeout(Duration::from_secs(2), peer.read_to_end(&mut wire))
        .await
        .expect("joined native writer EOF deadline")
        .expect("joined native writer EOF");
    assert!(!wire.is_empty(), "the native writer made real progress before cancellation");
    while let Ok(event) = events_rx.try_recv() {
        assert!(
            !matches!(event, HdlcStreamEvent::Write { .. } | HdlcStreamEvent::Keepalive),
            "a cancelled partial/pending frame cannot report successful output"
        );
    }
}

#[tokio::test]
async fn hdlc_shutdown_joins_full_receive_handoff() {
    let address = AddressHash::new([0x52; 16]);
    let (stream, mut peer) = tokio::io::duplex(1024);
    let (reader, writer) = tokio::io::split(stream);
    let cancel = CancellationToken::new();
    let iface_stop = CancellationToken::new();
    let (rx_channel, mut rx_messages) = mpsc::channel(1);
    let sentinel = RxMessage { address, packet: Packet::default(), source: IfaceSource::None };
    rx_channel.send(sentinel.clone()).await.expect("occupy actual receive channel");
    assert_eq!(rx_channel.capacity(), 0);
    let (_tx_sender, tx_receiver) = mpsc::channel(1);
    let (events_tx, mut events_rx) = mpsc::channel(8);
    let mut task = tokio::spawn(run_hdlc_stream_with_runtime_and_ifac(
        "blocked-input".into(),
        address,
        512,
        cancel,
        iface_stop.clone(),
        rx_channel,
        Arc::new(tokio::sync::Mutex::new(tx_receiver)),
        Arc::new(std::sync::RwLock::new(None)),
        Arc::new(std::sync::atomic::AtomicU64::new(0)),
        reader,
        writer,
        HdlcStreamRuntime::new().with_events(events_tx),
    ));
    let incoming =
        Packet { data: PacketDataBuffer::new_from_slice(b"blocked-input"), ..Default::default() };
    let plain = incoming.to_bytes().expect("native packet");
    assert_eq!(Packet::from_bytes(&plain).expect("valid native input").data, incoming.data);
    let wire = Hdlc::frame(&plain).expect("HDLC");
    peer.write_all(&wire).await.expect("complete native input frame");
    let event = timeout(Duration::from_secs(2), events_rx.recv())
        .await
        .expect("native read event barrier deadline")
        .expect("native read event");
    assert_eq!(event, HdlcStreamEvent::Read { bytes: wire.len() });
    // Current-thread scheduling resumes here after RX yields at the full send.
    assert_eq!(rx_messages.len(), 1);
    assert!(!task.is_finished());
    iface_stop.cancel();
    timeout(Duration::from_secs(2), &mut task)
        .await
        .expect("blocked HDLC receive handoff must join while queue stays full")
        .expect("both native HDLC tasks joined");
    timeout(Duration::from_secs(2), peer.read_to_end(&mut Vec::new()))
        .await
        .expect("joined native receive socket EOF deadline")
        .expect("joined native receive socket EOF");
    assert_eq!(rx_messages.len(), 1, "receive queue remained full through joined shutdown");
    assert_eq!(rx_messages.try_recv().expect("original queued message"), sentinel);
    assert!(rx_messages.try_recv().is_err(), "cancelled handoff never enters admission");
}

#[tokio::test]
async fn hdlc_shutdown_joins_blocked_packet_write() {
    writer_shutdown(false, false).await;
}

#[tokio::test]
async fn hdlc_shutdown_joins_blocked_packet_flush() {
    writer_shutdown(false, true).await;
}

#[tokio::test]
async fn hdlc_shutdown_joins_blocked_keepalive_write() {
    writer_shutdown(true, false).await;
}

#[tokio::test]
async fn hdlc_shutdown_joins_blocked_keepalive_flush() {
    writer_shutdown(true, true).await;
}
