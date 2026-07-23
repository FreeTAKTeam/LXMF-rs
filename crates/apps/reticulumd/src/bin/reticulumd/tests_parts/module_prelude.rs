use crate::bootstrap::{
    configure_startup_rpc_token_auth, enforce_rpc_bind_security, enforce_startup_policy,
    mark_interface_runtime_fields, mark_interface_startup_status, select_tcp_listener_device_ip,
    select_tcp_server_bind, InterfaceStartupFailure, RpcTlsConfig,
};

use crate::bridge::{
    validate_delivery_request, wait_for_propagation_signal, PeerCrypto, RequestedDeliveryMethod,
    TransportBridge,
};

use crate::bridge_helpers::opportunistic_payload;

use crate::interfaces::{kiss, lora, pipe, rnode_multi, serial, vrn76_kiss_ble};

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

#[cfg(any(target_os = "linux", target_os = "android"))]
use std::ffi::OsString;

use std::fs;

#[cfg(any(target_os = "linux", target_os = "android"))]
use std::os::unix::ffi::OsStringExt;

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
        Ok(0xf5)
    );
}

async fn test_transport_bridge_fixture() -> (Arc<RpcDaemon>, Arc<TransportBridge>) {
    let (daemon, bridge, _, _) = test_transport_bridge_fixture_with_peer().await;
    (daemon, bridge)
}

async fn test_transport_bridge_fixture_with_peer(
) -> (Arc<RpcDaemon>, Arc<TransportBridge>, PrivateIdentity, String) {
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system time after epoch")
        .as_nanos();
    let service_identity_dir =
        std::env::temp_dir().join(format!("reticulumd-test-service-identities-{unique}"));
    test_transport_bridge_fixture_with_peer_at(service_identity_dir).await
}

async fn test_transport_bridge_fixture_with_peer_at(
    service_identity_dir: PathBuf,
) -> (Arc<RpcDaemon>, Arc<TransportBridge>, PrivateIdentity, String) {
    let signer = PrivateIdentity::new_from_rand(rand_core::OsRng);
    let transport_identity = rns_transport::identity_bridge::to_transport_private_identity(&signer);
    let transport = Transport::new(TransportConfig::new("test", &transport_identity, true));
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
        service_identity_dir,
        signer,
        [0u8; 16],
        announce_destination,
        None,
        Vec::new(),
        None,
        encode_propagation_node_app_data(
            Some("Bridge Node"),
            PropagationNodeAnnounceConfig::default(),
        )
        .ok(),
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
    assert_eq!(pn_stamp_cost_from_app_data(app_data.as_slice()).expect("ok"), 22);
    assert_eq!(pn_stamp_cost_flexibility_from_app_data(app_data.as_slice()).expect("ok"), 6);
    assert_eq!(pn_peering_cost_from_app_data(app_data.as_slice()).expect("ok"), 17);
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
