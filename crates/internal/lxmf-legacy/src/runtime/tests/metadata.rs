use super::super::{
    annotate_peer_records_with_announce_metadata, annotate_response_meta, PeerAnnounceMeta,
};
use serde_json::Value;
use std::collections::HashMap;

#[test]
fn annotate_list_peers_result_with_app_data_hex() {
    let mut result = serde_json::json!({
        "peers": [
            { "peer": "aa11", "last_seen": 1 },
            { "peer": "bb22", "last_seen": 2 }
        ]
    });
    let mut metadata = HashMap::new();
    metadata
        .insert("aa11".to_string(), PeerAnnounceMeta { app_data_hex: Some("cafe".to_string()) });

    annotate_peer_records_with_announce_metadata(&mut result, &metadata);
    assert_eq!(result["peers"][0]["app_data_hex"], Value::String("cafe".to_string()));
    assert_eq!(result["peers"][1]["app_data_hex"], Value::Null);
}

#[test]
fn annotate_response_meta_populates_profile_and_rpc() {
    let mut result = serde_json::json!({
        "nodes": [],
        "meta": {
            "contract_version": "v2",
            "profile": null,
            "rpc_endpoint": null
        }
    });

    annotate_response_meta(&mut result, "weft2", "127.0.0.1:4243");
    assert_eq!(result["meta"]["contract_version"], "v2");
    assert_eq!(result["meta"]["profile"], "weft2");
    assert_eq!(result["meta"]["rpc_endpoint"], "127.0.0.1:4243");
}

#[test]
fn annotate_response_meta_creates_meta_when_missing() {
    let mut result = serde_json::json!({
        "messages": []
    });

    annotate_response_meta(&mut result, "weft2", "127.0.0.1:4243");
    assert_eq!(result["meta"]["contract_version"], "v2");
    assert_eq!(result["meta"]["profile"], "weft2");
    assert_eq!(result["meta"]["rpc_endpoint"], "127.0.0.1:4243");
}

#[test]
fn annotate_response_meta_preserves_existing_non_null_values() {
    let mut result = serde_json::json!({
        "messages": [],
        "meta": {
            "contract_version": "v9",
            "profile": "custom",
            "rpc_endpoint": "192.168.1.10:9999"
        }
    });

    annotate_response_meta(&mut result, "weft2", "127.0.0.1:4243");
    assert_eq!(result["meta"]["contract_version"], "v9");
    assert_eq!(result["meta"]["profile"], "custom");
    assert_eq!(result["meta"]["rpc_endpoint"], "192.168.1.10:9999");
}
