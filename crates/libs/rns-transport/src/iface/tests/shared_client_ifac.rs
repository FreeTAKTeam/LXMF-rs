use super::*;
use crate::buffer::OutputBuffer;
use crate::iface::hdlc::Hdlc;
use crate::iface::tcp_server::TcpServer;
use crate::packet::PacketDataBuffer;
use std::future::Future;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::time::timeout;

fn policy(secret: &str) -> InterfaceSharedConfig {
    InterfaceSharedConfig {
        network_name: Some("configured-local".to_string()),
        passphrase: Some(secret.to_string()),
        ..Default::default()
    }
}

fn state(config: &InterfaceSharedConfig) -> IfacState {
    Arc::new(std::sync::RwLock::new(config.ifac_context().expect("IFAC context")))
}

async fn exchange<S: AsyncRead + AsyncWrite + Unpin>(
    manager: &Arc<tokio::sync::Mutex<InterfaceManager>>,
    peer: &mut S,
    child: AddressHash,
    plaintext: bool,
    config: &InterfaceSharedConfig,
    rejected: &InterfaceSharedConfig,
    label: &[u8],
) {
    let packet = Packet { data: PacketDataBuffer::new_from_slice(label), ..Default::default() };
    let raw = packet.to_bytes().expect("packet bytes");
    // Stream ordering makes the accepted frame a barrier for earlier rejects.
    let mut rejected_frames = vec![encode_ifac(&state(rejected), &raw).expect("wrong-key frame")];
    if !plaintext {
        rejected_frames.push(raw.clone());
    }
    for frame in &rejected_frames {
        peer.write_all(&Hdlc::frame(frame).expect("rejected HDLC")).await.expect("negative frame");
    }
    let accepted = if plaintext {
        raw
    } else {
        encode_ifac(&state(config), &raw).expect("authenticated frame")
    };
    peer.write_all(&Hdlc::frame(&accepted).expect("accepted HDLC")).await.expect("positive frame");
    let receiver = manager.lock().await.receiver();
    let received = timeout(Duration::from_secs(2), async { receiver.lock().await.recv().await })
        .await
        .expect("accepted ingress deadline")
        .expect("accepted ingress");
    assert_eq!(received.address, child);
    assert_eq!(received.packet.data.as_slice(), label);
    assert_eq!(received.packet.ifac.is_some(), !plaintext);
    assert!(receiver.lock().await.try_recv().is_err(), "rejected frames never reach receiver");
    let trace = manager
        .lock()
        .await
        .send(TxMessage { tx_type: TxMessageType::Direct(child), packet })
        .await;
    assert_eq!(trace.sent_ifaces, 1);
    let mut framed = Vec::new();
    let mut chunk = [0; 4096];
    let (start, end) = timeout(Duration::from_secs(2), async {
        loop {
            let size = peer.read(&mut chunk).await.expect("egress read");
            assert!(size > 0, "peer remains open");
            framed.extend_from_slice(&chunk[..size]);
            if let Some(frame) = Hdlc::find(&framed) {
                break frame;
            }
        }
    })
    .await
    .expect("egress deadline");
    let mut raw = vec![0; end - start];
    let length =
        Hdlc::decode(&framed[start..=end], &mut OutputBuffer::new(&mut raw)).expect("egress HDLC");
    let decoded = if plaintext {
        assert_eq!(raw[0] & 0x80, 0);
        Packet::from_bytes(&raw[..length]).expect("plaintext egress")
    } else {
        assert!(decode_packet_ifac(&state(rejected), &raw[..length]).is_err());
        decode_packet_ifac(&state(config), &raw[..length]).expect("authenticated egress")
    };
    assert_eq!(decoded.data.as_slice(), label);
}

async fn child_after_connect(
    manager: &Arc<tokio::sync::Mutex<InterfaceManager>>,
    previous: Option<AddressHash>,
) -> AddressHash {
    timeout(Duration::from_secs(2), async {
        loop {
            if let Some(child) = manager
                .lock()
                .await
                .local_client_interfaces()
                .into_iter()
                .find(|child| Some(*child) != previous)
            {
                break child;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("accepted child registration")
}

async fn exercise<S, C, F>(
    manager: Arc<tokio::sync::Mutex<InterfaceManager>>,
    parent: AddressHash,
    mut first: S,
    connect: C,
    plaintext: bool,
) where
    S: AsyncRead + AsyncWrite + Unpin,
    C: FnOnce() -> F,
    F: Future<Output = S>,
{
    let original = policy("original");
    let rotated = policy("rotated");
    let wrong = policy("wrong");
    let child = child_after_connect(&manager, None).await;
    exchange(&manager, &mut first, child, plaintext, &original, &wrong, b"first ingress/egress")
        .await;
    assert!(manager
        .lock()
        .await
        .try_set_shared_config(parent, rotated.clone())
        .expect("rotate policy"));
    exchange(
        &manager,
        &mut first,
        child,
        plaintext,
        &rotated,
        &original,
        b"existing child after rotation",
    )
    .await;
    let mut second = connect().await;
    let second_child = child_after_connect(&manager, Some(child)).await;
    exchange(
        &manager,
        &mut second,
        second_child,
        plaintext,
        &rotated,
        &original,
        b"new child after rotation",
    )
    .await;
    let mut manager = manager.lock().await;
    for (address, phases) in [(child, 2), (second_child, 1)] {
        let violations = manager
            .traffic_snapshots()
            .into_iter()
            .find(|entry| entry.address == address)
            .expect("child traffic")
            .ifac_violations;
        assert_eq!(violations, phases * if plaintext { 1 } else { 2 });
    }
    let tasks = manager.take_worker_tree(parent);
    assert_eq!(tasks.len(), 3, "listener and both accepted children remain owned");
    assert!(manager.stop_interface(parent));
    assert!(manager.interface_hashes().is_empty());
    drop(manager);
    for worker in tasks {
        timeout(Duration::from_secs(2), worker).await.expect("worker tree joins").expect("worker");
    }
}

async fn tcp_case(plaintext: bool) {
    let probe = TcpListener::bind("127.0.0.1:0").await.expect("reserve address");
    let address = probe.local_addr().expect("address");
    drop(probe);
    let manager = Arc::new(tokio::sync::Mutex::new(InterfaceManager::new(16)));
    let (server, bound) = TcpServer::new(address.to_string(), manager.clone())
        .with_plaintext_shared_clients(plaintext)
        .with_startup_result();
    let parent = manager
        .lock()
        .await
        .try_spawn_as_with_mode(
            server,
            TcpServer::spawn,
            IfaceRole::Unicast,
            InterfaceMode::Full,
            |manager, parent| -> Result<(), crate::error::RnsError> {
                assert!(manager.try_set_shared_config(parent, policy("original"))?);
                assert!(manager.set_shared_instance(parent, true));
                Ok(())
            },
        )
        .expect("prepared TCP server");
    timeout(Duration::from_secs(2), bound)
        .await
        .expect("listener deadline")
        .expect("listener observer")
        .expect("listener bind");
    let first = TcpStream::connect(address).await.expect("first TCP client");
    exercise(
        manager,
        parent,
        first,
        || async { TcpStream::connect(address).await.expect("second TCP client") },
        plaintext,
    )
    .await;
}

#[tokio::test]
async fn native_tcp_shared_clients_keep_first_and_rotated_frames_plaintext() {
    tcp_case(true).await;
}

#[tokio::test]
async fn explicit_local_tcp_clients_enforce_first_and_rotated_ifac() {
    tcp_case(false).await;
}

#[cfg(unix)]
async fn unix_case(plaintext: bool) {
    use crate::iface::local::LocalUnixServer;
    use tokio::net::UnixStream;
    let temp = tempfile::tempdir().expect("socket directory");
    let path = temp.path().join("explicit.sock");
    let manager = Arc::new(tokio::sync::Mutex::new(InterfaceManager::new(16)));
    let server = LocalUnixServer::new(path.clone(), manager.clone())
        .with_plaintext_shared_clients(plaintext);
    let parent = manager
        .lock()
        .await
        .try_spawn_as_with_mode(
            server,
            LocalUnixServer::spawn,
            IfaceRole::Unicast,
            InterfaceMode::Full,
            |manager, parent| -> Result<(), crate::error::RnsError> {
                assert!(manager.try_set_shared_config(parent, policy("original"))?);
                assert!(manager.set_shared_instance(parent, true));
                Ok(())
            },
        )
        .expect("prepared Unix server");
    timeout(Duration::from_secs(2), async {
        while !path.exists() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("Unix listener readiness");
    let first = UnixStream::connect(&path).await.expect("first Unix client");
    exercise(
        manager,
        parent,
        first,
        || async { UnixStream::connect(&path).await.expect("second Unix client") },
        plaintext,
    )
    .await;
    assert!(!path.exists(), "joined Unix worker releases socket");
}

#[cfg(unix)]
#[tokio::test]
async fn explicit_local_unix_clients_enforce_first_and_rotated_ifac() {
    unix_case(false).await;
}

#[cfg(unix)]
#[tokio::test]
async fn native_unix_shared_clients_keep_first_and_rotated_frames_plaintext() {
    unix_case(true).await;
}
