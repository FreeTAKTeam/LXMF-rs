use super::*;
use crate::iface::{IfacState, InterfaceSharedConfig};
use std::sync::atomic::{AtomicBool, Ordering};
use tokio::io::DuplexStream;
use tokio::sync::{Mutex, Notify};
use tokio::time::{sleep, timeout};

const ENDPOINT: [u8; ENDPOINT_ID_LEN] = [0xd5; ENDPOINT_ID_LEN];

#[derive(Clone, Copy, PartialEq, Eq)]
enum Stage {
    Receive,
    DiscoveryWrite,
    DiscoveryFlush,
    PacketWrite,
    PacketFlush,
}

struct ObservedIo {
    inner: DuplexStream,
    armed: Arc<AtomicBool>,
    entered: Arc<Notify>,
    pending_flush: bool,
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
        let result = Pin::new(&mut self.inner).poll_write(context, bytes);
        if !self.pending_flush
            && self.armed.load(Ordering::Acquire)
            && matches!(result, Poll::Ready(Ok(length)) if length > 0)
        {
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

async fn read_frame(peer: &mut DuplexStream) -> Vec<u8> {
    timeout(Duration::from_secs(2), async {
        let mut bytes = Vec::new();
        loop {
            bytes.push(peer.read_u8().await.expect("native framed output"));
            assert!(bytes.len() <= 4096, "bounded native frame");
            if let Some((start, end)) = Hdlc::find(&bytes) {
                return decode_one_wire_frame(&bytes[start..=end]);
            }
        }
    })
    .await
    .expect("native frame deadline")
}

async fn write_frame(peer: &mut DuplexStream, bytes: &[u8]) {
    timeout(Duration::from_secs(2), peer.write_all(bytes))
        .await
        .expect("native peer input deadline")
        .expect("native peer input");
}

async fn ready_peer(
    peer: &mut DuplexStream,
    local_switch: [u8; SWITCH_ID_LEN],
    local_key: [u8; WEAVE_PUBKEY_SIZE],
    manager: &Arc<Mutex<InterfaceManager>>,
    parent: AddressHash,
) -> (AddressHash, [u8; SWITCH_ID_LEN]) {
    assert_eq!(
        read_frame(peer).await,
        weave_wdcl_frame(WDCL_BROADCAST, WDCL_T_DISCOVER, &local_switch)
    );
    let remote = PrivateIdentity::new_from_name("weave-native-pressure-peer");
    let remote_switch = switch_id_for_identity(&remote);
    let mut discovery = remote.as_identity().verifying_key_bytes().to_vec();
    discovery.extend_from_slice(&remote.sign(&local_switch).to_bytes());
    write_frame(
        peer,
        &weave_wire_frame(&weave_wdcl_frame(local_switch, WDCL_T_DISCOVER, &discovery)),
    )
    .await;
    let connect = read_frame(peer).await;
    assert_eq!(&connect[..SWITCH_ID_LEN], &remote_switch);
    assert_eq!(connect[SWITCH_ID_LEN], WDCL_T_CONNECT);
    assert_eq!(&connect[5..37], &local_key);
    assert!(crate::identity::verify(local_key, &remote_switch, &connect[37..]));
    write_frame(peer, &log_frame(local_switch, ET_PROTO_WDCL_CONNECTION, &[])).await;
    write_frame(peer, &log_frame(local_switch, ET_PROTO_WEAVE_EP_ALIVE, &ENDPOINT)).await;
    let child = timeout(Duration::from_secs(2), async {
        loop {
            if let Some(child) = manager
                .lock()
                .await
                .traffic_snapshots()
                .into_iter()
                .find(|iface| iface.parent == Some(parent))
            {
                break child.address;
            }
            sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .expect("native child registration barrier");
    (child, remote_switch)
}

async fn join(mut task: tokio::task::JoinHandle<()>) {
    match timeout(Duration::from_secs(2), &mut task).await {
        Ok(result) => result.expect("native stream joined"),
        Err(error) => {
            task.abort();
            assert!(task.await.expect_err("abort stalled stream").is_cancelled());
            panic!("Weave stream must join while native pressure remains held: {error}");
        }
    }
}

async fn blocked_shutdown(stage: Stage) {
    let initial = matches!(stage, Stage::DiscoveryWrite | Stage::DiscoveryFlush);
    let pending_flush = matches!(stage, Stage::DiscoveryFlush | Stage::PacketFlush);
    let capacity =
        if matches!(stage, Stage::DiscoveryWrite | Stage::PacketWrite) { 1 } else { 8192 };
    let (stream, mut peer) = duplex(capacity);
    let armed = Arc::new(AtomicBool::new(initial));
    let entered = Arc::new(Notify::new());
    let stream =
        ObservedIo { inner: stream, armed: armed.clone(), entered: entered.clone(), pending_flush };
    let mut manager = InterfaceManager::new(1);
    let channel = manager.new_channel_with_role(8, IfaceRole::Multicast);
    assert_eq!(channel.ifac_default_size_bytes, 16);
    let parent = channel.address;
    let parent_stop = channel.stop.clone();
    // Keep the parent live so stream cleanup must prove retirement of its children.
    let iface_stop = CancellationToken::new();
    let cancel = CancellationToken::new();
    let ifac_state: IfacState = channel.ifac_state.clone();
    let violations = channel.ifac_violations.clone();
    let policy = InterfaceSharedConfig {
        network_name: Some("weave-pressure-network".into()),
        passphrase: Some("weave-pressure-credential".into()),
        ..Default::default()
    };
    assert_eq!(policy.ifac_size, None);
    assert!(manager.try_set_shared_config(parent, policy).expect("prepared native IFAC"));
    let receiver = manager.receiver();
    let sentinel =
        RxMessage { address: parent, packet: Packet::default(), source: IfaceSource::None };
    if stage == Stage::Receive {
        channel.rx_channel.send(sentinel.clone()).await.expect("fill actual native RX queue");
        assert_eq!(channel.rx_channel.capacity(), 0);
    }
    let (rx_channel, tx_channel) = channel.split();
    let manager = Arc::new(Mutex::new(manager));
    let identity = PrivateIdentity::new_from_name("weave-native-pressure-host");
    let local_switch = switch_id_for_identity(&identity);
    let local_key = *identity.as_identity().verifying_key_bytes();
    let status = Arc::new(std::sync::Mutex::new(WeaveRuntimeStatus::new(
        "owned-duplex".into(),
        DEFAULT_BAUD_RATE,
        DEFAULT_MTU,
        local_switch,
    )));
    let task = tokio::spawn(run_weave_stream_with_ifac(
        stream,
        WeaveStreamOptions {
            parent_iface: parent,
            device: "owned-duplex".into(),
            mtu: DEFAULT_MTU,
            iface_manager: manager.clone(),
            switch_identity: identity,
            runtime_status: status.clone(),
        },
        WeaveStreamControl { cancel: cancel.clone(), iface_stop: iface_stop.clone() },
        rx_channel,
        Arc::new(Mutex::new(tx_channel)),
        unused_weave_management_rx(),
        IfacRuntime::from_parts(ifac_state.clone(), violations.clone()),
    ));
    let mut expected =
        weave_wire_frame(&weave_wdcl_frame(WDCL_BROADCAST, WDCL_T_DISCOVER, &local_switch));
    let mut before = (0, 0);
    if !initial {
        let (child, remote_switch) =
            ready_peer(&mut peer, local_switch, local_key, &manager, parent).await;
        {
            let status = status.lock().expect("native status");
            assert!(status.wdcl_connected);
            before = (status.frames_tx, status.bytes_tx);
            assert_eq!(before.0, 2);
        }
        let packet = Packet {
            destination: AddressHash::new([0xd6; 16]),
            data: PacketDataBuffer::new_from_slice(b"held-weave-native-packet"),
            ..Default::default()
        };
        let wire = encode_packet_ifac(&ifac_state, &packet).expect("native authenticated packet");
        assert_eq!(wire.len(), packet.to_bytes().expect("plain packet").len() + 16);
        assert_eq!(decode_packet_ifac(&ifac_state, &wire).expect("valid IFAC").data, packet.data);
        if stage == Stage::Receive {
            let mut payload = wire;
            payload.extend_from_slice(&ENDPOINT);
            write_frame(
                &mut peer,
                &weave_wire_frame(&weave_wdcl_frame(local_switch, WDCL_T_ENDPOINT_PKT, &payload)),
            )
            .await;
            timeout(Duration::from_secs(2), async {
                loop {
                    if status.lock().expect("native status").to_json()["endpoints"][0]["packets_rx"]
                        == 1
                    {
                        break;
                    }
                    sleep(Duration::from_millis(5)).await;
                }
            })
            .await
            .expect("authenticated processing reached the held native RX handoff");
            assert_eq!(receiver.lock().await.len(), 1);
            assert_eq!(violations.load(Ordering::Relaxed), 0);
            expected.clear();
        } else {
            expected =
                weave_wire_frame(&weave_endpoint_command_frame(remote_switch, ENDPOINT, &wire));
            armed.store(true, Ordering::Release);
            assert_eq!(
                manager
                    .lock()
                    .await
                    .send(TxMessage { tx_type: TxMessageType::Direct(child), packet })
                    .await
                    .sent_ifaces,
                1
            );
        }
    }
    if stage != Stage::Receive {
        timeout(Duration::from_secs(2), entered.notified())
            .await
            .expect("actual pending native write/flush entry");
    }
    if matches!(stage, Stage::DiscoveryWrite | Stage::PacketFlush) {
        iface_stop.cancel();
        assert!(!cancel.is_cancelled());
    } else {
        cancel.cancel();
        assert!(!iface_stop.is_cancelled());
    }
    join(task).await;
    {
        let status = status.lock().expect("native status");
        assert_eq!(
            (status.frames_tx, status.bytes_tx),
            before,
            "cancelled output has no published success"
        );
    }
    assert_eq!(status.lock().expect("native status").link_state, WeaveLinkState::Closed);
    assert!(!status.lock().expect("native status").wdcl_connected);
    assert!(!parent_stop.is_cancelled(), "stream cleanup does not retire the parent");
    assert_eq!(
        manager.lock().await.interface_hashes(),
        std::collections::HashSet::from([parent]),
        "children retire before the native parent"
    );
    let mut observed = Vec::new();
    timeout(Duration::from_secs(2), peer.read_to_end(&mut observed))
        .await
        .expect("retired native peer EOF deadline")
        .expect("retired native peer EOF");
    if matches!(stage, Stage::DiscoveryWrite | Stage::PacketWrite) {
        assert_eq!(observed, expected[..1]);
        assert!(Hdlc::find(&observed).is_none(), "partial frame never completes on retired stream");
    } else {
        assert_eq!(observed, expected);
    }
    if stage == Stage::Receive {
        assert_eq!(receiver.lock().await.len(), 1, "RX remains full through join/EOF");
        assert_eq!(receiver.lock().await.try_recv().expect("original sentinel retained"), sentinel);
    }
    assert!(receiver.lock().await.try_recv().is_err(), "cancelled ingress is not admitted");
    assert!(manager.lock().await.stop_interface(parent));
}

#[tokio::test]
async fn weave_shutdown_joins_full_native_rx() {
    blocked_shutdown(Stage::Receive).await;
}
#[tokio::test]
async fn weave_shutdown_joins_discovery_write() {
    blocked_shutdown(Stage::DiscoveryWrite).await;
}
#[tokio::test]
async fn weave_shutdown_joins_discovery_flush() {
    blocked_shutdown(Stage::DiscoveryFlush).await;
}
#[tokio::test]
async fn weave_shutdown_joins_packet_write() {
    blocked_shutdown(Stage::PacketWrite).await;
}
#[tokio::test]
async fn weave_shutdown_joins_packet_flush() {
    blocked_shutdown(Stage::PacketFlush).await;
}
