use crate::bootstrap::{
    enforce_startup_policy, mark_interface_runtime_fields, mark_interface_startup_status,
    select_tcp_server_bind, InterfaceStartupFailure,
};
use crate::bridge::{
    validate_delivery_request, PeerCrypto, RequestedDeliveryMethod, TransportBridge,
};
use crate::bridge_helpers::opportunistic_payload;
use crate::interfaces::{lora, serial};
use crate::{bootstrap, Args};
use futures::FutureExt;
use lxmf::WireMessage;
use reticulum_daemon::announce_names::{
    encode_propagation_node_app_data, pn_peering_cost_from_app_data,
    pn_stamp_cost_flexibility_from_app_data, pn_stamp_cost_from_app_data,
    PropagationNodeAnnounceConfig,
};
use reticulum_daemon::config::InterfaceConfig;
use rns_core::identity::PrivateIdentity;
use rns_rpc::{InterfaceRecord, MessagesStore, OutboundBridge, RpcDaemon, RpcRequest};
use rns_transport::destination::{link::LinkStatus, DestinationDesc, DestinationName};
use rns_transport::destination_hash::parse_destination_hash_required;
use rns_transport::transport::{Transport, TransportConfig};
use serde_json::json;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};
use tempfile::TempDir;

#[test]
fn opportunistic_payload_strips_destination_prefix() {
    let destination = [0xAA; 16];
    let mut payload = destination.to_vec();
    payload.extend_from_slice(&[1, 2, 3, 4]);
    assert_eq!(opportunistic_payload(&payload, &destination), &[1, 2, 3, 4]);
}

#[test]
fn opportunistic_payload_keeps_payload_without_prefix() {
    let destination = [0xAA; 16];
    let payload = vec![0xBB; 24];
    assert_eq!(opportunistic_payload(&payload, &destination), payload.as_slice());
}

#[test]
fn delivery_method_defaults_to_direct() {
    assert_eq!(
        RequestedDeliveryMethod::parse(None).expect("default method"),
        RequestedDeliveryMethod::Direct
    );
    assert_eq!(
        RequestedDeliveryMethod::parse(Some("  ")).expect("blank method"),
        RequestedDeliveryMethod::Direct
    );
}

#[test]
fn delivery_method_parses_supported_modes() {
    assert_eq!(
        RequestedDeliveryMethod::parse(Some("opportunistic")).expect("opportunistic"),
        RequestedDeliveryMethod::Opportunistic
    );
    assert_eq!(
        RequestedDeliveryMethod::parse(Some("PrOpAgAtEd")).expect("propagated"),
        RequestedDeliveryMethod::Propagated
    );
    assert_eq!(
        RequestedDeliveryMethod::parse(Some("paper")).expect("paper"),
        RequestedDeliveryMethod::Paper
    );
}

#[test]
fn propagated_delivery_requires_selected_node() {
    let err = validate_delivery_request(RequestedDeliveryMethod::Propagated, None)
        .expect_err("missing propagation node should fail");
    assert_eq!(err.kind(), std::io::ErrorKind::NotFound);

    validate_delivery_request(RequestedDeliveryMethod::Propagated, Some("deadbeef"))
        .expect("selected node should satisfy propagated delivery");
}

async fn test_transport_bridge_fixture() -> (Arc<RpcDaemon>, Arc<TransportBridge>) {
    let (daemon, bridge, _, _) = test_transport_bridge_fixture_with_peer().await;
    (daemon, bridge)
}

async fn test_transport_bridge_fixture_with_peer(
) -> (Arc<RpcDaemon>, Arc<TransportBridge>, PrivateIdentity, String) {
    let signer = PrivateIdentity::new_from_rand(rand_core::OsRng);
    let transport_identity = rns_transport::identity_bridge::to_transport_private_identity(&signer);
    let mut transport = Transport::new(TransportConfig::new("test", &transport_identity, true));
    let announce_destination = transport
        .add_destination(transport_identity.clone(), DestinationName::new("lxmf", "delivery"))
        .await;
    let transport = Arc::new(transport);

    let receipt_map = Arc::new(Mutex::new(HashMap::new()));
    let outbound_resource_map = Arc::new(Mutex::new(HashMap::new()));
    let peer_crypto = Arc::new(Mutex::new(HashMap::new()));
    let recipient = PrivateIdentity::new_from_rand(rand_core::OsRng);
    let recipient_hex = hex::encode(recipient.address_hash().as_slice());
    peer_crypto.lock().expect("peer map").insert(
        recipient_hex.clone(),
        PeerCrypto {
            identity: rns_transport::identity_bridge::to_transport_identity(
                recipient.as_identity(),
            ),
        },
    );
    let (receipt_tx, _receipt_rx) = tokio::sync::mpsc::unbounded_channel();

    let bridge = Arc::new(TransportBridge::new(
        transport,
        signer,
        [0u8; 16],
        announce_destination,
        None,
        None,
        encode_propagation_node_app_data(
            Some("Bridge Node"),
            PropagationNodeAnnounceConfig::default(),
        ),
        None,
        peer_crypto,
        receipt_map,
        outbound_resource_map,
        receipt_tx,
    ));

    let daemon = Arc::new(RpcDaemon::with_store_and_bridges(
        MessagesStore::in_memory().expect("in-memory store"),
        "bridge-test-node".to_string(),
        Some(bridge.clone() as Arc<dyn OutboundBridge>),
        None,
    ));
    bridge.set_daemon(daemon.clone());

    (daemon, bridge, recipient, recipient_hex)
}

#[tokio::test]
async fn transport_bridge_regenerates_propagation_app_data_from_daemon_state() {
    let (daemon, bridge) = test_transport_bridge_fixture().await;
    daemon
        .handle_rpc(RpcRequest {
            id: 300,
            method: "propagation_enable".into(),
            params: Some(json!({
                "enabled": true,
                "target_cost": 22,
                "stamp_cost_flexibility": 6,
                "peering_cost": 17,
            })),
        })
        .expect("enable propagation");

    let app_data =
        bridge.current_propagation_announce_app_data_for_test().expect("propagation app data");

    assert_eq!(pn_stamp_cost_from_app_data(app_data.as_slice()), Some(22));
    assert_eq!(pn_stamp_cost_flexibility_from_app_data(app_data.as_slice()), Some(6));
    assert_eq!(pn_peering_cost_from_app_data(app_data.as_slice()), Some(17));
}

#[tokio::test]
async fn transport_bridge_leaves_paper_messages_non_terminal_for_encoding() {
    let (daemon, _bridge) = test_transport_bridge_fixture().await;

    let send = daemon
        .handle_rpc(RpcRequest {
            id: 200,
            method: "send_message_v2".into(),
            params: Some(json!({
                "id": "paper-bridge-1",
                "source": "src",
                "destination": "0123456789abcdef0123456789abcdef",
                "title": "",
                "content": "hello",
                "method": "paper"
            })),
        })
        .expect("send");
    assert!(send.error.is_none(), "paper send should remain schedulable");

    let status = daemon
        .handle_rpc(RpcRequest {
            id: 201,
            method: "sdk_status_v2".into(),
            params: Some(json!({ "message_id": "paper-bridge-1" })),
        })
        .expect("status");
    assert_eq!(status.result.expect("result")["message"]["receipt_status"], json!("sending"));

    let encode = daemon
        .handle_rpc(RpcRequest {
            id: 202,
            method: "sdk_paper_encode_v2".into(),
            params: Some(json!({ "message_id": "paper-bridge-1" })),
        })
        .expect("paper encode");
    assert!(encode.error.is_none(), "paper encode should stay available on bridge-backed runtime");

    let final_status = daemon
        .handle_rpc(RpcRequest {
            id: 203,
            method: "sdk_status_v2".into(),
            params: Some(json!({ "message_id": "paper-bridge-1" })),
        })
        .expect("status after encode");
    assert_eq!(
        final_status.result.expect("result")["message"]["receipt_status"],
        json!("sent: paper")
    );
}

#[tokio::test]
async fn sdk_paper_encode_uses_real_lxm_uri_when_peer_identity_is_known() {
    let (daemon, _bridge, recipient, recipient_hex) =
        test_transport_bridge_fixture_with_peer().await;

    let send = daemon
        .handle_rpc(RpcRequest {
            id: 261,
            method: "send_message_v2".into(),
            params: Some(json!({
                "id": "paper-real-uri-1",
                "source": "src",
                "destination": recipient_hex,
                "title": "Paper URI Title",
                "content": "paper uri body",
                "method": "paper"
            })),
        })
        .expect("send");
    assert!(send.error.is_none(), "paper send should be accepted");

    let encode = daemon
        .handle_rpc(RpcRequest {
            id: 262,
            method: "sdk_paper_encode_v2".into(),
            params: Some(json!({ "message_id": "paper-real-uri-1" })),
        })
        .expect("paper encode");
    assert!(encode.error.is_none(), "paper encode should succeed");
    let uri =
        encode.result.expect("result")["envelope"]["uri"].as_str().expect("paper uri").to_string();
    assert!(uri.starts_with("lxm://"));
    assert!(
        !uri.trim_start_matches("lxm://").contains('/'),
        "real paper URI should be one encoded payload, not a placeholder path"
    );

    let decoded =
        WireMessage::unpack_paper_uri(uri.as_str(), &recipient).expect("decode real paper URI");
    assert_eq!(
        decoded.payload.title.as_ref().map(|title| String::from_utf8_lossy(title).to_string()),
        Some("Paper URI Title".to_string())
    );
    assert_eq!(
        decoded
            .payload
            .content
            .as_ref()
            .map(|content| String::from_utf8_lossy(content).to_string()),
        Some("paper uri body".to_string())
    );
}

#[tokio::test]
async fn transport_bridge_rejects_propagated_send_without_selected_node() {
    let (daemon, _bridge) = test_transport_bridge_fixture().await;

    let send = daemon
        .handle_rpc(RpcRequest {
            id: 210,
            method: "send_message_v2".into(),
            params: Some(json!({
                "id": "propagated-bridge-1",
                "source": "src",
                "destination": "0123456789abcdef0123456789abcdef",
                "title": "",
                "content": "hello",
                "method": "propagated"
            })),
        })
        .expect("send");
    let error = send.error.expect("propagated send should fail without node");
    assert_eq!(error.code, "DELIVERY_FAILED");
    assert!(
        error.message.contains("no outbound propagation node selected"),
        "unexpected error: {}",
        error.message
    );
}

#[tokio::test]
async fn propagation_link_cache_reuses_same_selected_node() {
    let (_daemon, bridge) = test_transport_bridge_fixture().await;
    let signer = PrivateIdentity::new_from_rand(rand_core::OsRng);
    let identity = rns_transport::identity_bridge::to_transport_private_identity(&signer);
    let destination = DestinationDesc {
        identity: *identity.as_identity(),
        address_hash: *identity.address_hash(),
        name: DestinationName::new("lxmf", "propagation"),
    };

    let first = bridge.propagation_link_for_test("peer-a", destination).await;
    let second = bridge.propagation_link_for_test("peer-a", destination).await;

    let first_id = *first.lock().await.id();
    let second_id = *second.lock().await.id();
    assert_eq!(first_id, second_id);
}

#[tokio::test]
async fn propagation_link_cache_does_not_close_previous_link_when_selected_node_changes() {
    let (_daemon, bridge) = test_transport_bridge_fixture().await;
    let signer_a = PrivateIdentity::new_from_rand(rand_core::OsRng);
    let identity_a = rns_transport::identity_bridge::to_transport_private_identity(&signer_a);
    let destination_a = DestinationDesc {
        identity: *identity_a.as_identity(),
        address_hash: *identity_a.address_hash(),
        name: DestinationName::new("lxmf", "propagation"),
    };
    let signer_b = PrivateIdentity::new_from_rand(rand_core::OsRng);
    let identity_b = rns_transport::identity_bridge::to_transport_private_identity(&signer_b);
    let destination_b = DestinationDesc {
        identity: *identity_b.as_identity(),
        address_hash: *identity_b.address_hash(),
        name: DestinationName::new("lxmf", "propagation"),
    };

    let first = bridge.propagation_link_for_test("peer-a", destination_a).await;
    let second = bridge.propagation_link_for_test("peer-b", destination_b).await;

    let first_id = *first.lock().await.id();
    let second_id = *second.lock().await.id();
    assert_ne!(first_id, second_id);
    assert_ne!(first.lock().await.status(), LinkStatus::Closed);
}

#[tokio::test]
async fn propagation_link_cache_recreates_closed_link_for_same_selected_node() {
    let (_daemon, bridge) = test_transport_bridge_fixture().await;
    let signer = PrivateIdentity::new_from_rand(rand_core::OsRng);
    let identity = rns_transport::identity_bridge::to_transport_private_identity(&signer);
    let destination = DestinationDesc {
        identity: *identity.as_identity(),
        address_hash: *identity.address_hash(),
        name: DestinationName::new("lxmf", "propagation"),
    };

    let first = bridge.propagation_link_for_test("peer-a", destination).await;
    let first_id = *first.lock().await.id();
    first.lock().await.close();

    let second = bridge.propagation_link_for_test("peer-a", destination).await;

    assert_ne!(first_id, *second.lock().await.id());
}

#[test]
fn parse_destination_hex_required_rejects_invalid_hashes() {
    let err = parse_destination_hash_required("not-hex").expect_err("invalid hash");
    assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
}

#[test]
fn serial_builder_rejects_missing_required_fields() {
    let iface = InterfaceConfig {
        kind: "serial".to_string(),
        enabled: Some(true),
        ..InterfaceConfig::default()
    };
    let result = serial::build_adapter(&iface);
    assert!(result.is_err(), "missing device/baud should fail");
    let err = result.err().unwrap_or_default();
    assert!(err.contains("serial.device"));
}

#[test]
fn serial_builder_rejects_zero_baud_rate() {
    let iface = InterfaceConfig {
        kind: "serial".to_string(),
        enabled: Some(true),
        device: Some("/dev/ttyUSB0".to_string()),
        baud_rate: Some(0),
        ..InterfaceConfig::default()
    };
    let err = serial::build_adapter(&iface).err().expect("zero baud rate should fail");
    assert!(err.contains("serial.baud_rate must be > 0"));
}

#[test]
fn lora_startup_persists_state_file() {
    let temp = TempDir::new().expect("temp dir");
    let state_path = temp.path().join("lora-state.json");

    let iface = InterfaceConfig {
        kind: "lora".to_string(),
        enabled: Some(true),
        name: Some("lora-main".to_string()),
        region: Some("US915".to_string()),
        state_path: Some(state_path.to_string_lossy().to_string()),
        ..InterfaceConfig::default()
    };

    lora::startup(&iface).expect("lora startup");
    let state = fs::read_to_string(&state_path).expect("state file exists");
    assert!(state.contains("\"version\": 1"));
}

#[test]
fn startup_status_metadata_is_embedded_in_interface_settings() {
    let mut record = InterfaceRecord {
        kind: "serial".to_string(),
        enabled: true,
        host: None,
        port: None,
        name: Some("serial-main".to_string()),
        settings: Some(json!({
            "device": "/dev/ttyUSB0",
            "baud_rate": 115200
        })),
    };

    mark_interface_startup_status(
        &mut record,
        "failed",
        Some("permission denied"),
        Some("deadbeef"),
    );

    let settings = record.settings.expect("settings should be present");
    let runtime = settings
        .get("_runtime")
        .and_then(|value| value.as_object())
        .expect("runtime metadata should be present");
    assert_eq!(runtime.get("startup_status").and_then(|value| value.as_str()), Some("failed"));
    assert_eq!(
        runtime.get("startup_error").and_then(|value| value.as_str()),
        Some("permission denied")
    );
    assert_eq!(runtime.get("iface").and_then(|value| value.as_str()), Some("deadbeef"));
}

#[test]
fn runtime_status_metadata_is_embedded_in_interface_settings() {
    let mut record = InterfaceRecord {
        kind: "ble_gatt".to_string(),
        enabled: true,
        host: None,
        port: None,
        name: Some("ble-main".to_string()),
        settings: Some(json!({
            "peripheral_id": "AA:BB:CC:DD:EE:FF"
        })),
    };

    mark_interface_startup_status(&mut record, "spawned", None, Some("beefcafe"));
    mark_interface_runtime_fields(&mut record, "running", 0);

    let settings = record.settings.expect("settings should be present");
    let runtime = settings
        .get("_runtime")
        .and_then(|value| value.as_object())
        .expect("runtime metadata should be present");
    assert_eq!(runtime.get("startup_status").and_then(|value| value.as_str()), Some("spawned"));
    assert_eq!(runtime.get("runtime_status").and_then(|value| value.as_str()), Some("running"));
    assert_eq!(runtime.get("reconnect_attempts").and_then(|value| value.as_u64()), Some(0));
    assert_eq!(runtime.get("iface").and_then(|value| value.as_str()), Some("beefcafe"));
}

#[test]
fn best_effort_startup_policy_allows_partial_failures() {
    let failures = vec![InterfaceStartupFailure {
        label: "lora-main".to_string(),
        kind: "lora".to_string(),
        error: "state marked uncertain".to_string(),
    }];
    enforce_startup_policy(false, &failures).expect("best-effort policy should not fail");
}

#[test]
fn strict_startup_policy_rejects_interface_failures() {
    let failures = vec![InterfaceStartupFailure {
        label: "lora-main".to_string(),
        kind: "lora".to_string(),
        error: "state marked uncertain".to_string(),
    }];
    let err = enforce_startup_policy(true, &failures).expect_err("strict policy should fail");
    assert!(err.contains("strict interface startup policy rejected"));
    assert!(err.contains("lora-main"));
}

#[test]
fn select_tcp_server_bind_uses_single_enabled_interface_when_transport_not_set() {
    let args = test_args(PathBuf::from("/tmp/db"), None, None, false);
    let config = reticulum_daemon::config::DaemonConfig {
        interfaces: vec![InterfaceConfig {
            kind: "tcp_server".to_string(),
            enabled: Some(true),
            host: None,
            port: Some(4242),
            ..InterfaceConfig::default()
        }],
    };

    let selected = select_tcp_server_bind(&args, Some(&config)).expect("select server");
    assert_eq!(selected.bind_addr.as_deref(), Some("0.0.0.0:4242"));
    assert_eq!(selected.selected_index, Some(0));
}

#[test]
fn select_tcp_server_bind_prefers_transport_override() {
    let args = test_args(PathBuf::from("/tmp/db"), None, Some("127.0.0.1:4333".to_string()), false);
    let config = reticulum_daemon::config::DaemonConfig {
        interfaces: vec![
            InterfaceConfig {
                kind: "tcp_server".to_string(),
                enabled: Some(true),
                host: Some("0.0.0.0".to_string()),
                port: Some(4242),
                ..InterfaceConfig::default()
            },
            InterfaceConfig {
                kind: "tcp_server".to_string(),
                enabled: Some(true),
                host: Some("127.0.0.1".to_string()),
                port: Some(4243),
                ..InterfaceConfig::default()
            },
        ],
    };

    let selected = select_tcp_server_bind(&args, Some(&config)).expect("transport override wins");
    assert_eq!(selected.bind_addr.as_deref(), Some("127.0.0.1:4333"));
    assert_eq!(selected.selected_index, None);
}

#[test]
fn select_tcp_server_bind_rejects_multiple_enabled_servers_without_override() {
    let args = test_args(PathBuf::from("/tmp/db"), None, None, false);
    let config = reticulum_daemon::config::DaemonConfig {
        interfaces: vec![
            InterfaceConfig {
                kind: "tcp_server".to_string(),
                enabled: Some(true),
                host: Some("0.0.0.0".to_string()),
                port: Some(4242),
                ..InterfaceConfig::default()
            },
            InterfaceConfig {
                kind: "tcp_server".to_string(),
                enabled: Some(true),
                host: Some("127.0.0.1".to_string()),
                port: Some(4243),
                ..InterfaceConfig::default()
            },
        ],
    };

    let err = select_tcp_server_bind(&args, Some(&config)).expect_err("multiple servers must fail");
    assert!(err.contains("multiple enabled tcp_server interfaces"));
}

#[test]
fn bootstrap_best_effort_starts_configured_interfaces_without_transport_flag() {
    let temp = TempDir::new().expect("temp dir");
    let db_path = temp.path().join("reticulum.db");
    let config_path = temp.path().join("daemon.toml");
    fs::write(
        &config_path,
        r#"
interfaces = [
  { type = "serial", enabled = true, name = "serial-main", device = "/dev/ttyUSB0", baud_rate = 115200 }
]
"#,
    )
    .expect("write config");

    let runtime =
        tokio::runtime::Builder::new_current_thread().enable_all().build().expect("runtime");
    let context = runtime.block_on(async {
        bootstrap::bootstrap(test_args(db_path.clone(), Some(config_path.clone()), None, false))
            .await
    });
    let response = context
        .daemon
        .handle_rpc(RpcRequest { id: 1, method: "list_interfaces".to_string(), params: None })
        .expect("list_interfaces");
    let interfaces = response
        .result
        .expect("result")
        .get("interfaces")
        .and_then(|value| value.as_array())
        .cloned()
        .expect("interfaces array");
    assert_eq!(interfaces.len(), 1);
    assert_eq!(
        interfaces[0]
            .get("settings")
            .and_then(|value| value.get("_runtime"))
            .and_then(|value| value.get("startup_status"))
            .and_then(|value| value.as_str()),
        Some("spawned")
    );
}

#[test]
fn bootstrap_starts_tcp_server_from_config_without_transport_flag() {
    let temp = TempDir::new().expect("temp dir");
    let db_path = temp.path().join("reticulum.db");
    let config_path = temp.path().join("daemon.toml");
    fs::write(
        &config_path,
        r#"
interfaces = [
  { type = "tcp_server", enabled = true, name = "server-main", host = "127.0.0.1", port = 0 }
]
"#,
    )
    .expect("write config");

    let runtime =
        tokio::runtime::Builder::new_current_thread().enable_all().build().expect("runtime");
    let context = runtime.block_on(async {
        bootstrap::bootstrap(test_args(db_path.clone(), Some(config_path.clone()), None, false))
            .await
    });
    let response = context
        .daemon
        .handle_rpc(RpcRequest { id: 1, method: "list_interfaces".to_string(), params: None })
        .expect("list_interfaces");
    let interfaces = response
        .result
        .expect("result")
        .get("interfaces")
        .and_then(|value| value.as_array())
        .cloned()
        .expect("interfaces array");

    let tcp_server = interfaces
        .iter()
        .find(|entry| entry.get("type").and_then(|value| value.as_str()) == Some("tcp_server"))
        .expect("tcp_server entry");
    assert_eq!(
        tcp_server
            .get("settings")
            .and_then(|value| value.get("_runtime"))
            .and_then(|value| value.get("startup_status"))
            .and_then(|value| value.as_str()),
        Some("active")
    );
}

#[test]
fn bootstrap_transport_override_shadows_configured_tcp_servers_without_failure() {
    let temp = TempDir::new().expect("temp dir");
    let db_path = temp.path().join("reticulum.db");
    let config_path = temp.path().join("daemon.toml");
    fs::write(
        &config_path,
        r#"
interfaces = [
  { type = "tcp_server", enabled = true, name = "server-a", host = "127.0.0.1", port = 4242 },
  { type = "tcp_server", enabled = true, name = "server-b", host = "127.0.0.1", port = 4243 }
]
"#,
    )
    .expect("write config");

    let runtime =
        tokio::runtime::Builder::new_current_thread().enable_all().build().expect("runtime");
    let context = runtime.block_on(async {
        bootstrap::bootstrap(test_args(
            db_path.clone(),
            Some(config_path.clone()),
            Some("127.0.0.1:0".to_string()),
            true,
        ))
        .await
    });
    let response = context
        .daemon
        .handle_rpc(RpcRequest { id: 1, method: "list_interfaces".to_string(), params: None })
        .expect("list_interfaces");
    let interfaces = response
        .result
        .expect("result")
        .get("interfaces")
        .and_then(|value| value.as_array())
        .cloned()
        .expect("interfaces array");

    let shadowed = interfaces
        .iter()
        .filter(|entry| {
            entry
                .get("settings")
                .and_then(|value| value.get("_runtime"))
                .and_then(|value| value.get("startup_status"))
                .and_then(|value| value.as_str())
                == Some("shadowed_by_transport_override")
        })
        .count();
    assert!(shadowed >= 2);
}

#[test]
fn bootstrap_transport_override_shadows_missing_port_tcp_server_without_strict_failure() {
    let temp = TempDir::new().expect("temp dir");
    let db_path = temp.path().join("reticulum.db");
    let config_path = temp.path().join("daemon.toml");
    fs::write(
        &config_path,
        r#"
interfaces = [
  { type = "tcp_server", enabled = true, name = "server-a", host = "127.0.0.1", port = 4242 },
  { type = "tcp_server", enabled = true, name = "server-b" }
]
"#,
    )
    .expect("write config");

    let runtime =
        tokio::runtime::Builder::new_current_thread().enable_all().build().expect("runtime");
    let context = runtime.block_on(async {
        bootstrap::bootstrap(test_args(
            db_path.clone(),
            Some(config_path.clone()),
            Some("127.0.0.1:0".to_string()),
            true,
        ))
        .await
    });
    let response = context
        .daemon
        .handle_rpc(RpcRequest { id: 1, method: "list_interfaces".to_string(), params: None })
        .expect("list_interfaces");
    let interfaces = response
        .result
        .expect("result")
        .get("interfaces")
        .and_then(|value| value.as_array())
        .cloned()
        .expect("interfaces array");

    let shadowed_missing_port = interfaces.iter().any(|entry| {
        entry.get("name").and_then(|value| value.as_str()) == Some("server-b")
            && entry
                .get("settings")
                .and_then(|value| value.get("_runtime"))
                .and_then(|value| value.get("startup_status"))
                .and_then(|value| value.as_str())
                == Some("shadowed_by_transport_override")
    });

    assert!(
        shadowed_missing_port,
        "shadowed tcp_server without a port should remain non-fatal under transport override"
    );
}

#[test]
fn reticulum_parity_matrix_mentions_config_driven_lxmd_tcp_server_startup() {
    let parity_matrix_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../docs/plans/reticulum-parity-matrix.md");
    let text = fs::read_to_string(&parity_matrix_path).expect("read reticulum parity matrix");

    assert!(
        text.contains("Python-style interface-driven `tcp_server` startup now works from config")
            && text.contains("without Rust-only transport overrides"),
        "reticulum parity matrix should document config-driven lxmd tcp_server startup parity"
    );
}

#[test]
fn bootstrap_starts_udp_interface_from_config() {
    let temp = TempDir::new().expect("temp dir");
    let db_path = temp.path().join("reticulum.db");
    let config_path = temp.path().join("daemon.toml");
    fs::write(
        &config_path,
        r#"
interfaces = [
  { type = "udp", enabled = true, name = "udp-main", host = "127.0.0.1", port = 0, target_host = "127.0.0.1", target_port = 4242 }
]
"#,
    )
    .expect("write config");

    let runtime =
        tokio::runtime::Builder::new_current_thread().enable_all().build().expect("runtime");
    let context = runtime.block_on(async {
        bootstrap::bootstrap(test_args(
            db_path.clone(),
            Some(config_path.clone()),
            Some("127.0.0.1:0".to_string()),
            false,
        ))
        .await
    });
    let response = context
        .daemon
        .handle_rpc(RpcRequest { id: 1, method: "list_interfaces".to_string(), params: None })
        .expect("list_interfaces");
    let interfaces = response
        .result
        .expect("result")
        .get("interfaces")
        .and_then(|value| value.as_array())
        .cloned()
        .expect("interfaces array");

    let udp = interfaces
        .iter()
        .find(|entry| entry.get("type").and_then(|value| value.as_str()) == Some("udp"))
        .expect("udp entry");
    assert_eq!(udp.get("host").and_then(|value| value.as_str()), Some("127.0.0.1"));
    assert_eq!(udp.get("port").and_then(|value| value.as_u64()), Some(0));
    assert_eq!(
        udp.get("settings")
            .and_then(|value| value.get("target_host"))
            .and_then(|value| value.as_str()),
        Some("127.0.0.1")
    );
    assert_eq!(
        udp.get("settings")
            .and_then(|value| value.get("target_port"))
            .and_then(|value| value.as_u64()),
        Some(4242)
    );
    assert_eq!(
        udp.get("settings")
            .and_then(|value| value.get("_runtime"))
            .and_then(|value| value.get("startup_status"))
            .and_then(|value| value.as_str()),
        Some("spawned")
    );
}

#[test]
fn bootstrap_strict_mode_rejects_unbindable_udp_interface() {
    let runtime =
        tokio::runtime::Builder::new_current_thread().enable_all().build().expect("runtime");
    runtime.block_on(async {
        let occupied = tokio::net::UdpSocket::bind("127.0.0.1:0").await.expect("bind port");
        let occupied_addr = occupied.local_addr().expect("local addr");

        let temp = TempDir::new().expect("temp dir");
        let db_path = temp.path().join("reticulum.db");
        let config_path = temp.path().join("daemon.toml");
        fs::write(
            &config_path,
            format!(
                "interfaces = [\n  {{ type = \"udp\", enabled = true, name = \"udp-main\", host = \"127.0.0.1\", port = {}, target_host = \"127.0.0.1\", target_port = 4242 }}\n]\n",
                occupied_addr.port()
            ),
        )
        .expect("write config");

        let result = std::panic::AssertUnwindSafe(bootstrap::bootstrap(test_args(
            db_path.clone(),
            Some(config_path.clone()),
            None,
            true,
        )))
        .catch_unwind()
        .await;

        let panic_payload = match result {
            Ok(_) => panic!("strict startup should panic on occupied udp port"),
            Err(panic_payload) => panic_payload,
        };
        let panic_message = if let Some(message) = panic_payload.downcast_ref::<String>() {
            message.clone()
        } else if let Some(message) = panic_payload.downcast_ref::<&str>() {
            (*message).to_string()
        } else {
            String::new()
        };
        assert!(panic_message.contains("strict interface startup policy rejected"));
        assert!(panic_message.contains("udp-main"));
    });
}

#[test]
fn bootstrap_strict_mode_panics_when_transport_is_disabled_for_enabled_interfaces() {
    let temp = TempDir::new().expect("temp dir");
    let db_path = temp.path().join("reticulum.db");
    let config_path = temp.path().join("daemon.toml");
    fs::write(
        &config_path,
        r#"
interfaces = [
  { type = "serial", enabled = true, name = "serial-main", device = "/dev/ttyUSB0", baud_rate = 115200 }
]
"#,
    )
    .expect("write config");

    let runtime =
        tokio::runtime::Builder::new_current_thread().enable_all().build().expect("runtime");
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        runtime.block_on(async {
            bootstrap::bootstrap(test_args(db_path.clone(), Some(config_path.clone()), None, true))
                .await;
        });
    }));
    assert!(result.is_err(), "strict mode should panic on startup failures");
}

#[test]
fn bootstrap_strict_mode_panics_on_serial_preflight_open_failure() {
    let temp = TempDir::new().expect("temp dir");
    let db_path = temp.path().join("reticulum.db");
    let config_path = temp.path().join("daemon.toml");
    fs::write(
        &config_path,
        r#"
interfaces = [
  { type = "serial", enabled = true, name = "serial-main", device = "__definitely_not_a_device__", baud_rate = 115200 }
]
"#,
    )
    .expect("write config");

    let runtime =
        tokio::runtime::Builder::new_current_thread().enable_all().build().expect("runtime");
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        runtime.block_on(async {
            bootstrap::bootstrap(test_args(
                db_path.clone(),
                Some(config_path.clone()),
                Some("127.0.0.1:0".to_string()),
                true,
            ))
            .await;
        });
    }));
    assert!(result.is_err(), "strict mode should panic when serial preflight open fails");
}

#[test]
fn bootstrap_strict_mode_panics_on_tcp_client_preflight_connect_failure() {
    let temp = TempDir::new().expect("temp dir");
    let db_path = temp.path().join("reticulum.db");
    let config_path = temp.path().join("daemon.toml");
    fs::write(
        &config_path,
        r#"
interfaces = [
  { type = "tcp_client", enabled = true, name = "tcp-main", host = "203.0.113.1", port = 65535 }
]
"#,
    )
    .expect("write config");

    let runtime =
        tokio::runtime::Builder::new_current_thread().enable_all().build().expect("runtime");
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        runtime.block_on(async {
            bootstrap::bootstrap(test_args(
                db_path.clone(),
                Some(config_path.clone()),
                Some("127.0.0.1:0".to_string()),
                true,
            ))
            .await;
        });
    }));
    assert!(result.is_err(), "strict mode should panic when tcp_client preflight connect fails");
}

#[test]
fn bootstrap_best_effort_marks_ble_validation_failure_as_failed() {
    let temp = TempDir::new().expect("temp dir");
    let db_path = temp.path().join("reticulum.db");
    let config_path = temp.path().join("daemon.toml");
    fs::write(
        &config_path,
        r#"
interfaces = [
  { type = "ble_gatt", enabled = true, name = "ble-main", adapter = "disabled", peripheral_id = "AA:BB:CC:DD:EE:FF", service_uuid = "12345678-1234-1234-1234-1234567890ab", write_char_uuid = "2A37", notify_char_uuid = "2A38" }
]
"#,
    )
    .expect("write config");

    let runtime =
        tokio::runtime::Builder::new_current_thread().enable_all().build().expect("runtime");
    let local = tokio::task::LocalSet::new();
    let context = runtime.block_on(local.run_until(async {
        bootstrap::bootstrap(test_args(
            db_path.clone(),
            Some(config_path.clone()),
            Some("127.0.0.1:0".to_string()),
            false,
        ))
        .await
    }));
    let response = context
        .daemon
        .handle_rpc(RpcRequest { id: 1, method: "list_interfaces".to_string(), params: None })
        .expect("list_interfaces");
    let interfaces = response
        .result
        .expect("result")
        .get("interfaces")
        .and_then(|value| value.as_array())
        .cloned()
        .expect("interfaces array");
    let ble_interface = interfaces
        .iter()
        .find(|entry| {
            entry
                .get("settings")
                .and_then(|value| value.get("_runtime"))
                .and_then(|value| value.get("startup_status"))
                .and_then(|value| value.as_str())
                == Some("failed")
        })
        .expect("failed interface should be present in snapshot");
    assert_eq!(
        ble_interface
            .get("settings")
            .and_then(|value| value.get("_runtime"))
            .and_then(|value| value.get("startup_status"))
            .and_then(|value| value.as_str()),
        Some("failed")
    );
    assert!(
        ble_interface
            .get("settings")
            .and_then(|value| value.get("_runtime"))
            .and_then(|value| value.get("startup_error"))
            .and_then(|value| value.as_str())
            .is_some(),
        "startup error should be populated for failed BLE startup"
    );
}

#[test]
fn bootstrap_strict_mode_panics_on_ble_validation_failure() {
    let temp = TempDir::new().expect("temp dir");
    let db_path = temp.path().join("reticulum.db");
    let config_path = temp.path().join("daemon.toml");
    fs::write(
        &config_path,
        r#"
interfaces = [
  { type = "ble_gatt", enabled = true, name = "ble-main", adapter = "disabled", peripheral_id = "AA:BB:CC:DD:EE:FF", service_uuid = "12345678-1234-1234-1234-1234567890ab", write_char_uuid = "2A37", notify_char_uuid = "2A38" }
]
"#,
    )
    .expect("write config");

    let runtime =
        tokio::runtime::Builder::new_current_thread().enable_all().build().expect("runtime");
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        runtime.block_on(async {
            bootstrap::bootstrap(test_args(
                db_path.clone(),
                Some(config_path.clone()),
                Some("127.0.0.1:0".to_string()),
                true,
            ))
            .await;
        });
    }));
    assert!(result.is_err(), "strict mode should panic when BLE startup validation fails");
}

#[test]
fn bootstrap_best_effort_marks_lora_stale_state_as_failed() {
    let temp = TempDir::new().expect("temp dir");
    let db_path = temp.path().join("reticulum.db");
    let config_path = temp.path().join("daemon.toml");
    let state_path = temp.path().join("lora-state.json");
    let stale_last_updated_unix_ms =
        now_unix_ms_for_test().saturating_sub(31 * 24 * 60 * 60 * 1000);
    fs::write(
        &state_path,
        serde_json::to_vec_pretty(&json!({
            "version": 1,
            "duty_cycle_debt_ms": 5000,
            "last_updated_unix_ms": stale_last_updated_unix_ms,
            "uncertain": false,
            "uncertainty_reason": null
        }))
        .expect("serialize lora state"),
    )
    .expect("write lora state");
    fs::write(
        &config_path,
        format!(
            r#"
interfaces = [
  {{ type = "lora", enabled = true, name = "lora-main", region = "US915", state_path = "{}" }}
]
"#,
            state_path.display()
        ),
    )
    .expect("write config");

    let runtime =
        tokio::runtime::Builder::new_current_thread().enable_all().build().expect("runtime");
    let local = tokio::task::LocalSet::new();
    let context = runtime.block_on(local.run_until(async {
        bootstrap::bootstrap(test_args(
            db_path.clone(),
            Some(config_path.clone()),
            Some("127.0.0.1:0".to_string()),
            false,
        ))
        .await
    }));
    let response = context
        .daemon
        .handle_rpc(RpcRequest { id: 1, method: "list_interfaces".to_string(), params: None })
        .expect("list_interfaces");
    let interfaces = response
        .result
        .expect("result")
        .get("interfaces")
        .and_then(|value| value.as_array())
        .cloned()
        .expect("interfaces array");
    let lora_interface = interfaces
        .iter()
        .find(|entry| {
            entry
                .get("settings")
                .and_then(|value| value.get("_runtime"))
                .and_then(|value| value.get("startup_status"))
                .and_then(|value| value.as_str())
                == Some("failed")
                && entry
                    .get("settings")
                    .and_then(|value| value.get("_runtime"))
                    .and_then(|value| value.get("startup_error"))
                    .and_then(|value| value.as_str())
                    .is_some_and(|error| error.contains("timestamp too old"))
        })
        .expect("lora interface should be present in snapshot");
    assert_eq!(
        lora_interface
            .get("settings")
            .and_then(|value| value.get("_runtime"))
            .and_then(|value| value.get("startup_status"))
            .and_then(|value| value.as_str()),
        Some("failed")
    );
    assert!(
        lora_interface
            .get("settings")
            .and_then(|value| value.get("_runtime"))
            .and_then(|value| value.get("startup_error"))
            .and_then(|value| value.as_str())
            .is_some_and(|error| error.contains("timestamp too old")),
        "startup_error should include stale timestamp fail-closed reason"
    );
}

#[test]
fn bootstrap_strict_mode_panics_on_lora_debt_overflow_fail_closed() {
    let temp = TempDir::new().expect("temp dir");
    let db_path = temp.path().join("reticulum.db");
    let config_path = temp.path().join("daemon.toml");
    let state_path = temp.path().join("lora-state.json");
    fs::write(
        &state_path,
        serde_json::to_vec_pretty(&json!({
            "version": 1,
            "duty_cycle_debt_ms": 86_400_001,
            "last_updated_unix_ms": now_unix_ms_for_test(),
            "uncertain": false,
            "uncertainty_reason": null
        }))
        .expect("serialize lora state"),
    )
    .expect("write lora state");
    fs::write(
        &config_path,
        format!(
            r#"
interfaces = [
  {{ type = "lora", enabled = true, name = "lora-main", region = "US915", state_path = "{}" }}
]
"#,
            state_path.display()
        ),
    )
    .expect("write config");

    let runtime =
        tokio::runtime::Builder::new_current_thread().enable_all().build().expect("runtime");
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        runtime.block_on(async {
            bootstrap::bootstrap(test_args(
                db_path.clone(),
                Some(config_path.clone()),
                Some("127.0.0.1:0".to_string()),
                true,
            ))
            .await;
        });
    }));
    assert!(
        result.is_err(),
        "strict mode should panic when lora state debt exceeds compliance bounds"
    );
}

fn now_unix_ms_for_test() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

fn test_args(
    db: PathBuf,
    config: Option<PathBuf>,
    transport: Option<String>,
    strict_interface_startup: bool,
) -> Args {
    Args {
        rpc: "127.0.0.1:0".to_string(),
        db,
        config,
        identity: None,
        announce_interval_secs: 0,
        transport,
        strict_interface_startup,
        rpc_tls_cert: None,
        rpc_tls_key: None,
        rpc_tls_client_ca: None,
        rpc_unix: None,
    }
}
