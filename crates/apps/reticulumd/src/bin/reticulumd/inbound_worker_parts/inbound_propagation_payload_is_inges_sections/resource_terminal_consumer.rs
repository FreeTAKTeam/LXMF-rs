use crate::bootstrap::PropagationControlContext;
use crate::outbound_resources::{
    track_outbound_resource, OutboundResourceMap, OutboundResourceTracking,
};
use crate::receipt_events::persist_receipt_update;
use crate::inbound_worker::spawn_inbound_worker;
use rns_transport::destination::link::LinkStatus;
use rns_transport::hash::AddressHash;
use rns_transport::iface::{IfaceSource, RxMessage};
use rns_transport::packet::{PacketContext, PacketType};
use rns_transport::resource::build_link_packet;
use std::sync::atomic::AtomicBool;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time::timeout;

#[derive(Clone, Copy, PartialEq, Eq)]
enum ResourcePeerMode {
    HoldResourceRequests,
    DropResourceAdvertisements,
    CompleteResourceTransfers,
    DropInboundResourceParts,
}

struct DaemonResourcePeer {
    daemon: Arc<RpcDaemon>,
    transport: Arc<Transport>,
    peer_transport: Arc<Transport>,
    link_id: AddressHash,
    resource_map: OutboundResourceMap,
    drop_inbound_resource_parts: Arc<AtomicBool>,
    receipt_rx: mpsc::Receiver<reticulum_daemon::receipt_bridge::ReceiptEvent>,
    resource_request_rx: mpsc::Receiver<()>,
}

fn daemon_with_sending_message(message_id: &str) -> Arc<RpcDaemon> {
    let store = MessagesStore::in_memory().expect("in-memory message store");
    store
        .insert_message(&MessageRecord {
            id: message_id.to_string(),
            source: "resource-consumer-source".to_string(),
            destination: "resource-consumer-peer".to_string(),
            title: String::new(),
            content: String::new(),
            timestamp: 0,
            direction: "out".to_string(),
            fields: None,
            receipt_status: Some("sending: link resource".to_string()),
        })
        .expect("insert outbound message");
    Arc::new(RpcDaemon::with_store(store, format!("daemon-{message_id}")))
}

async fn start_daemon_resource_peer(
    message_id: &str,
    mode: ResourcePeerMode,
) -> DaemonResourcePeer {
    let daemon = daemon_with_sending_message(message_id);
    let daemon_identity = PrivateIdentity::new_from_rand(OsRng);
    let mut daemon_config = TransportConfig::new(
        format!("daemon-{message_id}"),
        &daemon_identity,
        true,
    );
    daemon_config.set_resource_retry_interval_secs(1);
    daemon_config.set_resource_retry_limit(1);
    let transport = Arc::new(Transport::new(daemon_config));
    let peer_transport = Arc::new(Transport::new(TransportConfig::new(
        format!("peer-{message_id}"),
        &PrivateIdentity::new_from_rand(OsRng),
        true,
    )));
    let daemon_iface = transport.iface_manager().lock().await.new_channel(32);
    let peer_iface = peer_transport.iface_manager().lock().await.new_channel(32);
    let (mut daemon_tx, daemon_rx, daemon_addr) =
        (daemon_iface.tx_channel, daemon_iface.rx_channel, daemon_iface.address);
    let (mut peer_tx, peer_rx, peer_addr) =
        (peer_iface.tx_channel, peer_iface.rx_channel, peer_iface.address);
    let (request_tx, request_rx) = mpsc::channel(1);
    let drop_inbound_resource_parts = Arc::new(AtomicBool::new(
        mode == ResourcePeerMode::DropInboundResourceParts,
    ));
    let inbound_part_gate = drop_inbound_resource_parts.clone();

    tokio::spawn(async move {
        while let Some(message) = daemon_tx.recv().await {
            if mode == ResourcePeerMode::DropResourceAdvertisements
                && message.packet.context == PacketContext::ResourceAdvrtisement
            {
                continue;
            }
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
    tokio::spawn(async move {
        while let Some(message) = peer_tx.recv().await {
            if inbound_part_gate.load(std::sync::atomic::Ordering::SeqCst)
                && message.packet.context == PacketContext::Resource
            {
                continue;
            }
            if mode == ResourcePeerMode::HoldResourceRequests
                && message.packet.context == PacketContext::ResourceRequest
            {
                let _ = request_tx.send(()).await;
                continue;
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

    let peer_identity = PrivateIdentity::new_from_rand(OsRng);
    let peer_destination = peer_transport
        .add_destination(peer_identity, DestinationName::new("lxmf", "delivery"))
        .await;
    let destination = peer_destination.lock().await.desc;
    let link = transport.link(destination).await;
    timeout(Duration::from_secs(5), async {
        loop {
            if link.lock().await.status() == LinkStatus::Active {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("test peer Link becomes active");
    let link_id = *link.lock().await.id();

    let resource_map: OutboundResourceMap = Arc::new(Mutex::new(HashMap::new()));
    let (receipt_tx, receipt_rx) = mpsc::channel(8);
    spawn_inbound_worker(
        daemon.clone(),
        transport.clone(),
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
        resource_map.clone(),
    );

    DaemonResourcePeer {
        daemon,
        transport,
        peer_transport,
        link_id,
        resource_map,
        drop_inbound_resource_parts,
        receipt_rx,
        resource_request_rx: request_rx,
    }
}

fn track_message_resource(
    peer: &DaemonResourcePeer,
    message_id: &str,
    resource_hash: rns_transport::hash::Hash,
) {
    track_outbound_resource(
        &peer.resource_map,
        hex::encode(resource_hash.as_slice()),
        OutboundResourceTracking {
            message_id: message_id.to_string(),
            peer: "resource-consumer-peer".to_string(),
            bytes: 128,
            sent_status: "sent: link resource".to_string(),
        },
    );
}

#[tokio::test]
async fn production_daemon_consumer_persists_resource_completion_and_cleans_tracking() {
    let message_id = "daemon-resource-completion-consumer";
    let mut peer =
        start_daemon_resource_peer(message_id, ResourcePeerMode::CompleteResourceTransfers).await;
    tokio::task::yield_now().await;
    let payload = vec![0x3C; 128];
    let resource_hash = peer
        .transport
        .send_resource(&peer.link_id, payload, None)
        .await
        .expect("send Resource over active daemon Link");
    let resource_hash_hex = hex::encode(resource_hash.as_slice());
    track_message_resource(&peer, message_id, resource_hash);

    let receipt = timeout(Duration::from_secs(5), peer.receipt_rx.recv())
        .await
        .expect("daemon emits completion receipt")
        .expect("receipt channel remains open");
    assert_eq!(receipt.message_id, message_id);
    assert_eq!(receipt.status, "sent: link resource");
    assert_eq!(receipt.delivery_kind.as_deref(), Some("resource-complete"));
    assert_eq!(receipt.resource_hash.as_deref(), Some(resource_hash_hex.as_str()));
    assert_eq!(receipt.bytes, Some(128));
    assert!(peer.resource_map.lock().expect("resource map").is_empty());

    persist_receipt_update(
        peer.daemon.as_ref(),
        receipt,
        &Arc::new(Mutex::new(HashMap::new())),
        &peer.resource_map,
    )
    .expect("persist Resource completion status");
    assert_eq!(
        peer.daemon.message_receipt_status(message_id).expect("receipt status"),
        Some("sent: link resource".to_string())
    );
}

#[tokio::test]
async fn production_daemon_consumer_persists_peer_resource_rejection() {
    let message_id = "daemon-resource-rejection-consumer";
    let mut peer = start_daemon_resource_peer(message_id, ResourcePeerMode::HoldResourceRequests)
        .await;
    tokio::task::yield_now().await;
    let resource_hash = peer
        .transport
        .send_resource(&peer.link_id, vec![0xA5; 128], None)
        .await
        .expect("send Resource over active daemon Link");
    track_message_resource(&peer, message_id, resource_hash);

    timeout(Duration::from_secs(5), peer.resource_request_rx.recv())
        .await
        .expect("peer accepted the Resource advertisement")
        .expect("held request notification");
    let peer_link = peer
        .peer_transport
        .find_in_link(&peer.link_id)
        .await
        .expect("peer has the matching inbound Link");
    let rejection = {
        let link = peer_link.lock().await;
        build_link_packet(
            &link,
            PacketType::Data,
            PacketContext::ResourceReceiverCancel,
            resource_hash.as_slice(),
        )
        .expect("build peer Resource rejection packet")
    };
    peer.peer_transport.send_packet(rejection).await;

    let receipt = timeout(Duration::from_secs(5), peer.receipt_rx.recv())
        .await
        .expect("daemon emits the rejected Resource receipt")
        .expect("receipt channel remains open");
    assert_eq!(receipt.message_id, message_id);
    assert_eq!(receipt.status, "rejected");
    assert_eq!(receipt.delivery_kind.as_deref(), Some("resource-rejected"));
    assert_eq!(receipt.resource_hash.as_deref(), Some(hex::encode(resource_hash.as_slice()).as_str()));
    assert!(peer.resource_map.lock().expect("resource map").is_empty());

    persist_receipt_update(
        peer.daemon.as_ref(),
        receipt,
        &Arc::new(Mutex::new(HashMap::new())),
        &peer.resource_map,
    )
    .expect("persist Resource rejection status");
    assert_eq!(
        peer.daemon.message_receipt_status(message_id).expect("receipt status"),
        Some("rejected".to_string())
    );
}

#[tokio::test]
async fn production_daemon_consumer_cleans_cancelled_resource_without_success_receipt() {
    let message_id = "daemon-resource-cancel-consumer";
    let mut peer = start_daemon_resource_peer(
        message_id,
        ResourcePeerMode::DropResourceAdvertisements,
    )
    .await;
    tokio::task::yield_now().await;
    let resource_hash = peer
        .transport
        .send_resource(&peer.link_id, vec![0x5A; 128], None)
        .await
        .expect("send Resource over active daemon Link");
    track_message_resource(&peer, message_id, resource_hash);

    let response = peer
        .daemon
        .handle_rpc(RpcRequest {
            id: 610,
            method: "sdk_cancel_message_v2".to_string(),
            params: Some(json!({ "message_id": message_id })),
        })
        .expect("public daemon cancellation request");
    assert_eq!(response.result.expect("cancel result")["result"], json!("Accepted"));
    assert_eq!(
        peer.daemon.message_receipt_status(message_id).expect("cancelled status"),
        Some("cancelled".to_string())
    );

    assert!(peer
        .transport
        .cancel_resource(&peer.link_id, resource_hash)
        .await
        .expect("public Transport cancellation"));
    timeout(Duration::from_secs(5), async {
        loop {
            if peer.resource_map.lock().expect("resource map").is_empty() {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("daemon ResourceEvent consumer clears cancellation tracking");
    assert_eq!(
        peer.daemon.message_receipt_status(message_id).expect("cancelled status remains"),
        Some("cancelled".to_string())
    );
    assert!(matches!(
        peer.receipt_rx.try_recv(),
        Err(mpsc::error::TryRecvError::Empty)
    ));
}

#[tokio::test]
async fn production_daemon_consumer_reports_resource_retry_failure_and_cleans_tracking() {
    let message_id = "daemon-resource-retry-failure-consumer";
    let mut peer = start_daemon_resource_peer(message_id, ResourcePeerMode::HoldResourceRequests)
        .await;
    tokio::task::yield_now().await;
    let resource_hash = peer
        .transport
        .send_resource(&peer.link_id, vec![0xC3; 128], None)
        .await
        .expect("send Resource over active daemon Link");
    let resource_hash_hex = hex::encode(resource_hash.as_slice());
    track_message_resource(&peer, message_id, resource_hash);

    timeout(Duration::from_secs(5), async {
        loop {
            if peer.resource_request_rx.try_recv().is_ok() {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("peer receives and holds the Resource request");

    let receipt = timeout(Duration::from_secs(8), peer.receipt_rx.recv())
        .await
        .expect("daemon emits terminal failure receipt")
        .expect("receipt channel remains open");
    assert_eq!(receipt.message_id, message_id);
    assert_eq!(receipt.status, "failed: resource transfer failed");
    assert_eq!(receipt.delivery_kind.as_deref(), Some("resource-failed"));
    assert_eq!(receipt.resource_hash.as_deref(), Some(resource_hash_hex.as_str()));
    assert!(peer.resource_map.lock().expect("resource map").is_empty());

    persist_receipt_update(
        peer.daemon.as_ref(),
        receipt,
        &Arc::new(Mutex::new(HashMap::new())),
        &peer.resource_map,
    )
    .expect("persist Resource timeout status");
    assert_eq!(
        peer.daemon.message_receipt_status(message_id).expect("receipt status"),
        Some("failed: resource transfer failed".to_string())
    );
}
