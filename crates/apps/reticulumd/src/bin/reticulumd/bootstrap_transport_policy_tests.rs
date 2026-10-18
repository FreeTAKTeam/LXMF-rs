use super::*;
use clap::Parser;

#[path = "bootstrap_transport_restore_tests.rs"]
mod restore_tests;

#[path = "bootstrap_transport_restore_controls.rs"]
mod restore_controls;

#[path = "bootstrap_transport_restore_replacement.rs"]
mod restore_replacement;

#[path = "bootstrap_tcp_client_options_tests.rs"]
mod tcp_client_options;

#[path = "tests/bootstrap_tcp_family.rs"]
mod tcp_family;

#[path = "tests/bootstrap_udp_family.rs"]
mod udp_family;

#[cfg(target_os = "linux")]
#[path = "tests/bootstrap_pipe_family.rs"]
mod pipe_family;

#[cfg(target_os = "linux")]
#[path = "tests/bootstrap_serial_family.rs"]
mod serial_family;

#[path = "tests/bootstrap_i2p_family.rs"]
mod i2p_family;

async fn start_fixture(
    args: &Args,
    config: &DaemonConfig,
    root: &std::path::Path,
) -> TransportStartupArtifacts {
    let identity = PrivateIdentity::new_from_rand(rand_core::OsRng);
    let (receipt_tx, _receipt_rx) = tokio::sync::mpsc::channel(8);
    start_transport_and_interfaces(TransportStartupInput {
        args,
        daemon_config: Some(config),
        identity: &identity,
        reticulum_storage_path: root,
        local_display_name: None,
        local_announce_capabilities: &[],
        propagation_announce_app_data: None,
        configured_interfaces: config
            .interfaces
            .iter()
            .map(crate::bootstrap::interface_record_from_config)
            .collect(),
        receipt_map: Arc::new(Mutex::new(HashMap::new())),
        receipt_tx: receipt_tx.into(),
        probe_receipts: Arc::new(ProbeReceiptRegistry::default()),
        propagation_control_enabled: false,
        respond_to_probes: false,
        propagation_announce_config: PropagationNodeAnnounceConfig::default(),
        local_hops_delta: false,
        inbound_queue_limits: InboundQueueLimits::default(),
        discovery_enabled: false,
    })
    .await
}

async fn retire_fixture(startup: TransportStartupArtifacts) {
    let transport = startup.transport.expect("native transport");
    let manager = transport.iface_manager();
    let mut owner =
        crate::interface_hot_apply::InterfaceHotApplyBridge::spawn_with_configured_roots(
            transport,
            startup.interfaces.into_owned_roots(&startup.configured_interfaces),
            std::sync::Weak::new(),
            None,
            startup.configured_startup,
        );
    owner.shutdown().await.expect("join native fixture roots and reconnect worker");
    assert!(manager.lock().await.interface_hashes().is_empty());
}

#[tokio::test]
async fn selected_listener_config_strictness_does_not_change_recipe_preflight_policy() {
    let occupied = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("occupied endpoint");
    let port = occupied.local_addr().expect("endpoint").port();
    let unavailable = tokio::net::TcpSocket::new_v4().expect("unlistened socket");
    unavailable
        .bind("127.0.0.1:0".parse().expect("loopback"))
        .expect("reserve unavailable endpoint");
    let unavailable_port = unavailable.local_addr().expect("unavailable endpoint").port();
    let config = DaemonConfig::from_toml(&format!(
        "interfaces = [{{type='tcp_server',enabled=true, name='occupied', host='127.0.0.1', port={port}}}, {{type='tcp_client', enabled=true, name='unavailable', host='127.0.0.1', port={unavailable_port}}}]\n[reticulum]\nshare_instance=false\npanic_on_interface_error=true\n"
    )).expect("config");
    assert!(config.panic_on_interface_error);
    for strict in [false, true] {
        let root = tempfile::tempdir().expect("state");
        let mut args = Args::parse_from(["reticulumd"]);
        args.strict_interface_startup = strict;
        let startup = start_fixture(&args, &config, root.path()).await;
        assert_eq!(startup.selected_tcp_server.selected_index, Some(0));
        assert_eq!(startup.interfaces.startup_successes, usize::from(!strict));
        let failures = &startup.interfaces.startup_failures;
        assert_eq!(failures.len(), if strict { 2 } else { 1 });
        assert_eq!(failures[0].label, "occupied");
        assert_eq!(failures[0].kind, "tcp_server");
        assert!(!failures[0].error.is_empty());
        assert_eq!(startup.interfaces.configured_roots.len(), if strict { 1 } else { 2 });
        let listener = startup
            .interfaces
            .tcp_runtime_refreshes
            .iter()
            .find_map(|refresh| {
                if let TcpRuntimeStatusSource::Listener(status) = &refresh.status {
                    Some(status.to_json())
                } else {
                    None
                }
            })
            .expect("failed readiness retains native listener status/root");
        assert_eq!(listener["listener_state"], "bind_error");
        assert_eq!(listener["accepted_connections"], 0);
        if strict {
            assert_eq!(failures[1].label, "unavailable");
        }
        retire_fixture(startup).await;
    }
}

#[tokio::test]
async fn selected_listener_policy_rejection_keeps_original_index_and_failed_record() {
    let root = tempfile::tempdir().expect("state");
    let mut config = DaemonConfig::from_toml(
        "interfaces=[{type='tcp_client',enabled=false,name='sentinel',host='127.0.0.1',port=0},{type='tcp_server',enabled=true,name='rejected',host='127.0.0.1',port=0}]\n[reticulum]\nshare_instance=false\n",
    ).expect("config");
    // Reach the native preparation boundary directly; normal TOML validation rejects this.
    config.interfaces[1].ifac_size = Some(16);
    let args = Args::parse_from(["reticulumd"]);
    let startup = start_fixture(&args, &config, root.path()).await;
    assert_eq!(startup.selected_tcp_server.selected_index, Some(1));
    assert_eq!(startup.interfaces.startup_successes, 0);
    assert_eq!(startup.interfaces.startup_failures.len(), 1);
    assert_eq!(startup.interfaces.startup_failures[0].label, "rejected");
    assert!(startup.interfaces.startup_failures[0].error.contains("policy could not be installed"));
    assert!(startup.interfaces.configured_roots.is_empty());
    assert!(startup.interfaces.tcp_runtime_refreshes.is_empty());
    let record = &startup.configured_interfaces[1];
    assert_eq!(record.settings.as_ref().expect("settings")["_runtime"]["startup_status"], "failed");
    assert_eq!(startup.configured_interfaces[0].name.as_deref(), Some("sentinel"));
    retire_fixture(startup).await;
}

#[tokio::test]
async fn selected_listener_override_keeps_shadowed_recipe_out_of_native_preparation() {
    let root = tempfile::tempdir().expect("state");
    let mut config = DaemonConfig::from_toml(
        "interfaces=[{type='tcp_server',enabled=true,name='shadowed',host='127.0.0.1',port=0}]\n[reticulum]\nshare_instance=false\n",
    ).expect("config");
    config.interfaces[0].ifac_size = Some(16);
    let args = Args::parse_from([
        "reticulumd",
        "--transport",
        "127.0.0.1:0",
        "--strict-interface-startup",
    ]);
    let startup = start_fixture(&args, &config, root.path()).await;
    assert_eq!(startup.selected_tcp_server.selected_index, None);
    assert_eq!(startup.interfaces.startup_successes, 1);
    assert!(startup.interfaces.startup_failures.is_empty());
    assert_eq!(startup.interfaces.configured_roots.len(), 1);
    assert_eq!(
        startup.interfaces.configured_roots[0].0, 1,
        "synthetic override retains its own slot"
    );
    assert_eq!(startup.configured_interfaces.len(), 2);
    assert_eq!(
        startup.configured_interfaces[0].settings.as_ref().expect("settings")["_runtime"]
            ["startup_status"],
        "shadowed_by_transport_override"
    );
    assert!(matches!(&startup.interfaces.tcp_runtime_refreshes[0].status,
        TcpRuntimeStatusSource::Listener(status) if status.to_json()["listener_state"] == "listening"));
    retire_fixture(startup).await;
}

#[tokio::test]
async fn selected_listener_missing_recipe_never_falls_back_to_unconfigured_spawn() {
    let manager = Arc::new(tokio::sync::Mutex::new(rns_transport::iface::InterfaceManager::new(8)));
    let unrelated = manager.lock().await.new_channel(2);
    let selection = TcpServerSelection {
        bind_addr: Some("127.0.0.1:0".into()),
        selected_index: Some(7),
        kind: "backbone".into(),
        ..TcpServerSelection::default()
    };
    let mut record = crate::bootstrap::interface_record_from_config(
        &reticulum_daemon::config::InterfaceConfig {
            kind: "backbone".into(),
            enabled: Some(true),
            host: Some("127.0.0.1".into()),
            port: Some(0),
            ..reticulum_daemon::config::InterfaceConfig::default()
        },
    );
    let mut successes = 0;
    let mut failures = Vec::new();
    let started = startup_selected_tcp_server(
        &selection,
        None,
        &manager,
        true,
        Some(&mut record),
        &mut successes,
        &mut failures,
    )
    .await;
    assert!(started.is_none());
    assert_eq!(successes, 0);
    assert_eq!(failures.len(), 1);
    assert_eq!(failures[0].label, "backbone");
    assert_eq!(record.settings.as_ref().expect("settings")["_runtime"]["startup_status"], "failed");
    assert_eq!(
        manager.lock().await.interface_hashes(),
        std::collections::HashSet::from([*unrelated.address()])
    );
}
