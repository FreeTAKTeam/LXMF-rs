use super::tests::{discovery, has_named, info, named_active, transport};
use super::*;
use rns_rpc::RpcRequest;
use serde_json::json;
use tokio::net::{TcpListener, UdpSocket};
use tokio::time::timeout;

#[tokio::test]
async fn restored_tcp_bootstrap_publishes_its_live_retained_status() {
    let root = tempfile::tempdir().expect("root");
    let listener = TcpListener::bind("127.0.0.2:0").await.expect("peer");
    let recipe = InterfaceConfig {
        kind: "tcp_client".into(),
        name: Some("restored-tcp".into()),
        enabled: Some(true),
        host: Some("127.0.0.2".into()),
        port: Some(listener.local_addr().expect("port").port()),
        bootstrap_only: Some(true),
        ..Default::default()
    };
    let transport = transport(false, false);
    let manager = transport.iface_manager();
    let daemon = Arc::new(RpcDaemon::test_instance());
    let mut runtime = NativeDiscovery::new(
        NativeDiscoveryConfig::new(
            root.path().to_owned(),
            &ReticulumRuntimePolicy {
                discover_interfaces: true,
                max_autoconnected_interfaces: 1,
                ..Default::default()
            },
            false,
            &[recipe],
            (3600, 5, 0),
        )
        .expect("runtime"),
    );
    let refreshes = HotApplyRuntimeRefreshes::new();
    let mut managed = HashMap::new();
    runtime.monitor(&manager, &mut managed, &transport, &refreshes, &daemon).await;
    let (_peer, _) = timeout(Duration::from_secs(2), listener.accept())
        .await
        .expect("restored connection deadline")
        .expect("restored connection");
    let Some(HotApplyRuntimeStatus::TcpClient(status)) =
        &named_active(&managed, "restored-tcp").native.as_ref().expect("active native root").status
    else {
        panic!("retained TCP status");
    };
    timeout(Duration::from_secs(2), async {
        while status.to_json()["stream_state"] != "connected" {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("native client connected status");
    runtime.monitor(&manager, &mut managed, &transport, &refreshes, &daemon).await;
    assert_eq!(
        daemon.interface_records()[0].settings.as_ref().expect("status")["_runtime"]["tcp"]
            ["stream_status"]["stream_state"],
        "connected",
        "restored TCP status comes from its retained native handle"
    );
    for (_, active) in managed.drain() {
        stop_and_join(
            &manager,
            Some(&transport),
            active.native.as_ref().expect("active native root").address,
        )
        .await
        .expect("stop");
    }
}

#[tokio::test]
async fn discovery_literal_names_cannot_replace_hidden_keys_or_trimmed_public_records() {
    for configured_name in [None, Some("victim".to_owned())] {
        let root = tempfile::tempdir().expect("root");
        let configured_peer = TcpListener::bind("127.0.0.2:0").await.expect("configured peer");
        let discovered_peer = TcpListener::bind("127.0.0.2:0").await.expect("discovered peer");
        let record = InterfaceRecord {
            kind: "tcp_client".into(),
            enabled: true,
            name: configured_name.clone(),
            host: Some("127.0.0.2".into()),
            port: Some(configured_peer.local_addr().expect("port").port()),
            settings: None,
        };
        let key = hot_apply_interface_seed_key(&record).expect("configured key");
        let transport = transport(false, false);
        let manager = transport.iface_manager();
        let daemon = Arc::new(RpcDaemon::test_instance());
        daemon.replace_interfaces(vec![record.clone()]);
        let (address, runtime_status) =
            spawn_hot_apply_interface(&manager, Some(&transport), &record)
                .await
                .expect("configured worker");
        let (_configured_connection, _) = timeout(Duration::from_secs(2), configured_peer.accept())
            .await
            .expect("configured connection deadline")
            .expect("configured connection");
        let name = if configured_name.is_some() { " victim ".to_owned() } else { key.clone() };
        let candidate =
            info(&name, "127.0.0.2", discovered_peer.local_addr().expect("port").port());
        let mut managed = HashMap::from([(
            key.into(),
            ManagedHotApplyInterface::active(
                record.clone(),
                address,
                runtime_status,
                ManagedInterfaceOrigin::Configured,
            ),
        )]);
        let mut runtime = discovery(root.path(), 1, false);
        runtime.admit(&candidate, &manager, &mut managed, &transport, &daemon).await;
        let (_discovered_connection, _) = timeout(Duration::from_secs(2), discovered_peer.accept())
            .await
            .expect("discovered connection deadline")
            .expect("discovered connection");
        assert_eq!(
            managed.len(),
            2,
            "literal display name cannot replace a different private owner key"
        );
        assert!(managed.values().any(|active| active
            .native
            .as_ref()
            .expect("active native root")
            .address
            == address
            && active.origin.configuration_owned()));
        let records = daemon.interface_records();
        assert_eq!(
            records.len(),
            2,
            "trimmed key comparison cannot merge unrelated public records"
        );
        assert!(records.iter().any(|current| current.kind == record.kind
            && current.name == record.name
            && current.port == record.port));
        assert!(records.iter().any(|current| current.name.as_deref() == Some(name.as_str())
            && current.port == candidate.port));
        for (_, active) in managed.drain() {
            stop_and_join(
                &manager,
                Some(&transport),
                active.native.as_ref().expect("active native root").address,
            )
            .await
            .expect("stop");
        }
    }
}

#[tokio::test]
async fn bootstrap_discovery_creates_transport_without_an_existing_carrier_and_wires_override() {
    for (implementation, extra) in
        [("RNS", ""), ("reticulumd", "autoconnect_unverified_implementations=true\n")]
    {
        let root = tempfile::tempdir().expect("root");
        let listener = TcpListener::bind("127.0.0.2:0").await.expect("native peer");
        let mut candidate =
            info("persisted-bootstrap", "127.0.0.2", listener.local_addr().expect("port").port());
        candidate.impl_name = Some(implementation.into());
        InterfaceDiscoveryStore::new(root.path()).observe(candidate).expect("retained entry");
        let path = root.path().join("config.toml");
        std::fs::write(&path, format!("[reticulum]\nshare_instance=false\ndiscover_interfaces=true\nautoconnect_discovered_interfaces=1\n{extra}")).expect("config");
        let db = root.path().join("state.db");
        let rpc = root.path().join("rpc.sock");
        let args = <crate::Args as clap::Parser>::parse_from([
            "reticulumd",
            "--db",
            db.to_str().expect("db"),
            "--config",
            path.to_str().expect("config"),
            "--rpc-unix",
            rpc.to_str().expect("rpc"),
        ]);
        let context = crate::bootstrap::bootstrap(args).await;
        let (_peer, _) = timeout(Duration::from_secs(2), listener.accept())
            .await
            .expect("discovery-only native transport connection")
            .expect("connection");
        let records = context.daemon.interface_records();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].name.as_deref(), Some("persisted-bootstrap"));
        assert!(records[0].settings.as_ref().expect("status")["_runtime"]["discovery"]
            ["endpoint_hash"]
            .is_string());
    }
}

#[tokio::test]
async fn bootstrap_backbone_seed_prevents_initial_and_periodic_duplicate_connections() {
    let root = tempfile::tempdir().expect("root");
    let listener = TcpListener::bind("127.0.0.2:0").await.expect("native peer");
    let port = listener.local_addr().expect("port").port();
    InterfaceDiscoveryStore::new(root.path())
        .observe(info("discovery-copy", "127.0.0.2", port))
        .expect("retained candidate");
    let path = root.path().join("config.toml");
    std::fs::write(&path, format!("[reticulum]\nshare_instance=false\ndiscover_interfaces=true\nautoconnect_discovered_interfaces=3\n[[interfaces]]\ntype='backbone_client'\nname='configured-backbone'\nhost='127.0.0.2'\nport={port}\nenabled=true\nbootstrap_only=true\n")).expect("config");
    let db = root.path().join("state.db");
    let rpc = root.path().join("rpc.sock");
    let args = <crate::Args as clap::Parser>::parse_from([
        "reticulumd",
        "--db",
        db.to_str().expect("db"),
        "--config",
        path.to_str().expect("config"),
        "--rpc-unix",
        rpc.to_str().expect("rpc"),
    ]);
    let context = crate::bootstrap::bootstrap(args).await;
    let (_peer, _) = timeout(Duration::from_secs(2), listener.accept())
        .await
        .expect("configured connection")
        .expect("peer");
    let before = context.daemon.interface_records();
    assert_eq!(before.len(), 1);
    let address = before[0].settings.as_ref().expect("status")["_runtime"]["iface"].clone();
    tokio::time::pause();
    tokio::time::advance(Duration::from_secs(5)).await;
    for _ in 0..20 {
        tokio::task::yield_now().await;
    }
    assert!(
        timeout(Duration::from_millis(50), listener.accept()).await.is_err(),
        "trusted startup endpoint/bootstrap reservation prevents a duplicate native worker"
    );
    let after = context.daemon.interface_records();
    assert_eq!(after.len(), 1);
    assert_eq!(after[0].settings.as_ref().expect("status")["_runtime"]["iface"], address);
}

#[tokio::test(start_paused = true)]
async fn discovery_stop_joins_every_worker_even_when_the_first_worker_times_out() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    struct Stub;
    impl rns_transport::iface::Interface for Stub {
        fn mtu() -> usize {
            500
        }
    }
    struct OnDrop(Arc<AtomicUsize>);
    impl Drop for OnDrop {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }
    let dropped = Arc::new(AtomicUsize::new(0));
    let manager = Arc::new(tokio::sync::Mutex::new(InterfaceManager::new(4)));
    let worker = |counter: Arc<AtomicUsize>| {
        move |context| async move {
            let _context: rns_transport::iface::InterfaceContext<Stub> = context;
            let _drop = OnDrop(counter);
            std::future::pending::<()>().await;
        }
    };
    let address = manager.lock().await.spawn(Stub, worker(dropped.clone()));
    for _ in 0..2 {
        manager
            .lock()
            .await
            .spawn_inheriting(address, Stub, worker(dropped.clone()))
            .expect("child worker");
    }
    tokio::task::yield_now().await;
    let record = InterfaceRecord {
        kind: discovered_carrier_kind().into(),
        enabled: true,
        name: Some("uncooperative".into()),
        host: None,
        port: None,
        settings: None,
    };
    let daemon = Arc::new(RpcDaemon::test_instance());
    daemon.replace_interfaces(vec![record.clone()]);
    let mut managed = HashMap::from([(
        "uncooperative".into(),
        ManagedHotApplyInterface::active(
            record,
            address,
            None,
            ManagedInterfaceOrigin::Discovered {
                endpoint: [3; 32],
                source: "network".into(),
                down_since: None,
            },
        ),
    )]);
    let result = detach_discovered(
        &manager,
        &mut managed,
        "uncooperative",
        None,
        Some(&Arc::downgrade(&daemon)),
    )
    .await
    .expect_err("uncooperative workers must report timeout");
    assert_eq!(result.kind(), io::ErrorKind::TimedOut);
    assert_eq!(
        dropped.load(Ordering::SeqCst),
        3,
        "all owned worker futures must be retired before returning"
    );
    assert!(manager.lock().await.interface_hashes().is_empty());
    assert!(managed.is_empty());
    assert!(
        daemon.interface_records().is_empty(),
        "native absence must be published despite stop diagnostics"
    );
}

#[tokio::test]
async fn restored_bootstrap_binding_failure_uses_the_existing_status_refresh_owner() {
    let root = tempfile::tempdir().expect("root");
    let occupied = UdpSocket::bind("127.0.0.1:0").await.expect("occupied endpoint");
    let port = occupied.local_addr().expect("port").port();
    let recipe = InterfaceConfig {
        kind: "udp".into(),
        name: Some("bootstrap-status".into()),
        enabled: Some(true),
        host: Some("127.0.0.1".into()),
        port: Some(port),
        bootstrap_only: Some(true),
        ..Default::default()
    };
    let transport = transport(false, false);
    let manager = transport.iface_manager();
    let daemon = Arc::new(RpcDaemon::test_instance());
    let mut runtime = NativeDiscovery::new(
        NativeDiscoveryConfig::new(
            root.path().to_owned(),
            &ReticulumRuntimePolicy {
                discover_interfaces: true,
                max_autoconnected_interfaces: 1,
                ..Default::default()
            },
            false,
            &[recipe],
            (3600, 5, 0),
        )
        .expect("runtime"),
    );
    let mut managed = HashMap::new();
    let refreshes = HotApplyRuntimeRefreshes::new();
    runtime.monitor(&manager, &mut managed, &transport, &refreshes, &daemon).await;
    let Some(HotApplyRuntimeStatus::Udp(status)) = &named_active(&managed, "bootstrap-status")
        .native
        .as_ref()
        .expect("active native root")
        .status
    else {
        panic!("UDP status");
    };
    timeout(Duration::from_secs(2), async {
        while status.snapshot().link_state != "bind_failed" {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("native bind failure");
    assert_eq!(
        refresh_hot_apply_udp_runtime_status_once(&daemon, &refreshes.udp),
        1,
        "restored worker is registered with its existing refresher"
    );
    assert_eq!(
        daemon.interface_records()[0].settings.as_ref().expect("status")["_runtime"]["udp"]
            ["status"]["link_state"],
        "bind_failed"
    );
    for (_, active) in managed.drain() {
        stop_and_join(
            &manager,
            Some(&transport),
            active.native.as_ref().expect("active native root").address,
        )
        .await
        .expect("joined failed worker");
    }
}

#[test]
fn discovery_publication_preserves_unrelated_seeded_native_status() {
    let daemon = RpcDaemon::test_instance();
    let address = AddressHash::new_from_slice(&[42; 16]);
    let seed = InterfaceRecord {
        kind: "tcp_client".into(),
        enabled: true,
        name: Some("existing".into()),
        host: Some("127.0.0.2".into()),
        port: Some(4242),
        settings: Some(json!({"_runtime": {"iface": address.to_string()}})),
    };
    let mut current = seed.clone();
    current.settings.as_mut().expect("settings")["_runtime"]["tcp"] =
        json!({"stream_status": {"stream_state": "connected", "bytes_rx": 777}});
    daemon.replace_interfaces(vec![current.clone()]);
    let managed = HashMap::from([(
        "existing".into(),
        ManagedHotApplyInterface::active(seed, address, None, ManagedInterfaceOrigin::Configured),
    )]);
    publish_live_runtime_records(&managed, &daemon);
    assert_eq!(
        daemon.interface_records()[0],
        current,
        "discovery publication cannot roll back another native status owner's snapshot"
    );
}

#[tokio::test(start_paused = true)]
async fn discovery_down_time_is_anchored_after_a_contended_native_snapshot() {
    let root = tempfile::tempdir().expect("root");
    let transport = transport(false, false);
    let manager = transport.iface_manager();
    let daemon = Arc::new(RpcDaemon::test_instance());
    let channel = manager.lock().await.new_channel(1);
    channel.set_online(false);
    let record = InterfaceRecord {
        kind: discovered_carrier_kind().into(),
        enabled: true,
        name: Some("contended".into()),
        host: None,
        port: None,
        settings: None,
    };
    daemon.replace_interfaces(vec![record.clone()]);
    let mut managed = HashMap::from([(
        "contended".into(),
        ManagedHotApplyInterface::active(
            record,
            *channel.address(),
            None,
            ManagedInterfaceOrigin::Discovered {
                endpoint: [4; 32],
                source: "network".into(),
                down_since: None,
            },
        ),
    )]);
    let mut runtime = discovery(root.path(), 1, false);
    runtime.lifecycle.monitor_interface(configured_key("contended").monitor_id());
    let refreshes = HotApplyRuntimeRefreshes::new();
    {
        let guard = manager.lock().await;
        let snapshot = runtime.monitor(&manager, &mut managed, &transport, &refreshes, &daemon);
        tokio::pin!(snapshot);
        assert!(timeout(Duration::from_secs(20), &mut snapshot).await.is_err());
        drop(guard);
        snapshot.await;
    }
    tokio::time::advance(Duration::from_secs(5)).await;
    runtime.monitor(&manager, &mut managed, &transport, &refreshes, &daemon).await;
    assert!(
        has_named(&managed, "contended"),
        "only five seconds since the first actual native observation"
    );
    stop_and_join(&manager, Some(&transport), *channel.address()).await.expect("stop");
}

#[tokio::test]
async fn discovery_bootstrap_lifecycle_uses_unnamed_and_padded_managed_keys() {
    for name in [None, Some(" padded bootstrap ".to_owned())] {
        let root = tempfile::tempdir().expect("root");
        let reservation = UdpSocket::bind("127.0.0.1:0").await.expect("port");
        let port = reservation.local_addr().expect("endpoint").port();
        drop(reservation);
        let recipe = InterfaceConfig {
            kind: "udp".into(),
            name,
            host: Some("127.0.0.1".into()),
            port: Some(port),
            enabled: Some(true),
            bootstrap_only: Some(true),
            ..Default::default()
        };
        let record = crate::bootstrap::interface_record_from_config(&recipe);
        let key = hot_apply_interface_seed_key(&record).expect("existing trusted seed key");
        let transport = transport(false, false);
        let manager = transport.iface_manager();
        let daemon = Arc::new(RpcDaemon::test_instance());
        let (address, runtime_status) =
            spawn_hot_apply_interface(&manager, Some(&transport), &record).await.expect("worker");
        let channel = manager.lock().await.new_channel(1);
        channel.set_online(true);
        let auto = InterfaceRecord {
            kind: discovered_carrier_kind().into(),
            enabled: true,
            name: Some("auto".into()),
            host: None,
            port: None,
            settings: None,
        };
        daemon.replace_interfaces(vec![record.clone(), auto.clone()]);
        let mut managed = HashMap::from([
            (
                key.clone().into(),
                ManagedHotApplyInterface::active(
                    record,
                    address,
                    runtime_status,
                    ManagedInterfaceOrigin::Configured,
                ),
            ),
            (
                "auto".into(),
                ManagedHotApplyInterface::active(
                    auto,
                    *channel.address(),
                    None,
                    ManagedInterfaceOrigin::Discovered {
                        endpoint: [5; 32],
                        source: "network".into(),
                        down_since: None,
                    },
                ),
            ),
        ]);
        let mut runtime = NativeDiscovery::new(
            NativeDiscoveryConfig::new(
                root.path().to_owned(),
                &ReticulumRuntimePolicy {
                    discover_interfaces: true,
                    max_autoconnected_interfaces: 1,
                    ..Default::default()
                },
                false,
                &[recipe],
                (3600, 5, 0),
            )
            .expect("runtime"),
        );
        runtime.lifecycle.monitor_interface(configured_key("auto").monitor_id());
        let refreshes = HotApplyRuntimeRefreshes::new();
        runtime.monitor(&manager, &mut managed, &transport, &refreshes, &daemon).await;
        assert!(
            managed.get(&configured_key(&key)).is_some_and(|entry| entry.native.is_none()),
            "bootstrap disable retains its recipe with explicit inactive native state"
        );
        assert!(!manager.lock().await.interface_hashes().contains(&address));
        channel.set_online(false);
        runtime.monitor(&manager, &mut managed, &transport, &refreshes, &daemon).await;
        assert!(
            managed.contains_key(&configured_key(&key)),
            "trusted unnamed/padded recipe must restore with the same owner key"
        );
        for (_, active) in managed.drain() {
            stop_and_join(
                &manager,
                Some(&transport),
                active.native.as_ref().expect("active native root").address,
            )
            .await
            .expect("stop");
        }
    }
}

async fn barrier(bridge: &InterfaceHotApplyBridge) {
    let (reply, response) = std::sync::mpsc::sync_channel(1);
    bridge
        .tx
        .send(InterfaceHotApplyCommand::Manage {
            operation: "barrier-invalid-operation".into(),
            name: "not-a-managed-name".into(),
            configured: None,
            reply,
        })
        .await
        .expect("queue barrier");
    assert!(tokio::task::spawn_blocking(move || response.recv_timeout(Duration::from_secs(2)))
        .await
        .expect("barrier join")
        .expect("barrier response")
        .is_err());
}

#[tokio::test]
async fn discovery_rechecks_both_blackhole_identities_and_source_allowlist() {
    for blocked in
        ["native-transport", "native-network", "transport", "network", "disallowed-source"]
    {
        let root = tempfile::tempdir().expect("root");
        let transport = transport(false, false);
        let manager = transport.iface_manager();
        let daemon = Arc::new(RpcDaemon::test_instance());
        daemon.set_path_lookup_bridge(Arc::new(
            crate::bridge_path_lookup::DaemonPathLookupBridge::new(transport.clone()),
        ));
        let candidate = info("blocked", "127.0.0.2", 4242);
        let mut runtime = discovery(root.path(), 1, false);
        if blocked == "disallowed-source" {
            runtime.config.allowed_network_ids = vec!["33333333333333333333333333333333".into()];
        } else if blocked.starts_with("native-") {
            let identity = if blocked == "native-transport" {
                &candidate.transport_id
            } else {
                &candidate.network_id
            };
            let identity =
                rns_transport::destination_hash::parse_destination_hash_required(identity)
                    .expect("identity");
            transport.set_identity_blackholed(AddressHash::new(identity), true).await;
            assert!(!daemon
                .is_blackholed(&hex::encode(identity))
                .expect("RPC state is intentionally independent"));
        } else {
            let identity = if blocked == "transport" {
                &candidate.transport_id
            } else {
                &candidate.network_id
            };
            let response = daemon
                .handle_rpc(RpcRequest {
                    id: 1,
                    method: "blackhole_identity".into(),
                    params: Some(json!({"identity": identity})),
                })
                .expect("native blackhole authority");
            assert_eq!(response.result, Some(json!(true)));
        }
        let mut managed = HashMap::new();
        runtime.admit(&candidate, &manager, &mut managed, &transport, &daemon).await;
        assert!(managed.is_empty(), "admission must reject {blocked}");
        assert!(manager.lock().await.interface_hashes().is_empty());
    }
}

#[tokio::test]
async fn native_owner_detach_suppresses_wakeups_until_successful_configured_attach() {
    let root = tempfile::tempdir().expect("root");
    let listener = TcpListener::bind("127.0.0.2:0").await.expect("native peer");
    let port = listener.local_addr().expect("endpoint").port();
    let candidate = info("operator-peer", "127.0.0.2", port);
    let config_path = root.path().join("config.toml");
    let configured = format!("[reticulum]\nshare_instance=false\n[[interfaces]]\ntype='tcp_client'\nname='explicit-attach'\nhost='127.0.0.2'\nport={port}\nenabled=true\n");
    std::fs::write(&config_path, &configured).expect("config");
    let transport = transport(false, false);
    let manager = transport.iface_manager();
    let daemon = Arc::new(RpcDaemon::test_instance());
    let runtime = discovery(root.path(), 2, false);
    let bridge = InterfaceHotApplyBridge::spawn_with_discovery(
        transport.clone(),
        vec![],
        Arc::downgrade(&daemon),
        Some(runtime.config),
    )
    .with_management_config(Some(config_path.clone()), true);
    let observations = bridge.discovery_observations();
    observations.observed(candidate.clone()).await.expect("live observation");
    let (first_peer, _) = timeout(Duration::from_secs(2), listener.accept())
        .await
        .expect("connection deadline")
        .expect("connection");
    barrier(&bridge).await;
    let detach_bridge = bridge.clone();
    let result = tokio::task::spawn_blocking(move || {
        detach_bridge.manage_named_interface("detach", "operator-peer")
    })
    .await
    .expect("management task")
    .expect("native detach completion");
    assert_eq!(result["complete"], true);
    assert!(manager.lock().await.interface_hashes().is_empty());
    drop(first_peer);
    observations.observed(candidate.clone()).await.expect("queued after detach");
    barrier(&bridge).await;
    assert!(
        manager.lock().await.interface_hashes().is_empty(),
        "queued discovery cannot undo operator intent"
    );

    std::fs::write(&config_path, configured.clone() + "ifac_size=16\n")
        .expect("invalid attach config");
    let failed_bridge = bridge.clone();
    assert!(tokio::task::spawn_blocking(
        move || failed_bridge.manage_named_interface("attach", "explicit-attach")
    )
    .await
    .expect("failed attach task")
    .is_err());
    observations.observed(candidate.clone()).await.expect("wake after failure");
    barrier(&bridge).await;
    assert!(
        manager.lock().await.interface_hashes().is_empty(),
        "failed attach preserves suppression"
    );

    std::fs::write(&config_path, configured).expect("valid attach config");
    let attach_bridge = bridge.clone();
    let result = tokio::task::spawn_blocking(move || {
        attach_bridge.manage_named_interface("attach", "explicit-attach")
    })
    .await
    .expect("attach task")
    .expect("native readiness");
    assert_eq!(result["complete"], true);
    let (configured_peer, _) = timeout(Duration::from_secs(2), listener.accept())
        .await
        .expect("configured connection deadline")
        .expect("configured connection");
    let detach_bridge = bridge.clone();
    tokio::task::spawn_blocking(move || {
        detach_bridge.manage_named_interface("detach", "explicit-attach")
    })
    .await
    .expect("configured detach task")
    .expect("configured worker joined");
    drop(configured_peer);
    observations.observed(candidate).await.expect("wake after explicit success");
    let (last_peer, _) = timeout(Duration::from_secs(2), listener.accept())
        .await
        .expect("suppression cleared only after success")
        .expect("discovered connection");
    barrier(&bridge).await;
    assert_eq!(manager.lock().await.interface_hashes().len(), 1);
    drop(bridge);
    timeout(Duration::from_secs(2), async {
        while !manager.lock().await.interface_hashes().is_empty() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("weak observation sender does not keep mutation owner alive");
    drop(last_peer);
}

#[tokio::test]
async fn native_monitor_restores_the_trusted_udp_bootstrap_recipe_after_auto_peer_loss() {
    let root = tempfile::tempdir().expect("root");
    let reservation = UdpSocket::bind("127.0.0.1:0").await.expect("port reservation");
    let addr = reservation.local_addr().expect("endpoint");
    drop(reservation);
    let recipe = InterfaceConfig {
        kind: "udp".into(),
        name: Some("bootstrap".into()),
        enabled: Some(true),
        host: Some(addr.ip().to_string()),
        port: Some(addr.port()),
        bootstrap_only: Some(true),
        ..InterfaceConfig::default()
    };
    let record = crate::bootstrap::interface_record_from_config(&recipe);
    let transport = transport(false, false);
    let manager = transport.iface_manager();
    let daemon = Arc::new(RpcDaemon::test_instance());
    let (address, runtime_status) = spawn_hot_apply_interface(&manager, Some(&transport), &record)
        .await
        .expect("bootstrap worker");
    let channel = manager.lock().await.new_channel(1);
    channel.set_online(true);
    let auto = InterfaceRecord {
        kind: discovered_carrier_kind().into(),
        enabled: true,
        name: Some("auto".into()),
        host: Some("127.0.0.2".into()),
        port: Some(4242),
        settings: None,
    };
    daemon.replace_interfaces(vec![record.clone(), auto.clone()]);
    let mut managed = HashMap::from([
        (
            "bootstrap".into(),
            ManagedHotApplyInterface::active(
                record,
                address,
                runtime_status,
                ManagedInterfaceOrigin::Configured,
            ),
        ),
        (
            "auto".into(),
            ManagedHotApplyInterface::active(
                auto,
                *channel.address(),
                None,
                ManagedInterfaceOrigin::Discovered {
                    endpoint: [8; 32],
                    source: "network".into(),
                    down_since: None,
                },
            ),
        ),
    ]);
    let mut runtime = NativeDiscovery::new(
        NativeDiscoveryConfig::new(
            root.path().to_owned(),
            &ReticulumRuntimePolicy {
                discover_interfaces: true,
                max_autoconnected_interfaces: 1,
                ..Default::default()
            },
            false,
            &[recipe],
            (3600, 5, 0),
        )
        .expect("runtime"),
    );
    runtime.lifecycle.monitor_interface(configured_key("auto").monitor_id());
    let refreshes = HotApplyRuntimeRefreshes::new();
    runtime.monitor(&manager, &mut managed, &transport, &refreshes, &daemon).await;
    assert!(!has_named(&managed, "bootstrap"));
    let reservation =
        UdpSocket::bind(addr).await.expect("bootstrap stop joined and released exact port");
    drop(reservation);
    channel.set_online(false);
    runtime.monitor(&manager, &mut managed, &transport, &refreshes, &daemon).await;
    let restored = &named_active(&managed, "bootstrap");
    assert_ne!(restored.native.as_ref().expect("active native root").address, address);
    assert!(restored.origin.configuration_owned());
    assert_eq!(
        restored.record.settings.as_ref().expect("trusted settings")["bootstrap_only"],
        true
    );
    let Some(HotApplyRuntimeStatus::Udp(status)) =
        &restored.native.as_ref().expect("active native root").status
    else {
        panic!("restored UDP status");
    };
    timeout(Duration::from_secs(2), async {
        while status.snapshot().link_state != "bound" {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("trusted recipe bound exact port");
    assert!(UdpSocket::bind(addr).await.is_err());
    for (_, active) in managed.drain() {
        stop_and_join(
            &manager,
            Some(&transport),
            active.native.as_ref().expect("active native root").address,
        )
        .await
        .expect("stop");
    }
}

#[tokio::test]
async fn periodic_refill_uses_one_available_transport_candidate_and_reserves_a_quarter() {
    let root = tempfile::tempdir().expect("root");
    let transport = transport(false, false);
    let manager = transport.iface_manager();
    let daemon = Arc::new(RpcDaemon::test_instance());
    let mut runtime = discovery(root.path(), 4, false);
    let mut managed = HashMap::new();
    runtime.initial(&manager, &mut managed, &transport, &daemon).await;
    let store = InterfaceDiscoveryStore::new(root.path());
    let mut peers = Vec::new();
    for index in 0..3u8 {
        let peer = TcpListener::bind("127.0.0.2:0").await.expect("native candidate");
        let mut candidate = info(
            &format!("periodic-{index}"),
            "127.0.0.2",
            peer.local_addr().expect("port").port(),
        );
        candidate.discovery_hash = vec![index; 32];
        store.observe(candidate).expect("available transport candidate");
        peers.push(peer);
    }
    let mut stale = info("stale-not-periodic", "127.0.0.2", 4242);
    stale.received -= 4.0 * 86400.0;
    stale.discovery_hash = vec![8; 32];
    store.observe(stale).expect("stale");
    let mut nontransport = info("live-only", "127.0.0.2", 4243);
    nontransport.transport = false;
    nontransport.discovery_hash = vec![9; 32];
    store.observe(nontransport).expect("nontransport");
    let refreshes = HotApplyRuntimeRefreshes::new();
    runtime.monitor(&manager, &mut managed, &transport, &refreshes, &daemon).await;
    assert_eq!(managed.len(), 1, "one random candidate per periodic check");
    assert!(managed.values().all(|active| active
        .record
        .name
        .as_deref()
        .is_some_and(|name| name.starts_with("periodic-"))));
    let original = managed
        .values()
        .next()
        .expect("native candidate")
        .native
        .as_ref()
        .expect("active native root")
        .address;
    let mut records = daemon.interface_records();
    for index in 0..2u8 {
        let channel = manager.lock().await.new_channel(1);
        let record = InterfaceRecord {
            kind: discovered_carrier_kind().into(),
            enabled: true,
            name: Some(format!("reserved-{index}")),
            host: None,
            port: None,
            settings: None,
        };
        records.push(record.clone());
        managed.insert(
            format!("reserved-{index}").into(),
            ManagedHotApplyInterface::active(
                record,
                *channel.address(),
                None,
                ManagedInterfaceOrigin::Discovered {
                    endpoint: [index + 10; 32],
                    source: "network".into(),
                    down_since: None,
                },
            ),
        );
    }
    daemon.replace_interfaces(records);
    runtime.monitor(&manager, &mut managed, &transport, &refreshes, &daemon).await;
    assert_eq!(managed.len(), 3, "one free slot equals maximum / 4 reservation");
    assert!(managed.values().any(|active| active
        .native
        .as_ref()
        .expect("active native root")
        .address
        == original));
    for (_, active) in managed.drain() {
        stop_and_join(
            &manager,
            Some(&transport),
            active.native.as_ref().expect("active native root").address,
        )
        .await
        .expect("stop");
    }
    drop(peers);
}
