use crate::bootstrap::{
    configure_startup_rpc_token_auth, control_router_process_status_from_runtime,
    enforce_rpc_bind_security, enforce_startup_policy, mark_interface_runtime_fields,
    mark_interface_startup_status, select_tcp_server_bind, ControlRouterProcessRuntimeStatus,
    InterfaceStartupFailure, RpcTlsConfig,
};
use crate::bridge::{
    validate_delivery_request, PeerCrypto, RequestedDeliveryMethod, TransportBridge,
};
use crate::bridge_helpers::opportunistic_payload;
use crate::interface_worker_mode;
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
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};
use tempfile::TempDir;

#[test]
fn cli_defaults_to_local_unix_rpc_without_tcp_bind() {
    let args = <Args as clap::Parser>::parse_from(["reticulumd"]);
    assert_eq!(args.rpc, None);
    assert_eq!(args.rpc_unix, Some(PathBuf::from(crate::DEFAULT_RPC_UNIX_PATH)));
    assert!(!args.no_rpc_unix);
    assert_eq!(args.worker_process_count, 0);
    assert_eq!(args.worker_process_timeout_ms, 1_000);
    assert_eq!(args.worker_process_command, None);
    #[cfg(unix)]
    assert_eq!(args.worker_process_unix_socket, None);
    assert_eq!(args.worker_process_tcp, None);
    assert!(!args.interface_worker_stdio);
    assert!(!args.control_router_stdio);
    assert_eq!(args.interface_worker_udp_bind, None);
    assert_eq!(args.interface_worker_udp_forward, None);
    assert_eq!(args.interface_worker_tcp_connect, None);
    assert_eq!(args.interface_worker_tcp_listen, None);
    assert_eq!(args.interface_worker_address, None);
    assert_eq!(args.interface_worker_serial_device, None);
    assert_eq!(args.interface_worker_serial_baud_rate, None);
    assert_eq!(args.interface_worker_ble_adapter, None);
    assert_eq!(args.interface_worker_ble_peripheral_id, None);
    assert_eq!(args.interface_worker_ble_service_uuid, None);
    assert_eq!(args.interface_worker_ble_write_char_uuid, None);
    assert_eq!(args.interface_worker_ble_notify_char_uuid, None);
    assert_eq!(args.interface_worker_process_count, 0);
    assert_eq!(args.interface_worker_process_command, None);
    assert_eq!(args.interface_worker_process_shutdown_ms, 1_000);
    assert_eq!(
        args.interface_worker_process_restart_backoff_ms,
        interface_worker_mode::DEFAULT_INTERFACE_WORKER_RESTART_BACKOFF_MS
    );
    assert_eq!(args.control_router_process_count, 0);
    assert_eq!(args.control_router_process_timeout_ms, 1_000);
    assert_eq!(args.control_router_process_command, None);
}

#[test]
fn cli_parses_hidden_no_rpc_unix_option() {
    let args =
        <Args as clap::Parser>::parse_from(["reticulumd", "--rpc", "127.0.0.1:0", "--no-rpc-unix"]);
    assert_eq!(args.rpc.as_deref(), Some("127.0.0.1:0"));
    assert!(args.no_rpc_unix);
}

#[test]
fn control_router_process_status_defaults_when_pool_disabled() {
    let runtime =
        ControlRouterProcessRuntimeStatus { enabled: false, worker_count: 0, timeout_ms: 0 };
    let status = control_router_process_status_from_runtime(&runtime, None);
    assert!(!status.enabled);
    assert_eq!(status.worker_count, 0);
    assert_eq!(status.timeout_ms, 0);
    assert_eq!(status.idle_workers, 0);
    assert_eq!(status.busy_workers, 0);
    assert_eq!(status.request_timeouts, 0);
    assert_eq!(status.child_replacements, 0);
}

#[test]
fn cli_parses_hidden_control_router_stdio_option() {
    let args = <Args as clap::Parser>::parse_from(["reticulumd", "--control-router-stdio"]);
    assert!(args.control_router_stdio);
    assert!(!args.worker_stdio);
    assert!(!args.interface_worker_stdio);
}

#[test]
fn cli_parses_hidden_control_router_process_pool_options() {
    let args = <Args as clap::Parser>::parse_from([
        "reticulumd",
        "--control-router-process-count",
        "3",
        "--control-router-process-timeout-ms",
        "2500",
        "--control-router-process-command",
        "/opt/reticulumd-control-router",
    ]);
    assert_eq!(args.control_router_process_count, 3);
    assert_eq!(args.control_router_process_timeout_ms, 2_500);
    assert_eq!(
        args.control_router_process_command,
        Some(PathBuf::from("/opt/reticulumd-control-router"))
    );
    assert_eq!(
        bootstrap::control_router_process_executable_path(&args),
        PathBuf::from("/opt/reticulumd-control-router")
    );
}

#[cfg(unix)]
#[test]
fn bootstrap_spawns_configured_control_router_process_pool_status() {
    let temp = TempDir::new().expect("temp dir");
    let db_path = temp.path().join("reticulum.db");
    let worker = temp.path().join("configured-control-router.py");
    fs::write(
        &worker,
        r#"#!/usr/bin/env python3
import struct
import sys

while True:
    header = sys.stdin.buffer.read(4)
    if len(header) != 4:
        break
    length = struct.unpack(">I", header)[0]
    sys.stdin.buffer.read(length)
"#,
    )
    .expect("write configured control router worker");
    let mut permissions = fs::metadata(&worker).expect("worker metadata").permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&worker, permissions).expect("worker permissions");

    let mut args = test_args(db_path.clone(), None, None, false);
    args.control_router_process_count = 1;
    args.control_router_process_timeout_ms = 750;
    args.control_router_process_command = Some(worker);

    let runtime =
        tokio::runtime::Builder::new_current_thread().enable_all().build().expect("runtime");
    let context = runtime.block_on(async { bootstrap::bootstrap(args).await });
    let status = context
        .daemon
        .handle_rpc(RpcRequest { id: 5102, method: "daemon_status_ex".to_string(), params: None })
        .expect("daemon status")
        .result
        .expect("daemon status result");
    assert_eq!(status["control_router_processes"]["enabled"].as_bool(), Some(true));
    assert_eq!(status["control_router_processes"]["worker_count"].as_u64(), Some(1));
    assert_eq!(status["control_router_processes"]["timeout_ms"].as_u64(), Some(750));
    assert_eq!(status["control_router_processes"]["idle_workers"].as_u64(), Some(1));
    assert_eq!(status["control_router_processes"]["busy_workers"].as_u64(), Some(0));
    let child_args =
        context.control_router_process_pool.as_ref().expect("control router pool").child_args();
    assert!(child_args.iter().any(|arg| arg.as_os_str() == std::ffi::OsStr::new("--db")));
    assert!(child_args.iter().any(|arg| arg.as_os_str() == db_path.as_os_str()));
}

#[test]
fn cli_parses_hidden_worker_process_pool_options() {
    let args = <Args as clap::Parser>::parse_from([
        "reticulumd",
        "--worker-process-count",
        "2",
        "--worker-process-timeout-ms",
        "2500",
        "--worker-process-command",
        "/opt/reticulumd-worker",
    ]);
    assert_eq!(args.worker_process_count, 2);
    assert_eq!(args.worker_process_timeout_ms, 2_500);
    assert_eq!(args.worker_process_command, Some(PathBuf::from("/opt/reticulumd-worker")));
    assert_eq!(
        bootstrap::worker_process_executable_path(&args),
        PathBuf::from("/opt/reticulumd-worker")
    );
}

#[cfg(unix)]
#[test]
fn cli_parses_hidden_external_worker_socket_option() {
    let args = <Args as clap::Parser>::parse_from([
        "reticulumd",
        "--worker-process-count",
        "2",
        "--worker-process-unix-socket",
        "/tmp/reticulumd-worker.sock",
    ]);
    assert_eq!(args.worker_process_count, 2);
    assert_eq!(args.worker_process_unix_socket, Some(PathBuf::from("/tmp/reticulumd-worker.sock")));
    assert!(matches!(
        bootstrap::worker_process_endpoint(&args),
        crate::worker_mode::WorkerProcessEndpoint::UnixSocket { path }
            if path.as_path() == std::path::Path::new("/tmp/reticulumd-worker.sock")
    ));
}

#[test]
fn cli_parses_hidden_external_worker_tcp_option() {
    let args = <Args as clap::Parser>::parse_from([
        "reticulumd",
        "--worker-process-count",
        "2",
        "--worker-process-tcp",
        "127.0.0.1:7001",
    ]);
    let addr = "127.0.0.1:7001".parse().expect("socket addr");
    assert_eq!(args.worker_process_count, 2);
    assert_eq!(args.worker_process_tcp, Some(addr));
    assert!(matches!(
        bootstrap::worker_process_endpoint(&args),
        crate::worker_mode::WorkerProcessEndpoint::Tcp { addr: parsed } if parsed == addr
    ));
}

#[test]
fn cli_parses_hidden_interface_worker_process_options() {
    let args = <Args as clap::Parser>::parse_from([
        "reticulumd",
        "--interface-worker-process-count",
        "2",
        "--interface-worker-process-command",
        "/opt/reticulumd-interface-worker",
        "--interface-worker-process-shutdown-ms",
        "2500",
        "--interface-worker-process-restart-backoff-ms",
        "750",
    ]);
    assert_eq!(args.interface_worker_process_count, 2);
    assert_eq!(
        args.interface_worker_process_command,
        Some(PathBuf::from("/opt/reticulumd-interface-worker"))
    );
    assert_eq!(args.interface_worker_process_shutdown_ms, 2_500);
    assert_eq!(args.interface_worker_process_restart_backoff_ms, 750);
    assert_eq!(
        bootstrap::interface_worker_process_executable_path(&args),
        PathBuf::from("/opt/reticulumd-interface-worker")
    );
}

#[test]
fn cli_parses_hidden_udp_interface_worker_child_options() {
    let args = <Args as clap::Parser>::parse_from([
        "reticulumd",
        "--interface-worker-stdio",
        "--interface-worker-udp-bind",
        "127.0.0.1:4242",
        "--interface-worker-udp-forward",
        "127.0.0.1:4243",
        "--interface-worker-address",
        "00112233445566778899aabbccddeeff",
    ]);
    assert!(args.interface_worker_stdio);
    assert_eq!(args.interface_worker_udp_bind.as_deref(), Some("127.0.0.1:4242"));
    assert_eq!(args.interface_worker_udp_forward.as_deref(), Some("127.0.0.1:4243"));
    assert_eq!(args.interface_worker_address.as_deref(), Some("00112233445566778899aabbccddeeff"));
}

#[test]
fn cli_parses_hidden_tcp_client_interface_worker_child_options() {
    let args = <Args as clap::Parser>::parse_from([
        "reticulumd",
        "--interface-worker-stdio",
        "--interface-worker-tcp-connect",
        "127.0.0.1:4242",
        "--interface-worker-address",
        "00112233445566778899aabbccddeeff",
    ]);
    assert!(args.interface_worker_stdio);
    assert_eq!(args.interface_worker_tcp_connect.as_deref(), Some("127.0.0.1:4242"));
    assert_eq!(args.interface_worker_address.as_deref(), Some("00112233445566778899aabbccddeeff"));
}

#[test]
fn cli_parses_hidden_tcp_server_interface_worker_child_options() {
    let args = <Args as clap::Parser>::parse_from([
        "reticulumd",
        "--interface-worker-stdio",
        "--interface-worker-tcp-listen",
        "127.0.0.1:4242",
        "--interface-worker-address",
        "00112233445566778899aabbccddeeff",
    ]);
    assert!(args.interface_worker_stdio);
    assert_eq!(args.interface_worker_tcp_listen.as_deref(), Some("127.0.0.1:4242"));
    assert_eq!(args.interface_worker_address.as_deref(), Some("00112233445566778899aabbccddeeff"));
}

#[test]
fn cli_parses_hidden_ble_interface_worker_child_options() {
    let args = <Args as clap::Parser>::parse_from([
        "reticulumd",
        "--interface-worker-stdio",
        "--interface-worker-ble-adapter",
        "hci0",
        "--interface-worker-ble-peripheral-id",
        "AA:BB:CC:DD:EE:FF",
        "--interface-worker-ble-service-uuid",
        "12345678-1234-1234-1234-1234567890ab",
        "--interface-worker-ble-write-char-uuid",
        "2A37",
        "--interface-worker-ble-notify-char-uuid",
        "2A38",
        "--interface-worker-ble-mtu",
        "247",
        "--interface-worker-ble-scan-timeout-ms",
        "5000",
        "--interface-worker-ble-connect-timeout-ms",
        "10000",
        "--interface-worker-ble-reconnect-backoff-ms",
        "500",
        "--interface-worker-ble-max-reconnect-backoff-ms",
        "5000",
    ]);
    assert!(args.interface_worker_stdio);
    assert_eq!(args.interface_worker_ble_adapter.as_deref(), Some("hci0"));
    assert_eq!(args.interface_worker_ble_peripheral_id.as_deref(), Some("AA:BB:CC:DD:EE:FF"));
    assert_eq!(
        args.interface_worker_ble_service_uuid.as_deref(),
        Some("12345678-1234-1234-1234-1234567890ab")
    );
    assert_eq!(args.interface_worker_ble_write_char_uuid.as_deref(), Some("2A37"));
    assert_eq!(args.interface_worker_ble_notify_char_uuid.as_deref(), Some("2A38"));
    assert_eq!(args.interface_worker_ble_mtu, Some(247));
    assert_eq!(args.interface_worker_ble_scan_timeout_ms, Some(5_000));
    assert_eq!(args.interface_worker_ble_connect_timeout_ms, Some(10_000));
    assert_eq!(args.interface_worker_ble_reconnect_backoff_ms, Some(500));
    assert_eq!(args.interface_worker_ble_max_reconnect_backoff_ms, Some(5_000));
}

#[test]
fn cli_parses_hidden_serial_interface_worker_child_options() {
    let args = <Args as clap::Parser>::parse_from([
        "reticulumd",
        "--interface-worker-stdio",
        "--interface-worker-serial-device",
        "/dev/ttyUSB0",
        "--interface-worker-serial-baud-rate",
        "115200",
        "--interface-worker-serial-data-bits",
        "8",
        "--interface-worker-serial-stop-bits",
        "1",
        "--interface-worker-serial-parity",
        "none",
        "--interface-worker-serial-flow-control",
        "none",
        "--interface-worker-serial-mtu",
        "2048",
    ]);
    assert!(args.interface_worker_stdio);
    assert_eq!(args.interface_worker_serial_device.as_deref(), Some("/dev/ttyUSB0"));
    assert_eq!(args.interface_worker_serial_baud_rate, Some(115_200));
    assert_eq!(args.interface_worker_serial_data_bits, Some(8));
    assert_eq!(args.interface_worker_serial_stop_bits, Some(1));
    assert_eq!(args.interface_worker_serial_parity.as_deref(), Some("none"));
    assert_eq!(args.interface_worker_serial_flow_control.as_deref(), Some("none"));
    assert_eq!(args.interface_worker_serial_mtu, Some(2048));
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
async fn transport_bridge_marks_propagated_send_failed_without_selected_node() {
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
    assert!(send.error.is_none(), "propagated send should be queued for bridge delivery");

    let receipt_status = wait_for_receipt_status(&daemon, "propagated-bridge-1", |status| {
        status.starts_with("failed:")
    })
    .await;
    assert!(
        receipt_status.contains("no outbound propagation node selected"),
        "unexpected receipt status: {receipt_status}"
    );
}

async fn wait_for_receipt_status(
    daemon: &RpcDaemon,
    message_id: &str,
    predicate: impl Fn(&str) -> bool,
) -> String {
    for attempt in 0..50 {
        let status = daemon
            .handle_rpc(RpcRequest {
                id: 20_000 + attempt,
                method: "sdk_status_v2".into(),
                params: Some(json!({ "message_id": message_id })),
            })
            .expect("status while waiting for receipt");
        if let Some(receipt_status) =
            status.result.as_ref().and_then(|result| result["message"]["receipt_status"].as_str())
        {
            if predicate(receipt_status) {
                return receipt_status.to_string();
            }
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    panic!("timed out waiting for receipt status for {message_id}");
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
    assert!(!context.worker_process_runtime.enabled);
    assert_eq!(context.worker_process_runtime.worker_count, 0);
    assert_eq!(context.worker_process_runtime.timeout_ms, 1_000);
    let status = context
        .daemon
        .handle_rpc(RpcRequest { id: 99, method: "daemon_status_ex".to_string(), params: None })
        .expect("daemon status")
        .result
        .expect("daemon status result");
    assert_eq!(status["worker_processes"]["enabled"].as_bool(), Some(false));
    assert_eq!(status["worker_processes"]["worker_count"].as_u64(), Some(0));
    assert_eq!(status["worker_processes"]["timeout_ms"].as_u64(), Some(1_000));
    assert_eq!(status["worker_processes"]["idle_workers"].as_u64(), Some(0));
    assert_eq!(status["worker_processes"]["busy_workers"].as_u64(), Some(0));
    assert_eq!(status["worker_processes"]["request_timeouts"].as_u64(), Some(0));
    assert_eq!(status["worker_processes"]["child_replacements"].as_u64(), Some(0));
    assert_eq!(status["interface_worker_processes"]["enabled"].as_bool(), Some(false));
    assert_eq!(status["interface_worker_processes"]["worker_count"].as_u64(), Some(0));
    assert_eq!(status["interface_worker_processes"]["shutdown_timeout_ms"].as_u64(), Some(1_000));
    assert_eq!(
        status["interface_worker_processes"]["restart_backoff_ms"].as_u64(),
        Some(interface_worker_mode::DEFAULT_INTERFACE_WORKER_RESTART_BACKOFF_MS)
    );
    assert_eq!(status["interface_worker_processes"]["live_workers"].as_u64(), Some(0));
    assert_eq!(status["interface_worker_processes"]["stopped_workers"].as_u64(), Some(0));
    assert_eq!(status["interface_worker_processes"]["child_restarts"].as_u64(), Some(0));
    assert_eq!(status["interface_worker_processes"]["child_errors"].as_u64(), Some(0));
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

#[cfg(unix)]
#[test]
fn bootstrap_uses_configured_worker_process_command() {
    let temp = TempDir::new().expect("temp dir");
    let db_path = temp.path().join("reticulum.db");
    let worker = temp.path().join("custom-worker.py");
    fs::write(
        &worker,
        r#"#!/usr/bin/env python3
import time
time.sleep(30)
"#,
    )
    .expect("write custom worker");
    let mut permissions = fs::metadata(&worker).expect("worker metadata").permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&worker, permissions).expect("worker permissions");

    let mut args = test_args(db_path, None, None, false);
    args.worker_process_count = 1;
    args.worker_process_timeout_ms = 2_000;
    args.worker_process_command = Some(worker.clone());
    assert_eq!(bootstrap::worker_process_executable_path(&args), worker);

    let runtime =
        tokio::runtime::Builder::new_current_thread().enable_all().build().expect("runtime");
    let context = runtime.block_on(async { bootstrap::bootstrap(args).await });

    assert!(context.worker_process_runtime.enabled);
    assert_eq!(context.worker_process_runtime.worker_count, 1);
    assert!(context.worker_process_backend.is_some());
    let status = context
        .daemon
        .handle_rpc(RpcRequest { id: 100, method: "daemon_status_ex".to_string(), params: None })
        .expect("daemon status")
        .result
        .expect("daemon status result");
    assert_eq!(status["worker_processes"]["enabled"].as_bool(), Some(true));
    assert_eq!(status["worker_processes"]["worker_count"].as_u64(), Some(1));
    assert_eq!(status["worker_processes"]["idle_workers"].as_u64(), Some(1));
}

#[test]
fn bootstrap_uses_configured_external_worker_tcp_endpoint() {
    let temp = TempDir::new().expect("temp dir");
    let db_path = temp.path().join("reticulum.db");

    let runtime =
        tokio::runtime::Builder::new_current_thread().enable_all().build().expect("runtime");
    runtime.block_on(async {
        let listener =
            tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind worker supervisor");
        let addr = listener.local_addr().expect("worker supervisor address");
        let server = tokio::spawn(async move {
            let (_stream, _) = listener.accept().await.expect("accept worker process connection");
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        });

        let mut args = test_args(db_path, None, None, false);
        args.worker_process_count = 1;
        args.worker_process_timeout_ms = 2_000;
        args.worker_process_tcp = Some(addr);

        let context = bootstrap::bootstrap(args).await;

        assert!(context.worker_process_runtime.enabled);
        assert_eq!(context.worker_process_runtime.worker_count, 1);
        assert!(context.worker_process_backend.is_some());
        let status = context
            .daemon
            .handle_rpc(RpcRequest {
                id: 101,
                method: "daemon_status_ex".to_string(),
                params: None,
            })
            .expect("daemon status")
            .result
            .expect("daemon status result");
        assert_eq!(status["worker_processes"]["enabled"].as_bool(), Some(true));
        assert_eq!(status["worker_processes"]["worker_count"].as_u64(), Some(1));
        assert_eq!(status["worker_processes"]["idle_workers"].as_u64(), Some(1));
        server.await.expect("worker supervisor task");
    });
}

#[cfg(unix)]
#[test]
fn bootstrap_uses_configured_external_worker_unix_socket_endpoint() {
    let temp = TempDir::new().expect("temp dir");
    let db_path = temp.path().join("reticulum.db");
    let socket_path = temp.path().join("worker-supervisor.sock");

    let runtime =
        tokio::runtime::Builder::new_current_thread().enable_all().build().expect("runtime");
    runtime.block_on(async {
        let listener =
            tokio::net::UnixListener::bind(&socket_path).expect("bind worker supervisor socket");
        let server = tokio::spawn(async move {
            let (_stream, _) = listener.accept().await.expect("accept worker process connection");
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        });

        let mut args = test_args(db_path, None, None, false);
        args.worker_process_count = 1;
        args.worker_process_timeout_ms = 2_000;
        args.worker_process_unix_socket = Some(socket_path);

        let context = bootstrap::bootstrap(args).await;

        assert!(context.worker_process_runtime.enabled);
        assert_eq!(context.worker_process_runtime.worker_count, 1);
        assert!(context.worker_process_backend.is_some());
        let status = context
            .daemon
            .handle_rpc(RpcRequest {
                id: 102,
                method: "daemon_status_ex".to_string(),
                params: None,
            })
            .expect("daemon status")
            .result
            .expect("daemon status result");
        assert_eq!(status["worker_processes"]["enabled"].as_bool(), Some(true));
        assert_eq!(status["worker_processes"]["worker_count"].as_u64(), Some(1));
        assert_eq!(status["worker_processes"]["idle_workers"].as_u64(), Some(1));
        server.await.expect("worker supervisor task");
    });
}

#[cfg(unix)]
#[test]
fn bootstrap_registers_configured_interface_worker_process() {
    let temp = TempDir::new().expect("temp dir");
    let db_path = temp.path().join("reticulum.db");
    let worker = temp.path().join("custom-interface-worker.py");
    fs::write(
        &worker,
        r#"#!/usr/bin/env python3
import struct
import sys

header = sys.stdin.buffer.read(4)
if len(header) == 4:
    length = struct.unpack(">I", header)[0]
    sys.stdin.buffer.read(length)
"#,
    )
    .expect("write custom interface worker");
    let mut permissions = fs::metadata(&worker).expect("worker metadata").permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&worker, permissions).expect("worker permissions");

    let mut args = test_args(db_path, None, None, false);
    args.interface_worker_process_count = 1;
    args.interface_worker_process_command = Some(worker.clone());
    args.interface_worker_process_shutdown_ms = 2_000;
    assert_eq!(bootstrap::interface_worker_process_executable_path(&args), worker);

    let runtime =
        tokio::runtime::Builder::new_current_thread().enable_all().build().expect("runtime");
    let context = runtime.block_on(async { bootstrap::bootstrap(args).await });

    assert_eq!(context.interface_worker_bridges.len(), 1);
    let bridge_address = context.interface_worker_bridges[0].address.to_string();
    let status = context
        .daemon
        .handle_rpc(RpcRequest { id: 102, method: "daemon_status_ex".to_string(), params: None })
        .expect("daemon status")
        .result
        .expect("daemon status result");
    assert_eq!(status["interface_worker_processes"]["enabled"].as_bool(), Some(true));
    assert_eq!(status["interface_worker_processes"]["worker_count"].as_u64(), Some(1));
    assert_eq!(status["interface_worker_processes"]["shutdown_timeout_ms"].as_u64(), Some(2_000));
    assert_eq!(
        status["interface_worker_processes"]["restart_backoff_ms"].as_u64(),
        Some(interface_worker_mode::DEFAULT_INTERFACE_WORKER_RESTART_BACKOFF_MS)
    );
    assert_eq!(status["interface_worker_processes"]["live_workers"].as_u64(), Some(1));
    assert_eq!(status["interface_worker_processes"]["stopped_workers"].as_u64(), Some(0));
    assert_eq!(status["interface_worker_processes"]["child_restarts"].as_u64(), Some(0));
    assert_eq!(status["interface_worker_processes"]["child_errors"].as_u64(), Some(0));
    let response = context
        .daemon
        .handle_rpc(RpcRequest { id: 101, method: "list_interfaces".to_string(), params: None })
        .expect("list_interfaces");
    let interfaces = response
        .result
        .expect("result")
        .get("interfaces")
        .and_then(|value| value.as_array())
        .cloned()
        .expect("interfaces array");
    let interface_worker = interfaces
        .iter()
        .find(|entry| {
            entry.get("type").and_then(|value| value.as_str()) == Some("interface_worker_process")
        })
        .expect("interface worker process entry");
    assert_eq!(
        interface_worker
            .get("settings")
            .and_then(|value| value.get("_runtime"))
            .and_then(|value| value.get("iface"))
            .and_then(|value| value.as_str()),
        Some(bridge_address.as_str())
    );
    assert_eq!(
        interface_worker
            .get("settings")
            .and_then(|value| value.get("_runtime"))
            .and_then(|value| value.get("managed_by"))
            .and_then(|value| value.as_str()),
        Some("interface_worker_process")
    );
}

#[cfg(unix)]
#[test]
fn interface_worker_process_status_publisher_reports_restarted_child_live() {
    let temp = TempDir::new().expect("temp dir");
    let db_path = temp.path().join("reticulum.db");
    let worker = temp.path().join("exiting-interface-worker.py");
    fs::write(
        &worker,
        r#"#!/usr/bin/env python3
import sys
sys.exit(0)
"#,
    )
    .expect("write exiting interface worker");
    let mut permissions = fs::metadata(&worker).expect("worker metadata").permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&worker, permissions).expect("worker permissions");

    let mut args = test_args(db_path, None, None, false);
    args.interface_worker_process_count = 1;
    args.interface_worker_process_command = Some(worker);
    args.interface_worker_process_shutdown_ms = 500;
    args.interface_worker_process_restart_backoff_ms = 25;

    let runtime =
        tokio::runtime::Builder::new_current_thread().enable_all().build().expect("runtime");
    runtime.block_on(async {
        let context = bootstrap::bootstrap(args).await;
        for _ in 0..40 {
            let status = context
                .daemon
                .handle_rpc(RpcRequest {
                    id: 103,
                    method: "daemon_status_ex".to_string(),
                    params: None,
                })
                .expect("daemon status")
                .result
                .expect("daemon status result");
            if status["interface_worker_processes"]["live_workers"].as_u64() == Some(1)
                && status["interface_worker_processes"]["child_restarts"].as_u64().unwrap_or(0) >= 1
            {
                assert_eq!(
                    status["interface_worker_processes"]["stopped_workers"].as_u64(),
                    Some(0)
                );
                assert_eq!(status["interface_worker_processes"]["child_errors"].as_u64(), Some(0));
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
        panic!("interface worker process status did not report restarted child as live");
    });
}

#[cfg(unix)]
#[test]
fn interface_worker_restart_preserves_configured_interface_state() {
    let temp = TempDir::new().expect("temp dir");
    let db_path = temp.path().join("reticulum.db");
    let config_path = temp.path().join("daemon.toml");
    fs::write(
        &config_path,
        r#"
interfaces = [
  { type = "udp", enabled = true, name = "udp-restart", host = "127.0.0.1", port = 0 }
]
"#,
    )
    .expect("write config");
    let worker = temp.path().join("restarting-configured-interface-worker.py");
    let counter = temp.path().join("restart-count");
    fs::write(
        &worker,
        format!(
            r#"#!/usr/bin/env python3
import pathlib
import struct
import sys

counter = pathlib.Path({counter:?})
count = int(counter.read_text()) if counter.exists() else 0
counter.write_text(str(count + 1))
if count == 0:
    sys.exit(0)

while True:
    header = sys.stdin.buffer.read(4)
    if len(header) != 4:
        break
    length = struct.unpack(">I", header)[0]
    payload = sys.stdin.buffer.read(length)
    if len(payload) != length:
        break
"#,
            counter = counter.to_string_lossy(),
        ),
    )
    .expect("write restarting interface worker");
    let mut permissions = fs::metadata(&worker).expect("worker metadata").permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&worker, permissions).expect("worker permissions");

    let mut args = test_args(db_path, Some(config_path), None, false);
    args.interface_worker_process_count = 1;
    args.interface_worker_process_command = Some(worker);
    args.interface_worker_process_shutdown_ms = 500;
    args.interface_worker_process_restart_backoff_ms = 25;

    let runtime =
        tokio::runtime::Builder::new_current_thread().enable_all().build().expect("runtime");
    runtime.block_on(async {
        let context = bootstrap::bootstrap(args).await;
        let bridge_address = context.interface_worker_bridges[0].address.to_string();

        for _ in 0..80 {
            let restart_count = fs::read_to_string(&counter)
                .ok()
                .and_then(|value| value.parse::<usize>().ok())
                .unwrap_or_default();
            let status = context
                .daemon
                .handle_rpc(RpcRequest {
                    id: 109,
                    method: "daemon_status_ex".to_string(),
                    params: None,
                })
                .expect("daemon status")
                .result
                .expect("daemon status result");
            if restart_count >= 2
                && status["interface_worker_processes"]["child_restarts"]
                    .as_u64()
                    .unwrap_or_default()
                    >= 1
            {
                assert_eq!(status["interface_worker_processes"]["live_workers"].as_u64(), Some(1));
                assert_eq!(
                    status["interface_worker_processes"]["stopped_workers"].as_u64(),
                    Some(0)
                );
                assert_eq!(status["interface_worker_processes"]["child_errors"].as_u64(), Some(0));
                let response = context
                    .daemon
                    .handle_rpc(RpcRequest {
                        id: 110,
                        method: "list_interfaces".to_string(),
                        params: None,
                    })
                    .expect("list_interfaces");
                let interfaces = response
                    .result
                    .expect("result")
                    .get("interfaces")
                    .and_then(|value| value.as_array())
                    .cloned()
                    .expect("interfaces array");
                assert_eq!(interfaces.len(), 1);
                let udp = &interfaces[0];
                assert_eq!(udp.get("type").and_then(|value| value.as_str()), Some("udp"));
                assert_eq!(
                    udp.get("settings")
                        .and_then(|value| value.get("_runtime"))
                        .and_then(|value| value.get("iface"))
                        .and_then(|value| value.as_str()),
                    Some(bridge_address.as_str())
                );
                assert_eq!(
                    udp.get("settings")
                        .and_then(|value| value.get("_runtime"))
                        .and_then(|value| value.get("startup_status"))
                        .and_then(|value| value.as_str()),
                    Some("spawned_process")
                );
                assert_eq!(
                    udp.get("settings")
                        .and_then(|value| value.get("_runtime"))
                        .and_then(|value| value.get("managed_by"))
                        .and_then(|value| value.as_str()),
                    Some("interface_worker_process")
                );
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
        panic!("interface worker restart did not preserve configured interface state");
    });
}

#[cfg(unix)]
#[test]
fn bootstrap_uses_interface_worker_process_for_configured_udp() {
    let temp = TempDir::new().expect("temp dir");
    let db_path = temp.path().join("reticulum.db");
    let config_path = temp.path().join("daemon.toml");
    fs::write(
        &config_path,
        r#"
interfaces = [
  { type = "udp", enabled = true, name = "udp-process", host = "127.0.0.1", port = 0 }
]
"#,
    )
    .expect("write config");
    let worker = temp.path().join("configured-udp-worker.py");
    fs::write(
        &worker,
        r#"#!/usr/bin/env python3
import struct
import sys

header = sys.stdin.buffer.read(4)
if len(header) == 4:
    length = struct.unpack(">I", header)[0]
    sys.stdin.buffer.read(length)
"#,
    )
    .expect("write configured udp worker");
    let mut permissions = fs::metadata(&worker).expect("worker metadata").permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&worker, permissions).expect("worker permissions");

    let mut args = test_args(db_path, Some(config_path), None, false);
    args.interface_worker_process_count = 1;
    args.interface_worker_process_command = Some(worker);
    args.interface_worker_process_shutdown_ms = 2_000;

    let runtime =
        tokio::runtime::Builder::new_current_thread().enable_all().build().expect("runtime");
    let context = runtime.block_on(async { bootstrap::bootstrap(args).await });

    assert_eq!(context.interface_worker_bridges.len(), 1);
    let response = context
        .daemon
        .handle_rpc(RpcRequest { id: 104, method: "list_interfaces".to_string(), params: None })
        .expect("list_interfaces");
    let interfaces = response
        .result
        .expect("result")
        .get("interfaces")
        .and_then(|value| value.as_array())
        .cloned()
        .expect("interfaces array");
    assert_eq!(interfaces.len(), 1);
    let udp = &interfaces[0];
    assert_eq!(udp.get("type").and_then(|value| value.as_str()), Some("udp"));
    assert_eq!(
        udp.get("settings")
            .and_then(|value| value.get("_runtime"))
            .and_then(|value| value.get("startup_status"))
            .and_then(|value| value.as_str()),
        Some("spawned_process")
    );
    assert_eq!(
        udp.get("settings")
            .and_then(|value| value.get("_runtime"))
            .and_then(|value| value.get("managed_by"))
            .and_then(|value| value.as_str()),
        Some("interface_worker_process")
    );
}

#[cfg(unix)]
#[test]
fn bootstrap_uses_interface_worker_process_for_configured_serial() {
    let temp = TempDir::new().expect("temp dir");
    let db_path = temp.path().join("reticulum.db");
    let config_path = temp.path().join("daemon.toml");
    fs::write(
        &config_path,
        r#"
interfaces = [
  { type = "serial", enabled = true, name = "serial-process", device = "/dev/not-real", baud_rate = 115200 }
]
"#,
    )
    .expect("write config");
    let worker = temp.path().join("configured-serial-worker.py");
    fs::write(
        &worker,
        r#"#!/usr/bin/env python3
import struct
import sys

header = sys.stdin.buffer.read(4)
if len(header) == 4:
    length = struct.unpack(">I", header)[0]
    sys.stdin.buffer.read(length)
"#,
    )
    .expect("write configured serial worker");
    let mut permissions = fs::metadata(&worker).expect("worker metadata").permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&worker, permissions).expect("worker permissions");

    let mut args = test_args(db_path, Some(config_path), None, false);
    args.interface_worker_process_count = 1;
    args.interface_worker_process_command = Some(worker);
    args.interface_worker_process_shutdown_ms = 2_000;

    let runtime =
        tokio::runtime::Builder::new_current_thread().enable_all().build().expect("runtime");
    let context = runtime.block_on(async { bootstrap::bootstrap(args).await });

    assert_eq!(context.interface_worker_bridges.len(), 1);
    let response = context
        .daemon
        .handle_rpc(RpcRequest { id: 105, method: "list_interfaces".to_string(), params: None })
        .expect("list_interfaces");
    let interfaces = response
        .result
        .expect("result")
        .get("interfaces")
        .and_then(|value| value.as_array())
        .cloned()
        .expect("interfaces array");
    assert_eq!(interfaces.len(), 1);
    let serial = &interfaces[0];
    assert_eq!(serial.get("type").and_then(|value| value.as_str()), Some("serial"));
    assert_eq!(
        serial
            .get("settings")
            .and_then(|value| value.get("_runtime"))
            .and_then(|value| value.get("startup_status"))
            .and_then(|value| value.as_str()),
        Some("spawned_process")
    );
    assert_eq!(
        serial
            .get("settings")
            .and_then(|value| value.get("_runtime"))
            .and_then(|value| value.get("managed_by"))
            .and_then(|value| value.as_str()),
        Some("interface_worker_process")
    );
}

#[cfg(unix)]
#[test]
fn bootstrap_uses_interface_worker_process_for_configured_ble() {
    let temp = TempDir::new().expect("temp dir");
    let db_path = temp.path().join("reticulum.db");
    let config_path = temp.path().join("daemon.toml");
    fs::write(
        &config_path,
        r#"
interfaces = [
  { type = "ble_gatt", enabled = true, name = "ble-process", adapter = "disabled", peripheral_id = "AA:BB:CC:DD:EE:FF", service_uuid = "12345678-1234-1234-1234-1234567890ab", write_char_uuid = "2A37", notify_char_uuid = "2A38" }
]
"#,
    )
    .expect("write config");
    let worker = temp.path().join("configured-ble-worker.py");
    fs::write(
        &worker,
        r#"#!/usr/bin/env python3
import struct
import sys

header = sys.stdin.buffer.read(4)
if len(header) == 4:
    length = struct.unpack(">I", header)[0]
    sys.stdin.buffer.read(length)
"#,
    )
    .expect("write configured ble worker");
    let mut permissions = fs::metadata(&worker).expect("worker metadata").permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&worker, permissions).expect("worker permissions");

    let mut args = test_args(db_path, Some(config_path), None, false);
    args.interface_worker_process_count = 1;
    args.interface_worker_process_command = Some(worker);
    args.interface_worker_process_shutdown_ms = 2_000;

    let runtime =
        tokio::runtime::Builder::new_current_thread().enable_all().build().expect("runtime");
    let context = runtime.block_on(async { bootstrap::bootstrap(args).await });

    assert_eq!(context.interface_worker_bridges.len(), 1);
    let response = context
        .daemon
        .handle_rpc(RpcRequest { id: 107, method: "list_interfaces".to_string(), params: None })
        .expect("list_interfaces");
    let interfaces = response
        .result
        .expect("result")
        .get("interfaces")
        .and_then(|value| value.as_array())
        .cloned()
        .expect("interfaces array");
    assert_eq!(interfaces.len(), 1);
    let ble = &interfaces[0];
    assert_eq!(ble.get("type").and_then(|value| value.as_str()), Some("ble_gatt"));
    assert_eq!(
        ble.get("settings")
            .and_then(|value| value.get("_runtime"))
            .and_then(|value| value.get("startup_status"))
            .and_then(|value| value.as_str()),
        Some("spawned_process")
    );
    assert_eq!(
        ble.get("settings")
            .and_then(|value| value.get("_runtime"))
            .and_then(|value| value.get("managed_by"))
            .and_then(|value| value.as_str()),
        Some("interface_worker_process")
    );
}

#[cfg(unix)]
#[test]
fn bootstrap_uses_interface_worker_process_for_configured_tcp_client() {
    let temp = TempDir::new().expect("temp dir");
    let db_path = temp.path().join("reticulum.db");
    let config_path = temp.path().join("daemon.toml");
    fs::write(
        &config_path,
        r#"
interfaces = [
  { type = "tcp_client", enabled = true, name = "tcp-process", host = "127.0.0.1", port = 4242 }
]
"#,
    )
    .expect("write config");
    let worker = temp.path().join("configured-tcp-worker.py");
    fs::write(
        &worker,
        r#"#!/usr/bin/env python3
import struct
import sys

header = sys.stdin.buffer.read(4)
if len(header) == 4:
    length = struct.unpack(">I", header)[0]
    sys.stdin.buffer.read(length)
"#,
    )
    .expect("write configured tcp worker");
    let mut permissions = fs::metadata(&worker).expect("worker metadata").permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&worker, permissions).expect("worker permissions");

    let mut args = test_args(db_path, Some(config_path), None, false);
    args.interface_worker_process_count = 1;
    args.interface_worker_process_command = Some(worker);
    args.interface_worker_process_shutdown_ms = 2_000;

    let runtime =
        tokio::runtime::Builder::new_current_thread().enable_all().build().expect("runtime");
    let context = runtime.block_on(async { bootstrap::bootstrap(args).await });

    assert_eq!(context.interface_worker_bridges.len(), 1);
    let response = context
        .daemon
        .handle_rpc(RpcRequest { id: 106, method: "list_interfaces".to_string(), params: None })
        .expect("list_interfaces");
    let interfaces = response
        .result
        .expect("result")
        .get("interfaces")
        .and_then(|value| value.as_array())
        .cloned()
        .expect("interfaces array");
    assert_eq!(interfaces.len(), 1);
    let tcp = &interfaces[0];
    assert_eq!(tcp.get("type").and_then(|value| value.as_str()), Some("tcp_client"));
    assert_eq!(
        tcp.get("settings")
            .and_then(|value| value.get("_runtime"))
            .and_then(|value| value.get("startup_status"))
            .and_then(|value| value.as_str()),
        Some("spawned_process")
    );
    assert_eq!(
        tcp.get("settings")
            .and_then(|value| value.get("_runtime"))
            .and_then(|value| value.get("managed_by"))
            .and_then(|value| value.as_str()),
        Some("interface_worker_process")
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

#[cfg(unix)]
#[test]
fn bootstrap_uses_interface_worker_process_for_configured_tcp_server() {
    let temp = TempDir::new().expect("temp dir");
    let db_path = temp.path().join("reticulum.db");
    let config_path = temp.path().join("daemon.toml");
    fs::write(
        &config_path,
        r#"
interfaces = [
  { type = "tcp_server", enabled = true, name = "server-process", host = "127.0.0.1", port = 0 }
]
"#,
    )
    .expect("write config");
    let worker = temp.path().join("configured-tcp-server-worker.py");
    fs::write(
        &worker,
        r#"#!/usr/bin/env python3
import struct
import sys

header = sys.stdin.buffer.read(4)
if len(header) == 4:
    length = struct.unpack(">I", header)[0]
    sys.stdin.buffer.read(length)
"#,
    )
    .expect("write configured tcp server worker");
    let mut permissions = fs::metadata(&worker).expect("worker metadata").permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(&worker, permissions).expect("worker permissions");

    let mut args = test_args(db_path, Some(config_path), None, false);
    args.interface_worker_process_count = 1;
    args.interface_worker_process_command = Some(worker);
    args.interface_worker_process_shutdown_ms = 2_000;

    let runtime =
        tokio::runtime::Builder::new_current_thread().enable_all().build().expect("runtime");
    let context = runtime.block_on(async { bootstrap::bootstrap(args).await });

    assert_eq!(context.interface_worker_bridges.len(), 1);
    let response = context
        .daemon
        .handle_rpc(RpcRequest { id: 108, method: "list_interfaces".to_string(), params: None })
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
        no_rpc_unix: false,
        worker_stdio: false,
        interface_worker_stdio: false,
        control_router_stdio: false,
        interface_worker_udp_bind: None,
        interface_worker_udp_forward: None,
        interface_worker_tcp_connect: None,
        interface_worker_tcp_listen: None,
        interface_worker_address: None,
        interface_worker_serial_device: None,
        interface_worker_serial_baud_rate: None,
        interface_worker_serial_data_bits: None,
        interface_worker_serial_stop_bits: None,
        interface_worker_serial_parity: None,
        interface_worker_serial_flow_control: None,
        interface_worker_serial_mtu: None,
        interface_worker_serial_reconnect_backoff_ms: None,
        interface_worker_serial_max_reconnect_backoff_ms: None,
        interface_worker_ble_adapter: None,
        interface_worker_ble_peripheral_id: None,
        interface_worker_ble_service_uuid: None,
        interface_worker_ble_write_char_uuid: None,
        interface_worker_ble_notify_char_uuid: None,
        interface_worker_ble_mtu: None,
        interface_worker_ble_scan_timeout_ms: None,
        interface_worker_ble_connect_timeout_ms: None,
        interface_worker_ble_reconnect_backoff_ms: None,
        interface_worker_ble_max_reconnect_backoff_ms: None,
        worker_process_count: 0,
        worker_process_timeout_ms: 1_000,
        worker_process_command: None,
        #[cfg(unix)]
        worker_process_unix_socket: None,
        worker_process_tcp: None,
        interface_worker_process_count: 0,
        interface_worker_process_command: None,
        interface_worker_process_shutdown_ms: 1_000,
        interface_worker_process_restart_backoff_ms:
            interface_worker_mode::DEFAULT_INTERFACE_WORKER_RESTART_BACKOFF_MS,
        control_router_process_count: 0,
        control_router_process_timeout_ms: 1_000,
        control_router_process_command: None,
    }
}
