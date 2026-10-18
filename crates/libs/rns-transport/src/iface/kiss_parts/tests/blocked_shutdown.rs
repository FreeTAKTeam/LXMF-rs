use super::*;
use crate::iface::{IfaceRole, InterfaceManager, InterfaceSharedConfig, TxMessageType};
use crate::packet::{Packet, PacketDataBuffer};
use std::io;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::task::{Context, Poll};
use tokio::io::{duplex, DuplexStream, ReadBuf};
use tokio::sync::{mpsc, Notify};
use tokio::time::{sleep, timeout};

const INITIAL: [u8; 4] = [0xc0, 0x08, 0x73, 0xc0];
const MANAGEMENT: [u8; 4] = [0xc0, 0x41, 0x01, 0xc0];
const SHUTDOWN: [u8; 4] = [0xc0, 0x06, 0x00, 0xc0];

#[derive(Clone, Copy, PartialEq, Eq)]
enum Stage {
    Receive,
    InitialWrite,
    InitialFlush,
    PacketWrite,
    PacketFlush,
    ManagementWrite,
    ManagementFlush,
    ShutdownWrite,
    ShutdownFlush,
}

struct ObservedIo {
    inner: DuplexStream,
    armed: Arc<AtomicBool>,
    entered: Arc<Notify>,
    pending_flush: bool,
    fail_after_prefix: bool,
    wrote_prefix: bool,
}

impl AsyncRead for ObservedIo {
    fn poll_read(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_read(context, buffer)
    }
}

impl AsyncWrite for ObservedIo {
    fn poll_write(
        mut self: Pin<&mut Self>,
        context: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<io::Result<usize>> {
        if self.fail_after_prefix && self.wrote_prefix && !bytes.is_empty() {
            return Poll::Ready(Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "injected initial write failure",
            )));
        }
        let bytes = if self.fail_after_prefix && !bytes.is_empty() { &bytes[..1] } else { bytes };
        let result = Pin::new(&mut self.inner).poll_write(context, bytes);
        if matches!(result, Poll::Ready(Ok(length)) if length > 0) {
            self.wrote_prefix = true;
        }
        if !self.pending_flush && self.armed.load(Ordering::Acquire) && result.is_pending() {
            self.entered.notify_one();
        }
        result
    }

    fn poll_flush(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<io::Result<()>> {
        if self.pending_flush && self.armed.load(Ordering::Acquire) {
            self.entered.notify_one();
            return Poll::Pending;
        }
        Pin::new(&mut self.inner).poll_flush(context)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_shutdown(context)
    }
}

fn options(
    address: AddressHash,
    status: KissRuntimeStatusHandle,
    management: mpsc::Receiver<Vec<u8>>,
) -> KissStreamOptions {
    KissStreamOptions {
        iface_address: address,
        device: "owned-native-kiss-duplex".into(),
        mtu: 564,
        flow_control: false,
        flow_control_timeout: KISS_FLOW_CONTROL_TIMEOUT,
        read_frame_timeout: KISS_READ_FRAME_TIMEOUT,
        initial_frames: vec![INITIAL.to_vec()],
        shutdown_frames: vec![SHUTDOWN.to_vec()],
        id_beacon: None,
        activity_probe: None,
        payload_adapter: KissPayloadAdapter::Raw,
        strip_command_port_nibble: false,
        command_tx: None,
        data_rx_tx: None,
        management_frame_rx: Some(Arc::new(tokio::sync::Mutex::new(management))),
        runtime_status: Some(status),
    }
}

async fn joined(mut task: tokio::task::JoinHandle<()>) {
    match timeout(Duration::from_secs(2), &mut task).await {
        Ok(result) => result.expect("native KISS stream joined"),
        Err(error) => {
            task.abort();
            assert!(task.await.expect_err("abort stalled native stream").is_cancelled());
            panic!("native KISS must join while pressure remains held: {error}");
        }
    }
}

async fn initial_barrier(peer: &mut DuplexStream, status: &KissRuntimeStatusHandle) {
    let mut bytes = [0; INITIAL.len()];
    timeout(Duration::from_secs(2), peer.read_exact(&mut bytes))
        .await
        .expect("native initialization byte barrier deadline")
        .expect("native initialization byte barrier");
    assert_eq!(bytes, INITIAL);
    timeout(Duration::from_secs(2), async {
        while status.snapshot().init_frames_tx != 1 {
            sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("native initialization flush success barrier");
}

async fn blocked_shutdown(stage: Stage) {
    let initial = matches!(stage, Stage::InitialWrite | Stage::InitialFlush);
    let pending_flush = matches!(
        stage,
        Stage::InitialFlush | Stage::PacketFlush | Stage::ManagementFlush | Stage::ShutdownFlush
    );
    let write_pressure = matches!(
        stage,
        Stage::InitialWrite | Stage::PacketWrite | Stage::ManagementWrite | Stage::ShutdownWrite
    );
    let (stream, mut peer) = duplex(if write_pressure { 1 } else { 8192 });
    let armed = Arc::new(AtomicBool::new(initial));
    let entered = Arc::new(Notify::new());
    let stream = ObservedIo {
        inner: stream,
        armed: armed.clone(),
        entered: entered.clone(),
        pending_flush,
        fail_after_prefix: false,
        wrote_prefix: false,
    };
    let mut manager = InterfaceManager::new(1);
    let interface = KissInterface::new("software-only-kiss", 115_200);
    let status = interface.runtime_status_handle();
    let context = manager.new_context_with_role(interface, IfaceRole::Unicast);
    let address = *context.channel.address();
    assert_eq!(context.channel.ifac_default_size_bytes, 8);
    assert_eq!(manager.mtu(&address), Some(564));
    let parent_stop = context.channel.stop.clone();
    let state = context.channel.ifac_state.clone();
    let violations = context.channel.ifac_violations.clone();
    assert!(manager
        .try_set_shared_config(
            address,
            InterfaceSharedConfig {
                network_name: Some("kiss-pressure-network".into()),
                passphrase: Some("kiss-pressure-credential".into()),
                ..Default::default()
            }
        )
        .expect("prepared native eight-byte-default IFAC"));
    let packet = Packet {
        destination: AddressHash::new([0xc5; 16]),
        data: PacketDataBuffer::new_from_slice(b"held-native-kiss-packet"),
        ..Default::default()
    };
    let payload = encode_packet_ifac(&state, &packet).expect("valid authenticated packet");
    assert_eq!(payload.len(), packet.to_bytes().expect("plain bytes").len() + 8);
    assert_eq!(decode_packet_ifac(&state, &payload).expect("valid native IFAC").data, packet.data);
    let packet_wire = encode_data_frame(&payload);
    let (rx, tx) = context.channel.split();
    let sentinel = Packet {
        destination: AddressHash::new([0xc6; 16]),
        data: PacketDataBuffer::new_from_slice(b"held-native-kiss-sentinel"),
        ..Default::default()
    };
    if stage == Stage::Receive {
        rx.try_send(RxMessage { address, packet: sentinel.clone(), source: IfaceSource::None })
            .expect("fill exact native queue");
    }
    let receiver = manager.receiver();
    let (management, management_rx) = mpsc::channel(1);
    let cancel = CancellationToken::new();
    let task = tokio::spawn(run_kiss_stream_with_ifac(
        stream,
        options(address, status.clone(), management_rx),
        cancel.clone(),
        rx,
        Arc::new(tokio::sync::Mutex::new(tx)),
        state,
        violations.clone(),
    ));
    let mut expected = INITIAL.to_vec();
    let mut before_bytes = 0;
    if !initial {
        initial_barrier(&mut peer, &status).await;
        before_bytes = status.snapshot().bytes_tx;
        assert_eq!(before_bytes, INITIAL.len() as u64);
        match stage {
            Stage::Receive => {
                timeout(Duration::from_secs(2), peer.write_all(&packet_wire))
                    .await
                    .expect("native ingress write deadline")
                    .expect("native ingress bytes");
                // Each test uses one thread: there is no await between this processing
                // counter and the full native send, so the native task must yield there.
                timeout(Duration::from_secs(2), async {
                    while status.snapshot().data_frames_rx != 1 {
                        sleep(Duration::from_millis(5)).await;
                    }
                })
                .await
                .expect("valid processing reached held native receive handoff");
                assert_eq!(receiver.lock().await.len(), 1);
                assert_eq!(violations.load(Ordering::Relaxed), 0);
                expected.clear();
            }
            Stage::PacketWrite | Stage::PacketFlush => {
                armed.store(true, Ordering::Release);
                assert_eq!(
                    manager
                        .send(TxMessage {
                            tx_type: TxMessageType::Direct(address),
                            packet: packet.clone()
                        })
                        .await
                        .sent_ifaces,
                    1
                );
                expected = packet_wire;
            }
            Stage::ManagementWrite | Stage::ManagementFlush => {
                armed.store(true, Ordering::Release);
                management
                    .send(MANAGEMENT.to_vec())
                    .await
                    .expect("native management queue admission");
                expected = MANAGEMENT.to_vec();
            }
            Stage::ShutdownWrite | Stage::ShutdownFlush => {
                armed.store(true, Ordering::Release);
                peer.shutdown().await.expect("native EOF enters shutdown epilogue");
                expected = SHUTDOWN.to_vec();
            }
            Stage::InitialWrite | Stage::InitialFlush => unreachable!("initial path"),
        }
    }
    if stage != Stage::Receive {
        timeout(Duration::from_secs(2), entered.notified())
            .await
            .expect("actual Pending native write/flush barrier");
    }
    cancel.cancel();
    joined(task).await;
    assert!(!parent_stop.is_cancelled(), "generic stream does not retire its parent");
    assert!(manager.interface_hashes().contains(&address));
    assert!(management.is_closed(), "stream management receiver retired");
    let snapshot = status.snapshot();
    assert_eq!(snapshot.link_state, "closed");
    assert_eq!(snapshot.bytes_tx, before_bytes, "no cancelled output-success bytes");
    assert_eq!(snapshot.init_frames_tx, u64::from(!initial));
    assert_eq!(snapshot.packets_tx, 0);
    assert_eq!(snapshot.management_frames_tx, 0);
    assert_eq!(snapshot.shutdown_frames_tx, 0);
    assert_eq!(snapshot.packets_rx, 0, "processing is not native admission");
    if stage == Stage::Receive {
        let mut receiver = receiver.lock().await;
        let held = receiver.try_recv().expect("exact native sentinel remains");
        assert_eq!(held.packet.destination, sentinel.destination);
        assert_eq!(held.packet.data, sentinel.data);
        assert!(receiver.try_recv().is_err(), "cancelled packet was not admitted");
    }
    let mut wire = Vec::new();
    timeout(Duration::from_secs(2), peer.read_to_end(&mut wire))
        .await
        .expect("native retired stream EOF deadline")
        .expect("native retired stream EOF");
    if write_pressure {
        assert_eq!(wire, expected[..1], "partial frame retired without appended cleanup");
    } else {
        assert_eq!(wire, expected, "flush may leave complete bytes but no successful output");
    }
    manager.stop_interface(address);
    assert!(manager.interface_hashes().is_empty());
}

#[tokio::test(flavor = "current_thread")]
async fn kiss_shutdown_joins_full_native_rx() {
    blocked_shutdown(Stage::Receive).await;
}
#[tokio::test(flavor = "current_thread")]
async fn kiss_shutdown_joins_initial_write() {
    blocked_shutdown(Stage::InitialWrite).await;
}
#[tokio::test(flavor = "current_thread")]
async fn kiss_shutdown_joins_initial_flush() {
    blocked_shutdown(Stage::InitialFlush).await;
}
#[tokio::test(flavor = "current_thread")]
async fn kiss_shutdown_joins_packet_write() {
    blocked_shutdown(Stage::PacketWrite).await;
}
#[tokio::test(flavor = "current_thread")]
async fn kiss_shutdown_joins_packet_flush() {
    blocked_shutdown(Stage::PacketFlush).await;
}
#[tokio::test(flavor = "current_thread")]
async fn kiss_shutdown_joins_management_write() {
    blocked_shutdown(Stage::ManagementWrite).await;
}
#[tokio::test(flavor = "current_thread")]
async fn kiss_shutdown_joins_management_flush() {
    blocked_shutdown(Stage::ManagementFlush).await;
}
#[tokio::test(flavor = "current_thread")]
async fn kiss_shutdown_joins_shutdown_write() {
    blocked_shutdown(Stage::ShutdownWrite).await;
}
#[tokio::test(flavor = "current_thread")]
async fn kiss_shutdown_joins_shutdown_flush() {
    blocked_shutdown(Stage::ShutdownFlush).await;
}

async fn cleanup_control(initial_error: bool) {
    let (stream, mut peer) = duplex(8192);
    let stream = ObservedIo {
        inner: stream,
        armed: Arc::new(AtomicBool::new(false)),
        entered: Arc::new(Notify::new()),
        pending_flush: false,
        fail_after_prefix: initial_error,
        wrote_prefix: false,
    };
    let mut manager = InterfaceManager::new(1);
    let interface = KissInterface::new("software-only-cleanup-control", 115_200);
    let status = interface.runtime_status_handle();
    let context = manager.new_context(interface);
    let address = *context.channel.address();
    let state = context.channel.ifac_state.clone();
    let violations = context.channel.ifac_violations.clone();
    let (rx, tx) = context.channel.split();
    let (management, management_rx) = mpsc::channel(1);
    let cancel = CancellationToken::new();
    let task = tokio::spawn(run_kiss_stream_with_ifac(
        stream,
        options(address, status.clone(), management_rx),
        cancel.clone(),
        rx,
        Arc::new(tokio::sync::Mutex::new(tx)),
        state,
        violations,
    ));
    if !initial_error {
        initial_barrier(&mut peer, &status).await;
        cancel.cancel();
    }
    joined(task).await;
    let mut wire = Vec::new();
    timeout(Duration::from_secs(2), peer.read_to_end(&mut wire))
        .await
        .expect("control peer EOF deadline")
        .expect("control peer EOF");
    let snapshot = status.snapshot();
    assert_eq!(snapshot.packets_tx, 0);
    if initial_error {
        assert_eq!(wire, INITIAL[..1]);
        assert_eq!(snapshot.init_frames_tx, 0);
        assert_eq!(snapshot.shutdown_frames_tx, 0);
        assert_eq!(snapshot.bytes_tx, 0);
        assert_eq!(snapshot.link_state, "write_error");
        assert!(snapshot
            .last_error
            .as_deref()
            .is_some_and(|error| error.contains("injected initial write failure")));
    } else {
        assert_eq!(wire, SHUTDOWN);
        assert_eq!(snapshot.init_frames_tx, 1);
        assert_eq!(snapshot.shutdown_frames_tx, 1);
        assert_eq!(snapshot.bytes_tx, (INITIAL.len() + SHUTDOWN.len()) as u64);
        assert_eq!(snapshot.link_state, "closed");
        assert!(snapshot.last_error.is_none());
    }
    assert!(management.is_closed());
    manager.stop_interface(address);
}

#[tokio::test(flavor = "current_thread")]
async fn kiss_ready_shutdown_preserves_exact_cleanup_frames() {
    cleanup_control(false).await;
}
#[tokio::test(flavor = "current_thread")]
async fn kiss_initial_write_failure_never_appends_cleanup() {
    cleanup_control(true).await;
}
