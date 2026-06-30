use rand_core::OsRng;
use rns_transport::destination::{DestinationName, SingleInputDestination};
use rns_transport::hash::{AddressHash, ADDRESS_HASH_SIZE};
use rns_transport::identity::PrivateIdentity;
use rns_transport::iface::{IfaceSource, InterfaceChannel, RxMessage, TxMessage, TxMessageType};
use rns_transport::packet::{Packet, PacketContext};
use rns_transport::transport::{Transport, TransportConfig};
use tokio::time::{timeout, Duration};

fn retransmitting_transport(name: &str) -> Transport {
    let identity = PrivateIdentity::new_from_rand(OsRng);
    let mut config = TransportConfig::new(name, &identity, true);
    config.set_retransmit(true);
    Transport::new(config)
}

async fn new_probe_iface(transport: &Transport) -> InterfaceChannel {
    transport.iface_manager().lock().await.new_channel(16)
}

async fn feed_iface_packet(channel: &InterfaceChannel, packet: Packet) {
    channel
        .rx_channel
        .send(RxMessage { address: *channel.address(), packet, source: IfaceSource::None })
        .await
        .expect("probe interface rx should stay open");
}

async fn recv_tx(channel: &mut InterfaceChannel, label: &str) -> TxMessage {
    timeout(Duration::from_millis(2_500), channel.tx_channel.recv())
        .await
        .unwrap_or_else(|_| panic!("timed out waiting for {label}"))
        .unwrap_or_else(|| panic!("probe interface closed while waiting for {label}"))
}

async fn wait_for_known_path(transport: &Transport, destination: &AddressHash) {
    for _ in 0..40 {
        if transport.has_path(destination).await {
            return;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("transport did not learn path for {destination}");
}

#[tokio::test]
async fn scoped_path_request_dispatches_only_on_requested_iface() {
    let transport = retransmitting_transport("transport-scoped-path-request");
    let mut scoped_iface = new_probe_iface(&transport).await;
    let mut other_iface = new_probe_iface(&transport).await;
    let destination = AddressHash::new_from_rand(OsRng);
    let tag = vec![0xAB; ADDRESS_HASH_SIZE];

    let trace = transport
        .request_path(&destination, Some(*scoped_iface.address()), Some(tag.clone()))
        .await;

    assert_eq!(trace.matched_ifaces, 1);
    assert_eq!(trace.sent_ifaces, 1);
    assert_eq!(trace.failed_ifaces, 0);

    let scoped = recv_tx(&mut scoped_iface, "scoped path request").await;
    assert!(matches!(scoped.tx_type, TxMessageType::Broadcast(None)));
    assert_eq!(&scoped.packet.data.as_slice()[..ADDRESS_HASH_SIZE], destination.as_slice());
    assert_eq!(&scoped.packet.data.as_slice()[scoped.packet.data.len() - tag.len()..], tag);
    assert!(
        matches!(
            other_iface.tx_channel.try_recv(),
            Err(tokio::sync::mpsc::error::TryRecvError::Empty)
        ),
        "path request should not dispatch on unscoped interface"
    );
}

#[tokio::test]
async fn known_path_response_precedes_due_ordinary_announce_at_transport_boundary() {
    let responder = retransmitting_transport("transport-known-path-response-order");
    let learned_iface = new_probe_iface(&responder).await;
    let mut requesting_iface = new_probe_iface(&responder).await;

    let mut remote_destination = SingleInputDestination::new(
        PrivateIdentity::new_from_rand(OsRng),
        DestinationName::new("lxmf", "delivery"),
    );
    let mut announce = remote_destination.announce(OsRng, None).expect("valid announce");
    announce.header.hops = 2;
    let destination = announce.destination;
    let cached_announce_data = announce.data.clone();

    let requester = retransmitting_transport("transport-path-request-origin");
    let mut requester_iface = new_probe_iface(&requester).await;
    let tag = vec![0x45; ADDRESS_HASH_SIZE];
    let request_trace = requester
        .request_path(&destination, Some(*requester_iface.address()), Some(tag.clone()))
        .await;
    assert_eq!(request_trace.sent_ifaces, 1);
    let path_request = recv_tx(&mut requester_iface, "outbound path request").await.packet;
    assert_eq!(&path_request.data.as_slice()[..ADDRESS_HASH_SIZE], destination.as_slice());
    assert_eq!(&path_request.data.as_slice()[path_request.data.len() - tag.len()..], tag);

    feed_iface_packet(&learned_iface, announce).await;
    wait_for_known_path(&responder, &destination).await;
    feed_iface_packet(&requesting_iface, path_request).await;

    let first = recv_tx(&mut requesting_iface, "known-path response").await;
    assert!(matches!(
        first.tx_type,
        TxMessageType::Direct(iface) if iface == *requesting_iface.address()
    ));
    assert_eq!(first.packet.destination, destination);
    assert_eq!(first.packet.context, PacketContext::PathResponse);
    assert_eq!(first.packet.header.hops, 2);
    assert_eq!(first.packet.data.as_slice(), cached_announce_data.as_slice());

    let second = recv_tx(&mut requesting_iface, "ordinary announce after path response").await;
    assert!(matches!(
        second.tx_type,
        TxMessageType::Broadcast(Some(iface)) if iface == *learned_iface.address()
    ));
    assert_eq!(second.packet.destination, destination);
    assert_eq!(second.packet.context, PacketContext::None);
    assert_eq!(second.packet.data.as_slice(), cached_announce_data.as_slice());
}
