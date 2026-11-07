use super::*;

#[test]
fn parses_python_delivery_stamp_cost_from_second_app_data_slot() {
    let app_data = rmp_serde::to_vec_named(&rmpv::Value::Array(vec![
        rmpv::Value::Binary(b"Peer Name".to_vec()),
        rmpv::Value::from(19),
    ]))
    .expect("encode app data");

    assert_eq!(delivery_stamp_cost_from_app_data(app_data.as_slice()).expect("ok"), Some(19));
    assert!(pn_stamp_cost_from_app_data(app_data.as_slice()).is_err());
}

#[test]
fn rejects_python_invalid_delivery_stamp_costs_from_second_app_data_slot() {
    for invalid_cost in [0, 255, 256] {
        let app_data = rmp_serde::to_vec_named(&rmpv::Value::Array(vec![
            rmpv::Value::Binary(b"Peer Name".to_vec()),
            rmpv::Value::from(invalid_cost),
        ]))
        .expect("encode app data");

        assert_eq!(delivery_stamp_cost_from_app_data(app_data.as_slice()).expect("ok"), None);
    }
}

#[test]
fn encodes_python_delivery_app_data_with_optional_stamp_cost() {
    let app_data =
        encode_delivery_announce_app_data("Peer Name", Some(21)).expect("encode app data");

    assert_eq!(
        parse_peer_name_from_app_data(app_data.as_slice()).expect("ok"),
        Some(("Peer Name".to_string(), "delivery_app_data"))
    );
    assert_eq!(delivery_stamp_cost_from_app_data(app_data.as_slice()).expect("ok"), Some(21));

    let app_data_without_cost =
        encode_delivery_announce_app_data("Peer Name", Some(255)).expect("encode app data");
    assert_eq!(
        delivery_stamp_cost_from_app_data(app_data_without_cost.as_slice()).expect("ok"),
        None
    );
}

#[test]
fn encodes_delivery_app_data_with_rch_capability_payload() {
    let app_data = encode_delivery_announce_app_data_with_capabilities(
        "RCH Rust",
        Some(17),
        &["r3akt".to_string(), "topic_broker".to_string(), "telemetry_relay".to_string()],
    )
    .expect("encode app data");

    assert_eq!(
        parse_peer_name_from_app_data(app_data.as_slice()).expect("ok"),
        Some(("RCH Rust".to_string(), "delivery_app_data"))
    );
    assert_eq!(delivery_stamp_cost_from_app_data(app_data.as_slice()).expect("ok"), Some(17));
    let rmpv::Value::Array(entries) =
        rmp_serde::from_slice::<rmpv::Value>(app_data.as_slice()).expect("decode app data")
    else {
        panic!("expected app data array");
    };
    assert_eq!(entries.len(), 3);
    let rmpv::Value::Binary(payload) = &entries[2] else {
        panic!("expected capability payload");
    };
    let rmpv::Value::Map(metadata) =
        rmp_serde::from_slice::<rmpv::Value>(payload).expect("decode capabilities")
    else {
        panic!("expected capability metadata");
    };
    assert!(metadata
        .iter()
        .any(|(key, value)| key.as_str() == Some("app") && value.as_str() == Some("rch")));
    assert!(metadata.iter().any(|(key, value)| {
        key.as_str() == Some("caps")
            && matches!(value, rmpv::Value::Array(items) if items.len() == 3)
    }));
}

#[test]
fn encodes_python_propagation_app_data_with_configured_costs() {
    let app_data = encode_propagation_node_app_data(
        Some(" Peer Node "),
        PropagationNodeAnnounceConfig {
            timebase: 1_700_000_000,
            stamp_cost: 21,
            stamp_cost_flexibility: 5,
            peering_cost: 13,
            ..PropagationNodeAnnounceConfig::default()
        },
    )
    .expect("encode propagation app data");

    assert_eq!(
        parse_peer_name_from_app_data(app_data.as_slice()).expect("ok"),
        Some(("Peer Node".to_string(), "pn_meta"))
    );
    assert_eq!(pn_stamp_cost_from_app_data(app_data.as_slice()).expect("ok"), 21);
    assert_eq!(pn_stamp_cost_flexibility_from_app_data(app_data.as_slice()).expect("ok"), 5);
    assert_eq!(pn_peering_cost_from_app_data(app_data.as_slice()).expect("ok"), 13);
}
