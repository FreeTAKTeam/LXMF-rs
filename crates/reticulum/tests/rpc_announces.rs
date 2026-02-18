use reticulum::rpc::{RpcDaemon, RpcRequest};
use serde_json::json;

#[test]
fn announce_now_emits_event() {
    let daemon = RpcDaemon::test_instance();
    let resp = daemon
        .handle_rpc(RpcRequest { id: 1, method: "announce_now".into(), params: None })
        .unwrap();

    assert!(resp.result.is_some());
    let event = daemon.take_event().expect("announce event");
    assert_eq!(event.event_type, "announce_sent");
}

#[test]
fn announce_received_updates_peers() {
    let daemon = RpcDaemon::test_instance();
    let resp = daemon
        .handle_rpc(RpcRequest {
            id: 2,
            method: "announce_received".into(),
            params: Some(serde_json::json!({
                "peer": "peer-a",
                "timestamp": 123,
                "name": "Alice",
                "name_source": "pn_meta"
            })),
        })
        .unwrap();

    assert!(resp.result.is_some());
    let event = daemon.take_event().expect("announce event");
    assert_eq!(event.event_type, "announce_received");
    assert_eq!(event.payload["peer"], "peer-a");
    assert_eq!(event.payload["name"], "Alice");
    assert_eq!(event.payload["name_source"], "pn_meta");
    assert_eq!(event.payload["seen_count"], 1);

    let peers = daemon
        .handle_rpc(RpcRequest { id: 3, method: "list_peers".into(), params: None })
        .unwrap()
        .result
        .unwrap()
        .get("peers")
        .unwrap()
        .as_array()
        .unwrap()
        .clone();

    assert_eq!(peers.len(), 1);
    assert_eq!(peers[0].get("peer").unwrap(), "peer-a");
    assert_eq!(peers[0].get("name").unwrap(), "Alice");
    assert_eq!(peers[0].get("name_source").unwrap(), "pn_meta");
    assert_eq!(peers[0].get("first_seen").unwrap(), 123);
    assert_eq!(peers[0].get("last_seen").unwrap(), 123);
    assert_eq!(peers[0].get("seen_count").unwrap(), 1);

    let announces = daemon
        .handle_rpc(RpcRequest { id: 4, method: "list_announces".into(), params: None })
        .unwrap()
        .result
        .unwrap()
        .get("announces")
        .unwrap()
        .as_array()
        .unwrap()
        .clone();
    assert_eq!(announces.len(), 1);
    assert_eq!(announces[0].get("peer").unwrap(), "peer-a");
    assert_eq!(announces[0].get("name").unwrap(), "Alice");
}

#[test]
fn repeated_announces_increment_seen_count_and_preserve_first_seen() {
    let daemon = RpcDaemon::test_instance();

    daemon
        .handle_rpc(RpcRequest {
            id: 1,
            method: "announce_received".into(),
            params: Some(serde_json::json!({
                "peer": "peer-z",
                "timestamp": 100,
                "name": "Old Name",
                "name_source": "app_data_utf8"
            })),
        })
        .unwrap();

    daemon
        .handle_rpc(RpcRequest {
            id: 2,
            method: "announce_received".into(),
            params: Some(serde_json::json!({
                "peer": "peer-z",
                "timestamp": 150
            })),
        })
        .unwrap();

    daemon
        .handle_rpc(RpcRequest {
            id: 3,
            method: "announce_received".into(),
            params: Some(serde_json::json!({
                "peer": "peer-z",
                "timestamp": 200,
                "name": "New Name",
                "name_source": "pn_meta"
            })),
        })
        .unwrap();

    let peers = daemon
        .handle_rpc(RpcRequest { id: 4, method: "list_peers".into(), params: None })
        .unwrap()
        .result
        .unwrap();

    let peer = peers
        .get("peers")
        .and_then(|value| value.as_array())
        .and_then(|items| items.first())
        .cloned()
        .expect("peer record");

    assert_eq!(peer["peer"], "peer-z");
    assert_eq!(peer["first_seen"], 100);
    assert_eq!(peer["last_seen"], 200);
    assert_eq!(peer["seen_count"], 3);
    assert_eq!(peer["name"], "New Name");
    assert_eq!(peer["name_source"], "pn_meta");

    let announces = daemon
        .handle_rpc(RpcRequest { id: 5, method: "list_announces".into(), params: None })
        .unwrap()
        .result
        .unwrap()
        .get("announces")
        .unwrap()
        .as_array()
        .unwrap()
        .clone();
    assert_eq!(announces.len(), 3);
}

#[test]
fn announce_capabilities_are_normalized_and_persisted() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(RpcRequest {
            id: 1,
            method: "announce_received".into(),
            params: Some(json!({
                "peer": "relay-a",
                "timestamp": 321,
                "capabilities": [" Propagation ", "commands", "propagation", ""]
            })),
        })
        .expect("announce_received");

    let event = daemon.take_event().expect("announce event");
    assert_eq!(event.event_type, "announce_received");
    assert_eq!(event.payload["capabilities"], json!(["propagation", "commands"]));

    let announces = daemon
        .handle_rpc(RpcRequest { id: 2, method: "list_announces".into(), params: None })
        .expect("list_announces")
        .result
        .expect("result")
        .get("announces")
        .and_then(|value| value.as_array())
        .cloned()
        .expect("announce list");
    assert_eq!(announces[0]["peer"], "relay-a");
    assert_eq!(announces[0]["capabilities"], json!(["propagation", "commands"]));
}

#[test]
fn announce_capabilities_can_be_derived_from_app_data_hex() {
    let daemon = RpcDaemon::test_instance();
    let app_data = rmp_serde::to_vec(&json!([
        "node name",
        0,
        { "capabilities": ["Paper", "commands", "paper"] }
    ]))
    .expect("encode app data");

    daemon
        .handle_rpc(RpcRequest {
            id: 1,
            method: "announce_received".into(),
            params: Some(json!({
                "peer": "relay-b",
                "timestamp": 500,
                "app_data_hex": hex::encode(app_data),
            })),
        })
        .expect("announce_received");

    let announces = daemon
        .handle_rpc(RpcRequest { id: 2, method: "list_announces".into(), params: None })
        .expect("list_announces")
        .result
        .expect("result")
        .get("announces")
        .and_then(|value| value.as_array())
        .cloned()
        .expect("announce list");
    assert_eq!(announces[0]["peer"], "relay-b");
    assert_eq!(announces[0]["capabilities"], json!(["paper", "commands"]));
}

#[test]
fn announce_capabilities_can_be_derived_from_pn_app_data() {
    let daemon = RpcDaemon::test_instance();
    let app_data = rmp_serde::to_vec(&json!([
        "node name",
        1_700_000_321,
        true,
        10,
        20,
        [40, 50, 60],
        { "1": "Node Name" }
    ]))
    .expect("encode app data");

    daemon
        .handle_rpc(RpcRequest {
            id: 1,
            method: "announce_received".into(),
            params: Some(json!({
                "peer": "relay-pn",
                "timestamp": 700,
                "app_data_hex": hex::encode(app_data),
            })),
        })
        .expect("announce_received");

    let event = daemon.take_event().expect("announce event");
    assert_eq!(event.payload["peer"], "relay-pn");
    assert_eq!(event.payload["capabilities"], json!(["propagation"]));
}

#[test]
fn announce_received_exposes_stamp_cost_flexibility_and_peering_cost() {
    let daemon = RpcDaemon::test_instance();
    daemon
        .handle_rpc(RpcRequest {
            id: 1,
            method: "announce_received".into(),
            params: Some(json!({
                "peer": "peer-costs",
                "timestamp": 1000,
                "stamp_cost_flexibility": 4,
                "peering_cost": 12,
            })),
        })
        .expect("announce_received");

    let event = daemon.take_event().expect("announce event");
    assert_eq!(event.payload["peer"], "peer-costs");
    assert_eq!(event.payload["stamp_cost_flexibility"], 4);
    assert_eq!(event.payload["peering_cost"], 12);

    let announces = daemon
        .handle_rpc(RpcRequest { id: 2, method: "list_announces".into(), params: None })
        .expect("list_announces")
        .result
        .expect("result")
        .get("announces")
        .and_then(|value| value.as_array())
        .cloned()
        .expect("announce list");

    assert_eq!(announces[0]["peer"], "peer-costs");
    assert_eq!(announces[0]["stamp_cost_flexibility"], 4);
    assert_eq!(announces[0]["peering_cost"], 12);
}

#[test]
fn announce_received_falls_back_to_costs_from_pn_app_data() {
    let daemon = RpcDaemon::test_instance();
    let app_data = rmp_serde::to_vec(&json!([
        false,
        1_700_000_321,
        true,
        10,
        20,
        [40, 4, 9],
        { "name": "Node Name" }
    ]))
    .expect("encode app data");

    daemon
        .handle_rpc(RpcRequest {
            id: 1,
            method: "announce_received".into(),
            params: Some(json!({
                "peer": "peer-costs-fallback",
                "timestamp": 1200,
                "app_data_hex": hex::encode(app_data),
            })),
        })
        .expect("announce_received");

    let event = daemon.take_event().expect("announce event");
    assert_eq!(event.payload["peer"], "peer-costs-fallback");
    assert_eq!(event.payload["stamp_cost_flexibility"], 4);
    assert_eq!(event.payload["peering_cost"], 9);
}

#[test]
fn list_announces_applies_limit_and_before_ts() {
    let daemon = RpcDaemon::test_instance();
    for (id, (peer, timestamp)) in
        [("peer-1", 100_i64), ("peer-2", 200_i64), ("peer-3", 300_i64)].into_iter().enumerate()
    {
        daemon
            .handle_rpc(RpcRequest {
                id: id as u64 + 1,
                method: "announce_received".into(),
                params: Some(json!({
                    "peer": peer,
                    "timestamp": timestamp,
                })),
            })
            .expect("announce_received");
    }

    let latest = daemon
        .handle_rpc(RpcRequest {
            id: 10,
            method: "list_announces".into(),
            params: Some(json!({
                "limit": 2
            })),
        })
        .expect("list_announces latest")
        .result
        .expect("latest result")
        .get("announces")
        .and_then(|value| value.as_array())
        .cloned()
        .expect("latest announces");
    let latest_result = daemon
        .handle_rpc(RpcRequest {
            id: 12,
            method: "list_announces".into(),
            params: Some(json!({
                "limit": 2
            })),
        })
        .expect("list_announces with meta")
        .result
        .expect("result");
    assert_eq!(latest_result["meta"]["contract_version"], "v2");
    assert_eq!(latest_result["next_cursor"], "200:announce-200-peer-2-1");
    let latest_timestamps: Vec<i64> =
        latest.iter().map(|entry| entry["timestamp"].as_i64().expect("timestamp")).collect();
    assert_eq!(latest_timestamps, vec![300, 200]);

    let older = daemon
        .handle_rpc(RpcRequest {
            id: 11,
            method: "list_announces".into(),
            params: Some(json!({
                "limit": 5,
                "before_ts": 250
            })),
        })
        .expect("list_announces older")
        .result
        .expect("older result")
        .get("announces")
        .and_then(|value| value.as_array())
        .cloned()
        .expect("older announces");
    let older_timestamps: Vec<i64> =
        older.iter().map(|entry| entry["timestamp"].as_i64().expect("timestamp")).collect();
    assert_eq!(older_timestamps, vec![200, 100]);
}

#[test]
fn list_announces_accepts_cursor_and_returns_next_cursor() {
    let daemon = RpcDaemon::test_instance();
    for (id, (peer, timestamp)) in
        [("peer-1", 100_i64), ("peer-2", 200_i64), ("peer-3", 300_i64), ("peer-4", 400_i64)]
            .into_iter()
            .enumerate()
    {
        daemon
            .handle_rpc(RpcRequest {
                id: id as u64 + 1,
                method: "announce_received".into(),
                params: Some(json!({
                    "peer": peer,
                    "timestamp": timestamp,
                })),
            })
            .expect("announce_received");
    }

    let page_1 = daemon
        .handle_rpc(RpcRequest {
            id: 20,
            method: "list_announces".into(),
            params: Some(json!({
                "limit": 2
            })),
        })
        .expect("page 1")
        .result
        .expect("page 1 result");

    let cursor = page_1["next_cursor"].as_str().expect("next cursor").to_string();
    let page_1_timestamps: Vec<i64> = page_1["announces"]
        .as_array()
        .expect("page 1 announces")
        .iter()
        .map(|entry| entry["timestamp"].as_i64().expect("timestamp"))
        .collect();
    assert_eq!(page_1_timestamps, vec![400, 300]);

    let page_2 = daemon
        .handle_rpc(RpcRequest {
            id: 21,
            method: "list_announces".into(),
            params: Some(json!({
                "limit": 2,
                "cursor": cursor,
            })),
        })
        .expect("page 2")
        .result
        .expect("page 2 result");
    let page_2_timestamps: Vec<i64> = page_2["announces"]
        .as_array()
        .expect("page 2 announces")
        .iter()
        .map(|entry| entry["timestamp"].as_i64().expect("timestamp"))
        .collect();
    assert_eq!(page_2_timestamps, vec![200, 100]);
}
