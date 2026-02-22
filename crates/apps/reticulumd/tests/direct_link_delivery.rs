use rand_core::OsRng;
use rns_core::identity::PrivateIdentity;
use rns_transport::delivery::{send_via_link, LinkSendResult};
use rns_transport::destination::link::Link;
use rns_transport::destination::{DestinationDesc, DestinationName};
use rns_transport::iface::{Interface, InterfaceContext};
use rns_transport::packet::{DestinationType, PacketType};
use rns_transport::transport::{Transport, TransportConfig};
use tokio::time::Duration;

struct SinkInterface;

impl Interface for SinkInterface {
    fn mtu() -> usize {
        1500
    }
}

async fn sink_worker(context: InterfaceContext<SinkInterface>) {
    let (_rx_channel, mut tx_channel) = context.channel.split();
    while tx_channel.recv().await.is_some() {}
}

#[tokio::test]
async fn direct_send_uses_link_payloads() {
    let sender = PrivateIdentity::new_from_rand(OsRng);
    let receiver = PrivateIdentity::new_from_rand(OsRng);

    let sender = rns_transport::identity_bridge::to_transport_private_identity(&sender);
    let receiver = rns_transport::identity_bridge::to_transport_private_identity(&receiver);

    let transport = Transport::new(TransportConfig::new("test", &sender, true));
    transport.iface_manager().lock().await.spawn(SinkInterface, sink_worker);

    let destination = DestinationDesc {
        identity: *receiver.as_identity(),
        address_hash: *receiver.address_hash(),
        name: DestinationName::new("lxmf", "delivery"),
    };

    let link = transport.link(destination).await;
    let request = link.lock().await.request();

    let (event_tx, _) = tokio::sync::broadcast::channel(16);
    let mut input_link =
        Link::new_from_request(&request, receiver.sign_key().clone(), destination, event_tx)
            .expect("input link");
    let proof = input_link.prove();

    link.lock().await.handle_packet(&proof);
    tokio::time::sleep(Duration::from_millis(20)).await;

    let result = send_via_link(&transport, destination, b"hello link", Duration::from_secs(1))
        .await
        .expect("send via link");
    let LinkSendResult::Packet(packet) = result else {
        panic!("expected packet delivery for small payload")
    };

    assert_eq!(packet.header.destination_type, DestinationType::Link);
    assert_eq!(packet.header.packet_type, PacketType::Data);
}
