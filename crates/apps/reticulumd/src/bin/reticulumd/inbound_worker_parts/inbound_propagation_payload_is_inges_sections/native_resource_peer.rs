use crate::bootstrap::PropagationControlContext;
use crate::inbound_worker::spawn_inbound_worker;
use rns_transport::destination::link::LinkStatus;
use rns_transport::hash::AddressHash;
use rns_transport::iface::{IfaceSource, RxMessage};
use rns_transport::packet::PacketContext;
use std::sync::atomic::AtomicBool;
use std::time::Duration;
use tokio::time::timeout;

struct DaemonResourcePeer {
    transport: Arc<Transport>,
    peer_transport: Arc<Transport>,
    link_id: AddressHash,
    drop_inbound_resource_parts: Arc<AtomicBool>,
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
    let drop_inbound_resource_parts = Arc::new(AtomicBool::new(true));
    let inbound_part_gate = drop_inbound_resource_parts.clone();

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
    tokio::spawn(async move {
        while let Some(message) = peer_tx.recv().await {
            if inbound_part_gate.load(std::sync::atomic::Ordering::SeqCst)
                && message.packet.context == PacketContext::Resource
            {
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
    );

    DaemonResourcePeer {
        transport,
        peer_transport,
        link_id,
        drop_inbound_resource_parts,
    }
}
