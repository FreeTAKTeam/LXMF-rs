use super::*;
use rns_rpc::InterfaceMutationBridge;
use serde_json::json;
use tokio::io::AsyncReadExt;
use tokio::net::{TcpListener, TcpSocket};
use tokio::time::{timeout, Duration};

fn record(port: u16) -> InterfaceRecord {
    InterfaceRecord {
        kind: "tcp_client".into(),
        enabled: true,
        name: Some("client-options".into()),
        host: Some("127.0.0.1".into()),
        port: Some(port),
        settings: Some(json!({"connect_timeout":1,"max_reconnect_tries":0,"mtu":512})),
    }
}

#[tokio::test]
async fn hot_applied_tcp_client_honors_zero_reconnect_budget_on_native_refusal() {
    let reserved = TcpSocket::new_v4().expect("unlistened socket");
    reserved.bind("127.0.0.1:0".parse().expect("loopback")).expect("reserved endpoint");
    let manager = Arc::new(tokio::sync::Mutex::new(InterfaceManager::new(8)));
    let (address, status) = spawn_hot_apply_interface(
        NativeInterfaceContext { iface_manager: &manager, transport: None, reconnect_events: None },
        &record(reserved.local_addr().expect("endpoint").port()),
    )
    .await
    .expect("native client");
    let Some(HotApplyRuntimeStatus::TcpClient(status)) = status else {
        panic!("TCP status");
    };
    timeout(Duration::from_secs(2), async {
        while status.to_json()["stream_state"] != "closed" {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("zero-budget native worker stops after the first refused connection");
    assert_eq!(status.to_json()["mtu"], 512);
    native_shutdown::stop_and_join(&manager, None, address).await.expect("joined exhausted client");
    assert!(manager.lock().await.interface_hashes().is_empty());
}

#[tokio::test]
async fn changing_only_tcp_client_options_replaces_the_native_worker() {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("peer");
    let manager = Arc::new(tokio::sync::Mutex::new(InterfaceManager::new(8)));
    let mut managed = HashMap::new();
    let refreshes = HotApplyRuntimeRefreshes::new();
    let mut initial = record(listener.local_addr().expect("endpoint").port());
    apply_hot_apply_interface_records(
        NativeInterfaceContext { iface_manager: &manager, transport: None, reconnect_events: None },
        &mut managed,
        vec![initial.clone()],
        &refreshes,
        None,
    )
    .await;
    let (mut previous_peer, _) = timeout(Duration::from_secs(2), listener.accept())
        .await
        .expect("first deadline")
        .expect("first native");
    let mut previous = managed
        .get(&configured_key("client-options"))
        .expect("first")
        .native
        .as_ref()
        .expect("native")
        .address;
    let unrelated = manager.lock().await.new_channel(1);
    for (key, value) in [
        ("mtu", json!(1024)),
        ("connect_timeout", json!(2)),
        ("max_reconnect_tries", json!(1)),
        ("prefer_ipv6", json!(true)),
        ("i2p_tunneled", json!(true)),
    ] {
        initial.settings.as_mut().expect("options")[key] = value;
        apply_hot_apply_interface_records(
            NativeInterfaceContext {
                iface_manager: &manager,
                transport: None,
                reconnect_events: None,
            },
            &mut managed,
            vec![initial.clone()],
            &refreshes,
            None,
        )
        .await;
        let native = managed
            .get(&configured_key("client-options"))
            .expect("replacement")
            .native
            .as_ref()
            .expect("native");
        assert_ne!(native.address, previous, "{key} alone requires a new constructor");
        let Some(HotApplyRuntimeStatus::TcpClient(status)) = &native.status else {
            panic!("TCP status");
        };
        assert_eq!(status.to_json()["mtu"], 1024);
        let (next_peer, _) = timeout(Duration::from_secs(2), listener.accept())
            .await
            .expect("replacement deadline")
            .expect("new native connection");
        assert_eq!(manager.lock().await.online(&previous), None);
        assert_eq!(
            timeout(Duration::from_secs(2), previous_peer.read(&mut [0u8; 1]))
                .await
                .expect("old connection retirement")
                .expect("old connection EOF"),
            0
        );
        assert!(manager.lock().await.interface_hashes().contains(unrelated.address()));
        previous = native.address;
        previous_peer = next_peer;
    }
    for (_, active) in managed.drain() {
        native_shutdown::stop_owned_native(&manager, None, active.native.expect("native"))
            .await
            .expect("joined replacement");
    }
    assert_eq!(
        manager.lock().await.interface_hashes(),
        std::collections::HashSet::from([*unrelated.address()])
    );
}

#[tokio::test]
async fn malformed_tcp_client_constructor_options_reject_before_queueing() {
    let manager = Arc::new(tokio::sync::Mutex::new(InterfaceManager::new(8)));
    let bridge = InterfaceHotApplyBridge::spawn(manager, Vec::new());
    for (key, value) in [
        ("connect_timeout", json!(0)),
        ("mtu", json!(0)),
        ("max_reconnect_tries", json!("do-not-log-this-option-value")),
        ("prefer_ipv6", json!(1)),
        ("i2p_tunneled", json!("true")),
    ] {
        let mut invalid = record(4242);
        invalid.settings.as_mut().expect("options")[key] = value;
        let error = bridge
            .apply_interfaces(vec![invalid])
            .expect_err("invalid option cannot be acknowledged or replace a live root");
        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
        assert!(!error.to_string().contains("do-not-log-this-option-value"));
    }
}
