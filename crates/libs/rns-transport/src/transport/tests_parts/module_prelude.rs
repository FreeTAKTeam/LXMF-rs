use super::announce::{handle_announce, release_held_announces};

use super::announce_limits::{AnnounceLimits, AnnounceRateLimit};

use super::path::handle_link_request_as_intermediate;

use super::wire::{handle_data, handle_proof};

use super::*;

use crate::channel::{
    ChannelError, MessageState as ChannelMessageState, SystemMessageTypes, TypedMessage,
};

use crate::destination::link::{Link, LinkEvent, LinkEventData, LinkPayload};

use crate::destination::{DestinationDesc, DestinationName, SingleInputDestination};

use crate::error::RnsError;

use crate::identity::PrivateIdentity;

use crate::packet::{
    DestinationType, Header, HeaderType, PacketContext, PacketDataBuffer, PacketType, PACKET_MDU,
};

use crate::resource::{
    ResourceAdvertisement, ResourceEventKind, ResourceProof, ResourceRequest, MAPHASH_LEN,
};

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

    let link_id = AddressHash::new_from_rand(OsRng);
    let address_hash = AddressHash::new_from_rand(OsRng);
    let payload = LinkPayload::new_from_slice(b"hello");

    let _ = transport.link_in_event_tx.send(LinkEventData {
        id: link_id,
        address_hash,
        event: LinkEvent::Data(Box::new(payload)),
    });

    let received = timeout(Duration::from_millis(200), rx.recv())
        .await
        .expect("expected forwarded payload")
        .expect("broadcast receive");

    assert_eq!(received.destination, link_id);
    assert_eq!(received.data.as_slice(), b"hello");
    assert_eq!(received.payload_mode, ReceivedPayloadMode::FullWire);
}

#[tokio::test]
async fn link_out_payload_is_forwarded_to_received_data() {
    let identity = PrivateIdentity::new_from_rand(OsRng);
    let config = TransportConfig::new("test", &identity, true);
    let transport = Transport::new(config);

    let mut rx = transport.received_data_events();

    let link_id = AddressHash::new_from_rand(OsRng);
    let address_hash = AddressHash::new_from_rand(OsRng);
    let payload = LinkPayload::new_from_slice(b"outbound");

    let _ = transport.link_out_event_tx.send(LinkEventData {
        id: link_id,
        address_hash,
        event: LinkEvent::Data(Box::new(payload)),
    });

    let received = timeout(Duration::from_millis(200), rx.recv())
        .await
        .expect("expected forwarded payload")
        .expect("broadcast receive");

    assert_eq!(received.destination, link_id);
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

    handle_announce(
        &announce,
        handler.lock().await,
        next_hop_iface,
        crate::iface::IfaceSource::None,
    )
    .await;

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
async fn announce_lookup_key_uses_destination_hash() {
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
    handle_announce(&announce, handler.lock().await, iface, crate::iface::IfaceSource::None).await;

    let guard = handler.lock().await;
    let keyed_by_destination = guard.announce_table.packet_for_destination(&announced_destination);
    assert!(keyed_by_destination.is_some(), "announce lookup should be keyed by destination hash");
    let keyed_by_identity = guard.announce_table.packet_for_destination(&announced_identity);
    assert!(keyed_by_identity.is_none(), "identity hash must not be used as announce lookup key");
}

#[tokio::test]
async fn reticulum_path_table_persistence_restores_route_and_identity_from_cached_announce() {
    let temp = tempfile::tempdir().expect("tempdir");
    let local_identity = PrivateIdentity::new_from_rand(OsRng);
    let mut config = TransportConfig::new("test", &local_identity, true);
    config.set_retransmit(true);
    let transport = Transport::new(config);
    let iface = *transport.iface_manager().lock().await.new_channel(16).address();

    let remote_identity = PrivateIdentity::new_from_rand(OsRng);
    let mut remote_destination =
        SingleInputDestination::new(remote_identity, DestinationName::new("lxmf", "delivery"));
    let announce = remote_destination.announce(OsRng, None).expect("valid announce packet");
    let destination = announce.destination;
    let expected_identity = *remote_destination.identity.as_identity();

    handle_announce(
        &announce,
        transport.get_handler().lock().await,
        iface,
        crate::iface::IfaceSource::None,
    )
    .await;

    assert_eq!(transport.save_reticulum_path_table(temp.path()).await.expect("save"), 1);
    assert!(temp.path().join("destination_table").exists());
    let destination_table = std::fs::read(temp.path().join("destination_table")).expect("read");
    let value: rmpv::Value =
        rmpv::decode::read_value(&mut std::io::Cursor::new(destination_table)).expect("msgpack");
    let rmpv::Value::Array(entries) = value else {
        panic!("destination_table must be an array");
    };
    let rmpv::Value::Array(fields) = &entries[0] else {
        panic!("destination_table entry must be an array");
    };
    let rmpv::Value::Binary(interface_hash) = &fields[6] else {
        panic!("interface hash must be binary");
    };
    assert_eq!(interface_hash.len(), crate::hash::HASH_SIZE);
    assert!(temp
        .path()
        .join("cache")
        .join("announces")
        .join(hex::encode(announce.hash().as_slice()))
        .exists());

    let mut restored_config = TransportConfig::new("test", &local_identity, true);
    restored_config.set_retransmit(true);
    let restored = Transport::new(restored_config);
    let restored_iface = *restored.iface_manager().lock().await.new_channel(16).address();
    assert_eq!(restored_iface, iface, "test relies on deterministic iface hashes");
    assert_eq!(restored.restore_reticulum_path_table(temp.path()).await.expect("restore"), 1);
    let restored_identity = restored.destination_identity(&destination).await.expect("identity");
    assert_eq!(restored_identity.public_key_bytes(), expected_identity.public_key_bytes());
    assert_eq!(restored_identity.verifying_key_bytes(), expected_identity.verifying_key_bytes());
    assert!(restored.has_path(&destination).await, "path table entry should be restored");
}

#[tokio::test]
async fn reticulum_tunnel_table_persistence_restores_tunnel_paths_after_reappearance() {
    let temp = tempfile::tempdir().expect("tempdir");
    let local_identity = PrivateIdentity::new_from_rand(OsRng);
    let mut config = TransportConfig::new("test", &local_identity, true);
    config.set_retransmit(true);
    let transport = Transport::new(config);
    let iface = *transport.iface_manager().lock().await.new_channel(16).address();
    let iface_hash = transport.iface_manager().lock().await.full_hash(&iface).expect("iface hash");

    let tunnel_identity = PrivateIdentity::new_from_rand(OsRng);
    let tunnel_synth = super::tunnels::synthesize_tunnel_packet(&tunnel_identity, iface_hash);
    {
        let handler = transport.get_handler();
        let mut handler = handler.lock().await;
        super::tunnels::handle_tunnel_synthesize_packet(&tunnel_synth, &mut handler, iface).await;
    }

    let remote_identity = PrivateIdentity::new_from_rand(OsRng);
    let mut remote_destination =
        SingleInputDestination::new(remote_identity, DestinationName::new("lxmf", "delivery"));
    let announce = remote_destination.announce(OsRng, None).expect("valid announce packet");
    let destination = announce.destination;
    handle_announce(
        &announce,
        transport.get_handler().lock().await,
        iface,
        crate::iface::IfaceSource::None,
    )
    .await;

    assert_eq!(transport.save_reticulum_path_table(temp.path()).await.expect("save"), 1);
    assert!(temp.path().join("tunnels").exists());
    std::fs::remove_file(temp.path().join("destination_table")).expect("remove active path table");

    let mut restored_config = TransportConfig::new("test", &local_identity, true);
    restored_config.set_retransmit(true);
    let restored = Transport::new(restored_config);
    let restored_iface = *restored.iface_manager().lock().await.new_channel(16).address();
    let restored_iface_hash =
        restored.iface_manager().lock().await.full_hash(&restored_iface).expect("iface hash");
    assert_eq!(restored_iface_hash, iface_hash, "test relies on deterministic iface hashes");

    assert_eq!(restored.restore_reticulum_path_table(temp.path()).await.expect("restore"), 0);
    assert!(
        !restored.has_path(&destination).await,
        "tunnel table load should not restore active path before tunnel reappears"
    );

    let tunnel_synth =
        super::tunnels::synthesize_tunnel_packet(&tunnel_identity, restored_iface_hash);
    {
        let handler = restored.get_handler();
        let mut handler = handler.lock().await;
        super::tunnels::handle_tunnel_synthesize_packet(
            &tunnel_synth,
            &mut handler,
            restored_iface,
        )
        .await;
    }

    assert!(
        restored.has_path(&destination).await,
        "tunnel reappearance should restore the persisted tunnel path"
    );
    assert!(restored.destination_identity(&destination).await.is_some());
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
    handle_announce(&first_announce, handler.lock().await, iface, crate::iface::IfaceSource::None)
        .await;
    let first_event = timeout(Duration::from_millis(200), announce_rx.recv())
        .await
        .expect("first announce should emit")
        .expect("broadcast receive");
    assert_eq!(first_event.hops, 4);
    tokio::time::sleep(Duration::from_millis(1)).await;

    let mut higher_hop_destination = SingleInputDestination::new(
        PrivateIdentity::new_from_rand(OsRng),
        DestinationName::new("lxmf", "delivery"),
    );
    let mut higher_hop_announce = higher_hop_destination.announce(OsRng, None).expect("announce");
    higher_hop_announce.header.hops = 3;
    handle_announce(
        &higher_hop_announce,
        handler.lock().await,
        iface,
        crate::iface::IfaceSource::None,
    )
    .await;
    tokio::time::sleep(Duration::from_millis(1)).await;

    let mut lower_hop_destination = SingleInputDestination::new(
        PrivateIdentity::new_from_rand(OsRng),
        DestinationName::new("lxmf", "delivery"),
    );
    let mut lower_hop_announce = lower_hop_destination.announce(OsRng, None).expect("announce");
    lower_hop_announce.header.hops = 1;
    handle_announce(
        &lower_hop_announce,
        handler.lock().await,
        iface,
        crate::iface::IfaceSource::None,
    )
    .await;

    let mut immediate_hops = Vec::new();
    while let Ok(event) = announce_rx.try_recv() {
        immediate_hops.push(event.hops);
    }
    assert!(
        immediate_hops.iter().all(|hops| matches!(*hops, 1 | 3)),
        "unexpected immediate announce release sequence {immediate_hops:?}"
    );
    if let Some(hops) = immediate_hops.first().copied() {
        assert_eq!(hops, 3);
    }

    tokio::time::sleep(Duration::from_millis(80)).await;
    if immediate_hops.contains(&1) {
        release_held_announces(handler.lock().await).await;
        assert!(matches!(
            announce_rx.try_recv(),
            Err(tokio::sync::broadcast::error::TryRecvError::Empty)
        ));
    } else {
        let mut released_lowest = None;
        for _ in 0..4 {
            release_held_announces(handler.lock().await).await;
            if let Ok(event) = timeout(Duration::from_millis(120), announce_rx.recv()).await {
                released_lowest = Some(event.expect("broadcast receive"));
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        let released_lowest = released_lowest.expect("lowest-hop held announce should emit");
        assert_eq!(released_lowest.hops, 1);
    }

    tokio::time::sleep(Duration::from_millis(50)).await;
    release_held_announces(handler.lock().await).await;

    if immediate_hops.contains(&3) {
        assert!(matches!(
            announce_rx.try_recv(),
            Err(tokio::sync::broadcast::error::TryRecvError::Empty)
        ));
    } else {
        tokio::time::sleep(Duration::from_millis(25)).await;
        release_held_announces(handler.lock().await).await;
        let released_next = timeout(Duration::from_millis(200), announce_rx.recv())
            .await
            .expect("next held announce should emit")
            .expect("broadcast receive");
        assert_eq!(released_next.hops, 3);
    }
}
