#[cfg(test)]
mod peer_record_serde_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn peer_record_deserializes_legacy_seen_fields_from_last_seen() {
        let record: PeerRecord = serde_json::from_value(json!({
            "peer": "peer-legacy",
            "last_seen": 1_700_001_001,
        }))
        .expect("deserialize legacy peer");

        assert_eq!(record.first_seen, 1_700_001_001);
        assert_eq!(record.seen_count, 1);
    }

    #[test]
    fn peer_record_deserializes_explicit_seen_fields_without_rewriting_them() {
        let record: PeerRecord = serde_json::from_value(json!({
            "peer": "peer-current",
            "last_seen": 1_700_001_020,
            "first_seen": 1_700_001_000,
            "seen_count": 4,
        }))
        .expect("deserialize current peer");

        assert_eq!(record.first_seen, 1_700_001_000);
        assert_eq!(record.seen_count, 4);
    }

    #[test]
    fn peer_record_deserializes_unseen_legacy_peer_without_synthetic_seen_count() {
        let record: PeerRecord = serde_json::from_value(json!({
            "peer": "peer-static",
            "last_seen": 0,
        }))
        .expect("deserialize unseen peer");

        assert_eq!(record.first_seen, 0);
        assert_eq!(record.seen_count, 0);
    }

    #[test]
    fn peer_record_deserializes_python_status_aliases() {
        let record: PeerRecord = serde_json::from_value(json!({
            "peer": "peer-python-status",
            "last_heard": 1_700_001_004,
            "str": 4096.0,
            "offered": 7,
            "outgoing": 5,
            "incoming": 3,
            "transfer_limit": 333,
            "sync_limit": 444,
            "target_stamp_cost": 7,
            "stamp_cost_flexibility": 2,
        }))
        .expect("deserialize python status peer");

        assert_eq!(record.last_seen, 1_700_001_004);
        assert_eq!(record.first_seen, 1_700_001_004);
        assert_eq!(record.seen_count, 1);
        assert_eq!(record.sync_transfer_rate, 4096.0);
        assert_eq!(record.propagation_transfer_limit, Some(333));
        assert_eq!(record.propagation_sync_limit, Some(444));
        assert_eq!(record.propagation_stamp_cost, Some(7));
        assert_eq!(record.propagation_stamp_cost_flexibility, Some(2));
        let value = serde_json::to_value(record).expect("serialize python status peer");
        assert_eq!(value["offered"].as_u64(), Some(7));
        assert_eq!(value["outgoing"].as_u64(), Some(5));
        assert_eq!(value["incoming"].as_u64(), Some(3));
    }

    #[test]
    fn peer_record_deserializes_python_destination_hash_alias() {
        let record: PeerRecord = serde_json::from_value(json!({
            "destination_hash": "peer-python-destination",
            "last_heard": 1_700_001_007,
            "sync_strategy": 2,
            "peering_key": ["not-used-in-rust", 3],
            "handled_ids": [],
            "unhandled_ids": [],
            "offered": 2,
            "outgoing": 1,
            "incoming": 4,
            "peering_cost": 3,
        }))
        .expect("deserialize python serialized peer");

        assert_eq!(record.peer, "peer-python-destination");
        assert_eq!(record.last_seen, 1_700_001_007);
        assert_eq!(record.first_seen, 1_700_001_007);
        assert_eq!(record.seen_count, 1);
        assert_eq!(record.offered, 2);
        assert_eq!(record.outgoing, 1);
        assert_eq!(record.incoming, 4);
        assert_eq!(record.peering_cost, Some(3));
        assert_eq!(
            record.peering_key_stamp,
            Some(b"not-used-in-rust".to_vec())
        );
        assert_eq!(record.peering_key_value, Some(3));
        assert_eq!(record.sync_strategy, 2);
    }

    #[test]
    fn peer_record_roundtrips_python_metadata_like_lxmpeer() {
        let record: PeerRecord = serde_json::from_value(json!({
            "destination_hash": "peer-python-metadata",
            "last_heard": 1_700_001_009,
            "metadata": {
                "name": "Mesh Relay",
                "operator": "alpha"
            },
            "handled_ids": [],
            "unhandled_ids": [],
        }))
        .expect("deserialize python peer metadata");

        assert_eq!(record.metadata["name"].as_str(), Some("Mesh Relay"));
        let serialized = serde_json::to_value(&record).expect("serialize peer record");
        assert_eq!(serialized["metadata"]["operator"].as_str(), Some("alpha"));
    }

    #[test]
    fn peer_record_deserializes_python_msgpack_binary_peer_ids() {
        fn key(value: &str) -> rmpv::Value {
            rmpv::Value::String(value.into())
        }

        let destination_hash = (0x10_u8..0x20).collect::<Vec<_>>();
        let peering_key_stamp = vec![0xab; 32];
        let handled_id = (0x20_u8..0x40).collect::<Vec<_>>();
        let unhandled_id = (0x40_u8..0x60).collect::<Vec<_>>();
        let payload = rmpv::Value::Map(vec![
            (key("destination_hash"), rmpv::Value::Binary(destination_hash.clone())),
            (key("last_heard"), rmpv::Value::from(1_700_001_008_i64)),
            (key("sync_strategy"), rmpv::Value::from(2_u8)),
            (
                key("peering_key"),
                rmpv::Value::Array(vec![
                    rmpv::Value::Binary(peering_key_stamp.clone()),
                    rmpv::Value::from(3_u8),
                ]),
            ),
            (key("handled_ids"), rmpv::Value::Array(vec![rmpv::Value::Binary(handled_id.clone())])),
            (
                key("unhandled_ids"),
                rmpv::Value::Array(vec![rmpv::Value::Binary(unhandled_id.clone())]),
            ),
            (key("offered"), rmpv::Value::from(2_u8)),
            (key("outgoing"), rmpv::Value::from(1_u8)),
            (key("incoming"), rmpv::Value::from(4_u8)),
            (key("peering_cost"), rmpv::Value::from(3_u8)),
        ]);
        let encoded = rmp_serde::to_vec(&payload).expect("encode python peer record");
        let record: PeerRecord =
            rmp_serde::from_slice(encoded.as_slice()).expect("deserialize python binary peer");

        assert_eq!(record.peer, hex::encode(destination_hash));
        assert_eq!(record.restored_handled_ids, vec![hex::encode(handled_id)]);
        assert_eq!(record.restored_unhandled_ids, vec![hex::encode(unhandled_id)]);
        assert_eq!(record.last_seen, 1_700_001_008);
        assert_eq!(record.peering_key_stamp, Some(peering_key_stamp.clone()));
        assert_eq!(record.peering_key_value, Some(3));

        let reencoded = rmp_serde::to_vec(&record).expect("serialize python binary peer");
        let reencoded: rmpv::Value =
            rmp_serde::from_slice(reencoded.as_slice()).expect("decode serialized peer");
        let rmpv::Value::Map(entries) = reencoded else {
            panic!("serialized peer should be a map");
        };
        let peering_key = entries
            .iter()
            .find_map(|(key, value)| {
                (key.as_str() == Some("peering_key")).then_some(value)
            })
            .expect("serialized peering key");
        let rmpv::Value::Array(items) = peering_key else {
            panic!("serialized peering key should be a pair");
        };
        assert_eq!(items.first(), Some(&rmpv::Value::Binary(peering_key_stamp)));
        assert_eq!(items.get(1).and_then(rmpv::Value::as_u64), Some(3));

        let peer_hash = (0xa0_u8..0xb0).collect::<Vec<_>>();
        let peer_payload = rmpv::Value::Map(vec![
            (key("peer"), rmpv::Value::Binary(peer_hash.clone())),
            (key("last_heard"), rmpv::Value::from(1_700_001_009_i64)),
            (key("handled_ids"), rmpv::Value::Array(Vec::new())),
            (key("unhandled_ids"), rmpv::Value::Array(Vec::new())),
        ]);
        let encoded = rmp_serde::to_vec(&peer_payload).expect("encode binary peer record");
        let record: PeerRecord =
            rmp_serde::from_slice(encoded.as_slice()).expect("deserialize binary peer field");
        assert_eq!(record.peer, hex::encode(peer_hash));
    }

    #[test]
    fn peer_record_derives_python_acceptance_rate_when_alias_is_absent() {
        let record: PeerRecord = serde_json::from_value(json!({
            "destination_hash": "peer-python-acceptance",
            "last_heard": 1_700_001_008,
            "offered": 4,
            "outgoing": 1,
            "handled_ids": [],
            "unhandled_ids": [],
        }))
        .expect("deserialize python serialized peer");

        assert_eq!(record.acceptance_rate, 0.25);

        let duplicate_response_record: PeerRecord = serde_json::from_value(json!({
            "destination_hash": "peer-python-duplicate-acceptance",
            "last_heard": 1_700_001_009,
            "offered": 1,
            "outgoing": 2,
            "handled_ids": [],
            "unhandled_ids": [],
        }))
        .expect("deserialize python serialized peer with duplicate-response counters");

        assert_eq!(duplicate_response_record.acceptance_rate, 2.0);
    }

    #[test]
    fn peer_record_deserializes_python_serialized_kilobyte_limits_as_runtime_bytes() {
        let record: PeerRecord = serde_json::from_value(json!({
            "peer": "peer-python-serialized-limits",
            "last_seen": 1_700_001_004,
            "propagation_transfer_limit": 0.08,
            "propagation_sync_limit": 1,
        }))
        .expect("deserialize python serialized peer");

        assert_eq!(record.propagation_transfer_limit, Some(80));
        assert_eq!(record.propagation_sync_limit, Some(1_000));

        let transfer_only: PeerRecord = serde_json::from_value(json!({
            "peer": "peer-python-serialized-transfer-only",
            "last_seen": 1_700_001_004,
            "propagation_transfer_limit": 0.152,
        }))
        .expect("deserialize python serialized peer with transfer limit only");

        assert_eq!(transfer_only.propagation_transfer_limit, Some(152));
        assert_eq!(transfer_only.propagation_sync_limit, Some(152));
    }

    #[test]
    fn peer_record_prefers_internal_status_fields_over_aliases() {
        let record: PeerRecord = serde_json::from_value(json!({
            "peer": "peer-internal-status",
            "last_seen": 0,
            "last_heard": 1_700_001_004,
            "sync_transfer_rate": 0.0,
            "str": 4096.0,
            "propagation_transfer_limit": 0,
            "transfer_limit": 333,
        }))
        .expect("deserialize internal and alias status peer");

        assert_eq!(record.last_seen, 0);
        assert_eq!(record.first_seen, 0);
        assert_eq!(record.seen_count, 0);
        assert_eq!(record.sync_transfer_rate, 0.0);
        assert_eq!(record.propagation_transfer_limit, Some(0));
    }

    #[test]
    fn peer_record_serializes_python_status_aliases() {
        let record = PeerRecord {
            peer: "peer-python-status".to_string(),
            last_seen: 1_700_001_005,
            capabilities: vec!["propagation".to_string()],
            name: Some("Peer Python Status".to_string()),
            name_source: Some("announce".to_string()),
            metadata: JsonValue::Null,
            peer_type: Some("auto".to_string()),
            alive: true,
            last_sync_attempt: 1_700_001_000,
            next_sync_attempt: 1_700_001_720,
            sync_backoff: 720,
            sync_schedule_reason: Some("stamp_policy".to_string()),
            network_distance: 3,
            offered: 7,
            outgoing: 5,
            incoming: 3,
            rx_bytes: 123,
            tx_bytes: 456,
            sync_transfer_rate: 2048.0,
            acceptance_rate: 0.5,
            first_seen: 1_700_000_900,
            seen_count: 4,
            peering_timebase: 1_700_000_950,
            sync_strategy: 2,
            propagation_transfer_limit: Some(333),
            propagation_sync_limit: Some(444),
            propagation_stamp_cost: Some(7),
            propagation_stamp_cost_flexibility: Some(2),
            peering_cost: Some(9),
            peering_key_stamp: Some(b"python-stamp".to_vec()),
            peering_key_value: Some(9),
            restored_handled_ids: vec!["aa".repeat(32), "bb".repeat(32)],
            restored_unhandled_ids: vec!["cc".repeat(32)],
        };

        let value = serde_json::to_value(&record).expect("serialize peer record");
        assert_eq!(value["destination_hash"].as_str(), Some("peer-python-status"));
        assert_eq!(value["last_seen"].as_i64(), Some(1_700_001_005));
        assert_eq!(value["last_heard"].as_i64(), Some(1_700_001_005));
        assert_eq!(value["sync_transfer_rate"].as_f64(), Some(2048.0));
        assert_eq!(value["str"].as_f64(), Some(2048.0));
        assert_eq!(value["sync_schedule_reason"].as_str(), Some("stamp_policy"));
        assert_eq!(value["offered"].as_u64(), Some(7));
        assert_eq!(value["outgoing"].as_u64(), Some(5));
        assert_eq!(value["incoming"].as_u64(), Some(3));
        assert_eq!(value["propagation_transfer_limit"].as_f64(), Some(0.333));
        assert_eq!(value["transfer_limit"].as_u64(), Some(333));
        assert_eq!(value["propagation_sync_limit"].as_u64(), Some(1));
        assert_eq!(value["sync_limit"].as_u64(), Some(444));
        assert_eq!(value["propagation_stamp_cost"].as_u64(), Some(7));
        assert_eq!(value["target_stamp_cost"].as_u64(), Some(7));
        assert_eq!(value["propagation_stamp_cost_flexibility"].as_u64(), Some(2));
        assert_eq!(value["stamp_cost_flexibility"].as_u64(), Some(2));
        assert_eq!(
            value["peering_key"][0].as_array().map(|bytes| {
                bytes
                    .iter()
                    .map(|byte| byte.as_u64().expect("stamp byte") as u8)
                    .collect::<Vec<_>>()
            }),
            Some(b"python-stamp".to_vec())
        );
        assert_eq!(value["peering_key"][1].as_u64(), Some(9));
        assert_eq!(
            value["handled_ids"].as_array().expect("handled ids"),
            &[json!("aa".repeat(32)), json!("bb".repeat(32))]
        );
        assert_eq!(
            value["unhandled_ids"].as_array().expect("unhandled ids"),
            &[json!("cc".repeat(32))]
        );

        let without_stamp = PeerRecord {
            peering_key_stamp: None,
            ..record
        };
        let value = serde_json::to_value(without_stamp).expect("serialize peer without stamp");
        assert!(value.get("peering_key").is_none());
    }

    #[test]
    fn peer_record_serializes_python_limit_fields_as_kilobytes_with_byte_aliases() {
        let record = PeerRecord {
            peer: "peer-python-limits".to_string(),
            last_seen: 1_700_001_005,
            capabilities: vec!["propagation".to_string()],
            name: None,
            name_source: None,
            metadata: JsonValue::Null,
            peer_type: Some("auto".to_string()),
            alive: true,
            last_sync_attempt: 1_700_001_000,
            next_sync_attempt: 1_700_001_720,
            sync_backoff: 720,
            sync_schedule_reason: None,
            network_distance: 3,
            offered: 0,
            outgoing: 0,
            incoming: 0,
            rx_bytes: 0,
            tx_bytes: 0,
            sync_transfer_rate: 0.0,
            acceptance_rate: 0.0,
            first_seen: 1_700_000_900,
            seen_count: 4,
            peering_timebase: 1_700_000_950,
            sync_strategy: 2,
            propagation_transfer_limit: Some(333),
            propagation_sync_limit: Some(444),
            propagation_stamp_cost: Some(7),
            propagation_stamp_cost_flexibility: Some(2),
            peering_cost: Some(9),
            peering_key_stamp: None,
            peering_key_value: None,
            restored_handled_ids: Vec::new(),
            restored_unhandled_ids: Vec::new(),
        };

        let value = serde_json::to_value(record).expect("serialize peer record");
        assert_eq!(value["propagation_transfer_limit"].as_f64(), Some(0.333));
        assert_eq!(value["transfer_limit"].as_u64(), Some(333));
        assert_eq!(value["propagation_sync_limit"].as_u64(), Some(1));
        assert_eq!(value["sync_limit"].as_u64(), Some(444));
    }

    #[test]
    fn peer_record_serializes_python_sync_limit_as_integer_kilobytes() {
        let record = PeerRecord {
            peer: "peer-python-sync-limit".to_string(),
            last_seen: 1_700_001_005,
            capabilities: vec!["propagation".to_string()],
            name: None,
            name_source: None,
            metadata: JsonValue::Null,
            peer_type: Some("auto".to_string()),
            alive: true,
            last_sync_attempt: 1_700_001_000,
            next_sync_attempt: 1_700_001_720,
            sync_backoff: 720,
            sync_schedule_reason: None,
            network_distance: 3,
            offered: 0,
            outgoing: 0,
            incoming: 0,
            rx_bytes: 0,
            tx_bytes: 0,
            sync_transfer_rate: 0.0,
            acceptance_rate: 0.0,
            first_seen: 1_700_000_900,
            seen_count: 4,
            peering_timebase: 1_700_000_950,
            sync_strategy: 2,
            propagation_transfer_limit: Some(333),
            propagation_sync_limit: Some(444),
            propagation_stamp_cost: Some(7),
            propagation_stamp_cost_flexibility: Some(2),
            peering_cost: Some(9),
            peering_key_stamp: None,
            peering_key_value: None,
            restored_handled_ids: Vec::new(),
            restored_unhandled_ids: Vec::new(),
        };

        let value = serde_json::to_value(&record).expect("serialize peer record");
        assert_eq!(value["propagation_sync_limit"].as_u64(), Some(1));
        assert_eq!(value["sync_limit"].as_u64(), Some(444));

        let roundtrip: PeerRecord =
            serde_json::from_value(value).expect("roundtrip serialized peer record");
        assert_eq!(roundtrip.propagation_sync_limit, record.propagation_sync_limit);
    }

    #[test]
    fn peer_record_serialized_status_aliases_roundtrip() {
        let record = PeerRecord {
            peer: "peer-roundtrip-status".to_string(),
            last_seen: 1_700_001_006,
            capabilities: vec!["propagation".to_string(), "delivery".to_string()],
            name: Some("Peer Roundtrip Status".to_string()),
            name_source: Some("announce".to_string()),
            metadata: json!({"operator": "roundtrip"}),
            peer_type: Some("static".to_string()),
            alive: true,
            last_sync_attempt: 1_700_001_001,
            next_sync_attempt: 1_700_001_721,
            sync_backoff: 720,
            sync_schedule_reason: None,
            network_distance: 2,
            offered: 9,
            outgoing: 6,
            incoming: 4,
            rx_bytes: 12,
            tx_bytes: 34,
            sync_transfer_rate: 1024.0,
            acceptance_rate: 0.75,
            first_seen: 1_700_000_901,
            seen_count: 5,
            peering_timebase: 1_700_000_951,
            sync_strategy: 2,
            propagation_transfer_limit: Some(555),
            propagation_sync_limit: Some(666),
            propagation_stamp_cost: Some(8),
            propagation_stamp_cost_flexibility: Some(3),
            peering_cost: Some(10),
            peering_key_stamp: Some(b"roundtrip-stamp".to_vec()),
            peering_key_value: Some(10),
            restored_handled_ids: vec!["dd".repeat(32)],
            restored_unhandled_ids: vec!["ee".repeat(32), "ff".repeat(32)],
        };

        let value = serde_json::to_value(&record).expect("serialize peer record");
        let roundtrip: PeerRecord =
            serde_json::from_value(value).expect("deserialize serialized peer record");

        assert_eq!(roundtrip, record);
    }
}
