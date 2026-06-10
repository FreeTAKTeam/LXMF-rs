use crate::bootstrap::{
    configure_startup_rpc_token_auth, enforce_rpc_bind_security, enforce_startup_policy,
    mark_interface_runtime_fields, mark_interface_startup_status, select_tcp_server_bind,
    InterfaceStartupFailure, RpcTlsConfig,
};
use crate::bridge::{
    validate_delivery_request, wait_for_propagation_signal, PeerCrypto, RequestedDeliveryMethod,
    TransportBridge,
};
use crate::bridge_helpers::opportunistic_payload;
use crate::interfaces::{kiss, lora, serial, vrn76_kiss_ble};
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
use rns_transport::hash::AddressHash;
use rns_transport::iface::lora::{
    CMD_DETECT, CMD_FREQUENCY, CMD_LEAVE, CMD_MCU, CMD_RADIO_STATE, DETECT_REQ, RADIO_STATE_OFF,
};
use rns_transport::iface::tcp_client::TcpClient;
use rns_transport::iface::vrn76_kiss_ble::Vrn76FrameMode;
use rns_transport::packet::{PacketContext, PacketDataBuffer};
use rns_transport::transport::{ReceivedData, ReceivedPayloadMode, Transport, TransportConfig};
use serde_json::json;
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tempfile::TempDir;

#[test]
fn cli_defaults_to_local_unix_rpc_without_tcp_bind() {
    let args = <Args as clap::Parser>::parse_from(["reticulumd"]);
    assert_eq!(args.rpc, None);
    assert_eq!(args.rpc_unix, Some(PathBuf::from(crate::DEFAULT_RPC_UNIX_PATH)));
}

#[test]
fn rpc_bind_security_allows_loopback_tcp_without_remote_auth() {
    let daemon = RpcDaemon::test_instance();
    let addr = "127.0.0.1:4242".parse().expect("loopback addr");

    enforce_rpc_bind_security(Some(&addr), None, &daemon);
}

#[test]
#[should_panic(expected = "remote TCP RPC bind")]
fn rpc_bind_security_rejects_unspecified_tcp_without_remote_auth() {
    let daemon = RpcDaemon::test_instance();
    let addr = "0.0.0.0:4242".parse().expect("remote addr");

    enforce_rpc_bind_security(Some(&addr), None, &daemon);
}

#[test]
fn rpc_bind_security_allows_remote_tcp_with_mtls_client_ca() {
    let daemon = RpcDaemon::test_instance();
    let addr = "0.0.0.0:4242".parse().expect("remote addr");
    let tls = RpcTlsConfig {
        cert_chain_path: PathBuf::from("server.pem"),
        private_key_path: PathBuf::from("server.key"),
        client_ca_path: Some(PathBuf::from("client-ca.pem")),
    };

    enforce_rpc_bind_security(Some(&addr), Some(&tls), &daemon);
}

#[test]
fn rpc_bind_security_allows_remote_tcp_with_persisted_token_auth() {
    let daemon = RpcDaemon::test_instance();
    let response = daemon
        .handle_rpc(RpcRequest {
            id: 1,
            method: "sdk_negotiate_v2".to_string(),
            params: Some(json!({
                "supported_contract_versions": [2],
                "requested_capabilities": [],
                "config": {
                    "profile": "desktop-full",
                    "bind_mode": "remote",
                    "auth_mode": "token",
                    "rpc_backend": {
                        "token_auth": {
                            "issuer": "test-issuer",
                            "audience": "test-audience",
                            "jti_cache_ttl_ms": 30000,
                            "clock_skew_ms": 0,
                            "shared_secret": "test-secret"
                        }
                    }
                }
            })),
        })
        .expect("negotiate token auth");
    assert!(response.error.is_none());
    let addr = "0.0.0.0:4242".parse().expect("remote addr");

    enforce_rpc_bind_security(Some(&addr), None, &daemon);
}

#[test]
fn startup_token_auth_configures_remote_rpc_before_bind_guard() {
    let daemon = RpcDaemon::test_instance();
    let secret_env = format!("LXMF_TEST_RPC_SECRET_{}", now_unix_ms_for_test());
    std::env::set_var(&secret_env, "test-secret");
    let mut args = test_args(PathBuf::from(":memory:"), None, None, false);
    args.rpc = Some("0.0.0.0:4242".to_string());
    args.rpc_token_issuer = Some("test-issuer".to_string());
    args.rpc_token_audience = Some("test-audience".to_string());
    args.rpc_token_secret_env = Some(secret_env.clone());
    let addr = "0.0.0.0:4242".parse().expect("remote addr");

    configure_startup_rpc_token_auth(&args, &daemon);
    enforce_rpc_bind_security(Some(&addr), None, &daemon);

    std::env::remove_var(secret_env);
}

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

#[tokio::test]
async fn propagation_signal_waiter_detects_invalid_stamp_rejection() {
    let (tx, mut rx) = tokio::sync::broadcast::channel(4);
    let link_id = AddressHash::new_from_slice(&[0x11; 16]);
    let other_link_id = AddressHash::new_from_slice(&[0x22; 16]);
    let signal_payload = rmp_serde::to_vec(&vec![0xf5u8]).expect("signal msgpack");

    assert!(tx
        .send(ReceivedData {
            destination: other_link_id,
            data: PacketDataBuffer::new_from_slice(&signal_payload),
            payload_mode: ReceivedPayloadMode::FullWire,
            ratchet_used: false,
            context: Some(PacketContext::None),
            request_id: None,
            hops: None,
            interface: None,
        })
        .is_ok());
    assert!(tx
        .send(ReceivedData {
            destination: link_id,
            data: PacketDataBuffer::new_from_slice(&signal_payload),
            payload_mode: ReceivedPayloadMode::FullWire,
            ratchet_used: false,
            context: Some(PacketContext::None),
            request_id: None,
            hops: None,
            interface: None,
        })
        .is_ok());

    assert_eq!(
        wait_for_propagation_signal(&mut rx, link_id, Duration::from_millis(200)).await,
        Some(0xf5)
    );
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
    let (receipt_tx, _receipt_rx) = tokio::sync::mpsc::channel(16);

    let bridge = Arc::new(TransportBridge::new(
        transport,
        signer,
        [0u8; 16],
        announce_destination,
        None,
        Vec::new(),
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
                "propagation_limit": 333,
                "sync_limit": 999,
            })),
        })
        .expect("enable propagation");

    let app_data =
        bridge.current_propagation_announce_app_data_for_test().expect("propagation app data");
    let decoded = rmp_serde::from_slice::<rmpv::Value>(app_data.as_slice())
        .expect("decode propagation app data");
    let entries = decoded.as_array().expect("propagation app data array");

    assert_eq!(entries.get(3).and_then(rmpv::Value::as_u64), Some(333));
    assert_eq!(entries.get(4).and_then(rmpv::Value::as_u64), Some(999));
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
fn tcp_client_adapter_exposes_default_mtu() {
    let adapter = TcpClient::new("rmap.world:4242");
    assert_eq!(adapter.mtu_value(), TcpClient::DEFAULT_MTU);
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
fn serial_builder_accepts_python_serial_line_alias_values() {
    let iface = InterfaceConfig {
        kind: "serial".to_string(),
        enabled: Some(true),
        device: Some("/dev/ttyUSB0".to_string()),
        baud_rate: Some(19_200),
        data_bits: Some(7),
        parity: Some("N".to_string()),
        stop_bits: Some(2),
        ..InterfaceConfig::default()
    };

    let adapter = serial::build_adapter(&iface).expect("build serial adapter");
    assert_eq!(adapter.device(), "/dev/ttyUSB0");
    assert_eq!(adapter.baud_rate(), 19_200);
    assert_eq!(adapter.data_bits_value(), 7);
    assert_eq!(adapter.parity_name(), "none");
    assert_eq!(adapter.stop_bits_value(), 2);
}

#[test]
fn kiss_builder_rejects_missing_required_fields() {
    let iface = InterfaceConfig {
        kind: "kiss".to_string(),
        enabled: Some(true),
        ..InterfaceConfig::default()
    };
    let result = kiss::build_adapter(&iface);
    assert!(result.is_err(), "missing device/baud should fail");
    let err = result.err().unwrap_or_default();
    assert!(err.contains("kiss.device"));
}

#[test]
fn kiss_builder_uses_serial_line_settings() {
    let iface = InterfaceConfig {
        kind: "kiss".to_string(),
        enabled: Some(true),
        device: Some("/dev/ttyACM0".to_string()),
        baud_rate: Some(19_200),
        data_bits: Some(7),
        parity: Some("E".to_string()),
        stop_bits: Some(2),
        ..InterfaceConfig::default()
    };

    let adapter = kiss::build_adapter(&iface).expect("build kiss adapter");
    assert_eq!(adapter.device(), "/dev/ttyACM0");
    assert_eq!(adapter.baud_rate(), 19_200);
    assert_eq!(adapter.data_bits_value(), 7);
    assert_eq!(adapter.parity_name(), "even");
    assert_eq!(adapter.stop_bits_value(), 2);
}

#[test]
fn kiss_tcp_client_builder_rejects_missing_required_fields() {
    let iface = InterfaceConfig {
        kind: "kiss_tcp_client".to_string(),
        enabled: Some(true),
        ..InterfaceConfig::default()
    };
    let result = kiss::build_tcp_client_adapter(&iface);
    assert!(result.is_err(), "missing host/port should fail");
    let err = result.err().unwrap_or_default();
    assert!(err.contains("kiss_tcp_client.host"));
}

#[test]
fn kiss_tcp_client_builder_uses_endpoint_and_kiss_overrides() {
    let iface = InterfaceConfig {
        kind: "kiss_tcp_client".to_string(),
        enabled: Some(true),
        host: Some("192.0.2.10".to_string()),
        port: Some(8001),
        mtu: Some(512),
        preamble_ms: Some(410),
        tx_tail_ms: Some(30),
        persistence: Some(80),
        slot_time_ms: Some(40),
        kiss_flow_control: Some(true),
        id_callsign: Some("MYCALL-0".to_string()),
        id_interval: Some(600),
        reconnect_backoff_ms: Some(100),
        max_reconnect_backoff_ms: Some(200),
        ..InterfaceConfig::default()
    };

    let adapter = kiss::build_tcp_client_adapter(&iface).expect("build kiss tcp client adapter");
    assert_eq!(adapter.addr(), "192.0.2.10:8001");
    assert_eq!(adapter.mtu(), 512);
    assert_eq!(adapter.reconnect_backoff(), Duration::from_millis(100));
    assert_eq!(adapter.max_reconnect_backoff(), Duration::from_millis(200));
    assert_eq!(
        adapter.kiss_config(),
        rns_transport::iface::kiss::KissConfig {
            preamble_ms: 410,
            tx_tail_ms: 30,
            persistence: 80,
            slot_time_ms: 40,
            flow_control: true,
            id_beacon: Some(rns_transport::iface::kiss::KissIdBeaconConfig {
                callsign: b"MYCALL-0".to_vec(),
                interval: Duration::from_secs(600),
                min_payload_len: 15,
            }),
        }
    );
}

#[test]
fn kiss_tcp_client_builder_preserves_python_empty_id_beacon_when_callsign_missing() {
    let iface = InterfaceConfig {
        kind: "kiss_tcp_client".to_string(),
        enabled: Some(true),
        host: Some("192.0.2.10".to_string()),
        port: Some(8001),
        id_interval: Some(600),
        ..InterfaceConfig::default()
    };

    let adapter = kiss::build_tcp_client_adapter(&iface).expect("build kiss tcp client adapter");

    assert_eq!(
        adapter.kiss_config().id_beacon,
        Some(rns_transport::iface::kiss::KissIdBeaconConfig {
            callsign: Vec::new(),
            interval: Duration::from_secs(600),
            min_payload_len: 15,
        })
    );
}

#[test]
fn kiss_tcp_client_builder_supports_tcp_client_kiss_framing_alias_output() {
    let iface = InterfaceConfig {
        kind: "kiss_tcp_client".to_string(),
        enabled: Some(true),
        host: Some("192.0.2.10".to_string()),
        port: Some(8001),
        mtu: Some(512),
        ..InterfaceConfig::default()
    };

    let adapter = kiss::build_tcp_client_adapter(&iface).expect("build kiss tcp client adapter");
    assert_eq!(adapter.addr(), "192.0.2.10:8001");
    assert_eq!(adapter.mtu(), 512);
}

#[test]
fn lora_builder_uses_region_defaults_and_config_overrides() {
    let iface = InterfaceConfig {
        kind: "lora".to_string(),
        enabled: Some(true),
        region: Some("US915".to_string()),
        device: Some("/dev/ttyACM1".to_string()),
        baud_rate: Some(115200),
        bandwidth_hz: Some(250_000),
        spreading_factor: Some(8),
        coding_rate: Some("4/6".to_string()),
        tx_power_dbm: Some(14),
        airtime_limit_short: Some(33.0),
        airtime_limit_long: Some(1.5),
        max_payload_bytes: Some(180),
        flow_control: Some(toml::Value::Boolean(true)),
        ..InterfaceConfig::default()
    };

    let adapter = lora::build_adapter(&iface).expect("build lora adapter");
    assert_eq!(adapter.config().frequency_hz, 915_000_000);
    assert_eq!(adapter.config().bandwidth_hz, 250_000);
    assert_eq!(adapter.config().spreading_factor, 8);
    assert_eq!(adapter.config().coding_rate, 6);
    assert_eq!(adapter.config().tx_power_dbm, 14);
    assert_eq!(adapter.config().airtime_limit_short_hundredths, Some(3_300));
    assert_eq!(adapter.config().airtime_limit_long_hundredths, Some(150));
    assert_eq!(adapter.config().max_payload_bytes, 180);
    assert!(adapter.flow_control());
}

#[test]
fn lora_builder_supports_python_rnode_tcp_port() {
    let iface = InterfaceConfig {
        kind: "lora".to_string(),
        enabled: Some(true),
        region: Some("US915".to_string()),
        state_path: Some("/tmp/lora-state.json".to_string()),
        device: Some("tcp://192.0.2.10:8001".to_string()),
        frequency_hz: Some(915_000_000),
        bandwidth_hz: Some(125_000),
        spreading_factor: Some(9),
        coding_rate: Some("5".to_string()),
        tx_power_dbm: Some(17),
        ..InterfaceConfig::default()
    };

    let adapter = lora::build_adapter(&iface).expect("build tcp rnode adapter");

    assert_eq!(adapter.bearer(), rns_transport::iface::lora::LoraBearer::Tcp);
    assert_eq!(adapter.endpoint(), "192.0.2.10:8001");
    assert_eq!(adapter.baud_rate(), None);
}

#[test]
fn lora_builder_supports_python_high_bandwidth_rnode_config() {
    let iface = InterfaceConfig {
        kind: "lora".to_string(),
        enabled: Some(true),
        region: Some("US915".to_string()),
        state_path: Some("/tmp/lora-state.json".to_string()),
        device: Some("tcp://192.0.2.10:8001".to_string()),
        frequency_hz: Some(2_400_000_000),
        bandwidth_hz: Some(1_625_000),
        spreading_factor: Some(5),
        coding_rate: Some("5".to_string()),
        tx_power_dbm: Some(17),
        ..InterfaceConfig::default()
    };

    let adapter = lora::build_adapter(&iface).expect("build high-bandwidth rnode adapter");

    assert_eq!(adapter.config().frequency_hz, 2_400_000_000);
    assert_eq!(adapter.config().bandwidth_hz, 1_625_000);
    assert_eq!(adapter.config().spreading_factor, 5);
}

#[test]
fn lora_builder_uses_python_rnode_command_timeout() {
    let iface = InterfaceConfig {
        kind: "lora".to_string(),
        enabled: Some(true),
        region: Some("US915".to_string()),
        state_path: Some("/tmp/lora-state.json".to_string()),
        device: Some("/dev/ttyACM1".to_string()),
        baud_rate: Some(115200),
        connect_timeout_ms: Some(2_750),
        ..InterfaceConfig::default()
    };

    let adapter = lora::build_adapter(&iface).expect("build lora adapter");

    assert_eq!(adapter.startup_response_timeout(), Duration::from_millis(2_750));
}

#[test]
fn rnode_ble_builder_uses_native_ble_and_kiss_defaults() {
    let iface = InterfaceConfig {
        kind: "lora".to_string(),
        enabled: Some(true),
        name: Some("rnode-ble".to_string()),
        region: Some("US915".to_string()),
        state_path: Some("/tmp/lora-state.json".to_string()),
        device: Some("ble://RNode 1234".to_string()),
        adapter: Some("Bluetooth".to_string()),
        mtu: Some(512),
        max_write_len: Some(64),
        max_payload_bytes: Some(220),
        scan_timeout_ms: Some(3_000),
        ble_connect_timeout_ms: Some(7_000),
        connect_timeout_ms: Some(4_000),
        preamble_ms: Some(410),
        tx_tail_ms: Some(30),
        persistence: Some(80),
        slot_time_ms: Some(40),
        flow_control: Some(toml::Value::Boolean(true)),
        ..InterfaceConfig::default()
    };

    let config = lora::build_rnode_ble_config(&iface).expect("build rnode BLE config");

    assert_eq!(config.peripheral_id, "RNode 1234");
    assert_eq!(config.adapter.as_deref(), Some("Bluetooth"));
    assert_eq!(config.transport.mtu, 220);
    assert_eq!(config.transport.max_write_len, 64);
    assert_eq!(config.transport.scan_timeout, Duration::from_millis(3_000));
    assert_eq!(config.transport.connect_timeout, Duration::from_millis(7_000));
    assert_eq!(config.startup_response_timeout, Duration::from_millis(4_000));
    assert_eq!(config.transport.kiss.preamble_ms, 410);
    assert_eq!(config.transport.kiss.tx_tail_ms, 30);
    assert_eq!(config.transport.kiss.persistence, 80);
    assert_eq!(config.transport.kiss.slot_time_ms, 40);
    assert!(config.transport.kiss.flow_control);
    // initial_frames carries only probe frames (Phase 1: detect handshake)
    assert_eq!(
        config.transport.initial_frames.first(),
        Some(&rns_transport::kiss::encode_command_frame(CMD_DETECT, &[DETECT_REQ]))
    );
    assert_eq!(
        config.transport.initial_frames.last(),
        Some(&rns_transport::kiss::encode_command_frame(CMD_MCU, &[0x00]))
    );
    // deferred_frames carries radio config (Phase 2: sent after detect confirmed)
    assert_eq!(
        config.transport.deferred_frames.first(),
        Some(&rns_transport::kiss::encode_command_frame(
            CMD_FREQUENCY,
            &915_000_000_u32.to_be_bytes()
        ))
    );
    assert_eq!(
        config.transport.deferred_frames.last(),
        Some(&rns_transport::kiss::encode_command_frame(CMD_RADIO_STATE, &[1]))
    );
    assert_eq!(
        config.transport.shutdown_frames,
        vec![
            rns_transport::kiss::encode_command_frame(CMD_RADIO_STATE, &[RADIO_STATE_OFF]),
            rns_transport::kiss::encode_command_frame(CMD_LEAVE, &[0xff]),
        ]
    );
}

#[test]
fn rnode_ble_builder_keeps_ble_connect_timeout_distinct_from_rnode_command_timeout() {
    let iface = InterfaceConfig {
        kind: "lora".to_string(),
        enabled: Some(true),
        name: Some("rnode-ble".to_string()),
        region: Some("US915".to_string()),
        state_path: Some("/tmp/lora-state.json".to_string()),
        device: Some("ble://RNode 1234".to_string()),
        frequency_hz: Some(915_000_000),
        bandwidth_hz: Some(125_000),
        spreading_factor: Some(9),
        coding_rate: Some("5".to_string()),
        tx_power_dbm: Some(17),
        ..InterfaceConfig::default()
    };

    let config = lora::build_rnode_ble_config(&iface).expect("build rnode BLE config");

    // BLE physical connect timeout and RNode detect timeout are separate fields,
    // configured independently via ble_connect_timeout_ms and connect_timeout_ms.
    assert_eq!(config.transport.connect_timeout, Duration::from_millis(5_000));
    assert_eq!(config.startup_response_timeout, Duration::from_millis(5_000)); // matches Python's ble_detect_timeout
}

#[test]
fn vrn76_builder_rejects_missing_peripheral_id() {
    let iface = InterfaceConfig {
        kind: "vrn76_kiss_ble".to_string(),
        enabled: Some(true),
        ..InterfaceConfig::default()
    };
    let result = vrn76_kiss_ble::build_config(&iface);
    assert!(result.is_err(), "missing peripheral_id should fail");
    let err = result.err().unwrap_or_default();
    assert!(err.contains("vrn76_kiss_ble.peripheral_id"));
}

#[test]
fn vrn76_builder_uses_profile_defaults_and_kiss_overrides() {
    let iface = InterfaceConfig {
        kind: "vrn76_kiss_ble".to_string(),
        enabled: Some(true),
        peripheral_id: Some("VR-N76".to_string()),
        adapter: Some("Bluetooth".to_string()),
        mtu: Some(512),
        max_write_len: Some(128),
        preamble_ms: Some(410),
        tx_tail_ms: Some(30),
        persistence: Some(80),
        slot_time_ms: Some(40),
        kiss_flow_control: Some(true),
        scan_timeout_ms: Some(11_000),
        connect_timeout_ms: Some(4_000),
        ..InterfaceConfig::default()
    };

    let config = vrn76_kiss_ble::build_config(&iface).expect("build vrn76 config");
    assert_eq!(config.peripheral_id, "VR-N76");
    assert_eq!(config.adapter.as_deref(), Some("Bluetooth"));
    assert_eq!(config.transport.mtu, 512);
    assert_eq!(config.transport.max_write_len, 128);
    assert_eq!(config.transport.scan_timeout, Duration::from_millis(11_000));
    assert_eq!(config.transport.command_timeout, Duration::from_millis(4_000));
    assert_eq!(config.transport.kiss.preamble_ms, 410);
    assert_eq!(config.transport.kiss.tx_tail_ms, 30);
    assert_eq!(config.transport.kiss.persistence, 80);
    assert_eq!(config.transport.kiss.slot_time_ms, 40);
    assert!(config.transport.kiss.flow_control);
}

#[test]
fn vrn76_builder_carries_python_kiss_id_beacon_settings() {
    let iface = InterfaceConfig {
        kind: "vrn76_kiss_ble".to_string(),
        enabled: Some(true),
        peripheral_id: Some("VR-N76".to_string()),
        id_callsign: Some("MYCALL-0".to_string()),
        id_interval: Some(600),
        ..InterfaceConfig::default()
    };

    let config = vrn76_kiss_ble::build_config(&iface).expect("build vrn76 config");

    assert_eq!(
        config.transport.kiss.id_beacon,
        Some(rns_transport::iface::kiss::KissIdBeaconConfig {
            callsign: b"MYCALL-0".to_vec(),
            interval: Duration::from_secs(600),
            min_payload_len: 15,
        })
    );
}

#[test]
fn vrn76_builder_preserves_python_empty_id_beacon_when_callsign_missing() {
    let iface = InterfaceConfig {
        kind: "vrn76_kiss_ble".to_string(),
        enabled: Some(true),
        peripheral_id: Some("VR-N76".to_string()),
        id_interval: Some(600),
        ..InterfaceConfig::default()
    };

    let config = vrn76_kiss_ble::build_config(&iface).expect("build vrn76 config");

    assert_eq!(
        config.transport.kiss.id_beacon,
        Some(rns_transport::iface::kiss::KissIdBeaconConfig {
            callsign: Vec::new(),
            interval: Duration::from_secs(600),
            min_payload_len: 15,
        })
    );
}

#[test]
fn vrn76_builder_uses_raw_kiss_frame_mode() {
    let iface = InterfaceConfig {
        kind: "vrn76_kiss_ble".to_string(),
        enabled: Some(true),
        peripheral_id: Some("VR-N76".to_string()),
        frame_mode: Some("raw_kiss".to_string()),
        ..InterfaceConfig::default()
    };

    let config = vrn76_kiss_ble::build_config(&iface).expect("build vrn76 config");
    assert_eq!(config.transport.frame_mode, Vrn76FrameMode::RawKiss);
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
        display_name: None,
        announce_capabilities: Vec::new(),
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
        display_name: None,
        announce_capabilities: Vec::new(),
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
        display_name: None,
        announce_capabilities: Vec::new(),
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
fn bootstrap_best_effort_starts_kiss_interface_without_transport_flag() {
    let temp = TempDir::new().expect("temp dir");
    let db_path = temp.path().join("reticulum.db");
    let config_path = temp.path().join("daemon.toml");
    fs::write(
        &config_path,
        r#"
interfaces = [
  { type = "kiss", enabled = true, name = "kiss-main", device = "__definitely_not_a_device__", baud_rate = 9600, kiss_flow_control = true }
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
fn bootstrap_best_effort_starts_active_lora_interface_without_transport_flag() {
    let temp = TempDir::new().expect("temp dir");
    let db_path = temp.path().join("reticulum.db");
    let state_path = temp.path().join("lora-state.json");
    let config_path = temp.path().join("daemon.toml");
    fs::write(
        &config_path,
        format!(
            r#"
interfaces = [
  {{ type = "lora", enabled = true, name = "lora-main", region = "US915", state_path = "{}", device = "__definitely_not_a_device__", baud_rate = 115200, max_payload_bytes = 220 }}
]
"#,
            state_path.to_string_lossy().replace('\\', "\\\\")
        ),
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
    assert!(state_path.exists(), "active lora startup should still persist compliance state");
}

#[cfg(not(feature = "vrn76-kiss-ble"))]
#[test]
fn bootstrap_best_effort_marks_vrn76_kiss_ble_feature_disabled_as_failed() {
    let temp = TempDir::new().expect("temp dir");
    let db_path = temp.path().join("reticulum.db");
    let config_path = temp.path().join("daemon.toml");
    fs::write(
        &config_path,
        r#"
interfaces = [
  { type = "vrn76_kiss_ble", enabled = true, name = "vrn76-main", peripheral_id = "VR-N76" }
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
    let runtime = interfaces[0]
        .get("settings")
        .and_then(|value| value.get("_runtime"))
        .expect("runtime settings");
    assert_eq!(runtime.get("startup_status").and_then(|value| value.as_str()), Some("failed"));
    assert!(runtime
        .get("startup_error")
        .and_then(|value| value.as_str())
        .is_some_and(|error| error.contains("requires reticulumd feature vrn76-kiss-ble")));
}

#[cfg(not(feature = "rnode-ble"))]
#[test]
fn bootstrap_best_effort_marks_rnode_ble_feature_disabled_as_failed() {
    let temp = TempDir::new().expect("temp dir");
    let db_path = temp.path().join("reticulum.db");
    let state_path = temp.path().join("lora-state.json");
    let config_path = temp.path().join("daemon.toml");
    fs::write(
        &config_path,
        format!(
            r#"
interfaces = [
  {{ type = "RNodeInterface", enabled = true, name = "rnode-ble", region = "US915", state_path = "{}", port = "ble://RNode 1234", frequency = 915000000, bandwidth = 125000, spreadingfactor = 9, codingrate = 5, txpower = 17 }}
]
"#,
            state_path.to_string_lossy().replace('\\', "\\\\")
        ),
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
    let runtime = interfaces[0]
        .get("settings")
        .and_then(|value| value.get("_runtime"))
        .expect("runtime settings");
    assert_eq!(runtime.get("startup_status").and_then(|value| value.as_str()), Some("failed"));
    assert!(runtime
        .get("startup_error")
        .and_then(|value| value.as_str())
        .is_some_and(|error| error.contains("requires reticulumd feature rnode-ble")));
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
        .join("../../../docs/status/reticulum-parity-matrix.md");
    let text = fs::read_to_string(&parity_matrix_path).expect("read reticulum parity matrix");

    assert!(
        text.contains("Python-style interface-driven `tcp_server` startup now works from config")
            && text.contains("without Rust-only transport overrides"),
        "reticulum parity matrix should document config-driven lxmd tcp_server startup parity"
    );
}

#[test]
fn kiss_docs_document_bearers_and_vtn76_bluetooth() {
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let kiss_runbook =
        fs::read_to_string(repo_root.join("docs/runbooks/reticulumd-kiss-interface.md"))
            .expect("read KISS runbook");
    let vrn76_interface = fs::read_to_string(repo_root.join("docs/interfaces/vrn76-kiss-ble.md"))
        .expect("read VR-N76 KISS BLE interface doc");

    assert!(
        kiss_runbook.contains("serial, Bluetooth, Wi-Fi/TCP"),
        "KISS runbook should document the supported connection bearers"
    );
    assert!(
        vrn76_interface.contains("VT-N76/VR-N76")
            && vrn76_interface.contains("Bluetooth KISS operation"),
        "VR-N76 interface doc should state that VT-N76/VR-N76 KISS uses Bluetooth"
    );
    assert!(
        vrn76_interface.contains("Host Bluetooth Boundary")
            && vrn76_interface.contains("outside this repository")
            && vrn76_interface.contains("adapter drivers")
            && vrn76_interface.contains("pairing or bonding"),
        "VR-N76 interface doc should separate repo-owned KISS/Benshi logic from OS Bluetooth setup"
    );
}

#[test]
fn android_ble_native_target_gates_include_android() {
    let repo_root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let reticulumd_manifest =
        fs::read_to_string(repo_root.join("crates/apps/reticulumd/Cargo.toml"))
            .expect("read reticulumd manifest");
    let rns_tools_manifest = fs::read_to_string(repo_root.join("crates/apps/rns-tools/Cargo.toml"))
        .expect("read rns-tools manifest");
    let ble_mod = fs::read_to_string(
        repo_root.join("crates/apps/reticulumd/src/bin/reticulumd/interfaces/ble/mod.rs"),
    )
    .expect("read reticulumd BLE module");
    let rnx_ble = fs::read_to_string(repo_root.join("crates/apps/rns-tools/src/bin/rnx/ble.rs"))
        .expect("read rns-tools BLE commands");

    for (label, text) in [
        ("reticulumd target dependencies", reticulumd_manifest.as_str()),
        ("rns-tools target dependencies", rns_tools_manifest.as_str()),
        ("reticulumd BLE dispatch", ble_mod.as_str()),
        ("rns-tools BLE commands", rnx_ble.as_str()),
    ] {
        assert!(
            text.contains("target_os = \"android\""),
            "{label} should include android in native BLE target gates"
        );
    }
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
fn bootstrap_reports_auto_interface_as_spawned_runtime() {
    let temp = TempDir::new().expect("temp dir");
    let db_path = temp.path().join("reticulum.db");
    let config_path = temp.path().join("daemon.toml");
    fs::write(
        &config_path,
        r#"
interfaces = [
  { type = "AutoInterface", enabled = true, name = "auto-main", devices = ["codex-nonexistent-auto-test"] }
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

    let auto = interfaces
        .iter()
        .find(|entry| entry.get("type").and_then(|value| value.as_str()) == Some("auto"))
        .expect("auto entry");
    let runtime =
        auto.get("settings").and_then(|value| value.get("_runtime")).expect("runtime settings");
    assert_eq!(
        auto.get("settings")
            .and_then(|value| value.get("discovery_multicast_address"))
            .and_then(|value| value.as_str()),
        Some("ff12:0:d70b:fb1c:16e4:5e39:485e:31e1")
    );
    assert_eq!(runtime.get("startup_status").and_then(|value| value.as_str()), Some("spawned"));
    assert_eq!(runtime.get("runtime_status").and_then(|value| value.as_str()), Some("running"));
    assert_eq!(runtime.get("startup_error"), None);
    assert!(runtime.get("iface").and_then(|value| value.as_str()).is_some());
    let auto_runtime = runtime.get("auto").expect("auto runtime plan metadata");
    assert_eq!(
        auto_runtime.get("auto_runtime_status").and_then(|value| value.as_str()),
        Some("complete")
    );
    assert!(auto_runtime.get("startup_plan").is_some(), "auto startup plan missing: {runtime:?}");
    assert!(
        auto_runtime.get("initial_peer_announces").is_some(),
        "auto initial peer-announce plan missing: {runtime:?}"
    );
    assert!(
        auto_runtime
            .get("planned_discovery_socket_binds")
            .and_then(|value| value.as_array())
            .is_some(),
        "auto discovery socket bind plan missing: {runtime:?}"
    );
    assert!(
        auto_runtime.get("planned_data_socket_binds").and_then(|value| value.as_array()).is_some(),
        "auto peer data socket bind plan missing: {runtime:?}"
    );
    let discovery_runtime =
        runtime.get("auto_discovery_runtime").expect("auto discovery runtime metadata");
    assert_eq!(
        discovery_runtime.get("bound_socket_count").and_then(|value| value.as_u64()),
        Some(0)
    );
    assert_eq!(
        discovery_runtime.get("receive_loop_count").and_then(|value| value.as_u64()),
        Some(0)
    );
    assert_eq!(
        discovery_runtime.get("initial_peer_announce_count").and_then(|value| value.as_u64()),
        Some(0)
    );
    assert_eq!(
        discovery_runtime
            .get("repeat_peer_announce_scheduler_count")
            .and_then(|value| value.as_u64()),
        Some(0)
    );
    assert_eq!(
        discovery_runtime.get("peer_job_scheduler_count").and_then(|value| value.as_u64()),
        Some(0)
    );
    assert_eq!(
        discovery_runtime.get("data_socket_count").and_then(|value| value.as_u64()),
        Some(0)
    );
    assert_eq!(
        discovery_runtime.get("data_receive_loop_count").and_then(|value| value.as_u64()),
        Some(0)
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
            state_path.to_string_lossy().replace('\\', "\\\\")
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
            state_path.to_string_lossy().replace('\\', "\\\\")
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
        rpc: Some("127.0.0.1:0".to_string()),
        db,
        config,
        identity: None,
        announce_interval_secs: 0,
        transport,
        strict_interface_startup,
        rpc_tls_cert: None,
        rpc_tls_key: None,
        rpc_tls_client_ca: None,
        rpc_token_issuer: None,
        rpc_token_audience: None,
        rpc_token_secret_env: None,
        rpc_token_jti_ttl_ms: 60_000,
        rpc_token_clock_skew_ms: 5_000,
        rpc_unix: None,
        #[cfg(feature = "zmq-pipeline-rpc")]
        zmq_rpc_command: None,
    }
}
