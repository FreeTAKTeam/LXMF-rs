use crate::bootstrap::PropagationControlContext;
use crate::inbound_worker::spawn_inbound_worker;
use rns_transport::destination::link::LinkStatus;
use rns_transport::destination::DestinationName;
use rns_transport::iface::{IfaceSource, RxMessage};
use rns_transport::packet::PacketContext;
use rns_transport::resource::ResourceEventKind;
use std::time::Duration;
use tokio::sync::{mpsc, oneshot};
use tokio::time::timeout;

#[tokio::test]
async fn daemon_reports_partial_inbound_resource_failure_after_gated_link_teardown() {
    let message_id = "partial-inbound-resource";
    let daemon = super::daemon_with_sending_message(message_id);
    let daemon_transport = Arc::new(Transport::new(TransportConfig::new(
        "partial-resource-daemon",
        &PrivateIdentity::new_from_rand(OsRng),
        true,
    )));
    let peer_transport = Arc::new(Transport::new(TransportConfig::new(
        "partial-resource-peer",
        &PrivateIdentity::new_from_rand(OsRng),
        true,
    )));
    let daemon_iface = daemon_transport.iface_manager().lock().await.new_channel(32);
    let peer_iface = peer_transport.iface_manager().lock().await.new_channel(32);
    let (mut daemon_tx, daemon_rx, daemon_addr) =
        (daemon_iface.tx_channel, daemon_iface.rx_channel, daemon_iface.address);
    let (mut peer_tx, peer_rx, peer_addr) =
        (peer_iface.tx_channel, peer_iface.rx_channel, peer_iface.address);
    let (held_tx, held_rx) = oneshot::channel();

    tokio::spawn(async move {
        let mut held_tx = Some(held_tx);
        let mut forwarded_resource_parts = 0usize;
        while let Some(message) = peer_tx.recv().await {
            if message.packet.context == PacketContext::Resource {
                forwarded_resource_parts += 1;
                if forwarded_resource_parts > 1 {
                    if let Some(held_tx) = held_tx.take() {
                        let _ = held_tx.send(());
                    }
                    // Keep later data fragments behind the gate, but continue
                    // forwarding LinkClose and other control packets.
                    continue;
                }
            }
            if daemon_rx
                .send(RxMessage {
                    address: daemon_addr,
                    packet: message.packet,
                    source: IfaceSource::None,
                })
                .await
                .is_err()
            {
                break;
            }
        }
    });
    tokio::spawn(async move {
        while let Some(message) = daemon_tx.recv().await {
            if peer_rx
                .send(RxMessage {
                    address: peer_addr,
                    packet: message.packet,
                    source: IfaceSource::None,
                })
                .await
                .is_err()
            {
                break;
            }
        }
    });

    let peer_identity = PrivateIdentity::new_from_rand(OsRng);
    let peer_destination = peer_transport
        .add_destination(peer_identity, DestinationName::new("lxmf", "delivery"))
        .await;
    let destination = peer_destination.lock().await.desc;
    let link = daemon_transport.link(destination).await;
    timeout(Duration::from_secs(5), async {
        loop {
            if link.lock().await.status() == LinkStatus::Active {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("daemon-to-peer Link becomes active");
    let link_id = *link.lock().await.id();

    let mut resource_events = daemon_transport.resource_events();
    let (receipt_tx, mut receipt_rx) = mpsc::channel(8);
    spawn_inbound_worker(
        daemon.clone(),
        daemon_transport.clone(),
        PropagationControlContext {
            enabled: false,
            local_identity_hash: [0; 16],
            propagation_destination_hash_hex: None,
            control_destination_hash_hex: None,
            delivery_destination: None,
            allowed_control_identities: Vec::new(),
            validated_peer_links: Arc::new(Mutex::new(HashSet::new())),
            identified_peer_links: Arc::new(Mutex::new(HashMap::new())),
        },
        None,
        receipt_tx,
        Arc::new(Mutex::new(HashMap::new())),
    );

    let payload = (0..512 * 1024)
        .map(|index| ((index * 73 + index / 251) & 0xff) as u8)
        .collect::<Vec<_>>();
    let resource_hash = peer_transport
        .send_resource(&link_id, payload, None)
        .await
        .expect("peer advertises inbound Resource over active Link");

    timeout(Duration::from_secs(5), async {
        loop {
            match resource_events.recv().await {
                Ok(event) if event.hash == resource_hash => match event.kind {
                    ResourceEventKind::Progress(progress) if progress.received_parts > 0 => {
                        assert!(progress.received_parts < progress.total_parts);
                        break;
                    }
                    ResourceEventKind::Complete(_) => {
                        panic!("gated transfer completed before Link teardown")
                    }
                    _ => {}
                },
                Ok(_) | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                    panic!("Resource event stream closed before partial progress")
                }
            }
        }
    })
    .await
    .expect("receiver observes partial Resource progress");
    timeout(Duration::from_secs(5), held_rx)
        .await
        .expect("later Resource traffic reaches the gate")
        .expect("gate reports the held packet");

    let peer_link = peer_transport
        .find_in_link(&link_id)
        .await
        .expect("peer retains Link while sender traffic is gated");
    let teardown = { peer_link.lock().await.teardown() };
    if let Some(teardown) = teardown {
        peer_transport.send_packet(teardown).await;
    }

    let failure = timeout(Duration::from_secs(5), async {
        loop {
            match resource_events.recv().await {
                Ok(event) if event.hash == resource_hash => match event.kind {
                    ResourceEventKind::InboundFailed(failure) => break failure,
                    ResourceEventKind::Complete(_) => {
                        panic!("partial inbound Resource must not complete")
                    }
                    _ => {}
                },
                Ok(_) | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                    panic!("Resource event stream closed before terminal failure")
                }
            }
        }
    })
    .await
    .expect("Link teardown yields one inbound terminal failure");
    assert!(failure.progress.received_parts > 0);
    assert!(failure.progress.received_parts < failure.progress.total_parts);

    assert!(matches!(
        resource_events.try_recv(),
        Err(tokio::sync::broadcast::error::TryRecvError::Empty)
    ));
    assert!(matches!(
        receipt_rx.try_recv(),
        Err(mpsc::error::TryRecvError::Empty)
    ));
    assert_eq!(
        daemon.message_receipt_status(message_id).expect("status read"),
        Some("sending: link resource".to_string())
    );
    let listed = daemon
        .handle_rpc(RpcRequest { id: 610, method: "list_messages".to_string(), params: None })
        .expect("list stored messages")
        .result
        .expect("list result");
    let messages = listed["messages"].as_array().expect("message list");
    assert_eq!(messages.len(), 1, "partial payload does not add delivered content");
    assert_eq!(messages[0]["id"], message_id);
    assert_eq!(messages[0]["content"], "");
}
