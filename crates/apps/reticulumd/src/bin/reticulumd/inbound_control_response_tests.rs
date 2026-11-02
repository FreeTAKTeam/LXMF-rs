use super::*;
use rand_core::OsRng;
use rns_transport::destination::link::LinkStatus;
use rns_transport::destination::DestinationName;
use rns_transport::identity::PrivateIdentity;
use rns_transport::iface::{IfaceRole, IfaceSource, InterfaceChannel, RxMessage};
use rns_transport::packet::{Packet, PacketContext};
use rns_transport::transport::TransportConfig;
use std::time::Duration;

async fn forward(channel: &InterfaceChannel, packet: Packet) {
    channel
        .rx_channel
        .send(RxMessage { address: *channel.address(), packet, source: IfaceSource::None })
        .await
        .expect("native ingress");
}

async fn next(channel: &mut InterfaceChannel) -> Packet {
    tokio::time::timeout(Duration::from_secs(2), channel.tx_channel.recv())
        .await
        .expect("native packet timeout")
        .expect("native interface")
        .packet
}

/// An active Link whose node side answers control requests.
async fn control_link() -> (Transport, AddressHash, InterfaceChannel, InterfaceChannel) {
    let client_identity = PrivateIdentity::new_from_rand(OsRng);
    let client = Transport::new(TransportConfig::new("control-client", &client_identity, true));
    let node_identity = PrivateIdentity::new_from_rand(OsRng);
    let node = Transport::new(TransportConfig::new("control-node", &node_identity, true));
    let destination =
        node.add_destination(node_identity, DestinationName::new("lxmf", "propagation")).await;
    let destination = destination.lock().await.desc;
    let mut client_iface =
        client.iface_manager().lock().await.new_channel_with_role(16, IfaceRole::Unicast);
    let mut node_iface =
        node.iface_manager().lock().await.new_channel_with_role(16, IfaceRole::Unicast);
    let link = client.link(destination).await;
    forward(&node_iface, next(&mut client_iface).await).await;
    forward(&client_iface, next(&mut node_iface).await).await;
    tokio::time::timeout(Duration::from_secs(2), async {
        while link.lock().await.status() != LinkStatus::Active {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("native Link activation");
    forward(&node_iface, next(&mut client_iface).await).await;
    let link_id = *link.lock().await.id();
    (node, link_id, node_iface, client_iface)
}

/// Large `/get` batches stalled Python receivers: the daemon sent responses
/// above the negotiated Link MDU as one oversized packet, which Python drops
/// as an invalid frame. Responses must follow `RNS.Link.handle_request`.
#[tokio::test]
async fn control_response_above_link_mdu_uses_resource_like_python() {
    let (node, link_id, mut node_iface, _client_iface) = control_link().await;
    let node_link = node.find_in_link(&link_id).await.expect("node side Link");
    let mdu = node_link.lock().await.link_mdu();

    send_control_response(
        &node,
        &link_id,
        [7u8; 16],
        ControlResponse::Rmpv(rmpv::Value::Binary(vec![0x5a; mdu * 4])),
    )
    .await
    .expect("control response");

    let sent = next(&mut node_iface).await;
    assert_eq!(sent.context, PacketContext::ResourceAdvrtisement, "response above MDU {mdu}");
}

#[tokio::test]
async fn control_response_within_link_mdu_stays_one_packet() {
    let (node, link_id, mut node_iface, _client_iface) = control_link().await;

    send_control_response(&node, &link_id, [7u8; 16], ControlResponse::Bool(true))
        .await
        .expect("control response");

    assert_eq!(next(&mut node_iface).await.context, PacketContext::Response);
}
