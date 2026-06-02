use super::*;
use lxmf_core::announce::encode_delivery_display_name_app_data;

#[test]
fn announce_record_from_raw_extracts_lxmf_delivery_display_name() {
    let app_data =
        encode_delivery_display_name_app_data("Alice Router").expect("encoded display name");

    let record = AnnounceRecord::from_raw(
        "dest",
        "identity",
        DESTINATION_KIND_LXMF_DELIVERY,
        app_data.as_slice(),
        2,
        "iface",
        42,
    );

    assert_eq!(record.destination_hex, "dest");
    assert_eq!(record.identity_hex, "identity");
    assert_eq!(record.destination_kind, DESTINATION_KIND_LXMF_DELIVERY);
    assert_eq!(record.app_data, hex::encode(app_data));
    assert_eq!(record.display_name.as_deref(), Some("Alice Router"));
    assert_eq!(record.hops, 2);
    assert_eq!(record.interface_hex, "iface");
    assert_eq!(record.received_at_ms, 42);
}

#[test]
fn announce_record_from_raw_preserves_text_app_data() {
    let record = AnnounceRecord::from_raw(
        "appdest",
        "identity",
        DESTINATION_KIND_APP,
        b"R3AKT;EMergencyMessages;name=Bravo",
        1,
        "iface",
        100,
    );

    assert_eq!(record.app_data, "R3AKT;EMergencyMessages;name=Bravo");
    assert!(record.display_name.is_none());
}

#[test]
fn announce_record_from_raw_hex_encodes_malformed_binary_app_data() {
    let record = AnnounceRecord::from_raw(
        "otherdest",
        "identity",
        DESTINATION_KIND_OTHER,
        &[0xff, 0xfe, 0x00],
        1,
        "iface",
        100,
    );

    assert_eq!(record.app_data, "fffe00");
    assert!(record.display_name.is_none());
}

#[test]
fn non_delivery_announce_does_not_parse_lxmf_display_name() {
    let app_data =
        encode_delivery_display_name_app_data("Relay Name").expect("encoded display name");

    let record = AnnounceRecord::from_raw(
        "propdest",
        "identity",
        DESTINATION_KIND_LXMF_PROPAGATION,
        app_data.as_slice(),
        1,
        "iface",
        100,
    );

    assert_eq!(record.destination_kind, DESTINATION_KIND_LXMF_PROPAGATION);
    assert_eq!(record.app_data, hex::encode(app_data));
    assert!(record.display_name.is_none());
}

#[test]
fn peer_projection_merges_app_and_lxmf_announces() {
    let mut store = MessagingStore::default();
    store.record_announce(AnnounceRecord {
        destination_hex: "appdest".into(),
        identity_hex: "identity".into(),
        destination_kind: "app".into(),
        app_data: "R3AKT".into(),
        display_name: None,
        hops: 1,
        interface_hex: "iface".into(),
        received_at_ms: 10,
    });
    store.record_announce(AnnounceRecord {
        destination_hex: "lxmfdest".into(),
        identity_hex: "identity".into(),
        destination_kind: "lxmf_delivery".into(),
        app_data: "chat".into(),
        display_name: Some("Alice".into()),
        hops: 1,
        interface_hex: "iface".into(),
        received_at_ms: 20,
    });

    let peers = store.list_peers(["lxmfdest"]);
    assert_eq!(peers.len(), 1);
    assert_eq!(peers[0].destination_hex, "appdest");
    assert_eq!(peers[0].lxmf_destination_hex.as_deref(), Some("lxmfdest"));
    assert_eq!(peers[0].display_name.as_deref(), Some("Alice"));
    assert_eq!(peers[0].state, PeerState::Connected);
}

#[test]
fn conversation_projection_uses_lxmf_destination_for_peer_lookup() {
    let mut store = MessagingStore::default();
    store.record_announce(AnnounceRecord {
        destination_hex: "appdest".into(),
        identity_hex: "identity".into(),
        destination_kind: "app".into(),
        app_data: "R3AKT".into(),
        display_name: None,
        hops: 1,
        interface_hex: "iface".into(),
        received_at_ms: 10,
    });
    store.record_announce(AnnounceRecord {
        destination_hex: "lxmfdest".into(),
        identity_hex: "identity".into(),
        destination_kind: "lxmf_delivery".into(),
        app_data: "chat".into(),
        display_name: Some("Alice".into()),
        hops: 1,
        interface_hex: "iface".into(),
        received_at_ms: 20,
    });
    store.upsert_message(MessageRecord {
        message_id_hex: "msg".into(),
        conversation_id: "lxmfdest".into(),
        direction: MessageDirection::Outbound,
        destination_hex: "lxmfdest".into(),
        source_hex: None,
        title: None,
        body_utf8: "hello".into(),
        method: MessageMethod::Direct,
        state: MessageState::Delivered,
        detail: None,
        sent_at_ms: Some(30),
        received_at_ms: None,
        updated_at_ms: 30,
    });

    let conversations = store.list_conversations(std::iter::empty::<&str>());
    assert_eq!(conversations.len(), 1);
    assert_eq!(conversations[0].peer_display_name.as_deref(), Some("Alice"));
}
