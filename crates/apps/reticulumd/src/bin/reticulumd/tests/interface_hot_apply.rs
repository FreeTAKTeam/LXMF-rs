use super::{
    apply_hot_apply_interface_records, refresh_hot_apply_udp_runtime_status_once,
    HotApplyUdpRefresh, InterfaceHotApplyBridge, InterfaceManager, InterfaceMutationBridge,
    InterfaceRecord, ManagedHotApplyInterface,
};
use rns_rpc::{RpcDaemon, RpcRequest};
use rns_transport::iface::{IfaceRole, InterfaceMode, InterfaceSharedConfig};
use serde_json::json;
use std::collections::HashMap;
use std::io;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::time::{timeout, Duration};

fn tcp_record(name: &str, host: &str, port: u16) -> InterfaceRecord {
    InterfaceRecord {
        kind: "tcp_client".to_string(),
        enabled: true,
        host: Some(host.to_string()),
        port: Some(port),
        name: Some(name.to_string()),
        settings: None,
    }
}

fn udp_record(name: &str, host: &str, port: u16) -> InterfaceRecord {
    InterfaceRecord {
        kind: "udp".to_string(),
        enabled: true,
        host: Some(host.to_string()),
        port: Some(port),
        name: Some(name.to_string()),
        settings: None,
    }
}

fn udp_peer_record(
    name: &str,
    host: &str,
    port: u16,
    target_host: &str,
    target_port: u16,
) -> InterfaceRecord {
    let mut record = udp_record(name, host, port);
    record.settings = Some(json!({
        "target_host": target_host,
        "target_port": target_port
    }));
    record
}

fn udp_refreshes() -> Arc<std::sync::Mutex<HashMap<String, HotApplyUdpRefresh>>> {
    Arc::new(std::sync::Mutex::new(HashMap::new()))
}

fn test_bridge(
    tx: tokio::sync::mpsc::Sender<super::InterfaceHotApplyCommand>,
) -> InterfaceHotApplyBridge {
    InterfaceHotApplyBridge { tx, udp_refreshes: udp_refreshes() }
}

#[tokio::test(flavor = "current_thread")]
async fn hot_apply_spawns_tcp_client_connections() {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind listener");
    let addr = listener.local_addr().expect("listener addr");
    let iface_manager = Arc::new(tokio::sync::Mutex::new(InterfaceManager::new(8)));
    let bridge = InterfaceHotApplyBridge::spawn(iface_manager, Vec::new());

    let applied = bridge
        .apply_interfaces(vec![tcp_record("loopback", "127.0.0.1", addr.port())])
        .expect("apply interfaces");
    assert_eq!(applied.len(), 1);
    let runtime = applied[0]
        .settings
        .as_ref()
        .and_then(|value| value.get("_runtime"))
        .expect("runtime metadata");
    assert_eq!(runtime.get("startup_status").and_then(|value| value.as_str()), Some("spawned"));
    assert_eq!(runtime.get("runtime_status").and_then(|value| value.as_str()), Some("running"));

    let accept = timeout(Duration::from_secs(2), listener.accept())
        .await
        .expect("tcp client should connect");
    let (_stream, peer_addr) = accept.expect("accept connection");
    assert!(peer_addr.ip().is_loopback());
}

#[tokio::test(flavor = "current_thread")]
async fn hot_apply_spawns_tcp_client_with_record_runtime_settings() {
    let iface_manager = Arc::new(tokio::sync::Mutex::new(InterfaceManager::new(8)));
    let mut managed = HashMap::new();
    let mut record = tcp_record("loopback", "127.0.0.1", 1);
    record.settings = Some(json!({
        "interface_mode": "gateway",
        "outgoing": false,
        "bitrate": 1200,
        "announce_cap": 5,
        "announce_rate_target": 120,
        "announce_rate_grace": 2,
        "announce_rate_penalty": 30,
        "network_name": "field-net",
        "discoverable": true,
        "announce_interval": 21600
    }));

    let refreshes = udp_refreshes();
    apply_hot_apply_interface_records(&iface_manager, &mut managed, vec![record], &refreshes, None)
        .await;

    let address = managed.get("loopback").expect("managed tcp client").address;
    let manager = iface_manager.lock().await;
    assert_eq!(manager.mode(&address), Some(InterfaceMode::Gateway));
    assert_eq!(manager.outgoing(&address), Some(false));
    assert_eq!(manager.announce_pacing(&address), Some((1200, 5)));
    assert_eq!(
        manager.shared_config(&address),
        Some(&InterfaceSharedConfig {
            announce_rate_target: Some(120),
            announce_rate_grace: Some(2),
            announce_rate_penalty: Some(30),
            network_name: Some("field-net".to_string()),
            discoverable: Some(true),
            announce_interval: Some(21_600),
            ..InterfaceSharedConfig::default()
        })
    );
}

#[tokio::test(flavor = "current_thread")]
async fn hot_apply_updates_existing_tcp_client_runtime_settings() {
    let iface_manager = Arc::new(tokio::sync::Mutex::new(InterfaceManager::new(8)));
    let address = {
        let mut manager = iface_manager.lock().await;
        *manager.new_channel(8).address()
    };
    let mut managed = HashMap::from([(
        "loopback".to_string(),
        ManagedHotApplyInterface { record: tcp_record("loopback", "127.0.0.1", 1), address },
    )]);
    let mut record = tcp_record("loopback", "127.0.0.1", 1);
    record.settings = Some(json!({
        "interface_mode": "access_point",
        "outgoing": false,
        "passphrase": "shared-secret",
        "publish_ifac": true
    }));

    let refreshes = udp_refreshes();
    apply_hot_apply_interface_records(&iface_manager, &mut managed, vec![record], &refreshes, None)
        .await;

    let manager = iface_manager.lock().await;
    assert_eq!(manager.mode(&address), Some(InterfaceMode::AccessPoint));
    assert_eq!(manager.outgoing(&address), Some(false));
    assert_eq!(
        manager.shared_config(&address),
        Some(&InterfaceSharedConfig {
            passphrase: Some("shared-secret".to_string()),
            publish_ifac: Some(true),
            ..InterfaceSharedConfig::default()
        })
    );
}

#[tokio::test(flavor = "current_thread")]
async fn hot_apply_spawns_udp_unicast_listener_with_runtime_metadata() {
    let iface_manager = Arc::new(tokio::sync::Mutex::new(InterfaceManager::new(8)));
    let bridge = InterfaceHotApplyBridge::spawn(iface_manager.clone(), Vec::new());
    let mut managed = HashMap::new();
    let record = udp_record("udp-loopback", "127.0.0.1", 0);

    let refreshes = udp_refreshes();
    apply_hot_apply_interface_records(
        &iface_manager,
        &mut managed,
        vec![record.clone()],
        &refreshes,
        None,
    )
    .await;
    let applied = bridge.apply_interfaces(vec![record]).expect("apply udp interface");
    let runtime = applied[0]
        .settings
        .as_ref()
        .and_then(|value| value.get("_runtime"))
        .expect("runtime metadata");
    assert_eq!(runtime.get("startup_status").and_then(|value| value.as_str()), Some("spawned"));
    assert_eq!(runtime.get("runtime_status").and_then(|value| value.as_str()), Some("running"));
    let udp_status =
        runtime.get("udp").and_then(|value| value.get("status")).expect("udp runtime status");
    assert_eq!(udp_status.get("link_state").and_then(|value| value.as_str()), Some("configured"));
    assert_eq!(udp_status.get("role").and_then(|value| value.as_str()), Some("listener"));
    assert_eq!(udp_status.get("bind_addr").and_then(|value| value.as_str()), Some("127.0.0.1:0"));
    assert!(udp_status.get("forward_addr").is_some_and(|value| value.is_null()));
    assert!(udp_status.get("iface").is_some_and(|value| value.is_null()));

    let address = managed.get("udp-loopback").expect("managed udp").address;
    let manager = iface_manager.lock().await;
    assert_eq!(manager.role(&address), Some(IfaceRole::Unicast));
    assert_eq!(manager.mode(&address), Some(InterfaceMode::Full));
    assert_eq!(managed.len(), 1);
}

#[tokio::test(flavor = "current_thread")]
async fn hot_apply_udp_refresh_attaches_runtime_iface_and_live_status() {
    let daemon = Arc::new(RpcDaemon::test_instance());
    let iface_manager = Arc::new(tokio::sync::Mutex::new(InterfaceManager::new(8)));
    let bridge = InterfaceHotApplyBridge::spawn_with_daemon(
        iface_manager,
        Vec::new(),
        Arc::downgrade(&daemon),
    );
    let record = udp_record("udp-loopback", "127.0.0.1", 0);

    let applied = bridge.apply_interfaces(vec![record]).expect("apply udp");
    daemon.replace_interfaces(applied);

    let (runtime_iface, refresh) = wait_for_hot_apply_udp_refresh(&bridge).await;
    refresh.status.update(|status| {
        status.link_state = "bound".to_string();
        status.bytes_rx = 12;
        status.decode_errors = 1;
        status.last_error = Some("couldn't decode packet".to_string());
    });

    assert_eq!(refresh_hot_apply_udp_runtime_status_once(&daemon, &bridge.udp_refreshes), 1);
    let result = daemon
        .handle_rpc(RpcRequest { id: 771, method: "daemon_status_ex".to_string(), params: None })
        .expect("daemon status")
        .result
        .expect("daemon status result");
    let status = &result["interfaces"][0]["settings"]["_runtime"]["udp"]["status"];

    assert_eq!(
        result["interfaces"][0]["settings"]["_runtime"]["iface"].as_str(),
        Some(runtime_iface.to_string().as_str())
    );
    assert_eq!(status["link_state"].as_str(), Some("bound"));
    assert_eq!(status["role"].as_str(), Some("listener"));
    assert_eq!(status["iface"].as_str(), Some(runtime_iface.to_string().as_str()));
    assert_eq!(status["bytes_rx"].as_u64(), Some(12));
    assert_eq!(status["decode_errors"].as_u64(), Some(1));
    assert_eq!(status["last_error"].as_str(), Some("couldn't decode packet"));
}

async fn wait_for_hot_apply_udp_refresh(
    bridge: &InterfaceHotApplyBridge,
) -> (rns_transport::hash::AddressHash, HotApplyUdpRefresh) {
    for _ in 0..20 {
        if let Some(refresh) = bridge
            .udp_refreshes
            .lock()
            .expect("udp refresh mutex poisoned")
            .values()
            .next()
            .cloned()
        {
            return (refresh.runtime_iface, refresh);
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    panic!("hot-applied udp refresh was not registered");
}

#[tokio::test(flavor = "current_thread")]
async fn hot_apply_spawns_udp_unicast_peer_with_runtime_metadata() {
    let iface_manager = Arc::new(tokio::sync::Mutex::new(InterfaceManager::new(8)));
    let bridge = InterfaceHotApplyBridge::spawn(iface_manager.clone(), Vec::new());
    let record = udp_peer_record("udp-peer", "127.0.0.1", 0, "127.0.0.1", 4242);

    let applied = bridge.apply_interfaces(vec![record]).expect("apply udp interface");

    let runtime = applied[0]
        .settings
        .as_ref()
        .and_then(|value| value.get("_runtime"))
        .expect("runtime metadata");
    let udp_status =
        runtime.get("udp").and_then(|value| value.get("status")).expect("udp runtime status");
    assert_eq!(udp_status.get("role").and_then(|value| value.as_str()), Some("peer"));
    assert_eq!(udp_status.get("bind_addr").and_then(|value| value.as_str()), Some("127.0.0.1:0"));
    assert_eq!(
        udp_status.get("forward_addr").and_then(|value| value.as_str()),
        Some("127.0.0.1:4242")
    );
}

#[tokio::test(flavor = "current_thread")]
async fn hot_apply_replaces_udp_when_bind_changes() {
    let iface_manager = Arc::new(tokio::sync::Mutex::new(InterfaceManager::new(8)));
    let mut managed = HashMap::new();

    let refreshes = udp_refreshes();
    apply_hot_apply_interface_records(
        &iface_manager,
        &mut managed,
        vec![udp_record("udp-loopback", "127.0.0.1", 0)],
        &refreshes,
        None,
    )
    .await;
    let first = managed.get("udp-loopback").expect("first udp").address;
    apply_hot_apply_interface_records(
        &iface_manager,
        &mut managed,
        vec![udp_record("udp-loopback", "127.0.0.2", 0)],
        &refreshes,
        None,
    )
    .await;

    let second = managed.get("udp-loopback").expect("second udp").address;
    assert_ne!(first, second);
    let manager = iface_manager.lock().await;
    assert_eq!(manager.role(&first), None);
    assert_eq!(manager.role(&second), Some(IfaceRole::Unicast));
}

#[tokio::test(flavor = "current_thread")]
async fn hot_apply_replaces_startup_seeded_udp_when_bind_changes() {
    let iface_manager = Arc::new(tokio::sync::Mutex::new(InterfaceManager::new(8)));
    let first = {
        let mut manager = iface_manager.lock().await;
        let channel =
            manager.new_channel_with_role_and_mode(8, IfaceRole::Unicast, InterfaceMode::Full);
        channel.address
    };
    let mut managed = HashMap::from([(
        "udp-loopback".to_string(),
        ManagedHotApplyInterface {
            record: udp_record("udp-loopback", "127.0.0.1", 4242),
            address: first,
        },
    )]);

    let refreshes = udp_refreshes();
    apply_hot_apply_interface_records(
        &iface_manager,
        &mut managed,
        vec![udp_record("udp-loopback", "127.0.0.2", 0)],
        &refreshes,
        None,
    )
    .await;

    let second = managed.get("udp-loopback").expect("replacement udp").address;
    assert_ne!(first, second);
    let manager = iface_manager.lock().await;
    assert_eq!(manager.role(&first), None);
    assert_eq!(manager.role(&second), Some(IfaceRole::Unicast));
}

#[tokio::test(flavor = "current_thread")]
async fn hot_apply_removes_startup_seeded_udp_when_disabled() {
    let iface_manager = Arc::new(tokio::sync::Mutex::new(InterfaceManager::new(8)));
    let address = {
        let mut manager = iface_manager.lock().await;
        let channel =
            manager.new_channel_with_role_and_mode(8, IfaceRole::Unicast, InterfaceMode::Full);
        channel.address
    };
    let mut managed = HashMap::from([(
        "udp-loopback".to_string(),
        ManagedHotApplyInterface { record: udp_record("udp-loopback", "127.0.0.1", 4242), address },
    )]);
    let mut disabled = udp_record("udp-loopback", "127.0.0.1", 4242);
    disabled.enabled = false;

    let refreshes = udp_refreshes();
    apply_hot_apply_interface_records(
        &iface_manager,
        &mut managed,
        vec![disabled],
        &refreshes,
        None,
    )
    .await;

    assert!(managed.is_empty());
    let manager = iface_manager.lock().await;
    assert_eq!(manager.role(&address), None);
}

#[test]
fn hot_apply_rejects_multicast_udp_records() {
    let (tx, _rx) = tokio::sync::mpsc::channel(1);
    let bridge = test_bridge(tx);

    let err = bridge
        .apply_interfaces(vec![udp_record("udp-mcast", "239.255.0.1", 4242)])
        .expect_err("multicast udp hot apply should be rejected");

    assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
    assert!(err.to_string().contains("multicast"));
}

#[test]
fn hot_apply_rejects_partial_udp_forward_target() {
    let (tx, _rx) = tokio::sync::mpsc::channel(1);
    let bridge = test_bridge(tx);
    let mut record = udp_record("udp-partial", "127.0.0.1", 4242);
    record.settings = Some(json!({ "target_host": "127.0.0.1" }));

    let err = bridge.apply_interfaces(vec![record]).expect_err("partial target should fail");

    assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
    assert!(err.to_string().contains("target_host and target_port"));
}

#[test]
fn hot_apply_rejects_device_bound_udp_records() {
    let (tx, _rx) = tokio::sync::mpsc::channel(1);
    let bridge = test_bridge(tx);
    let mut record = udp_record("udp-device", "127.0.0.1", 4242);
    record.settings = Some(json!({ "device": "eth0" }));

    let err =
        bridge.apply_interfaces(vec![record]).expect_err("device-bound udp should require restart");

    assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
    assert!(err.to_string().contains("device-bound"));
}

#[test]
fn hot_apply_rejects_duplicate_udp_bind_addresses() {
    let (tx, _rx) = tokio::sync::mpsc::channel(1);
    let bridge = test_bridge(tx);

    let err = bridge
        .apply_interfaces(vec![
            udp_record("udp-a", "127.0.0.1", 4242),
            udp_record("udp-b", "127.0.0.1", 4242),
        ])
        .expect_err("duplicate udp binds should fail before queueing");

    assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
    assert!(err.to_string().contains("duplicate udp bind address"));
}

#[test]
fn hot_apply_queue_is_bounded_and_reports_pressure() {
    let (tx, _rx) = tokio::sync::mpsc::channel(1);
    let bridge = test_bridge(tx);

    bridge
        .apply_interfaces(vec![tcp_record("first", "127.0.0.1", 1)])
        .expect("first command fits bounded queue");
    let err = bridge
        .apply_interfaces(vec![tcp_record("second", "127.0.0.1", 2)])
        .expect_err("second command should hit queue capacity");

    assert_eq!(err.kind(), io::ErrorKind::WouldBlock);
    assert!(err.to_string().contains("interface mutation queue is full"));
}
