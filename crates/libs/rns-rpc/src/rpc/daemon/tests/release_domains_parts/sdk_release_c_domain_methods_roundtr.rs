#[test]
fn sdk_release_c_domain_methods_roundtrip() {
    let daemon = RpcDaemon::test_instance();
    let registry = daemon
        .handle_rpc(rpc_request(119, "sdk_operation_registry_v2", json!({})))
        .expect("operation registry");
    assert!(registry.error.is_none());
    assert!(registry.result.expect("registry result")["registry"]["entries"]
        .as_array()
        .expect("registry entries")
        .iter()
        .any(|entry| entry["id"] == json!("app.identity.list")));
    let registry_entries = daemon
        .handle_rpc(rpc_request(1191, "sdk_operation_registry_v2", json!({})))
        .expect("operation registry")
        .result
        .expect("registry result")["registry"]["entries"]
        .as_array()
        .expect("registry entries")
        .clone();
    assert!(registry_entries.iter().any(|entry| entry["id"] == json!("app.identity.announce")));
    assert!(registry_entries
        .iter()
        .any(|entry| entry["id"] == json!("app.identity.presence.list")));
    assert!(registry_entries.iter().any(|entry| entry["id"] == json!("app.contact.update")));
    assert!(registry_entries.iter().any(|entry| entry["id"] == json!("app.identity.bootstrap")));
    assert!(registry_entries.iter().any(|entry| entry["id"] == json!("app.peer.connect")));
    assert!(registry_entries.iter().any(|entry| entry["id"] == json!("app.peer.disconnect")));
    assert!(registry_entries.iter().any(|entry| entry["id"] == json!("app.peer.reconnect")));
    assert!(registry_entries.iter().any(|entry| entry["id"] == json!("app.voice.session.open")));
    assert!(registry_entries.iter().any(|entry| entry["id"] == json!("app.voice.session.update")));
    assert!(registry_entries.iter().any(|entry| entry["id"] == json!("app.voice.session.close")));
    assert!(registry_entries.iter().any(|entry| entry["id"] == json!("app.workflow.peer_ready")));
    assert!(registry_entries.iter().any(|entry| entry["id"] == json!("app.workflow.topic_sync")));
    assert!(registry_entries
        .iter()
        .any(|entry| entry["id"] == json!("app.workflow.attachment_report_publish")));
    assert!(registry_entries
        .iter()
        .any(|entry| entry["id"] == json!("app.workflow.mission_update_send")));
    assert!(registry_entries.iter().any(|entry| entry["id"] == json!("app.delivery.send_batch")));
    assert!(registry_entries.iter().any(|entry| entry["id"] == json!("app.delivery.cancel")));
    assert!(registry_entries
        .iter()
        .any(|entry| entry["id"] == json!("app.propagation.peer_sync")));
    assert!(registry_entries
        .iter()
        .any(|entry| entry["id"] == json!("app.propagation.remote_status")));
    assert!(registry_entries
        .iter()
        .any(|entry| entry["id"] == json!("app.propagation.remote_fetch")));
    assert!(registry_entries
        .iter()
        .any(|entry| entry["id"] == json!("app.propagation.remote_download")));
    assert!(registry_entries
        .iter()
        .any(|entry| entry["id"] == json!("app.propagation.remote_sync")));
    assert!(registry_entries
        .iter()
        .any(|entry| entry["id"] == json!("app.propagation.remote_unpeer")));
    assert!(registry_entries
        .iter()
        .any(|entry| entry["id"] == json!("app.propagation.acknowledge_sync_completion")));
    assert!(registry_entries.iter().any(|entry| entry["id"] == json!("app.propagation.node.get")));
    assert!(registry_entries.iter().any(|entry| entry["id"] == json!("app.propagation.node.set")));
    assert!(registry_entries
        .iter()
        .any(|entry| entry["id"] == json!("app.propagation.node.list")));
    assert!(registry_entries.iter().any(|entry| entry["id"] == json!("app.propagation.status")));
    assert!(registry_entries.iter().any(|entry| entry["id"] == json!("app.propagation.enable")));
    assert!(registry_entries
        .iter()
        .any(|entry| entry["id"] == json!("app.propagation.delivery_policy.get")));
    assert!(registry_entries
        .iter()
        .any(|entry| entry["id"] == json!("app.propagation.delivery_policy.set")));
    assert!(registry_entries
        .iter()
        .any(|entry| entry["id"] == json!("app.propagation.peer_maintenance")));
    assert!(registry_entries.iter().any(|entry| entry["id"] == json!("app.propagation.ingest")));
    assert!(registry_entries.iter().any(|entry| entry["id"] == json!("app.propagation.fetch")));

    let list_before =
        daemon.handle_rpc(rpc_request(120, "sdk_identity_list_v2", json!({}))).expect("list");
    assert!(list_before.error.is_none());
    assert!(!list_before.result.expect("result")["identities"]
        .as_array()
        .expect("identity array")
        .is_empty());

    let identity_envelope = daemon
        .handle_rpc(rpc_request(
            1201,
            "sdk_envelope_execute_v2",
            json!({
                "operation_id": "app.identity.list",
                "kind": "query",
                "payload": {},
            }),
        ))
        .expect("identity envelope");
    assert!(identity_envelope.error.is_none());
    assert!(!identity_envelope.result.expect("identity envelope result")["response"]["payload"]
        .as_array()
        .expect("identity payload")
        .is_empty());

    let announce_envelope = daemon
        .handle_rpc(rpc_request(
            1202,
            "sdk_envelope_execute_v2",
            json!({
                "operation_id": "sdk_identity_announce_now_v2",
                "kind": "command",
                "payload": {},
            }),
        ))
        .expect("announce envelope");
    assert!(announce_envelope.error.is_none());
    let announce_response = announce_envelope.result.expect("announce result");
    assert_eq!(announce_response["response"]["operation_id"], json!("app.identity.announce"));
    assert_eq!(announce_response["response"]["payload"]["accepted"], json!(true));

    let batch_envelope = daemon
        .handle_rpc(rpc_request(
            1203,
            "sdk_envelope_execute_v2",
            json!({
                "operation_id": "app.delivery.send_batch",
                "kind": "command",
                "correlation_id": "batch-corr-1",
                "payload": {
                    "batch_id": "batch-envelope-1",
                    "source": "src",
                    "messages": [
                        {
                            "id": "batch-envelope-msg-1",
                            "destination": "dst-a",
                            "title": "hello a",
                            "content": "payload a"
                        },
                        {
                            "id": "batch-envelope-msg-2",
                            "destination": "dst-b",
                            "title": "hello b",
                            "content": "payload b",
                            "method": "direct",
                            "include_ticket": false,
                            "try_propagation_on_fail": true
                        }
                    ]
                },
            }),
        ))
        .expect("batch send envelope");
    assert!(batch_envelope.error.is_none());
    let batch_response = batch_envelope.result.expect("batch envelope result");
    assert_eq!(batch_response["response"]["operation_id"], json!("app.delivery.send_batch"));
    assert_eq!(batch_response["response"]["correlation_id"], json!("batch-corr-1"));
    assert_eq!(batch_response["response"]["payload"]["batch_id"], json!("batch-envelope-1"));
    assert_eq!(batch_response["response"]["payload"]["accepted_count"], json!(2));
    assert_eq!(batch_response["response"]["payload"]["rejected_count"], json!(0));
    assert_eq!(
        batch_response["response"]["payload"]["results"][0]["message_id"],
        json!("batch-envelope-msg-1")
    );
    assert_eq!(
        batch_response["response"]["payload"]["results"][1]["message_id"],
        json!("batch-envelope-msg-2")
    );

    let cancel_envelope = daemon
        .handle_rpc(rpc_request(
            1204,
            "sdk_envelope_execute_v2",
            json!({
                "operation_id": "app.delivery.cancel",
                "kind": "command",
                "correlation_id": "cancel-corr-1",
                "payload": {
                    "message_id": "batch-envelope-msg-1"
                },
            }),
        ))
        .expect("cancel envelope");
    assert!(cancel_envelope.error.is_none());
    let cancel_response = cancel_envelope.result.expect("cancel envelope result");
    assert_eq!(cancel_response["response"]["operation_id"], json!("app.delivery.cancel"));
    assert_eq!(cancel_response["response"]["correlation_id"], json!("cancel-corr-1"));
    assert_eq!(cancel_response["response"]["payload"]["message_id"], json!("batch-envelope-msg-1"));
    assert_eq!(cancel_response["response"]["payload"]["result"], json!("TooLateToCancel"));
    let cancelled_status = daemon
        .handle_rpc(rpc_request(
            1205,
            "sdk_status_v2",
            json!({ "message_id": "batch-envelope-msg-1" }),
        ))
        .expect("cancelled status");
    let cancelled_status_result = cancelled_status.result.expect("cancelled status result");
    assert!(cancelled_status_result["message"]["receipt_status"]
        .as_str()
        .expect("receipt status")
        .starts_with("sent"));

    let peer_sync_envelope = daemon
        .handle_rpc(rpc_request(
            1206,
            "sdk_envelope_execute_v2",
            json!({
                "operation_id": "app.propagation.peer_sync",
                "kind": "command",
                "correlation_id": "peer-sync-corr-1",
                "payload": {
                    "peer": "peer-sdk-prop",
                    "force_sync": true,
                    "transfer_limit_kb": 42.5
                },
            }),
        ))
        .expect("peer sync envelope");
    assert!(peer_sync_envelope.error.is_none());
    let peer_sync_response = peer_sync_envelope.result.expect("peer sync envelope result");
    assert_eq!(
        peer_sync_response["response"]["operation_id"],
        json!("app.propagation.peer_sync")
    );
    assert_eq!(peer_sync_response["response"]["correlation_id"], json!("peer-sync-corr-1"));
    assert_eq!(peer_sync_response["response"]["payload"]["peer"], json!("peer-sdk-prop"));
    assert!(peer_sync_response["response"]["payload"]["messages"].is_object());
    assert!(peer_sync_response["response"]["payload"]["propagation"].is_object());

    daemon.set_remote_control_bridge(std::sync::Arc::new(
        ReleasePropagationRemoteStatusBridge,
    ));
    let remote_status_envelope = daemon
        .handle_rpc(rpc_request(
            1207,
            "sdk_envelope_execute_v2",
            json!({
                "operation_id": "app.propagation.remote_status",
                "kind": "query",
                "correlation_id": "remote-status-corr-1",
                "payload": {
                    "remote": "remote-sdk-prop",
                    "identity_private_key_hex": "feedface",
                    "timeout_secs": 2.5
                },
            }),
        ))
        .expect("remote status envelope");
    assert!(remote_status_envelope.error.is_none());
    let remote_status_response =
        remote_status_envelope.result.expect("remote status envelope result");
    assert_eq!(
        remote_status_response["response"]["operation_id"],
        json!("app.propagation.remote_status")
    );
    assert_eq!(
        remote_status_response["response"]["correlation_id"],
        json!("remote-status-corr-1")
    );
    assert_eq!(
        remote_status_response["response"]["payload"]["remote"],
        json!("remote-sdk-prop")
    );
    assert_eq!(
        remote_status_response["response"]["payload"]["status"]["state"],
        json!("online")
    );
    assert_eq!(
        remote_status_response["response"]["payload"]["status"]["identity_private_key_hex"],
        json!("feedface")
    );

    let ack_envelope = daemon
        .handle_rpc(rpc_request(
            1208,
            "sdk_envelope_execute_v2",
            json!({
                "operation_id": "app.propagation.acknowledge_sync_completion",
                "kind": "command",
                "correlation_id": "propagation-ack-corr-1",
                "payload": {
                    "reset_state": true,
                    "failure_state": 254
                },
            }),
        ))
        .expect("propagation acknowledge envelope");
    assert!(ack_envelope.error.is_none());
    let ack_response = ack_envelope.result.expect("acknowledge envelope result");
    assert_eq!(
        ack_response["response"]["operation_id"],
        json!("app.propagation.acknowledge_sync_completion")
    );
    assert_eq!(
        ack_response["response"]["correlation_id"],
        json!("propagation-ack-corr-1")
    );
    assert_eq!(
        ack_response["response"]["payload"]["propagation"]["sync_state"],
        json!(254)
    );
    assert_eq!(
        ack_response["response"]["payload"]["propagation"]["state_name"],
        json!("failed")
    );

    let node_set_envelope = daemon
        .handle_rpc(rpc_request(
            1209,
            "sdk_envelope_execute_v2",
            json!({
                "operation_id": "app.propagation.node.set",
                "kind": "command",
                "correlation_id": "propagation-node-set-corr-1",
                "payload": {
                    "peer": "router-sdk-prop"
                },
            }),
        ))
        .expect("set propagation node envelope");
    assert!(node_set_envelope.error.is_none());
    let node_set_response = node_set_envelope.result.expect("node set envelope result");
    assert_eq!(
        node_set_response["response"]["operation_id"],
        json!("app.propagation.node.set")
    );
    assert_eq!(
        node_set_response["response"]["correlation_id"],
        json!("propagation-node-set-corr-1")
    );
    assert_eq!(node_set_response["response"]["payload"]["peer"], json!("router-sdk-prop"));

    let node_get_envelope = daemon
        .handle_rpc(rpc_request(
            1210,
            "sdk_envelope_execute_v2",
            json!({
                "operation_id": "app.propagation.node.get",
                "kind": "query",
                "correlation_id": "propagation-node-get-corr-1",
                "payload": {},
            }),
        ))
        .expect("get propagation node envelope");
    assert!(node_get_envelope.error.is_none());
    let node_get_response = node_get_envelope.result.expect("node get envelope result");
    assert_eq!(
        node_get_response["response"]["operation_id"],
        json!("app.propagation.node.get")
    );
    assert_eq!(
        node_get_response["response"]["correlation_id"],
        json!("propagation-node-get-corr-1")
    );
    assert_eq!(node_get_response["response"]["payload"]["peer"], json!("router-sdk-prop"));

    let node_list_envelope = daemon
        .handle_rpc(rpc_request(
            1211,
            "sdk_envelope_execute_v2",
            json!({
                "operation_id": "app.propagation.node.list",
                "kind": "query",
                "correlation_id": "propagation-node-list-corr-1",
                "payload": {},
            }),
        ))
        .expect("list propagation nodes envelope");
    assert!(node_list_envelope.error.is_none());
    let node_list_response = node_list_envelope.result.expect("node list envelope result");
    assert_eq!(
        node_list_response["response"]["operation_id"],
        json!("app.propagation.node.list")
    );
    assert_eq!(
        node_list_response["response"]["correlation_id"],
        json!("propagation-node-list-corr-1")
    );
    assert!(node_list_response["response"]["payload"]["nodes"]
        .as_array()
        .expect("node list")
        .iter()
        .any(|node| node["peer"] == json!("router-sdk-prop") && node["selected"] == json!(true)));

    let propagation_status_envelope = daemon
        .handle_rpc(rpc_request(
            1212,
            "sdk_envelope_execute_v2",
            json!({
                "operation_id": "app.propagation.status",
                "kind": "query",
                "correlation_id": "propagation-status-corr-1",
                "payload": {},
            }),
        ))
        .expect("propagation status envelope");
    assert!(propagation_status_envelope.error.is_none());
    let propagation_status_response =
        propagation_status_envelope.result.expect("propagation status envelope result");
    assert_eq!(
        propagation_status_response["response"]["operation_id"],
        json!("app.propagation.status")
    );
    assert_eq!(
        propagation_status_response["response"]["correlation_id"],
        json!("propagation-status-corr-1")
    );
    assert_eq!(
        propagation_status_response["response"]["payload"]["propagation"]["selected_node"],
        json!("router-sdk-prop")
    );

    let propagation_enable_envelope = daemon
        .handle_rpc(rpc_request(
            1213,
            "sdk_envelope_execute_v2",
            json!({
                "operation_id": "app.propagation.enable",
                "kind": "command",
                "correlation_id": "propagation-enable-corr-1",
                "payload": {
                    "enabled": true,
                    "auth_required": true,
                    "static_peers": ["router-sdk-prop"],
                    "sync_limit": 64
                },
            }),
        ))
        .expect("propagation enable envelope");
    assert!(propagation_enable_envelope.error.is_none());
    let propagation_enable_response =
        propagation_enable_envelope.result.expect("propagation enable envelope result");
    assert_eq!(
        propagation_enable_response["response"]["operation_id"],
        json!("app.propagation.enable")
    );
    assert_eq!(
        propagation_enable_response["response"]["correlation_id"],
        json!("propagation-enable-corr-1")
    );
    assert_eq!(
        propagation_enable_response["response"]["payload"]["propagation"]["enabled"],
        json!(true)
    );
    assert_eq!(
        propagation_enable_response["response"]["payload"]["propagation"]["auth_required"],
        json!(true)
    );

    let delivery_policy_get_envelope = daemon
        .handle_rpc(rpc_request(
            1214,
            "sdk_envelope_execute_v2",
            json!({
                "operation_id": "app.propagation.delivery_policy.get",
                "kind": "query",
                "correlation_id": "delivery-policy-get-corr-1",
                "payload": {},
            }),
        ))
        .expect("delivery policy get envelope");
    assert!(delivery_policy_get_envelope.error.is_none());
    let delivery_policy_get_response =
        delivery_policy_get_envelope.result.expect("delivery policy get envelope result");
    assert_eq!(
        delivery_policy_get_response["response"]["operation_id"],
        json!("app.propagation.delivery_policy.get")
    );
    assert_eq!(
        delivery_policy_get_response["response"]["payload"]["policy"]["auth_required"],
        json!(false)
    );

    let delivery_policy_set_envelope = daemon
        .handle_rpc(rpc_request(
            1215,
            "sdk_envelope_execute_v2",
            json!({
                "operation_id": "app.propagation.delivery_policy.set",
                "kind": "command",
                "correlation_id": "delivery-policy-set-corr-1",
                "payload": {
                    "auth_required": true,
                    "denied_destinations": ["denied-sdk"],
                    "ignored_destinations": ["ignored-sdk"]
                },
            }),
        ))
        .expect("delivery policy set envelope");
    assert!(delivery_policy_set_envelope.error.is_none());
    let delivery_policy_set_response =
        delivery_policy_set_envelope.result.expect("delivery policy set envelope result");
    assert_eq!(
        delivery_policy_set_response["response"]["operation_id"],
        json!("app.propagation.delivery_policy.set")
    );
    assert_eq!(
        delivery_policy_set_response["response"]["correlation_id"],
        json!("delivery-policy-set-corr-1")
    );
    assert_eq!(
        delivery_policy_set_response["response"]["payload"]["policy"]["denied_destinations"],
        json!(["denied-sdk"])
    );
    assert_eq!(
        delivery_policy_set_response["response"]["payload"]["policy"]["ignored_destinations"],
        json!(["ignored-sdk"])
    );

    let peer_maintenance_envelope = daemon
        .handle_rpc(rpc_request(
            1216,
            "sdk_envelope_execute_v2",
            json!({
                "operation_id": "app.propagation.peer_maintenance",
                "kind": "command",
                "correlation_id": "peer-maintenance-corr-1",
                "payload": {},
            }),
        ))
        .expect("peer maintenance envelope");
    assert!(peer_maintenance_envelope.error.is_none());
    let peer_maintenance_response =
        peer_maintenance_envelope.result.expect("peer maintenance envelope result");
    assert_eq!(
        peer_maintenance_response["response"]["operation_id"],
        json!("app.propagation.peer_maintenance")
    );
    assert_eq!(
        peer_maintenance_response["response"]["correlation_id"],
        json!("peer-maintenance-corr-1")
    );
    assert!(peer_maintenance_response["response"]["payload"]["culled"].is_number());
    assert!(peer_maintenance_response["response"]["payload"]["rotated"].is_number());
    assert!(peer_maintenance_response["response"]["payload"]["peer_sync"].is_object()
        || peer_maintenance_response["response"]["payload"]["peer_sync"].is_null());

    let propagation_payload = b"sdk-local-propagation-payload";
    let propagation_payload_hex = hex::encode(propagation_payload);
    let propagation_transient_id = {
        use sha2::{Digest, Sha256};
        hex::encode(Sha256::digest(propagation_payload))
    };
    let propagation_ingest_envelope = daemon
        .handle_rpc(rpc_request(
            1217,
            "sdk_envelope_execute_v2",
            json!({
                "operation_id": "app.propagation.ingest",
                "kind": "command",
                "correlation_id": "propagation-ingest-corr-1",
                "payload": {
                    "payload_hex": propagation_payload_hex
                },
            }),
        ))
        .expect("propagation ingest envelope");
    assert!(propagation_ingest_envelope.error.is_none());
    let propagation_ingest_response =
        propagation_ingest_envelope.result.expect("propagation ingest envelope result");
    assert_eq!(
        propagation_ingest_response["response"]["operation_id"],
        json!("app.propagation.ingest")
    );
    assert_eq!(
        propagation_ingest_response["response"]["correlation_id"],
        json!("propagation-ingest-corr-1")
    );
    assert_eq!(
        propagation_ingest_response["response"]["payload"]["transient_id"],
        json!(propagation_transient_id)
    );
    assert_eq!(
        propagation_ingest_response["response"]["payload"]["ingested_count"],
        json!(1)
    );

    let propagation_fetch_envelope = daemon
        .handle_rpc(rpc_request(
            1218,
            "sdk_envelope_execute_v2",
            json!({
                "operation_id": "app.propagation.fetch",
                "kind": "command",
                "correlation_id": "propagation-fetch-corr-1",
                "payload": {
                    "transient_id": propagation_transient_id
                },
            }),
        ))
        .expect("propagation fetch envelope");
    assert!(propagation_fetch_envelope.error.is_none());
    let propagation_fetch_response =
        propagation_fetch_envelope.result.expect("propagation fetch envelope result");
    assert_eq!(
        propagation_fetch_response["response"]["operation_id"],
        json!("app.propagation.fetch")
    );
    assert_eq!(
        propagation_fetch_response["response"]["correlation_id"],
        json!("propagation-fetch-corr-1")
    );
    assert_eq!(
        propagation_fetch_response["response"]["payload"]["payload_hex"],
        json!(propagation_payload_hex)
    );
    assert_eq!(
        propagation_fetch_response["response"]["payload"]["payload_bytes"],
        json!(propagation_payload.len())
    );

    let identity_bundle = json!({
        "identity": "node-b",
        "public_key": "node-b-pub",
        "display_name": "Node B",
        "capabilities": ["ops"],
        "extensions": {}
    });
    let identity_import = daemon
        .handle_rpc(rpc_request(
            121,
            "sdk_identity_import_v2",
            json!({
                "bundle_base64": BASE64_STANDARD.encode(identity_bundle.to_string().as_bytes()),
                "passphrase": null
            }),
        ))
        .expect("identity import");
    assert!(identity_import.error.is_none());
    assert_eq!(identity_import.result.expect("result")["identity"]["identity"], json!("node-b"));

    let identity_resolve = daemon
        .handle_rpc(rpc_request(122, "sdk_identity_resolve_v2", json!({ "hash": "node-b-pub" })))
        .expect("identity resolve");
    assert!(identity_resolve.error.is_none());
    assert_eq!(identity_resolve.result.expect("result")["identity"], json!("node-b"));

    let contact_update = daemon
        .handle_rpc(rpc_request(
            1221,
            "sdk_identity_contact_update_v2",
            json!({
                "identity": "node-b",
                "display_name": "Node Bravo",
                "trust_level": "untrusted",
                "bootstrap": false,
                "metadata": { "source": "manual" }
            }),
        ))
        .expect("contact update");
    assert!(contact_update.error.is_none());
    assert_eq!(
        contact_update.result.expect("result")["contact"]["trust_level"],
        json!("untrusted")
    );

    let contact_list = daemon
        .handle_rpc(rpc_request(1222, "sdk_identity_contact_list_v2", json!({ "limit": 10 })))
        .expect("contact list");
    assert!(contact_list.error.is_none());
    assert!(contact_list.result.expect("result")["contact_list"]["contacts"]
        .as_array()
        .expect("contact rows")
        .iter()
        .any(|row| row["identity"] == json!("node-b")));

    let bootstrap = daemon
        .handle_rpc(rpc_request(
            1223,
            "sdk_identity_bootstrap_v2",
            json!({ "identity": "node-b", "auto_sync": true }),
        ))
        .expect("bootstrap");
    assert!(bootstrap.error.is_none());
    let bootstrap_result = bootstrap.result.expect("bootstrap result");
    assert_eq!(bootstrap_result["synced"], json!(true));
    assert_eq!(bootstrap_result["contact"]["trust_level"], json!("trusted"));
    assert_eq!(bootstrap_result["contact"]["bootstrap"], json!(true));

    let presence = daemon
        .handle_rpc(rpc_request(
            1224,
            "sdk_identity_presence_list_v2",
            json!({ "cursor": null, "limit": 10 }),
        ))
        .expect("presence list");
    assert!(presence.error.is_none());
    assert!(presence.result.expect("result")["presence_list"]["peers"]
        .as_array()
        .expect("presence rows")
        .iter()
        .any(|row| {
            row["peer_id"] == json!("node-b")
                && row["trust_level"] == json!("trusted")
                && row["bootstrap"] == json!(true)
        }));

    let filtered_presence = daemon
        .handle_rpc(rpc_request(
            12240,
            "sdk_identity_presence_list_v2",
            json!({ "cursor": null, "limit": 10, "min_last_seen_ts_ms": 1_700_000_000 }),
        ))
        .expect("filtered presence list");
    assert!(filtered_presence.error.is_none());
    assert!(filtered_presence.result.expect("result")["presence_list"]["peers"]
        .as_array()
        .expect("filtered presence rows")
        .iter()
        .all(|row| row["last_seen_ts_ms"].as_i64().is_some_and(|last_seen| {
            last_seen >= 1_700_000_000
        })));

    let announce_now = daemon
        .handle_rpc(rpc_request(1225, "sdk_identity_announce_now_v2", json!({})))
        .expect("identity announce now");
    assert!(announce_now.error.is_none());
    assert_eq!(announce_now.result.expect("result")["accepted"], json!(true));

    let identity_export = daemon
        .handle_rpc(rpc_request(123, "sdk_identity_export_v2", json!({ "identity": "node-b" })))
        .expect("identity export");
    assert!(identity_export.error.is_none());
    assert!(!identity_export.result.expect("result")["bundle"]["bundle_base64"]
        .as_str()
        .expect("export bundle")
        .is_empty());

    let _ = daemon
        .handle_rpc(rpc_request(
            124,
            "send_message_v2",
            json!({
                "id": "paper-msg-1",
                "source": "src",
                "destination": "dst",
                "title": "",
                "content": "paper body"
            }),
        ))
        .expect("send message for paper");
    let paper_encode = daemon
        .handle_rpc(rpc_request(125, "sdk_paper_encode_v2", json!({ "message_id": "paper-msg-1" })))
        .expect("paper encode");
    assert!(paper_encode.error.is_none());
    let uri = paper_encode.result.expect("result")["envelope"]["uri"]
        .as_str()
        .expect("paper uri")
        .to_string();
    assert!(uri.starts_with("lxm://"));

    let paper_decode = daemon
        .handle_rpc(rpc_request(126, "sdk_paper_decode_v2", json!({ "uri": uri })))
        .expect("paper decode");
    assert!(paper_decode.error.is_none());
    assert_eq!(paper_decode.result.expect("result")["accepted"], json!(true));

    let pre_command_poll = daemon
        .handle_rpc(rpc_request(
            1269,
            "sdk_poll_events_v2",
            json!({ "cursor": null, "max": 200 }),
        ))
        .expect("pre-command poll");
    assert!(pre_command_poll.error.is_none());
    let pre_command_cursor = pre_command_poll.result.expect("pre-command poll result")["next_cursor"]
        .as_str()
        .expect("pre-command cursor")
        .to_string();

    let command = daemon
        .handle_rpc(rpc_request(
            127,
            "sdk_command_invoke_v2",
            json!({
                "command": "ping",
                "target": "node-b",
                "payload": { "body": "hello" },
                "timeout_ms": 1000
            }),
        ))
        .expect("command invoke");
    assert!(command.error.is_none());
    let correlation_id = command.result.expect("result")["response"]["payload"]["correlation_id"]
        .as_str()
        .expect("correlation id")
        .to_string();
    let command_session = daemon
        .handle_rpc(rpc_request(
            1271,
            "sdk_command_session_get_v2",
            json!({ "correlation_id": correlation_id.clone() }),
        ))
        .expect("command session get");
    assert!(command_session.error.is_none());
    let command_session_result = command_session.result.expect("command session result");
    assert_eq!(command_session_result["session"]["correlation_id"], json!(correlation_id.clone()));
    assert_eq!(command_session_result["session"]["command_state"], json!("dispatched"));

    let command_sessions = daemon
        .handle_rpc(rpc_request(
            1272,
            "sdk_command_session_list_v2",
            json!({ "limit": 10 }),
        ))
        .expect("command session list");
    assert!(command_sessions.error.is_none());
    assert!(command_sessions.result.expect("command session list result")["session_list"]["sessions"]
        .as_array()
        .expect("command session rows")
        .iter()
        .any(|row| row["correlation_id"] == json!(correlation_id.clone())));
    let dispatched_events = daemon
        .handle_rpc(rpc_request(
            1273,
            "sdk_poll_events_v2",
            json!({ "cursor": pre_command_cursor, "max": 50 }),
        ))
        .expect("poll dispatched events");
    assert!(dispatched_events.error.is_none());
    let dispatched_events_result = dispatched_events.result.expect("dispatched events result");
    let post_dispatch_cursor = dispatched_events_result["next_cursor"]
        .as_str()
        .expect("post-dispatch cursor")
        .to_string();
    assert!(dispatched_events_result["events"]
        .as_array()
        .expect("events")
        .iter()
        .any(|event| {
            event["event_type"] == json!("command.dispatched")
                && event["payload"]["command"] == json!("ping")
        }));

    let command_reply = daemon
        .handle_rpc(rpc_request(
            128,
            "sdk_command_reply_v2",
            json!({
                "correlation_id": correlation_id,
                "accepted": true,
                "payload": { "reply": "pong" }
            }),
        ))
        .expect("command reply");
    assert!(command_reply.error.is_none());
    assert_eq!(command_reply.result.expect("result")["accepted"], json!(true));
    let command_session_after_reply = daemon
        .handle_rpc(rpc_request(
            1282,
            "sdk_command_session_get_v2",
            json!({ "correlation_id": correlation_id.clone() }),
        ))
        .expect("command session after reply");
    assert!(command_session_after_reply.error.is_none());
    let command_session_after_reply_result =
        command_session_after_reply.result.expect("command session after reply result");
    assert_eq!(command_session_after_reply_result["session"]["command_state"], json!("completed"));
    assert_eq!(
        command_session_after_reply_result["session"]["response_payload"]["reply"],
        json!("pong")
    );
    let completion_events = daemon
        .handle_rpc(rpc_request(
            1283,
            "sdk_poll_events_v2",
            json!({ "cursor": post_dispatch_cursor, "max": 50 }),
        ))
        .expect("poll completion events");
    assert!(completion_events.error.is_none());
    assert!(completion_events.result.expect("completion events result")["events"]
        .as_array()
        .expect("events")
        .iter()
        .any(|event| {
            event["event_type"] == json!("command.completed")
                && event["payload"]["accepted"] == json!(true)
        }));

    let envelope_command = daemon
        .handle_rpc(rpc_request(
            1281,
            "sdk_envelope_execute_v2",
            json!({
                "operation_id": "vendor.example.custom",
                "kind": "command",
                "correlation_id": "env-corr-1",
                "target": "node-b",
                "payload": { "body": "hello" },
                "timeout_ms": 500,
                "extensions": { "via": "test" }
            }),
        ))
        .expect("custom envelope command");
    assert!(envelope_command.error.is_none());
    let envelope_response = &envelope_command.result.expect("envelope command result")["response"];
    assert_eq!(envelope_response["accepted"], json!(true));
    assert_eq!(envelope_response["extensions"]["via"], json!("test"));
    assert_eq!(envelope_response["correlation_id"], json!("env-corr-1"));
    let envelope_payload = &envelope_response["payload"];
    assert_eq!(envelope_payload["command"], json!("vendor.example.custom"));
    assert_eq!(envelope_payload["command_state"], json!("dispatched"));
    assert!(envelope_payload["correlation_id"].as_str().is_some());
    assert_eq!(envelope_payload["target"], json!("node-b"));

    let voice_open = daemon
        .handle_rpc(rpc_request(
            129,
            "sdk_voice_session_open_v2",
            json!({ "peer_id": "node-b", "codec_hint": "opus" }),
        ))
        .expect("voice open");
    assert!(voice_open.error.is_none());
    let session_id =
        voice_open.result.expect("result")["session_id"].as_str().expect("session id").to_string();

    let voice_update = daemon
        .handle_rpc(rpc_request(
            130,
            "sdk_voice_session_update_v2",
            json!({ "session_id": session_id.clone(), "state": "active" }),
        ))
        .expect("voice update");
    assert!(voice_update.error.is_none());
    assert_eq!(voice_update.result.expect("result")["state"], json!("active"));

    let voice_close = daemon
        .handle_rpc(rpc_request(
            131,
            "sdk_voice_session_close_v2",
            json!({ "session_id": session_id }),
        ))
        .expect("voice close");
    assert!(voice_close.error.is_none());
    assert_eq!(voice_close.result.expect("result")["accepted"], json!(true));

    let voice_open_envelope = daemon
        .handle_rpc(rpc_request(
            1311,
            "sdk_envelope_execute_v2",
            json!({
                "operation_id": "sdk_voice_session_open_v2",
                "kind": "command",
                "payload": { "peer_id": "node-b", "codec_hint": "opus" },
            }),
        ))
        .expect("voice open envelope");
    assert!(voice_open_envelope.error.is_none());
    let voice_session_id = voice_open_envelope.result.expect("voice open envelope result")
        ["response"]["payload"]
        .as_str()
        .expect("voice session id")
        .to_string();

    let voice_update_envelope = daemon
        .handle_rpc(rpc_request(
            1312,
            "sdk_envelope_execute_v2",
            json!({
                "operation_id": "app.voice.session.update",
                "kind": "command",
                "payload": { "session_id": voice_session_id, "state": "active" },
            }),
        ))
        .expect("voice update envelope");
    assert!(voice_update_envelope.error.is_none());
    assert_eq!(
        voice_update_envelope.result.expect("voice update envelope result")["response"]["payload"],
        json!("active")
    );
}

struct ReleasePropagationRemoteStatusBridge;

impl RemoteControlBridge for ReleasePropagationRemoteStatusBridge {
    fn propagation_remote_status(
        &self,
        remote: &str,
        identity_private_key_hex: Option<&str>,
        timeout_secs: f64,
    ) -> Result<JsonValue, std::io::Error> {
        Ok(json!({
            "state": "online",
            "remote": remote,
            "identity_private_key_hex": identity_private_key_hex,
            "timeout_secs": timeout_secs
        }))
    }

    fn propagation_remote_sync(
        &self,
        _remote: &str,
        _peer: &str,
        _identity_private_key_hex: Option<&str>,
        _timeout_secs: f64,
        _transfer_limit_kb: Option<f64>,
    ) -> Result<JsonValue, std::io::Error> {
        Ok(json!({ "synced": false, "postponed": true }))
    }

    fn propagation_remote_fetch(
        &self,
        _remote: &str,
        _identity_private_key_hex: Option<&str>,
        _timeout_secs: f64,
        _transfer_limit_kb: Option<f64>,
    ) -> Result<JsonValue, std::io::Error> {
        Ok(json!({ "synced": false, "postponed": true }))
    }

    fn propagation_remote_download(
        &self,
        _remote: &str,
        _identity_private_key_hex: Option<&str>,
        _timeout_secs: f64,
        _transfer_limit_kb: Option<f64>,
    ) -> Result<JsonValue, std::io::Error> {
        Ok(json!({ "synced": false, "postponed": true }))
    }

    fn propagation_remote_unpeer(
        &self,
        _remote: &str,
        _peer: &str,
        _identity_private_key_hex: Option<&str>,
        _timeout_secs: f64,
    ) -> Result<JsonValue, std::io::Error> {
        Ok(json!({ "accepted": true }))
    }
}

#[test]
fn sdk_identity_presence_list_filters_stale_peers_by_last_seen() {
    let daemon = RpcDaemon::test_instance();
    let fresh: PeerRecord = serde_json::from_value(json!({
        "destination_hash": "fresh-peer",
        "last_seen": 1_700_000_800,
        "last_heard": 1_700_000_800,
        "first_seen": 1_700_000_700,
        "seen_count": 2,
        "name": "Fresh Peer",
        "name_source": "announce",
        "alive": true,
    }))
    .expect("fresh peer record");
    let stale: PeerRecord = serde_json::from_value(json!({
        "destination_hash": "stale-peer",
        "last_seen": 1_700_000_100,
        "last_heard": 1_700_000_100,
        "first_seen": 1_700_000_000,
        "seen_count": 1,
        "name": "Stale Peer",
        "name_source": "announce",
        "alive": false,
    }))
    .expect("stale peer record");
    {
        let mut peers = daemon.peers.lock().expect("peers mutex poisoned");
        peers.insert(fresh.peer.clone(), fresh);
        peers.insert(stale.peer.clone(), stale);
    }

    let response = daemon
        .handle_rpc(rpc_request(
            12241,
            "sdk_identity_presence_list_v2",
            json!({ "cursor": null, "limit": 10, "min_last_seen_ts_ms": 1_700_000_500 }),
        ))
        .expect("filtered presence list");
    assert!(response.error.is_none());
    let result = response.result.expect("presence result");
    let rows = result["presence_list"]["peers"].as_array().expect("presence rows");
    assert!(rows.iter().any(|row| row["peer_id"] == json!("fresh-peer")));
    assert!(!rows.iter().any(|row| row["peer_id"] == json!("stale-peer")));
    assert!(rows
        .iter()
        .all(|row| row["last_seen_ts_ms"].as_i64().is_some_and(|last_seen| {
            last_seen >= 1_700_000_500
        })));
}

#[test]
fn sdk_peer_lifecycle_methods_roundtrip_through_daemon_dispatch() {
    let daemon = RpcDaemon::test_instance();
    let request = json!({
        "identity": "peer-lifecycle",
        "display_name": "RCH Relay",
        "correlation_id": "peer-life-corr",
        "metadata": {
            "callsign": "RCH-1",
            "capability_flags": ["rem.direct_chat"],
            "announce_slots": ["rch.broadcast"]
        },
        "extensions": {
            "source": "rem-rch"
        }
    });

    let connected = daemon
        .handle_rpc(rpc_request(12242, "sdk_peer_connect_v2", request.clone()))
        .expect("peer connect");
    assert!(connected.error.is_none());
    let connected_peer = connected.result.expect("connect result")["peer"].clone();
    assert_eq!(connected_peer["identity"], json!("peer-lifecycle"));
    assert_eq!(connected_peer["state"], json!("connected"));
    assert_eq!(connected_peer["connected"], json!(true));
    assert_eq!(connected_peer["display_name"], json!("RCH Relay"));
    assert_eq!(connected_peer["metadata"]["callsign"], json!("RCH-1"));
    assert_eq!(connected_peer["metadata"]["capability_flags"][0], json!("rem.direct_chat"));
    assert_eq!(connected_peer["metadata"]["announce_slots"][0], json!("rch.broadcast"));
    assert_eq!(connected_peer["extensions"]["source"], json!("rem-rch"));

    let disconnected = daemon
        .handle_rpc(rpc_request(12243, "sdk_peer_disconnect_v2", request.clone()))
        .expect("peer disconnect");
    assert!(disconnected.error.is_none());
    let disconnected_peer = disconnected.result.expect("disconnect result")["peer"].clone();
    assert_eq!(disconnected_peer["identity"], json!("peer-lifecycle"));
    assert_eq!(disconnected_peer["state"], json!("disconnected"));
    assert_eq!(disconnected_peer["connected"], json!(false));

    let reconnected = daemon
        .handle_rpc(rpc_request(12244, "sdk_peer_reconnect_v2", request))
        .expect("peer reconnect");
    assert!(reconnected.error.is_none());
    let reconnected_peer = reconnected.result.expect("reconnect result")["peer"].clone();
    assert_eq!(reconnected_peer["identity"], json!("peer-lifecycle"));
    assert_eq!(reconnected_peer["state"], json!("reconnected"));
    assert_eq!(reconnected_peer["connected"], json!(true));
}
