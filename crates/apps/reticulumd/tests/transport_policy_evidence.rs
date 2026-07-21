use rand_core::OsRng;
use rns_transport::destination::link::Link;
use rns_transport::destination::{DestinationName, SingleInputDestination};
use rns_transport::hash::{AddressHash, ADDRESS_HASH_SIZE};
use rns_transport::identity::PrivateIdentity;
use rns_transport::iface::{
    IfaceRole, IfaceSource, InterfaceChannel, InterfaceMode, InterfaceSharedConfig, RxMessage,
    TxMessage, TxMessageType,
};
use rns_transport::packet::{HeaderType, Packet, PacketContext, PropagationType};
use rns_transport::transport::{Transport, TransportConfig};
use tokio::time::{timeout, Duration};

fn retransmitting_transport(name: &str) -> Transport {
    let identity = PrivateIdentity::new_from_rand(OsRng);
    let mut config = TransportConfig::new(name, &identity, true);
    config.set_retransmit(true);
    Transport::new(config)
}

fn non_transport_instance(name: &str) -> Transport {
    let identity = PrivateIdentity::new_from_rand(OsRng);
    let mut config = TransportConfig::new(name, &identity, true);
    config.set_transport_enabled(false);
    Transport::new(config)
}

async fn new_probe_iface(transport: &Transport) -> InterfaceChannel {
    transport.iface_manager().lock().await.new_channel(16)
}

async fn new_probe_iface_with_mode(transport: &Transport, mode: InterfaceMode) -> InterfaceChannel {
    transport.iface_manager().lock().await.new_channel_with_role_and_mode(
        16,
        IfaceRole::Unicast,
        mode,
    )
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

fn assert_no_tx(channel: &mut InterfaceChannel, label: &str) {
    assert!(
        matches!(channel.tx_channel.try_recv(), Err(tokio::sync::mpsc::error::TryRecvError::Empty)),
        "{label} should not receive transport output"
    );
}

async fn assert_no_tx_within(channel: &mut InterfaceChannel, label: &str, duration: Duration) {
    assert!(
        timeout(duration, channel.tx_channel.recv()).await.is_err(),
        "{label} should not receive transport output"
    );
    assert_no_tx(channel, label);
}

fn assert_ordinary_rebroadcast(message: &TxMessage, source: &Packet, learned_iface: AddressHash) {
    assert!(matches!(
        message.tx_type,
        TxMessageType::Broadcast(Some(iface)) if iface == learned_iface
    ));
    assert_eq!(message.packet.destination, source.destination);
    assert_eq!(message.packet.context, PacketContext::None);
    assert_eq!(message.packet.header.header_type, HeaderType::Type2);
    assert_eq!(message.packet.header.propagation_type, PropagationType::Broadcast);
    assert_eq!(message.packet.data.as_slice(), source.data.as_slice());
    assert!(
        message.packet.transport.is_some(),
        "ordinary rebroadcast should stamp the local transport id"
    );
}

async fn path_request_packet(destination: &AddressHash) -> Packet {
    let requester = retransmitting_transport("transport-path-request-origin");
    let mut requester_iface = new_probe_iface(&requester).await;
    let tag = vec![0x52; ADDRESS_HASH_SIZE];
    let request_trace = requester
        .request_path(destination, Some(*requester_iface.address()), Some(tag.clone()))
        .await;
    assert_eq!(request_trace.sent_ifaces, 1);

    let path_request = recv_tx(&mut requester_iface, "outbound path request").await.packet;
    assert_eq!(&path_request.data.as_slice()[..ADDRESS_HASH_SIZE], destination.as_slice());
    assert_eq!(&path_request.data.as_slice()[path_request.data.len() - tag.len()..], tag);
    path_request
}

async fn learn_remote_announce(
    transport: &Transport,
    iface: &InterfaceChannel,
    aspect: &str,
) -> Packet {
    let mut remote_destination = SingleInputDestination::new(
        PrivateIdentity::new_from_rand(OsRng),
        DestinationName::new("lxmf", aspect),
    );
    let mut announce = remote_destination.announce(OsRng, None).expect("valid announce");
    announce.header.hops = 2;
    let destination = announce.destination;
    feed_iface_packet(iface, announce.clone()).await;
    wait_for_known_path(transport, &destination).await;
    announce
}

fn link_request(destination: rns_transport::destination::DestinationDesc) -> Packet {
    let (link_events, _keep) = tokio::sync::broadcast::channel(4);
    Link::new(destination, link_events).request()
}

#[tokio::test]
async fn disabled_transport_does_not_relay_known_path_link_requests() {
    let app = non_transport_instance("issue-491-disabled-app");
    let mut host_iface = new_probe_iface_with_mode(&app, InterfaceMode::AccessPoint).await;
    let attacker_iface = new_probe_iface_with_mode(&app, InterfaceMode::AccessPoint).await;
    let mut host_destination = SingleInputDestination::new(
        PrivateIdentity::new_from_rand(OsRng),
        DestinationName::new("lxmf", "issue-491-host"),
    );
    let mut announce = host_destination.announce(OsRng, None).expect("valid host announce");
    announce.header.hops = 1;
    let destination = announce.destination;

    feed_iface_packet(&host_iface, announce).await;
    wait_for_known_path(&app, &destination).await;
    feed_iface_packet(&attacker_iface, link_request(host_destination.desc)).await;

    assert_no_tx_within(
        &mut host_iface,
        "host-facing access-point interface on a disabled transport",
        Duration::from_millis(250),
    )
    .await;
}

#[tokio::test]
async fn disabled_transport_still_accepts_link_requests_for_local_destinations() {
    let mut app = non_transport_instance("disabled-app-local-destination");
    let local_destination = app
        .add_destination(
            PrivateIdentity::new_from_rand(OsRng),
            DestinationName::new("lxmf", "disabled-app-local"),
        )
        .await;
    let destination = local_destination.lock().await.desc;
    let mut requester_iface = new_probe_iface_with_mode(&app, InterfaceMode::AccessPoint).await;

    feed_iface_packet(&requester_iface, link_request(destination)).await;

    let proof = recv_tx(&mut requester_iface, "local destination link proof").await;
    assert!(
        matches!(proof.tx_type, TxMessageType::Direct(iface) if iface == *requester_iface.address())
    );
    assert_eq!(proof.packet.context, PacketContext::LinkRequestProof);
}

fn fresh_announce(destination: &mut SingleInputDestination, hops: u8) -> Packet {
    let mut announce = destination.announce(OsRng, None).expect("valid announce");
    announce.header.hops = hops;
    announce
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
async fn announce_rebroadcast_policy_uses_learned_next_hop_mode_at_transport_boundary() {
    let full_transport = retransmitting_transport("transport-announce-policy-full");
    let mut learned_full = new_probe_iface_with_mode(&full_transport, InterfaceMode::Full).await;
    let mut access_point =
        new_probe_iface_with_mode(&full_transport, InterfaceMode::AccessPoint).await;
    let mut roaming = new_probe_iface_with_mode(&full_transport, InterfaceMode::Roaming).await;
    let mut boundary = new_probe_iface_with_mode(&full_transport, InterfaceMode::Boundary).await;
    let full_announce = learn_remote_announce(&full_transport, &learned_full, "fanout-full").await;

    let roaming_rebroadcast = recv_tx(&mut roaming, "roaming ordinary rebroadcast").await;
    let boundary_rebroadcast = recv_tx(&mut boundary, "boundary ordinary rebroadcast").await;
    assert_ordinary_rebroadcast(&roaming_rebroadcast, &full_announce, *learned_full.address());
    assert_ordinary_rebroadcast(&boundary_rebroadcast, &full_announce, *learned_full.address());
    assert_no_tx(&mut learned_full, "learned full ingress");
    assert_no_tx(&mut access_point, "access point");

    let roaming_transport = retransmitting_transport("transport-announce-policy-roaming");
    let mut learned_roaming =
        new_probe_iface_with_mode(&roaming_transport, InterfaceMode::Roaming).await;
    let mut full = new_probe_iface_with_mode(&roaming_transport, InterfaceMode::Full).await;
    let mut blocked_boundary =
        new_probe_iface_with_mode(&roaming_transport, InterfaceMode::Boundary).await;
    let roaming_announce =
        learn_remote_announce(&roaming_transport, &learned_roaming, "fanout-roaming").await;

    let full_rebroadcast = recv_tx(&mut full, "full ordinary rebroadcast").await;
    assert_ordinary_rebroadcast(&full_rebroadcast, &roaming_announce, *learned_roaming.address());
    assert_no_tx(&mut learned_roaming, "learned roaming ingress");
    assert_no_tx(&mut blocked_boundary, "boundary with roaming learned next-hop");

    let boundary_transport = retransmitting_transport("transport-announce-policy-boundary");
    let mut learned_boundary =
        new_probe_iface_with_mode(&boundary_transport, InterfaceMode::Boundary).await;
    let mut full = new_probe_iface_with_mode(&boundary_transport, InterfaceMode::Full).await;
    let mut blocked_roaming =
        new_probe_iface_with_mode(&boundary_transport, InterfaceMode::Roaming).await;
    let boundary_announce =
        learn_remote_announce(&boundary_transport, &learned_boundary, "fanout-boundary").await;

    let full_rebroadcast = recv_tx(&mut full, "full ordinary rebroadcast").await;
    assert_ordinary_rebroadcast(&full_rebroadcast, &boundary_announce, *learned_boundary.address());
    assert_no_tx(&mut learned_boundary, "learned boundary ingress");
    assert_no_tx(&mut blocked_roaming, "roaming with boundary learned next-hop");
}

#[tokio::test]
async fn announce_rate_target_suppresses_rebroadcast_after_grace_at_transport_boundary() {
    let transport = retransmitting_transport("transport-announce-rate-target");
    let learned_iface = new_probe_iface_with_mode(&transport, InterfaceMode::Full).await;
    let mut boundary_iface = new_probe_iface_with_mode(&transport, InterfaceMode::Boundary).await;
    {
        let manager = transport.iface_manager();
        let mut manager = manager.lock().await;
        assert!(manager.set_shared_config(
            *learned_iface.address(),
            InterfaceSharedConfig {
                announce_rate_target: Some(1),
                announce_rate_grace: Some(1),
                announce_rate_penalty: Some(0),
                ..Default::default()
            },
        ));
    }
    let mut remote_destination = SingleInputDestination::new(
        PrivateIdentity::new_from_rand(OsRng),
        DestinationName::new("lxmf", "announce-rate-target"),
    );

    let first_announce = fresh_announce(&mut remote_destination, 2);
    let destination = first_announce.destination;
    feed_iface_packet(&learned_iface, first_announce.clone()).await;
    wait_for_known_path(&transport, &destination).await;
    let first = recv_tx(&mut boundary_iface, "first announce-rate rebroadcast").await;
    assert_ordinary_rebroadcast(&first, &first_announce, *learned_iface.address());

    let second_announce = fresh_announce(&mut remote_destination, 2);
    feed_iface_packet(&learned_iface, second_announce.clone()).await;
    let second = recv_tx(&mut boundary_iface, "announce-rate rebroadcast within grace").await;
    assert_ordinary_rebroadcast(&second, &second_announce, *learned_iface.address());

    let third_announce = fresh_announce(&mut remote_destination, 2);
    feed_iface_packet(&learned_iface, third_announce).await;
    assert_no_tx_within(
        &mut boundary_iface,
        "announce-rate rebroadcast after grace exceeded",
        Duration::from_millis(200),
    )
    .await;

    tokio::time::sleep(Duration::from_millis(1_100)).await;

    let recovered_announce = fresh_announce(&mut remote_destination, 2);
    feed_iface_packet(&learned_iface, recovered_announce.clone()).await;
    let recovered = recv_tx(&mut boundary_iface, "announce-rate rebroadcast after penalty").await;
    assert_ordinary_rebroadcast(&recovered, &recovered_announce, *learned_iface.address());
}

#[tokio::test]
async fn roaming_same_iface_known_path_request_is_suppressed_at_transport_boundary() {
    let full_transport = retransmitting_transport("transport-full-same-iface-path-response");
    let mut full_iface = new_probe_iface_with_mode(&full_transport, InterfaceMode::Full).await;

    let mut remote_destination = SingleInputDestination::new(
        PrivateIdentity::new_from_rand(OsRng),
        DestinationName::new("lxmf", "delivery"),
    );
    let mut announce = remote_destination.announce(OsRng, None).expect("valid announce");
    announce.header.hops = 2;
    let destination = announce.destination;
    let cached_announce_data = announce.data.clone();
    let path_request = path_request_packet(&destination).await;

    feed_iface_packet(&full_iface, announce.clone()).await;
    wait_for_known_path(&full_transport, &destination).await;
    feed_iface_packet(&full_iface, path_request.clone()).await;

    let full_response = recv_tx(&mut full_iface, "full-mode same-iface path response").await;
    assert!(matches!(
        full_response.tx_type,
        TxMessageType::Direct(iface) if iface == *full_iface.address()
    ));
    assert_eq!(full_response.packet.destination, destination);
    assert_eq!(full_response.packet.context, PacketContext::PathResponse);
    assert_eq!(full_response.packet.header.hops, 2);
    assert_eq!(full_response.packet.data.as_slice(), cached_announce_data.as_slice());

    let roaming_transport = retransmitting_transport("transport-roaming-same-iface-path-response");
    let mut roaming_iface =
        new_probe_iface_with_mode(&roaming_transport, InterfaceMode::Roaming).await;

    feed_iface_packet(&roaming_iface, announce).await;
    wait_for_known_path(&roaming_transport, &destination).await;
    feed_iface_packet(&roaming_iface, path_request).await;

    assert!(
        timeout(Duration::from_millis(100), roaming_iface.tx_channel.recv()).await.is_err(),
        "roaming same-iface path requests should not emit a cached path response"
    );
    assert!(
        matches!(
            roaming_iface.tx_channel.try_recv(),
            Err(tokio::sync::mpsc::error::TryRecvError::Empty)
        ),
        "roaming same-iface path requests should not emit a cached path response"
    );
}

#[tokio::test]
async fn roaming_diff_iface_known_path_response_waits_extra_grace_at_transport_boundary() {
    let transport = retransmitting_transport("transport-roaming-diff-iface-path-response");
    let learned_iface = new_probe_iface_with_mode(&transport, InterfaceMode::Full).await;
    let mut requesting_iface = new_probe_iface_with_mode(&transport, InterfaceMode::Roaming).await;

    let mut remote_destination = SingleInputDestination::new(
        PrivateIdentity::new_from_rand(OsRng),
        DestinationName::new("lxmf", "delivery"),
    );
    let mut announce = remote_destination.announce(OsRng, None).expect("valid announce");
    announce.header.hops = 2;
    let destination = announce.destination;
    let cached_announce_data = announce.data.clone();
    let path_request = path_request_packet(&destination).await;

    feed_iface_packet(&learned_iface, announce).await;
    wait_for_known_path(&transport, &destination).await;
    feed_iface_packet(&requesting_iface, path_request).await;

    assert!(
        timeout(Duration::from_millis(450), requesting_iface.tx_channel.recv()).await.is_err(),
        "roaming different-iface known-path response should wait the extra Python grace"
    );

    let response =
        recv_tx(&mut requesting_iface, "roaming different-iface delayed path response").await;
    assert!(matches!(
        response.tx_type,
        TxMessageType::Direct(iface) if iface == *requesting_iface.address()
    ));
    assert_eq!(response.packet.destination, destination);
    assert_eq!(response.packet.context, PacketContext::PathResponse);
    assert_eq!(response.packet.header.hops, 2);
    assert_eq!(response.packet.data.as_slice(), cached_announce_data.as_slice());
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
