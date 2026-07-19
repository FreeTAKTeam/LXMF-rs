#[test]
fn announce_received_parses_delivery_stamp_cost_from_python_app_data() {
    let daemon = RpcDaemon::test_instance();
    let destination = "00112233445566778899aabbccddeeff";
    let app_data = rmp_serde::to_vec_named(&MsgPackValue::Array(vec![
        MsgPackValue::Binary(b"Peer Stamp".to_vec()),
        MsgPackValue::from(22),
    ]))
    .expect("encode app data");

    let announce = daemon
        .handle_rpc(rpc_request(
            45,
            "announce_received",
            json!({
                "peer": destination,
                "timestamp": 1_700_000_012i64,
                "app_data_hex": hex::encode(app_data),
                "aspect": "lxmf.delivery",
            }),
        ))
        .expect("announce received");
    assert!(announce.error.is_none());

    assert_eq!(
        daemon.outbound_stamp_cost_for(destination).expect("stamp cost lookup"),
        Some(22)
    );

    let queried = daemon
        .handle_rpc(rpc_request(
            46,
            "get_outbound_stamp_cost",
            json!({ "destination_hash": destination }),
        ))
        .expect("query outbound stamp cost");
    assert!(queried.error.is_none());
    let result = queried.result.expect("query result");
    assert_eq!(result["destination"].as_str(), Some(destination));
    assert_eq!(result["stamp_cost"].as_u64(), Some(22));

    let envelope = daemon
        .handle_rpc(rpc_request(
            47,
            "sdk_envelope_execute_v2",
            json!({
                "operation_id": "get_outbound_stamp_cost",
                "kind": "query",
                "payload": { "destination": destination },
            }),
        ))
        .expect("query outbound stamp cost through sdk envelope");
    assert!(envelope.error.is_none());
    let response = envelope.result.expect("envelope result")["response"].clone();
    assert_eq!(response["operation_id"].as_str(), Some("app.delivery.outbound_stamp_cost"));
    assert_eq!(response["payload"]["destination"].as_str(), Some(destination));
    assert_eq!(response["payload"]["stamp_cost"].as_u64(), Some(22));
}

#[test]
fn delivery_announce_does_not_create_propagation_peer_or_queue_entries() {
    let daemon = RpcDaemon::test_instance();
    let peer = "bb".repeat(16);
    daemon
        .handle_rpc(rpc_request(
            46,
            "propagation_enable",
            json!({
                "enabled": true,
                "autopeer": true,
            }),
        ))
        .expect("enable propagation");
    let entry = PropagationEntryRecord {
        transient_id: "d1".repeat(32),
        destination: "16".repeat(16),
        payload_hex: "11".repeat(32),
        received_at: 1_700_000_010,
        size_bytes: 32,
        stamp_value: None,
    };
    daemon.store.upsert_propagation_entry(&entry).expect("store entry");
    let app_data = rmp_serde::to_vec_named(&MsgPackValue::Array(vec![
        MsgPackValue::Binary(b"Delivery Only".to_vec()),
        MsgPackValue::from(12),
    ]))
    .expect("encode app data");

    let announce = daemon
        .handle_rpc(rpc_request(
            47,
            "announce_received",
            json!({
                "peer": peer.as_str(),
                "timestamp": 1_700_000_012i64,
                "app_data_hex": hex::encode(app_data),
                "aspect": "lxmf.delivery",
                "hops": 1,
            }),
        ))
        .expect("delivery announce");
    assert!(announce.error.is_none());

    assert_eq!(daemon.outbound_stamp_cost_for(peer.as_str()).expect("stamp cost lookup"), Some(12));
    assert!(!daemon.peer_record_exists(peer.as_str(), false));
    assert!(
        daemon
            .store
            .list_peer_unhandled_propagation(peer.as_str())
            .expect("pending propagation")
            .is_empty()
    );
    let announce_event = daemon
        .event_queue
        .lock()
        .expect("event queue")
        .iter()
        .find(|event| event.event_type == "announce_received")
        .cloned()
        .expect("announce event");
    assert_eq!(announce_event.payload["aspect"], json!("lxmf.delivery"));
    assert_eq!(announce_event.payload["stamp_cost"], json!(12));
}

#[test]
fn announce_received_ignores_python_invalid_delivery_stamp_cost_from_app_data() {
    let daemon = RpcDaemon::test_instance();
    let app_data = rmp_serde::to_vec_named(&MsgPackValue::Array(vec![
        MsgPackValue::Binary(b"Peer Stamp".to_vec()),
        MsgPackValue::from(255),
    ]))
    .expect("encode app data");

    let announce = daemon
        .handle_rpc(rpc_request(
            46,
            "announce_received",
            json!({
                "peer": "peer-invalid-delivery-stamp",
                "timestamp": 1_700_000_012i64,
                "app_data_hex": hex::encode(app_data),
                "aspect": "lxmf.delivery",
            }),
        ))
        .expect("announce received");
    assert!(announce.error.is_none());

    assert_eq!(
        daemon.outbound_stamp_cost_for("peer-invalid-delivery-stamp").expect("stamp cost lookup"),
        None
    );
}

#[test]
fn announce_received_wakes_pending_direct_and_opportunistic_outbound() {
    struct RecordingOutboundBridge {
        tx: mpsc::Sender<(String, Option<String>)>,
    }

    impl OutboundBridge for RecordingOutboundBridge {
        fn deliver(
            &self,
            record: &MessageRecord,
            options: &OutboundDeliveryOptions,
        ) -> Result<(), std::io::Error> {
            self.tx
                .send((record.id.clone(), options.method.clone()))
                .expect("outbound observer remains connected");
            Ok(())
        }
    }

    fn queued_outbound(
        id: &str,
        destination: &str,
        method: Option<&str>,
        receipt_status: Option<&str>,
    ) -> MessageRecord {
        let fields = method.map(|method| json!({ "_lxmf": { "method": method } }));
        MessageRecord {
            id: id.to_string(),
            source: "source-peer".to_string(),
            destination: destination.to_string(),
            title: String::new(),
            content: "pending outbound".to_string(),
            timestamp: 1_700_000_000,
            direction: "out".to_string(),
            fields,
            receipt_status: receipt_status.map(ToOwned::to_owned),
        }
    }

    let store = MessagesStore::in_memory().expect("in-memory store");
    for record in [
        queued_outbound("pending-direct", "peer-delivery-wake", Some("direct"), Some("queued")),
        queued_outbound(
            "pending-deferred-direct",
            "peer-delivery-wake",
            Some("direct"),
            Some("queued: waiting for announce"),
        ),
        queued_outbound(
            "pending-opportunistic",
            "peer-delivery-wake",
            Some("opportunistic"),
            Some("queued"),
        ),
        queued_outbound("pending-default-direct", "peer-delivery-wake", None, Some("queued")),
        queued_outbound("pending-propagated", "peer-delivery-wake", Some("propagated"), Some("queued")),
        queued_outbound("pending-paper", "peer-delivery-wake", Some("paper"), Some("queued")),
        queued_outbound("terminal-cancelled", "peer-delivery-wake", Some("direct"), Some("cancelled")),
        queued_outbound("terminal-expired-spaces", "peer-delivery-wake", Some("direct"), Some(" expired ")),
        queued_outbound("already-sending", "peer-delivery-wake", Some("direct"), Some("sending")),
        queued_outbound("other-destination", "peer-other", Some("direct"), Some("queued")),
    ] {
        store.insert_message(&record).expect("seed outbound message");
    }

    let (tx, rx) = mpsc::channel();
    let daemon = RpcDaemon::with_store_and_bridges(
        store,
        "local-node".to_string(),
        Some(Arc::new(RecordingOutboundBridge { tx })),
        None,
    );

    let propagation_announce = daemon
        .handle_rpc(rpc_request(
            46,
            "announce_received",
            json!({
                "peer": "peer-delivery-wake",
                "timestamp": 1_700_000_011i64,
                "aspect": "lxmf.propagation",
            }),
        ))
        .expect("propagation announce received");
    assert!(propagation_announce.error.is_none());
    assert!(
        rx.recv_timeout(std::time::Duration::from_millis(150)).is_err(),
        "propagation announces must not wake direct/opportunistic delivery work"
    );

    let announce = daemon
        .handle_rpc(rpc_request(
            47,
            "announce_received",
            json!({
                "peer": "peer-delivery-wake",
                "timestamp": 1_700_000_012i64,
                "aspect": "lxmf.delivery",
            }),
        ))
        .expect("announce received");
    assert!(announce.error.is_none());

    let mut delivered = Vec::new();
    for _ in 0..4 {
        delivered.push(rx.recv_timeout(std::time::Duration::from_secs(1)).expect("woken delivery"));
    }
    assert!(
        rx.recv_timeout(std::time::Duration::from_millis(150)).is_err(),
        "only pending direct/opportunistic messages for the announced peer should wake"
    );
    delivered.sort();
    assert_eq!(
        delivered,
        vec![
            ("pending-default-direct".to_string(), None),
            ("pending-deferred-direct".to_string(), Some("direct".to_string())),
            ("pending-direct".to_string(), Some("direct".to_string())),
            ("pending-opportunistic".to_string(), Some("opportunistic".to_string())),
        ]
    );

    let duplicate_announce = daemon
        .handle_rpc(rpc_request(
            48,
            "announce_received",
            json!({
                "peer": "peer-delivery-wake",
                "timestamp": 1_700_000_013i64,
                "aspect": "lxmf.delivery",
            }),
        ))
        .expect("duplicate delivery announce received");
    assert!(duplicate_announce.error.is_none());
    assert!(
        rx.recv_timeout(std::time::Duration::from_millis(150)).is_err(),
        "messages already marked sending must not be woken again by duplicate delivery announces"
    );

    for (message_id, expected_status) in [
        ("pending-direct", "sending"),
        ("pending-deferred-direct", "sending"),
        ("pending-opportunistic", "sending"),
        ("pending-default-direct", "sending"),
        ("pending-propagated", "queued"),
        ("pending-paper", "queued"),
        ("terminal-cancelled", "cancelled"),
        ("terminal-expired-spaces", " expired "),
        ("already-sending", "sending"),
        ("other-destination", "queued"),
    ] {
        let status = daemon
            .store
            .get_message(message_id)
            .expect("lookup message")
            .expect("message exists")
            .receipt_status
            .expect("receipt status");
        assert_eq!(status, expected_status, "{message_id}");
    }
}

#[test]
fn selected_propagation_announce_wakes_pending_propagated_outbound() {
    struct RecordingOutboundBridge {
        tx: mpsc::Sender<(String, Option<String>)>,
    }

    impl OutboundBridge for RecordingOutboundBridge {
        fn deliver(
            &self,
            record: &MessageRecord,
            options: &OutboundDeliveryOptions,
        ) -> Result<(), std::io::Error> {
            self.tx
                .send((record.id.clone(), options.method.clone()))
                .expect("outbound observer remains connected");
            Ok(())
        }
    }

    fn queued_outbound(
        id: &str,
        destination: &str,
        method: Option<&str>,
        receipt_status: Option<&str>,
    ) -> MessageRecord {
        let fields = method.map(|method| json!({ "_lxmf": { "method": method } }));
        MessageRecord {
            id: id.to_string(),
            source: "source-peer".to_string(),
            destination: destination.to_string(),
            title: String::new(),
            content: "pending outbound".to_string(),
            timestamp: 1_700_000_000,
            direction: "out".to_string(),
            fields,
            receipt_status: receipt_status.map(ToOwned::to_owned),
        }
    }

    let store = MessagesStore::in_memory().expect("in-memory store");
    for record in [
        queued_outbound("pending-propagated", "peer-propagation-wake", Some("propagated"), Some("queued")),
        queued_outbound(
            "pending-propagated-waiting",
            "peer-propagation-wake",
            Some("propagated"),
            Some("queued: waiting for propagation node"),
        ),
        queued_outbound("pending-direct", "peer-propagation-wake", Some("direct"), Some("queued")),
        queued_outbound(
            "pending-opportunistic",
            "peer-propagation-wake",
            Some("opportunistic"),
            Some("queued"),
        ),
        queued_outbound("pending-paper", "peer-propagation-wake", Some("paper"), Some("queued")),
        queued_outbound(
            "terminal-cancelled",
            "peer-propagation-wake",
            Some("propagated"),
            Some("cancelled"),
        ),
        queued_outbound(
            "already-sending",
            "peer-propagation-wake",
            Some("propagated"),
            Some("sending"),
        ),
        queued_outbound("other-destination", "peer-other", Some("propagated"), Some("queued")),
    ] {
        store.insert_message(&record).expect("seed outbound message");
    }

    let (tx, rx) = mpsc::channel();
    let daemon = RpcDaemon::with_store_and_bridges(
        store,
        "local-node".to_string(),
        Some(Arc::new(RecordingOutboundBridge { tx })),
        None,
    );

    let unselected = daemon
        .handle_rpc(rpc_request(
            49,
            "announce_received",
            json!({
                "peer": "peer-propagation-wake",
                "timestamp": 1_700_000_014i64,
                "aspect": "lxmf.propagation",
            }),
        ))
        .expect("unselected propagation announce received");
    assert!(unselected.error.is_none());
    assert!(
        rx.recv_timeout(std::time::Duration::from_millis(150)).is_err(),
        "unselected propagation announces must not wake outbound messages"
    );

    let selected = daemon
        .handle_rpc(rpc_request(
            50,
            "set_outbound_propagation_node",
            json!({ "peer": "peer-propagation-wake" }),
        ))
        .expect("select propagation node");
    assert!(selected.error.is_none());

    let announce = daemon
        .handle_rpc(rpc_request(
            51,
            "announce_received",
            json!({
                "peer": "peer-propagation-wake",
                "timestamp": 1_700_000_015i64,
                "aspect": "lxmf.propagation",
            }),
        ))
        .expect("selected propagation announce received");
    assert!(announce.error.is_none());

    let mut delivered = Vec::new();
    for _ in 0..2 {
        delivered.push(rx.recv_timeout(std::time::Duration::from_secs(1)).expect("woken propagated"));
    }
    assert!(
        rx.recv_timeout(std::time::Duration::from_millis(150)).is_err(),
        "only queued propagated messages for the selected propagation node should wake"
    );
    delivered.sort();
    assert_eq!(
        delivered,
        vec![
            ("pending-propagated".to_string(), Some("propagated".to_string())),
            ("pending-propagated-waiting".to_string(), Some("propagated".to_string())),
        ]
    );

    for (message_id, expected_status) in [
        ("pending-propagated", "sending"),
        ("pending-propagated-waiting", "sending"),
        ("pending-direct", "queued"),
        ("pending-opportunistic", "queued"),
        ("pending-paper", "queued"),
        ("terminal-cancelled", "cancelled"),
        ("already-sending", "sending"),
        ("other-destination", "queued"),
    ] {
        let status = daemon
            .store
            .get_message(message_id)
            .expect("lookup message")
            .expect("message exists")
            .receipt_status
            .expect("receipt status");
        assert_eq!(status, expected_status, "{message_id}");
    }
}

#[test]
fn announce_received_does_not_wake_cancelled_deferred_outbound() {
    struct RecordingOutboundBridge {
        tx: mpsc::Sender<String>,
    }

    impl OutboundBridge for RecordingOutboundBridge {
        fn deliver(
            &self,
            record: &MessageRecord,
            _options: &OutboundDeliveryOptions,
        ) -> Result<(), std::io::Error> {
            self.tx.send(record.id.clone()).expect("outbound observer remains connected");
            Ok(())
        }
    }

    let store = MessagesStore::in_memory().expect("in-memory store");
    store
        .insert_message(&MessageRecord {
            id: "cancelled-deferred-direct".to_string(),
            source: "source-peer".to_string(),
            destination: "peer-delivery-cancel".to_string(),
            title: String::new(),
            content: "pending outbound".to_string(),
            timestamp: 1_700_000_000,
            direction: "out".to_string(),
            fields: Some(json!({ "_lxmf": { "method": "direct" } })),
            receipt_status: Some("queued: waiting for announce".to_string()),
        })
        .expect("seed deferred message");

    let (tx, rx) = mpsc::channel();
    let daemon = RpcDaemon::with_store_and_bridges(
        store,
        "local-node".to_string(),
        Some(Arc::new(RecordingOutboundBridge { tx })),
        None,
    );

    let cancel = daemon
        .handle_rpc(rpc_request(
            49,
            "sdk_cancel_message_v2",
            json!({ "message_id": "cancelled-deferred-direct" }),
        ))
        .expect("cancel deferred");
    assert!(cancel.error.is_none());
    assert_eq!(cancel.result.expect("cancel result")["result"], json!("Accepted"));

    let announce = daemon
        .handle_rpc(rpc_request(
            50,
            "announce_received",
            json!({
                "peer": "peer-delivery-cancel",
                "timestamp": 1_700_000_014i64,
                "aspect": "lxmf.delivery",
            }),
        ))
        .expect("delivery announce received");
    assert!(announce.error.is_none());
    assert!(
        rx.recv_timeout(std::time::Duration::from_millis(150)).is_err(),
        "cancelled deferred messages must not wake after delivery announce"
    );

    let status = daemon
        .store
        .get_message("cancelled-deferred-direct")
        .expect("lookup message")
        .expect("message exists")
        .receipt_status
        .expect("receipt status");
    assert_eq!(status, "cancelled");
}

#[test]
fn announce_received_parses_propagation_peer_limits_from_python_app_data() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(
            46,
            "propagation_enable",
            json!({
                "enabled": true,
                "autopeer": true,
            }),
        ))
        .expect("enable propagation");
    let app_data = rmp_serde::to_vec_named(&MsgPackValue::Array(vec![
        MsgPackValue::Boolean(false),
        MsgPackValue::from(1_700_000_013i64),
        MsgPackValue::Boolean(true),
        MsgPackValue::from(333),
        MsgPackValue::from(999),
        MsgPackValue::Array(vec![
            MsgPackValue::from(8),
            MsgPackValue::from(2),
            MsgPackValue::from(5),
        ]),
        MsgPackValue::Map(Vec::new()),
    ]))
    .expect("encode propagation app data");

    let announce = daemon
        .handle_rpc(rpc_request(
            47,
            "announce_received",
            json!({
                "peer": "peer-propagation-limits",
                "timestamp": 1_700_000_013i64,
                "app_data_hex": hex::encode(app_data),
                "aspect": "lxmf.propagation",
                "hops": 1,
            }),
        ))
        .expect("announce received");
    assert!(announce.error.is_none());

    let peers = daemon
        .handle_rpc(RpcRequest { id: 48, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let row = peers["peers"].as_array().and_then(|rows| rows.first()).expect("peer row");
    assert_eq!(row["propagation_transfer_limit"].as_u64(), Some(333_000));
    assert_eq!(row["propagation_sync_limit"].as_u64(), Some(999_000));
    assert_eq!(row["propagation_stamp_cost"].as_u64(), Some(8));
    assert_eq!(row["propagation_stamp_cost_flexibility"].as_u64(), Some(2));
    assert_eq!(row["peering_cost"].as_u64(), Some(5));
    assert_eq!(row["type"].as_str(), Some("discovered"));
    assert_eq!(row["state"].as_u64(), Some(0));
    assert_eq!(row["sync_strategy"].as_u64(), Some(2));
    assert_eq!(row["ler"].as_u64(), Some(0));
    assert_eq!(row["str"].as_u64(), Some(0));
    assert_eq!(row["last_heard"].as_i64(), Some(1_700_000_013));
    assert_eq!(row["transfer_limit"].as_u64(), Some(333_000));
    assert_eq!(row["sync_limit"].as_u64(), Some(999_000));
    assert_eq!(row["target_stamp_cost"].as_u64(), Some(8));
    assert_eq!(row["stamp_cost_flexibility"].as_u64(), Some(2));
}

#[test]
fn peer_sync_treats_python_announced_limits_as_kilobytes() {
    let daemon = RpcDaemon::test_instance();
    let peer = "ab".repeat(16);
    daemon
        .handle_rpc(rpc_request(
            48,
            "propagation_enable",
            json!({
                "enabled": true,
                "autopeer": true,
            }),
        ))
        .expect("enable propagation");
    let app_data = rmp_serde::to_vec_named(&MsgPackValue::Array(vec![
        MsgPackValue::Boolean(false),
        MsgPackValue::from(1_700_000_015i64),
        MsgPackValue::Boolean(true),
        MsgPackValue::from(1),
        MsgPackValue::from(1),
        MsgPackValue::Array(vec![
            MsgPackValue::from(0),
            MsgPackValue::from(0),
            MsgPackValue::from(0),
        ]),
        MsgPackValue::Map(Vec::new()),
    ]))
    .expect("encode propagation app data");
    daemon
        .handle_rpc(rpc_request(
            49,
            "announce_received",
            json!({
                "peer": peer.as_str(),
                "timestamp": 1_700_000_015i64,
                "app_data_hex": hex::encode(app_data),
                "aspect": "lxmf.propagation",
                "hops": 1,
            }),
        ))
        .expect("announce received");
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let record = peers.get_mut(peer.as_str()).expect("peer record");
        record.peering_key_value = Some(0);
    }
    let entry = PropagationEntryRecord {
        transient_id: "e1".repeat(32),
        destination: "16".repeat(16),
        payload_hex: "16".repeat(100),
        received_at: 1_700_000_616,
        size_bytes: 100,
        stamp_value: None,
    };
    daemon.store.upsert_propagation_entry(&entry).expect("store entry");
    daemon
        .store
        .mark_peer_unhandled_propagation(peer.as_str(), entry.transient_id.as_str())
        .expect("mark unhandled");

    let result = daemon
        .handle_rpc(rpc_request(50, "peer_sync", json!({ "peer": peer.as_str() })))
        .expect("peer sync")
        .result
        .expect("peer sync result");

    assert_eq!(result["propagation"]["transfer_limit"].as_u64(), Some(1_000));
    assert_eq!(result["propagation"]["sync_limit"].as_u64(), Some(1_000));
    assert_eq!(result["propagation"]["transferred"].as_u64(), Some(1));
    assert_eq!(result["propagation"]["transfer_limited"].as_u64(), Some(0));
    assert_eq!(
        daemon
            .store
            .list_peer_handled_propagation_ids(peer.as_str())
            .expect("handled ids"),
        vec![entry.transient_id.clone()]
    );
    assert!(
        daemon
            .store
            .list_peer_unhandled_propagation(peer.as_str())
            .expect("pending propagation")
            .is_empty()
    );
}

#[test]
fn peer_sync_applies_fractional_python_announced_transfer_limit() {
    let daemon = RpcDaemon::test_instance();
    let peer = "ac".repeat(16);
    daemon
        .handle_rpc(rpc_request(
            51,
            "propagation_enable",
            json!({
                "enabled": true,
                "autopeer": true,
            }),
        ))
        .expect("enable propagation");
    let app_data = rmp_serde::to_vec_named(&MsgPackValue::Array(vec![
        MsgPackValue::Boolean(false),
        MsgPackValue::from(1_700_000_016i64),
        MsgPackValue::Boolean(true),
        MsgPackValue::F64(0.08),
        MsgPackValue::from(1),
        MsgPackValue::Array(vec![
            MsgPackValue::from(0),
            MsgPackValue::from(0),
            MsgPackValue::from(0),
        ]),
        MsgPackValue::Map(Vec::new()),
    ]))
    .expect("encode propagation app data");
    daemon
        .handle_rpc(rpc_request(
            52,
            "announce_received",
            json!({
                "peer": peer.as_str(),
                "timestamp": 1_700_000_016i64,
                "app_data_hex": hex::encode(app_data),
                "aspect": "lxmf.propagation",
                "hops": 1,
            }),
        ))
        .expect("announce received");
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        let record = peers.get_mut(peer.as_str()).expect("peer record");
        record.peering_key_value = Some(0);
    }
    let oversized = PropagationEntryRecord {
        transient_id: "e2".repeat(32),
        destination: "16".repeat(16),
        payload_hex: "16".repeat(100),
        received_at: 1_700_000_617,
        size_bytes: 100,
        stamp_value: None,
    };
    daemon.store.upsert_propagation_entry(&oversized).expect("store entry");
    daemon
        .store
        .mark_peer_unhandled_propagation(peer.as_str(), oversized.transient_id.as_str())
        .expect("mark unhandled");

    let result = daemon
        .handle_rpc(rpc_request(53, "peer_sync", json!({ "peer": peer.as_str() })))
        .expect("peer sync")
        .result
        .expect("peer sync result");

    assert_eq!(result["propagation"]["transfer_limit"].as_u64(), Some(80));
    assert_eq!(result["propagation"]["sync_limit"].as_u64(), Some(1_000));
    assert_eq!(result["propagation"]["transferred"].as_u64(), Some(0));
    assert_eq!(result["propagation"]["transfer_limited"].as_u64(), Some(1));
    assert_eq!(
        result["propagation"]["transfer_limited_ids"].as_array().expect("limited ids"),
        &[json!(oversized.transient_id.as_str())]
    );
    assert_eq!(
        daemon
            .store
            .list_peer_handled_propagation_ids(peer.as_str())
            .expect("handled ids"),
        vec![oversized.transient_id.clone()]
    );
    assert!(
        daemon
            .store
            .list_peer_unhandled_propagation(peer.as_str())
            .expect("pending propagation")
            .is_empty()
    );
}

#[test]
fn announce_received_clamps_sync_limit_below_transfer_limit_like_python() {
    let store = MessagesStore::in_memory().expect("store");
    let daemon = RpcDaemon::with_store(store, hex::encode([2u8; 16]));
    daemon
        .handle_rpc(rpc_request(
            48,
            "propagation_enable",
            json!({
                "enabled": true,
                "autopeer": true,
            }),
        ))
        .expect("enable propagation");
    let app_data = rmp_serde::to_vec_named(&MsgPackValue::Array(vec![
        MsgPackValue::Boolean(false),
        MsgPackValue::from(1_700_000_014i64),
        MsgPackValue::Boolean(true),
        MsgPackValue::from(333),
        MsgPackValue::from(100),
        MsgPackValue::Array(vec![
            MsgPackValue::from(8),
            MsgPackValue::from(2),
            MsgPackValue::from(1),
        ]),
        MsgPackValue::Map(Vec::new()),
    ]))
    .expect("encode propagation app data");
    let peer = hex::encode([3u8; 16]);

    daemon
        .handle_rpc(rpc_request(
            49,
            "announce_received",
            json!({
                "peer": peer,
                "timestamp": 1_700_000_014i64,
                "app_data_hex": hex::encode(app_data),
                "aspect": "lxmf.propagation",
                "hops": 1,
            }),
        ))
        .expect("announce received");

    let peers = daemon
        .handle_rpc(RpcRequest { id: 50, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let row = peers["peers"].as_array().and_then(|rows| rows.first()).expect("peer row");
    assert_eq!(row["propagation_transfer_limit"].as_u64(), Some(333_000));
    assert_eq!(row["propagation_sync_limit"].as_u64(), Some(333_000));

    let entry = PropagationEntryRecord {
        transient_id: "de".repeat(32),
        destination: "35".repeat(16),
        payload_hex: "35".repeat(100),
        received_at: 1_700_000_015,
        size_bytes: 100,
        stamp_value: None,
    };
    daemon.store.upsert_propagation_entry(&entry).expect("store entry");
    daemon
        .store
        .mark_peer_unhandled_propagation(peer.as_str(), entry.transient_id.as_str())
        .expect("mark peer unhandled");

    let result = daemon
        .handle_rpc(rpc_request(51, "peer_sync", json!({ "peer": peer })))
        .expect("peer sync")
        .result
        .expect("peer sync result");
    assert_eq!(result["propagation"]["sync_limit"].as_u64(), Some(333_000));
    assert_eq!(result["propagation"]["transferred"].as_u64(), Some(1));
    assert_eq!(
        result["propagation"]["transferred_ids"].as_array().expect("transferred ids"),
        &[json!(entry.transient_id.as_str())]
    );
    assert_eq!(result["propagation"]["skipped"].as_u64(), Some(0));
}

#[test]
fn announce_received_uses_python_propagation_node_timebase_for_peer_state() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(rpc_request(
            49,
            "propagation_enable",
            json!({
                "enabled": true,
                "autopeer": true,
            }),
        ))
        .expect("enable propagation");
    let app_data = rmp_serde::to_vec_named(&MsgPackValue::Array(vec![
        MsgPackValue::Boolean(false),
        MsgPackValue::from(1_700_000_021i64),
        MsgPackValue::Boolean(true),
        MsgPackValue::from(333),
        MsgPackValue::from(999),
        MsgPackValue::Array(vec![
            MsgPackValue::from(8),
            MsgPackValue::from(2),
            MsgPackValue::from(5),
        ]),
        MsgPackValue::Map(Vec::new()),
    ]))
    .expect("encode propagation app data");

    let announce = daemon
        .handle_rpc(rpc_request(
            50,
            "announce_received",
            json!({
                "peer": "peer-pn-timebase",
                "timestamp": 1_700_000_099i64,
                "app_data_hex": hex::encode(app_data),
                "aspect": "lxmf.propagation",
                "hops": 1,
            }),
        ))
        .expect("announce received");
    assert!(announce.error.is_none());

    let peers = daemon
        .handle_rpc(RpcRequest { id: 51, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let row = peers["peers"].as_array().and_then(|rows| rows.first()).expect("peer row");
    assert_eq!(row["last_heard"].as_i64(), Some(1_700_000_099));
    assert_eq!(row["peering_timebase"].as_i64(), Some(1_700_000_021));
}
