use super::tests::{named_active, transport};
use super::*;
use tokio::net::TcpListener;
use tokio::time::timeout;

fn discovery_for_recipes(root: &std::path::Path, recipes: &[InterfaceConfig]) -> NativeDiscovery {
    NativeDiscovery::new(
        NativeDiscoveryConfig::new(
            root.to_owned(),
            &ReticulumRuntimePolicy {
                discover_interfaces: true,
                max_autoconnected_interfaces: 1,
                ..Default::default()
            },
            false,
            recipes,
            (3600, 5, 0),
        )
        .expect("discovery"),
    )
}

#[tokio::test]
async fn restore_preserves_explicit_degraded_and_validation_only_outcomes() {
    let root = tempfile::tempdir().expect("state");
    let transport = transport(false, false);
    let startup = ConfiguredStartupRuntime::for_test(&transport, root.path(), &[], false).await;
    for (index, recipe, expected, has_native) in [
        (
            3,
            InterfaceConfig {
                kind: "reticulum_ble".into(),
                enabled: Some(true),
                ..Default::default()
            },
            "degraded",
            true,
        ),
        (
            4,
            InterfaceConfig {
                kind: "lora".into(),
                enabled: Some(true),
                rnode_profile: true,
                ..Default::default()
            },
            "",
            false,
        ),
    ] {
        let (root, failure) = startup.restore(index, &recipe, &transport).await;
        assert!(failure.is_none(), "{failure:?}");
        let status = &root.record.settings.as_ref().expect("runtime")["_runtime"];
        if has_native {
            assert_eq!(status["runtime_status"], expected);
        } else {
            assert_eq!(status["startup_status"], "validated_startup_only");
            assert!(status["runtime_status"].is_null());
            assert!(status["managed_by"].is_null());
        }
        assert_eq!(root.native.is_some(), has_native);
        if let Some(native) = root.native {
            stop_owned_native(&transport.iface_manager(), Some(&transport), native)
                .await
                .expect("joined root");
        }
    }
    let recipe = InterfaceConfig {
        kind: "reticulum_ble".into(),
        enabled: Some(true),
        bootstrap_only: Some(true),
        mtu: Some(1),
        ..Default::default()
    };
    let daemon = Arc::new(RpcDaemon::test_instance());
    daemon.replace_interfaces(vec![crate::bootstrap::interface_record_from_config(&recipe)]);
    let mut runtime = discovery_for_recipes(root.path(), &[recipe]);
    let mut managed = HashMap::new();
    runtime
        .monitor(
            &transport.iface_manager(),
            &mut managed,
            &transport,
            &HotApplyRuntimeRefreshes::new(),
            &daemon,
            Some(&startup),
        )
        .await;
    let records = daemon.interface_records();
    let status = &records[0].settings.as_ref().expect("failed runtime")["_runtime"];
    assert_eq!(status["startup_status"], "failed");
    assert_eq!(status["runtime_status"], "degraded");
    assert!(status["iface"].is_null());
    assert!(managed.values().all(|entry| entry.native.is_none()));
    timeout(Duration::from_secs(2), startup.into_reconnect_completion())
        .await
        .expect("reconnect deadline")
        .expect("reconnect joined");
    assert!(transport.interface_hashes().await.is_empty());
}

#[tokio::test]
async fn restored_control_is_published_before_a_following_sam_preparation_can_wait() {
    use tokio::io::AsyncBufReadExt;
    let root = tempfile::tempdir().expect("state");
    let sam = TcpListener::bind("127.0.0.1:0").await.expect("test-owned SAM endpoint");
    let recipes = [
        InterfaceConfig {
            kind: "weave".into(),
            enabled: Some(true),
            bootstrap_only: Some(true),
            name: Some("restored-weave".into()),
            device: Some(root.path().join("absent-software-device").to_string_lossy().into_owned()),
            baud_rate: Some(115200),
            ..Default::default()
        },
        InterfaceConfig {
            kind: "i2p".into(),
            enabled: Some(true),
            bootstrap_only: Some(true),
            name: Some("held-sam".into()),
            sam_host: Some("127.0.0.1".into()),
            sam_port: Some(sam.local_addr().expect("SAM endpoint").port()),
            connectable: Some(true),
            ..Default::default()
        },
    ];
    let transport = transport(false, false);
    let manager = transport.iface_manager();
    let startup = ConfiguredStartupRuntime::for_test(&transport, root.path(), &[], false).await;
    let daemon = Arc::new(RpcDaemon::test_instance());
    daemon.replace_interfaces(
        recipes.iter().map(crate::bootstrap::interface_record_from_config).collect(),
    );
    let mut runtime = discovery_for_recipes(root.path(), &recipes);
    let mut managed = HashMap::new();
    let refreshes = HotApplyRuntimeRefreshes::new();
    let restore =
        runtime.monitor(&manager, &mut managed, &transport, &refreshes, &daemon, Some(&startup));
    let observe = async {
        let (mut stream, _) = timeout(Duration::from_secs(1), sam.accept())
            .await
            .expect("SAM preparation barrier")
            .expect("SAM connection");
        let mut hello = String::new();
        timeout(
            Duration::from_secs(1),
            tokio::io::BufReader::new(&mut stream).read_line(&mut hello),
        )
        .await
        .expect("SAM greeting deadline")
        .expect("SAM greeting");
        assert!(hello.starts_with("HELLO"));
        assert!(daemon.interface_records()[0].settings.as_ref().expect("published root")
            ["_runtime"]["iface"]
            .is_string());
        let response = daemon.handle_rpc(rns_rpc::RpcRequest {
            id: 1, method: "weave_remote_display_control".into(),
            params: Some(serde_json::json!({"iface":"restored-weave", "enable":true,"remote_switch_id_hex":"10203040"})),
        }).expect("native control RPC");
        assert!(response.error.is_none(), "first call sees retained native control: {response:?}");
        drop(stream);
    };
    tokio::join!(restore, observe);
    for (_, entry) in managed.drain() {
        if let Some(native) = entry.native {
            stop_owned_native(&manager, Some(&transport), native).await.expect("joined root");
        }
    }
    record_publication::publish_control_bindings(&managed, &daemon);
    timeout(Duration::from_secs(2), startup.into_reconnect_completion())
        .await
        .expect("reconnect deadline")
        .expect("reconnect joined");
    assert!(manager.lock().await.interface_hashes().is_empty());
}

#[tokio::test]
async fn failed_selected_listener_restore_retires_the_attempt_and_retries_the_original_recipe() {
    let root = tempfile::tempdir().expect("state");
    let reservation = TcpListener::bind("127.0.0.1:0").await.expect("port");
    let endpoint = reservation.local_addr().expect("endpoint");
    drop(reservation);
    let recipe = InterfaceConfig {
        kind: "tcp_server".into(),
        name: Some("retry-listener".into()),
        host: Some(endpoint.ip().to_string()),
        port: Some(endpoint.port()),
        enabled: Some(true),
        bootstrap_only: Some(true),
        ..Default::default()
    };
    let sentinel =
        InterfaceConfig { kind: "udp".into(), enabled: Some(false), ..Default::default() };
    let recipes = [sentinel.clone(), sentinel, recipe];
    let transport = transport(false, false);
    let manager = transport.iface_manager();
    let startup = ConfiguredStartupRuntime::for_test(&transport, root.path(), &recipes, true).await;
    let daemon = Arc::new(RpcDaemon::test_instance());
    daemon.replace_interfaces(
        recipes.iter().map(crate::bootstrap::interface_record_from_config).collect(),
    );
    let mut runtime = NativeDiscovery::new(
        NativeDiscoveryConfig::new(
            root.path().to_owned(),
            &ReticulumRuntimePolicy {
                discover_interfaces: true,
                max_autoconnected_interfaces: 1,
                ..Default::default()
            },
            false,
            &recipes,
            (3600, 5, 0),
        )
        .expect("discovery"),
    );
    let refreshes = HotApplyRuntimeRefreshes::new();
    let mut managed = HashMap::new();
    runtime.monitor(&manager, &mut managed, &transport, &refreshes, &daemon, Some(&startup)).await;
    let first =
        named_active(&managed, "retry-listener").native.as_ref().expect("first native").address;
    let first_peer = TcpListener::bind(endpoint).await.expect_err("first root is bound");
    assert_eq!(first_peer.kind(), io::ErrorKind::AddrInUse);
    assert_eq!(
        named_active(&managed, "retry-listener").configured.as_ref().expect("binding").recipe_index,
        2
    );

    let auto = manager.lock().await.new_channel(1);
    auto.set_online(true);
    let auto_key = ManagedInterfaceKey::Discovered([9; 32]);
    managed.insert(
        auto_key.clone(),
        ManagedHotApplyInterface::active(
            InterfaceRecord {
                kind: discovered_carrier_kind().into(),
                name: Some("replacement".into()),
                enabled: true,
                host: None,
                port: None,
                settings: None,
            },
            *auto.address(),
            None,
            ManagedInterfaceOrigin::Discovered {
                endpoint: [9; 32],
                source: "network".into(),
                down_since: None,
            },
        ),
    );
    runtime.lifecycle.monitor_interface(auto_key.monitor_id());
    runtime.monitor(&manager, &mut managed, &transport, &refreshes, &daemon, Some(&startup)).await;
    assert!(named_active(&managed, "retry-listener").native.is_none());
    assert_eq!(manager.lock().await.online(&first), None);

    let occupied = TcpListener::bind(endpoint).await.expect("retired endpoint released");
    auto.set_online(false);
    runtime.monitor(&manager, &mut managed, &transport, &refreshes, &daemon, Some(&startup)).await;
    let failed = named_active(&managed, "retry-listener");
    assert!(failed.native.is_none(), "known readiness failure cannot pin a stale native root");
    let record = &daemon.interface_records()[2];
    let status = &record.settings.as_ref().expect("runtime")["_runtime"];
    assert_eq!(status["startup_status"], "failed");
    assert_eq!(status["runtime_status"], "failed");
    assert!(status["iface"].is_null());
    assert_eq!(
        manager.lock().await.interface_hashes(),
        std::collections::HashSet::from([*auto.address()])
    );
    assert_eq!(failed.configured.as_ref().expect("original binding").recipe_index, 2);
    assert_eq!(failed.configured.as_ref().expect("current slot").position, 2);

    drop(occupied);
    runtime.monitor(&manager, &mut managed, &transport, &refreshes, &daemon, Some(&startup)).await;
    let restored =
        named_active(&managed, "retry-listener").native.as_ref().expect("new native after failure");
    assert_ne!(restored.address, first);
    assert!(TcpListener::bind(endpoint).await.is_err());
    let client = timeout(Duration::from_secs(2), tokio::net::TcpStream::connect(endpoint))
        .await
        .expect("new listener deadline")
        .expect("new listener accepts");
    drop(client);
    for (_, active) in managed.drain() {
        if let Some(native) = active.native {
            stop_owned_native(&manager, Some(&transport), native).await.expect("joined native");
        }
    }
    timeout(Duration::from_secs(2), startup.into_reconnect_completion())
        .await
        .expect("reconnect deadline")
        .expect("joined reconnect");
    assert!(manager.lock().await.interface_hashes().is_empty());
}
