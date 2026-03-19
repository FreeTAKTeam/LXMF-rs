use super::announce::{handle_announce, release_held_announces};
use super::announce_limits::{AnnounceLimits, AnnounceRateLimit};
use super::path::handle_link_request_as_intermediate;
use super::wire::{handle_data, handle_proof};
use super::*;

use crate::channel::{
    ChannelError, MessageState as ChannelMessageState, SystemMessageTypes, TypedMessage,
};
use crate::destination::link::{Link, LinkEvent, LinkEventData, LinkPayload};
use crate::destination::{DestinationName, SingleInputDestination};
use crate::error::RnsError;
use crate::identity::PrivateIdentity;
use crate::packet::{Header, HeaderType};
use rand_core::OsRng;
use std::sync::Mutex as StdMutex;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use tokio::time::{timeout, Duration};

#[tokio::test]
async fn link_in_payload_is_forwarded_to_received_data() {
    let identity = PrivateIdentity::new_from_rand(OsRng);
    let config = TransportConfig::new("test", &identity, true);
    let transport = Transport::new(config);

    let mut rx = transport.received_data_events();

    let address_hash = AddressHash::new_from_rand(OsRng);
    let payload = LinkPayload::new_from_slice(b"hello");

    let _ = transport.link_in_event_tx.send(LinkEventData {
        id: AddressHash::new_from_rand(OsRng),
        address_hash,
        event: LinkEvent::Data(Box::new(payload)),
    });

    let received = timeout(Duration::from_millis(200), rx.recv())
        .await
        .expect("expected forwarded payload")
        .expect("broadcast receive");

    assert_eq!(received.destination, address_hash);
    assert_eq!(received.data.as_slice(), b"hello");
    assert_eq!(received.payload_mode, ReceivedPayloadMode::FullWire);
}

#[tokio::test]
async fn link_out_payload_is_forwarded_to_received_data() {
    let identity = PrivateIdentity::new_from_rand(OsRng);
    let config = TransportConfig::new("test", &identity, true);
    let transport = Transport::new(config);

    let mut rx = transport.received_data_events();

    let address_hash = AddressHash::new_from_rand(OsRng);
    let payload = LinkPayload::new_from_slice(b"outbound");

    let _ = transport.link_out_event_tx.send(LinkEventData {
        id: AddressHash::new_from_rand(OsRng),
        address_hash,
        event: LinkEvent::Data(Box::new(payload)),
    });

    let received = timeout(Duration::from_millis(200), rx.recv())
        .await
        .expect("expected forwarded payload")
        .expect("broadcast receive");

    assert_eq!(received.destination, address_hash);
    assert_eq!(received.data.as_slice(), b"outbound");
    assert_eq!(received.payload_mode, ReceivedPayloadMode::FullWire);
}

#[tokio::test]
async fn drop_duplicates() {
    let mut config: TransportConfig = Default::default();
    config.set_retransmit(true);

    let transport = Transport::new(config);
    let handler = transport.get_handler();

    let _source1 = AddressHash::new_from_slice(&[1u8; 32]);
    let _source2 = AddressHash::new_from_slice(&[2u8; 32]);
    let next_hop_iface = AddressHash::new_from_slice(&[3u8; 32]);
    let destination = AddressHash::new_from_slice(&[4u8; 32]);

    let mut announce: Packet = Default::default();
    announce.header.header_type = HeaderType::Type2;
    announce.header.packet_type = PacketType::Announce;
    announce.header.hops = 3;
    announce.transport = Some(destination);

    assert!(handler.lock().await.filter_duplicate_packets(&announce).await);

    handle_announce(&announce, handler.lock().await, next_hop_iface).await;

    let data_packet: Packet = Packet {
        data: PacketDataBuffer::new_from_slice(b"foo"),
        destination,
        ..Default::default()
    };
    let duplicate: Packet = data_packet;

    let mut different_packet = data_packet;
    different_packet.data = PacketDataBuffer::new_from_slice(b"bar");

    assert!(handler.lock().await.filter_duplicate_packets(&data_packet).await);
    assert!(!handler.lock().await.filter_duplicate_packets(&duplicate).await);
    assert!(handler.lock().await.filter_duplicate_packets(&different_packet).await);

    tokio::time::sleep(Duration::from_secs(2)).await;
    handler.lock().await.packet_cache.lock().await.release(Duration::from_secs(1));

    // Packet should have been removed from cache (stale)
    assert!(handler.lock().await.filter_duplicate_packets(&duplicate).await);
}

#[tokio::test]
async fn announce_retransmit_key_uses_destination_hash() {
    let local_identity = PrivateIdentity::new_from_rand(OsRng);
    let mut config = TransportConfig::new("test", &local_identity, true);
    config.set_retransmit(true);
    let transport = Transport::new(config);
    let handler = transport.get_handler();

    let remote_identity = PrivateIdentity::new_from_rand(OsRng);
    let mut remote_destination =
        SingleInputDestination::new(remote_identity, DestinationName::new("lxmf", "delivery"));
    let announce = remote_destination.announce(OsRng, None).expect("valid announce packet");

    let announced_destination = announce.destination;
    let announced_identity = *remote_destination.identity.address_hash();
    assert_ne!(
        announced_destination, announced_identity,
        "destination hash must differ from identity hash for named destinations"
    );

    let iface = AddressHash::new_from_rand(OsRng);
    handle_announce(&announce, handler.lock().await, iface).await;
    tokio::time::sleep(Duration::from_millis(550)).await;

    let mut guard = handler.lock().await;
    let transport_id = *guard.config.identity.address_hash();
    let keyed_by_destination =
        guard.announce_table.new_packet(&announced_destination, &transport_id);
    assert!(
        keyed_by_destination.is_some(),
        "announce retransmit should be keyed by destination hash"
    );
    let keyed_by_identity = guard.announce_table.new_packet(&announced_identity, &transport_id);
    assert!(
        keyed_by_identity.is_none(),
        "identity hash must not be used as announce retransmit key"
    );
}

#[tokio::test]
async fn unknown_announces_are_held_per_interface_and_released_by_lowest_hops() {
    let local_identity = PrivateIdentity::new_from_rand(OsRng);
    let config = TransportConfig::new("test", &local_identity, true);
    let transport = Transport::new(config);
    let handler = transport.get_handler();
    let mut announce_rx = transport.recv_announces().await;

    handler.lock().await.announce_limits = AnnounceLimits::with_rate_limit(AnnounceRateLimit {
        incoming_freq_samples: 3,
        max_held_announces: 8,
        new_time: Duration::from_secs(3600),
        burst_freq_new: 100.0,
        burst_freq: 100.0,
        burst_hold: Duration::from_millis(20),
        burst_penalty: Duration::from_millis(20),
        held_release_interval: Duration::from_millis(10),
    });

    let iface = AddressHash::new_from_rand(OsRng);

    let mut first_destination = SingleInputDestination::new(
        PrivateIdentity::new_from_rand(OsRng),
        DestinationName::new("lxmf", "delivery"),
    );
    let mut first_announce = first_destination.announce(OsRng, None).expect("announce");
    first_announce.header.hops = 4;
    handle_announce(&first_announce, handler.lock().await, iface).await;
    let first_event = timeout(Duration::from_millis(200), announce_rx.recv())
        .await
        .expect("first announce should emit")
        .expect("broadcast receive");
    assert_eq!(first_event.hops, 4);
    tokio::time::sleep(Duration::from_millis(5)).await;

    let mut higher_hop_destination = SingleInputDestination::new(
        PrivateIdentity::new_from_rand(OsRng),
        DestinationName::new("lxmf", "delivery"),
    );
    let mut higher_hop_announce = higher_hop_destination.announce(OsRng, None).expect("announce");
    higher_hop_announce.header.hops = 3;
    handle_announce(&higher_hop_announce, handler.lock().await, iface).await;
    tokio::time::sleep(Duration::from_millis(5)).await;

    let mut lower_hop_destination = SingleInputDestination::new(
        PrivateIdentity::new_from_rand(OsRng),
        DestinationName::new("lxmf", "delivery"),
    );
    let mut lower_hop_announce = lower_hop_destination.announce(OsRng, None).expect("announce");
    lower_hop_announce.header.hops = 1;
    handle_announce(&lower_hop_announce, handler.lock().await, iface).await;

    assert!(matches!(
        announce_rx.try_recv(),
        Err(tokio::sync::broadcast::error::TryRecvError::Empty)
    ));

    tokio::time::sleep(Duration::from_millis(55)).await;
    release_held_announces(handler.lock().await).await;
    assert!(matches!(
        announce_rx.try_recv(),
        Err(tokio::sync::broadcast::error::TryRecvError::Empty)
    ));

    tokio::time::sleep(Duration::from_millis(25)).await;
    release_held_announces(handler.lock().await).await;

    let released_lowest = timeout(Duration::from_millis(200), announce_rx.recv())
        .await
        .expect("lowest-hop held announce should emit")
        .expect("broadcast receive");
    assert_eq!(released_lowest.hops, 1);

    tokio::time::sleep(Duration::from_millis(15)).await;
    release_held_announces(handler.lock().await).await;

    let released_next = timeout(Duration::from_millis(200), announce_rx.recv())
        .await
        .expect("next held announce should emit")
        .expect("broadcast receive");
    assert_eq!(released_next.hops, 3);
}

#[tokio::test]
async fn learned_announces_are_not_held_after_route_is_known() {
    let local_identity = PrivateIdentity::new_from_rand(OsRng);
    let config = TransportConfig::new("test", &local_identity, true);
    let transport = Transport::new(config);
    let handler = transport.get_handler();
    let mut announce_rx = transport.recv_announces().await;

    handler.lock().await.announce_limits = AnnounceLimits::with_rate_limit(AnnounceRateLimit {
        incoming_freq_samples: 3,
        max_held_announces: 8,
        new_time: Duration::from_secs(3600),
        burst_freq_new: 100.0,
        burst_freq: 100.0,
        burst_hold: Duration::from_millis(20),
        burst_penalty: Duration::from_millis(20),
        held_release_interval: Duration::from_millis(10),
    });

    let iface = AddressHash::new_from_rand(OsRng);
    let mut destination = SingleInputDestination::new(
        PrivateIdentity::new_from_rand(OsRng),
        DestinationName::new("lxmf", "delivery"),
    );
    let announce = destination.announce(OsRng, None).expect("announce");

    handle_announce(&announce, handler.lock().await, iface).await;
    timeout(Duration::from_millis(200), announce_rx.recv())
        .await
        .expect("first announce should emit")
        .expect("broadcast receive");

    tokio::time::sleep(Duration::from_millis(5)).await;
    handle_announce(&announce, handler.lock().await, iface).await;

    let repeated = timeout(Duration::from_millis(200), announce_rx.recv())
        .await
        .expect("known announce should bypass ingress hold")
        .expect("broadcast receive");
    assert_eq!(repeated.hops, announce.header.hops);
}

#[tokio::test]
async fn send_packet_with_outcome_reports_missing_identity() {
    let identity = PrivateIdentity::new_from_rand(OsRng);
    let config = TransportConfig::new("test", &identity, true);
    let transport = Transport::new(config);

    let packet = Packet { destination: AddressHash::new_from_rand(OsRng), ..Default::default() };
    let outcome = transport.send_packet_with_outcome(packet).await;

    assert_eq!(outcome, SendPacketOutcome::DroppedMissingDestinationIdentity);
}

#[tokio::test]
async fn send_packet_with_outcome_reports_no_route() {
    let identity = PrivateIdentity::new_from_rand(OsRng);
    let config = TransportConfig::new("test", &identity, false);
    let transport = Transport::new(config);

    let packet = Packet {
        header: Header { packet_type: PacketType::Data, ..Default::default() },
        context: PacketContext::KeepAlive,
        data: PacketDataBuffer::new_from_slice(&[KEEP_ALIVE_REQUEST]),
        destination: AddressHash::new_from_rand(OsRng),
        ..Default::default()
    };
    let outcome = transport.send_packet_with_outcome(packet).await;

    assert_eq!(outcome, SendPacketOutcome::DroppedNoRoute);
}

#[tokio::test]
async fn send_packet_with_outcome_drops_announce_without_route() {
    let identity = PrivateIdentity::new_from_rand(OsRng);
    let config = TransportConfig::new("test", &identity, false);
    let transport = Transport::new(config);

    let packet = Packet {
        header: Header { packet_type: PacketType::Announce, ..Default::default() },
        destination: AddressHash::new_from_rand(OsRng),
        ..Default::default()
    };
    let outcome = transport.send_packet_with_outcome(packet).await;

    assert_eq!(outcome, SendPacketOutcome::DroppedNoRoute);
}

struct CountingReceiptHandler {
    count: Arc<AtomicUsize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TestTypedMessage {
    value: Vec<u8>,
}

impl TypedMessage for TestTypedMessage {
    const MSG_TYPE: u16 = 0x7777;

    fn encode(&self) -> Vec<u8> {
        self.value.clone()
    }

    fn decode(payload: &[u8]) -> Result<Self, crate::channel::ChannelError> {
        Ok(Self { value: payload.to_vec() })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ReservedTypedMessage;

impl TypedMessage for ReservedTypedMessage {
    const MSG_TYPE: u16 = SystemMessageTypes::StreamData as u16;

    fn encode(&self) -> Vec<u8> {
        Vec::new()
    }

    fn decode(_payload: &[u8]) -> Result<Self, crate::channel::ChannelError> {
        Ok(Self)
    }
}

impl ReceiptHandler for CountingReceiptHandler {
    fn on_receipt(&self, _receipt: &DeliveryReceipt) {
        self.count.fetch_add(1, Ordering::SeqCst);
    }
}

#[tokio::test]
async fn handle_inbound_for_test_rejects_forged_destination_proof() {
    let local_identity = PrivateIdentity::new_from_rand(OsRng);
    let config = TransportConfig::new("test", &local_identity, true);
    let mut transport = Transport::new(config);
    let handler = transport.get_handler();

    let remote_identity = PrivateIdentity::new_from_rand(OsRng);
    let mut remote_destination =
        SingleInputDestination::new(remote_identity, DestinationName::new("lxmf", "delivery"));
    let announce = remote_destination.announce(OsRng, None).expect("valid announce packet");
    handle_announce(&announce, handler.lock().await, AddressHash::new_from_rand(OsRng)).await;

    let count = Arc::new(AtomicUsize::new(0));
    transport.set_receipt_handler(Box::new(CountingReceiptHandler { count: count.clone() })).await;

    let packet_hash = [0x44u8; HASH_SIZE];
    let mut data = PacketDataBuffer::new();
    data.safe_write(&packet_hash);
    data.safe_write(&[0xAA; ed25519_dalek::SIGNATURE_LENGTH]);
    let packet = Packet {
        header: Header { packet_type: PacketType::Proof, ..Default::default() },
        destination: announce.destination,
        context: PacketContext::None,
        data,
        ..Default::default()
    };

    transport.handle_inbound_for_test(packet).await;

    assert_eq!(count.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn handle_inbound_for_test_accepts_valid_destination_proof() {
    let local_identity = PrivateIdentity::new_from_rand(OsRng);
    let config = TransportConfig::new("test", &local_identity, true);
    let mut transport = Transport::new(config);
    let handler = transport.get_handler();

    let remote_identity = PrivateIdentity::new_from_rand(OsRng);
    let mut remote_destination =
        SingleInputDestination::new(remote_identity, DestinationName::new("lxmf", "delivery"));
    let announce = remote_destination.announce(OsRng, None).expect("valid announce packet");
    handle_announce(&announce, handler.lock().await, AddressHash::new_from_rand(OsRng)).await;

    let count = Arc::new(AtomicUsize::new(0));
    transport.set_receipt_handler(Box::new(CountingReceiptHandler { count: count.clone() })).await;

    let packet_hash = [0x55u8; HASH_SIZE];
    let signature = remote_destination.identity.sign(&packet_hash).to_bytes();
    let mut data = PacketDataBuffer::new();
    data.safe_write(&packet_hash);
    data.safe_write(&signature);
    let packet = Packet {
        header: Header { packet_type: PacketType::Proof, ..Default::default() },
        destination: announce.destination,
        context: PacketContext::None,
        data,
        ..Default::default()
    };

    transport.handle_inbound_for_test(packet).await;

    assert_eq!(count.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn routed_link_request_proof_requires_matching_iface_and_signature() {
    let local_identity = PrivateIdentity::new_from_rand(OsRng);
    let mut config = TransportConfig::new("test", &local_identity, true);
    config.set_retransmit(true);
    let transport = Transport::new(config);
    let handler = transport.get_handler();

    let remote_identity = PrivateIdentity::new_from_rand(OsRng);
    let mut remote_destination =
        SingleInputDestination::new(remote_identity, DestinationName::new("lxmf", "delivery"));
    let announce = remote_destination.announce(OsRng, None).expect("valid announce packet");
    handle_announce(&announce, handler.lock().await, AddressHash::new_from_rand(OsRng)).await;

    let received_from = AddressHash::new_from_slice(&[1u8; 16]);
    let next_hop = AddressHash::new_from_slice(&[2u8; 16]);
    let next_hop_iface = AddressHash::new_from_slice(&[3u8; 16]);

    let (tx, _) = tokio::sync::broadcast::channel(4);
    let mut outbound_link =
        crate::destination::link::Link::new(remote_destination.desc, tx.clone());
    let request = outbound_link.request();
    handle_link_request_as_intermediate(
        received_from,
        next_hop,
        next_hop_iface,
        &request,
        handler.lock().await,
    )
    .await;

    let mut inbound_link = crate::destination::link::Link::new_from_request(
        &request,
        remote_destination.sign_key().clone(),
        remote_destination.desc,
        tx,
    )
    .expect("link from request");

    let valid_proof = inbound_link.prove();
    handle_proof(valid_proof, handler.clone(), AddressHash::new_from_slice(&[9u8; 16])).await;
    {
        let guard = handler.lock().await;
        assert!(
            guard.link_table.original_destination(outbound_link.id()).is_none(),
            "proof from wrong interface must not validate"
        );
    }

    let mut bad_signature_proof = inbound_link.prove();
    bad_signature_proof.data.as_mut_slice()[0] ^= 0x01;
    handle_proof(bad_signature_proof, handler.clone(), next_hop_iface).await;
    {
        let guard = handler.lock().await;
        assert!(
            guard.link_table.original_destination(outbound_link.id()).is_none(),
            "invalid proof signature must not validate"
        );
    }

    let valid_proof = inbound_link.prove();
    handle_proof(valid_proof, handler.clone(), next_hop_iface).await;
    {
        let guard = handler.lock().await;
        assert_eq!(
            guard.link_table.original_destination(outbound_link.id()),
            Some(request.destination)
        );
    }
}

#[tokio::test]
async fn transport_register_channel_handler_dispatches_inbound_channel_message() {
    let local_identity = PrivateIdentity::new_from_rand(OsRng);
    let config = TransportConfig::new("test", &local_identity, true);
    let transport = Transport::new(config);
    let handler = transport.get_handler();

    let signer = PrivateIdentity::new_from_rand(OsRng);
    let identity = *signer.as_identity();
    let destination = crate::destination::DestinationDesc {
        identity,
        address_hash: identity.address_hash,
        name: DestinationName::new("lxmf", "delivery"),
    };
    let (tx, _) = tokio::sync::broadcast::channel(8);
    let mut outbound = Link::new(destination, tx.clone());
    let request = outbound.request();
    let mut inbound = Link::new_from_request(&request, signer.sign_key().clone(), destination, tx)
        .expect("link request should parse");
    let iface = AddressHash::new_from_rand(OsRng);
    assert!(matches!(
        outbound.handle_packet(&inbound.prove(), iface),
        crate::destination::link::LinkHandleResult::Activated
    ));

    let link_id = *outbound.id();
    handler.lock().await.out_links.insert(destination.address_hash, Arc::new(Mutex::new(outbound)));

    let seen = Arc::new(StdMutex::new(Vec::new()));
    let seen_clone = seen.clone();
    transport
        .register_channel_handler(&link_id, 0x4444, move |envelope| {
            seen_clone.lock().expect("lock").push(envelope);
            true
        })
        .await
        .expect("register handler");

    let (_sequence, packet) = inbound
        .send_channel_message(0x4444, b"transport-channel".to_vec())
        .expect("channel message");
    handle_data(&packet, iface, handler.lock().await).await;

    let seen = seen.lock().expect("lock");
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].msg_type, 0x4444);
    assert_eq!(seen[0].payload, b"transport-channel");
}

#[tokio::test]
async fn transport_channel_message_state_tracks_delivery() {
    let local_identity = PrivateIdentity::new_from_rand(OsRng);
    let config = TransportConfig::new("test", &local_identity, true);
    let transport = Transport::new(config);
    let handler = transport.get_handler();

    let signer = PrivateIdentity::new_from_rand(OsRng);
    let identity = *signer.as_identity();
    let destination = crate::destination::DestinationDesc {
        identity,
        address_hash: identity.address_hash,
        name: DestinationName::new("lxmf", "delivery"),
    };
    let (tx, _) = tokio::sync::broadcast::channel(8);
    let mut outbound = Link::new(destination, tx.clone());
    let request = outbound.request();
    let mut inbound = Link::new_from_request(&request, signer.sign_key().clone(), destination, tx)
        .expect("link request should parse");
    let iface = AddressHash::new_from_rand(OsRng);
    assert!(matches!(
        outbound.handle_packet(&inbound.prove(), iface),
        crate::destination::link::LinkHandleResult::Activated
    ));

    let link_id = *outbound.id();
    let outbound = Arc::new(Mutex::new(outbound));
    handler.lock().await.out_links.insert(destination.address_hash, outbound.clone());
    inbound.register_channel_handler(0x55AA, |_| true);

    let (sequence, packet) = {
        let mut outbound = outbound.lock().await;
        outbound.send_channel_message(0x55AA, b"tracked".to_vec()).expect("channel message")
    };
    assert_eq!(
        transport.channel_message_state(&link_id, sequence).await.expect("state"),
        ChannelMessageState::Sent
    );

    let proof = match inbound.handle_packet(&packet, iface) {
        crate::destination::link::LinkHandleResult::Proof(proof) => proof,
        _ => panic!("channel packet should generate proof"),
    };
    {
        let mut outbound = outbound.lock().await;
        assert!(matches!(
            outbound.handle_packet(&proof, iface),
            crate::destination::link::LinkHandleResult::None
        ));
    }
    assert_eq!(
        transport.channel_message_state(&link_id, sequence).await.expect("state"),
        ChannelMessageState::Delivered
    );
}

#[tokio::test]
async fn transport_channel_handle_reports_missing_link() {
    let local_identity = PrivateIdentity::new_from_rand(OsRng);
    let config = TransportConfig::new("test", &local_identity, true);
    let transport = Transport::new(config);

    let link_id = AddressHash::new_from_rand(OsRng);
    let channel = transport.channel(link_id);

    assert_eq!(channel.link_id(), link_id);
    assert!(matches!(
        channel.message_state(0).await,
        Err(crate::channel::ChannelError::LinkNotReady)
    ));
}

#[tokio::test]
async fn transport_channel_handle_supports_typed_messages() {
    let local_identity = PrivateIdentity::new_from_rand(OsRng);
    let config = TransportConfig::new("test", &local_identity, true);
    let transport = Transport::new(config);
    let handler = transport.get_handler();

    let signer = PrivateIdentity::new_from_rand(OsRng);
    let identity = *signer.as_identity();
    let destination = crate::destination::DestinationDesc {
        identity,
        address_hash: identity.address_hash,
        name: DestinationName::new("lxmf", "delivery"),
    };
    let (tx, _) = tokio::sync::broadcast::channel(8);
    let mut outbound = Link::new(destination, tx.clone());
    let request = outbound.request();
    let mut inbound = Link::new_from_request(&request, signer.sign_key().clone(), destination, tx)
        .expect("link request should parse");
    let iface = AddressHash::new_from_rand(OsRng);
    assert!(matches!(
        outbound.handle_packet(&inbound.prove(), iface),
        crate::destination::link::LinkHandleResult::Activated
    ));

    let link_id = *outbound.id();
    handler.lock().await.out_links.insert(destination.address_hash, Arc::new(Mutex::new(outbound)));
    let channel = transport.channel(link_id);

    let seen = Arc::new(StdMutex::new(Vec::new()));
    let seen_clone = seen.clone();
    channel
        .register_typed_handler::<TestTypedMessage, _>(move |message| {
            seen_clone.lock().expect("lock").push(message);
            true
        })
        .await
        .expect("typed handler");

    let message = TestTypedMessage { value: b"typed-payload".to_vec() };
    let (_sequence, packet) = inbound
        .send_channel_message(TestTypedMessage::MSG_TYPE, message.encode())
        .expect("typed channel packet");
    handle_data(&packet, iface, handler.lock().await).await;

    let seen = seen.lock().expect("lock");
    assert_eq!(seen.as_slice(), &[message]);
}

#[tokio::test]
async fn transport_channel_handle_can_remove_handlers() {
    let local_identity = PrivateIdentity::new_from_rand(OsRng);
    let config = TransportConfig::new("test", &local_identity, true);
    let transport = Transport::new(config);
    let handler = transport.get_handler();

    let signer = PrivateIdentity::new_from_rand(OsRng);
    let identity = *signer.as_identity();
    let destination = crate::destination::DestinationDesc {
        identity,
        address_hash: identity.address_hash,
        name: DestinationName::new("lxmf", "delivery"),
    };
    let (tx, _) = tokio::sync::broadcast::channel(8);
    let mut outbound = Link::new(destination, tx.clone());
    let request = outbound.request();
    let mut inbound = Link::new_from_request(&request, signer.sign_key().clone(), destination, tx)
        .expect("link request should parse");
    let iface = AddressHash::new_from_rand(OsRng);
    assert!(matches!(
        outbound.handle_packet(&inbound.prove(), iface),
        crate::destination::link::LinkHandleResult::Activated
    ));

    let link_id = *outbound.id();
    handler.lock().await.out_links.insert(destination.address_hash, Arc::new(Mutex::new(outbound)));
    let channel = transport.channel(link_id);

    let seen = Arc::new(StdMutex::new(Vec::new()));
    let seen_clone = seen.clone();
    let handler_id = channel
        .register_handler(0x7777, move |envelope| {
            seen_clone.lock().expect("lock").push(envelope);
            true
        })
        .await
        .expect("register handler");
    assert!(channel.remove_handler(handler_id).await.expect("remove handler"));
    assert!(!channel.remove_handler(handler_id).await.expect("remove handler twice"));

    let (_sequence, packet) =
        inbound.send_channel_message(0x7777, b"removed".to_vec()).expect("channel message");
    handle_data(&packet, iface, handler.lock().await).await;

    assert!(seen.lock().expect("lock").is_empty());
}

#[tokio::test]
async fn transport_channel_handle_rejects_reserved_typed_messages_by_default() {
    let local_identity = PrivateIdentity::new_from_rand(OsRng);
    let config = TransportConfig::new("test", &local_identity, true);
    let transport = Transport::new(config);

    let link_id = AddressHash::new_from_rand(OsRng);
    let channel = transport.channel(link_id);

    assert!(matches!(
        channel.register_typed_handler::<ReservedTypedMessage, _>(|_message| true).await,
        Err(ChannelError::InvalidMessageType)
    ));
}

#[tokio::test]
async fn transport_channel_handle_can_open_channel_without_handlers() {
    let local_identity = PrivateIdentity::new_from_rand(OsRng);
    let config = TransportConfig::new("test", &local_identity, true);
    let transport = Transport::new(config);
    let handler = transport.get_handler();

    let signer = PrivateIdentity::new_from_rand(OsRng);
    let identity = *signer.as_identity();
    let destination = crate::destination::DestinationDesc {
        identity,
        address_hash: identity.address_hash,
        name: DestinationName::new("lxmf", "delivery"),
    };
    let (tx, _) = tokio::sync::broadcast::channel(8);
    let mut outbound = Link::new(destination, tx.clone());
    let request = outbound.request();
    let mut inbound = Link::new_from_request(&request, signer.sign_key().clone(), destination, tx)
        .expect("link request should parse");
    let iface = AddressHash::new_from_rand(OsRng);
    assert!(matches!(
        outbound.handle_packet(&inbound.prove(), iface),
        crate::destination::link::LinkHandleResult::Activated
    ));

    let link_id = *outbound.id();
    let outbound = Arc::new(Mutex::new(outbound));
    handler.lock().await.out_links.insert(destination.address_hash, outbound.clone());
    let channel = transport.channel(link_id);
    channel.open().await.expect("open channel");

    let (_sequence, packet) =
        inbound.send_channel_message(0xEEEE, b"open-no-handler".to_vec()).expect("channel message");
    let result = outbound.lock().await.handle_packet(&packet, iface);
    assert!(matches!(result, crate::destination::link::LinkHandleResult::Proof(_)));
}

#[tokio::test]
async fn send_resource_returns_error_when_advertisement_dispatch_drops() {
    let local_identity = PrivateIdentity::new_from_rand(OsRng);
    let config = TransportConfig::new("test", &local_identity, true);
    let transport = Transport::new(config);
    let handler = transport.get_handler();

    let signer = PrivateIdentity::new_from_rand(OsRng);
    let identity = *signer.as_identity();
    let destination = crate::destination::DestinationDesc {
        identity,
        address_hash: identity.address_hash,
        name: DestinationName::new("lxmf", "delivery"),
    };
    let (tx, _) = tokio::sync::broadcast::channel(8);
    let mut outbound = Link::new(destination, tx.clone());
    let request = outbound.request();
    let mut inbound = Link::new_from_request(&request, signer.sign_key().clone(), destination, tx)
        .expect("link request should parse");
    let iface = AddressHash::new_from_rand(OsRng);
    assert!(matches!(
        outbound.handle_packet(&inbound.prove(), iface),
        crate::destination::link::LinkHandleResult::Activated
    ));

    let link_id = *outbound.id();
    handler.lock().await.out_links.insert(destination.address_hash, Arc::new(Mutex::new(outbound)));

    let result = transport.send_resource(&link_id, b"resource".to_vec(), None).await;
    assert!(matches!(result, Err(RnsError::ConnectionError)));

    let guard = handler.lock().await;
    assert!(guard.resource_manager.has_no_outbound_state());
}
