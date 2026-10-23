use super::*;
use crate::iface::{IfaceRole, Interface, InterfaceContext, InterfaceMode};
use std::future::{poll_fn, Future};
use std::task::Poll;
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;

struct GatedCarrier;

impl Interface for GatedCarrier {
    fn mtu() -> usize {
        500
    }
}

async fn prepare_gated_worker(
    transport: &Transport,
    constructed: Arc<AtomicUsize>,
) -> (AddressHash, oneshot::Sender<()>, CancellationToken, tokio::task::JoinHandle<()>) {
    let mut manager = transport.iface_manager.lock().await;
    let (address, activate, cancel) = manager
        .try_spawn_deferred_as_with_mode(
            GatedCarrier,
            move |context: InterfaceContext<GatedCarrier>| {
                constructed.fetch_add(1, Ordering::SeqCst);
                async move { context.cancel.cancelled().await }
            },
            IfaceRole::Unicast,
            InterfaceMode::Full,
            |manager, address| -> Result<(), RnsError> {
                assert!(manager.set_shared_instance(address, true));
                Ok(())
            },
        )
        .expect("prepare owned deferred carrier");
    let mut tasks = manager.take_worker_tree(address);
    assert_eq!(tasks.len(), 1);
    (address, activate, cancel, tasks.pop().expect("owned worker"))
}

#[tokio::test]
async fn cancelled_shared_activation_does_not_wait_for_blocked_handler() {
    let transport = Transport::new(TransportConfig::default());
    let constructed = Arc::new(AtomicUsize::new(0));
    let (address, activate, cancel, worker) =
        prepare_gated_worker(&transport, constructed.clone()).await;
    let handler = transport.get_handler();
    let guard = handler.lock().await;
    let activation = transport.activate_shared_instance_interface(activate, &cancel);
    tokio::pin!(activation);
    poll_fn(|cx| {
        assert!(activation.as_mut().poll(cx).is_pending());
        Poll::Ready(())
    })
    .await;
    cancel.cancel();
    assert!(timeout(Duration::from_secs(1), activation)
        .await
        .expect("cancelled startup must finish while handler remains blocked")
        .is_err());
    timeout(Duration::from_secs(1), worker).await.expect("gated worker joins").expect("worker");
    assert_eq!(constructed.load(Ordering::SeqCst), 0);
    assert!(!guard.config.connected_to_shared_instance);
    drop(guard);
    transport.iface_manager.lock().await.stop_interface(address);
    transport.cancel.cancel();
}

#[tokio::test]
async fn dropped_shared_activation_cancels_and_joins_unconstructed_worker() {
    let transport = Transport::new(TransportConfig::default());
    let constructed = Arc::new(AtomicUsize::new(0));
    let (address, activate, cancel, worker) =
        prepare_gated_worker(&transport, constructed.clone()).await;
    let handler = transport.get_handler();
    let guard = handler.lock().await;
    {
        let activation = transport.activate_shared_instance_interface(activate, &cancel);
        tokio::pin!(activation);
        poll_fn(|cx| {
            assert!(activation.as_mut().poll(cx).is_pending());
            Poll::Ready(())
        })
        .await;
    }
    timeout(Duration::from_secs(1), worker).await.expect("dropped startup joins").expect("worker");
    assert!(cancel.is_cancelled());
    assert_eq!(constructed.load(Ordering::SeqCst), 0);
    assert!(!guard.config.connected_to_shared_instance);
    drop(guard);
    let mut manager = transport.iface_manager.lock().await;
    manager.cleanup();
    assert!(!manager.interface_hashes().contains(&address));
    transport.cancel.cancel();
}

#[tokio::test]
async fn manager_stop_joins_gated_shared_worker_and_cancels_waiting_activation() {
    let transport = Transport::new(TransportConfig::default());
    let constructed = Arc::new(AtomicUsize::new(0));
    let (address, activate, cancel, worker) =
        prepare_gated_worker(&transport, constructed.clone()).await;
    let handler = transport.get_handler();
    let guard = handler.lock().await;
    let activation = transport.activate_shared_instance_interface(activate, &cancel);
    tokio::pin!(activation);
    poll_fn(|cx| {
        assert!(activation.as_mut().poll(cx).is_pending());
        Poll::Ready(())
    })
    .await;
    assert!(transport.iface_manager.lock().await.stop_interface(address));
    timeout(Duration::from_secs(1), worker).await.expect("manager stop joins").expect("worker");
    assert!(timeout(Duration::from_secs(1), activation)
        .await
        .expect("manager cancellation interrupts activation lock wait")
        .is_err());
    assert_eq!(constructed.load(Ordering::SeqCst), 0);
    assert!(!guard.config.connected_to_shared_instance);
    transport.cancel.cancel();
}

#[tokio::test]
async fn failed_shared_gate_preserves_both_prior_transport_modes() {
    for initial in [false, true] {
        let mut config = TransportConfig::default();
        config.set_connected_to_shared_instance(initial);
        let transport = Transport::new(config);
        let (activate, activated) = oneshot::channel();
        drop(activated);
        assert!(transport
            .activate_shared_instance_interface(activate, &CancellationToken::new())
            .await
            .is_err());
        assert_eq!(
            transport.get_handler().lock().await.config.connected_to_shared_instance,
            initial
        );
        transport.cancel.cancel();
    }
}

#[tokio::test]
async fn already_cancelled_shared_activation_never_acquires_blocked_handler() {
    let transport = Transport::new(TransportConfig::default());
    let handler = transport.get_handler();
    let guard = handler.lock().await;
    let (activate, mut activated) = oneshot::channel();
    let cancel = CancellationToken::new();
    cancel.cancel();
    assert!(timeout(
        Duration::from_secs(1),
        transport.activate_shared_instance_interface(activate, &cancel)
    )
    .await
    .expect("pre-cancelled activation is prompt")
    .is_err());
    assert!(activated.try_recv().is_err());
    assert!(!guard.config.connected_to_shared_instance);
    transport.cancel.cancel();
}

#[tokio::test]
async fn shared_tcp_attachment_commits_mode_before_first_plaintext_frame() {
    use crate::iface::hdlc::Hdlc;
    use crate::iface::tcp_client::TcpClient;
    use tokio::io::AsyncWriteExt;
    use tokio::net::{TcpListener, TcpStream};

    let listener = TcpListener::bind("127.0.0.1:0").await.expect("owned loopback listener");
    let client =
        TcpStream::connect(listener.local_addr().expect("listener address")).await.expect("client");
    let (mut peer, _) = listener.accept().await.expect("peer");
    let transport = Transport::new(TransportConfig::default());
    let mut ingress = transport.iface_rx();
    let handler = transport.get_handler();
    let constructed = Arc::new(AtomicUsize::new(0));
    let observed = constructed.clone();
    let (address, activate, cancel, worker) = {
        let mut manager = transport.iface_manager.lock().await;
        let (address, activate, cancel) = manager
            .try_spawn_deferred_as_with_mode(
                TcpClient::new_from_stream("shared-owner", client),
                move |context| {
                    observed.fetch_add(1, Ordering::SeqCst);
                    async move {
                        assert!(handler.lock().await.config.connected_to_shared_instance);
                        TcpClient::spawn(context).await;
                    }
                },
                IfaceRole::Unicast,
                InterfaceMode::Full,
                |manager, address| -> Result<(), RnsError> {
                    assert!(manager.set_shared_instance(address, true));
                    Ok(())
                },
            )
            .expect("prepare real TCP attachment");
        let worker = manager.take_worker_tree(address).pop().expect("owned TCP worker");
        (address, activate, cancel, worker)
    };
    let packet = Packet {
        data: PacketDataBuffer::new_from_slice(b"first shared plaintext"),
        ..Default::default()
    };
    peer.write_all(&Hdlc::frame(&packet.to_bytes().expect("packet bytes")).expect("HDLC"))
        .await
        .expect("first frame");
    assert_eq!(constructed.load(Ordering::SeqCst), 0);
    transport
        .activate_shared_instance_interface(activate, &cancel)
        .await
        .expect("activate attachment");
    let received = timeout(Duration::from_secs(2), ingress.recv())
        .await
        .expect("first ingress deadline")
        .expect("first ingress");
    assert_eq!(received.address, address);
    assert_eq!(received.packet.data.as_slice(), b"first shared plaintext");
    assert_eq!(constructed.load(Ordering::SeqCst), 1);
    assert!(transport.iface_manager.lock().await.is_shared_instance(&address));
    assert!(transport.iface_manager.lock().await.stop_interface(address));
    timeout(Duration::from_secs(2), worker).await.expect("TCP worker joins").expect("worker");
    assert!(
        transport.get_handler().lock().await.config.connected_to_shared_instance,
        "successful handoff commits mode even after this worker stops"
    );
    transport.cancel.cancel();
}
