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
async fn daemon_observes_remote_cancel_of_partial_inbound_resource_without_false_delivery() {
    let message_id = "remote-cancelled-inbound-resource";
    let daemon = super::daemon_with_sending_message(message_id);
    let daemon_transport = Arc::new(Transport::new(TransportConfig::new(
        "remote-cancel-resource-daemon",
        &PrivateIdentity::new_from_rand(OsRng),
        true,
    )));
    let peer_transport = Arc::new(Transport::new(TransportConfig::new(
        "remote-cancel-resource-peer",
        &PrivateIdentity::new_from_rand(OsRng),
        true,
    )));
    let daemon_iface = daemon_transport.iface_manager().lock().await.new_channel(32);
    let peer_iface = peer_transport.iface_manager().lock().await.new_channel(32);
    let (mut daemon_tx, daemon_rx, daemon_addr) =
        (daemon_iface.tx_channel, daemon_iface.rx_channel, daemon_iface.address);
    let (mut peer_tx, peer_rx, peer_addr) =
        (peer_iface.tx_channel, peer_iface.rx_channel, peer_iface.address);
    let (later_part_tx, later_part_rx) = oneshot::channel();

    tokio::spawn(async move {
        let mut later_part_tx = Some(later_part_tx);
        let mut forwarded_parts = 0usize;
        let mut gate_active = true;
        while let Some(message) = peer_tx.recv().await {
            let context = message.packet.context;
            if gate_active && context == PacketContext::Resource {
                forwarded_parts += 1;
                if forwarded_parts > 1 {
                    if let Some(signal) = later_part_tx.take() {
                        let _ = signal.send(());
                    }
                    continue;
                }
            }
            if context == PacketContext::ResourceInitiatorCancel {
                gate_active = false;
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

    let peer_destination = peer_transport
        .add_destination(
            PrivateIdentity::new_from_rand(OsRng),
            DestinationName::new("lxmf", "delivery"),
        )
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
        .map(|index| ((index * 37 + index / 193) & 0xff) as u8)
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
                        panic!("gated Resource completed before peer cancellation")
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
    .expect("daemon observes partial inbound Resource progress");
    timeout(Duration::from_secs(5), later_part_rx)
        .await
        .expect("gate observes later Resource traffic")
        .expect("gate signals the held Resource packet");

    assert!(peer_transport
        .cancel_resource(&link_id, resource_hash)
        .await
        .expect("peer cancels its outgoing Resource"));
    timeout(Duration::from_secs(5), async {
        loop {
            match resource_events.recv().await {
                Ok(event) if event.hash == resource_hash => match event.kind {
                    ResourceEventKind::InboundFailed(failure) => {
                        assert_eq!(failure.reason, "remote_cancelled");
                        assert!(failure.progress.received_parts > 0);
                        assert!(failure.progress.received_parts < failure.progress.total_parts);
                        break;
                    }
                    ResourceEventKind::Complete(_) => {
                        panic!("cancelled inbound Resource must not complete")
                    }
                    _ => {}
                },
                Ok(_) | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                    panic!("Resource event stream closed before remote-cancel failure")
                }
            }
        }
    })
    .await
    .expect("peer cancellation yields a correlated inbound failure");

    assert!(matches!(
        resource_events.try_recv(),
        Err(tokio::sync::broadcast::error::TryRecvError::Empty)
    ));
    assert!(matches!(
        receipt_rx.try_recv(),
        Err(mpsc::error::TryRecvError::Empty)
    ));
    assert_eq!(
        daemon.message_receipt_status(message_id).expect("message status"),
        Some("sending: link resource".to_string())
    );
    let listed = daemon
        .handle_rpc(RpcRequest { id: 610, method: "list_messages".to_string(), params: None })
        .expect("list stored messages")
        .result
        .expect("list result");
    let messages = listed["messages"].as_array().expect("message list");
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0]["id"], message_id);
    assert_eq!(messages[0]["content"], "");

    let recovery_payload = b"same Link accepts a new Resource after remote cancellation".to_vec();
    let recovery_hash = peer_transport
        .send_resource(&link_id, recovery_payload.clone(), None)
        .await
        .expect("send recovery Resource on the still-active Link");
    timeout(Duration::from_secs(5), async {
        loop {
            match resource_events.recv().await {
                Ok(event) if event.hash == recovery_hash => match event.kind {
                    ResourceEventKind::Complete(complete) => {
                        assert_eq!(complete.data, recovery_payload);
                        break;
                    }
                    ResourceEventKind::InboundFailed(failure) => {
                        panic!("same-Link Resource recovery failed: {}", failure.reason)
                    }
                    _ => {}
                },
                Ok(_) | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                    panic!("Resource event stream closed before same-Link recovery")
                }
            }
        }
    })
    .await
    .expect("same Link accepts a new Resource after cancellation cleanup");
}
