use lxmf::lxmd::config::LxmdConfig;
use lxmf::lxmd::runtime::{execute_with_runtime, InboundEvent, LxmdCommand, LxmdRuntime};

#[test]
fn lxmd_config_defaults_are_stable() {
    let config = LxmdConfig::default();
    assert!(!config.propagation_node);
    assert_eq!(config.announce_interval_secs, 3600);
    assert_eq!(config.service_tick_interval_secs, 1);
    assert!(config.on_inbound.is_none());
}

#[test]
fn lxmd_config_loads_from_toml() {
    let temp = tempfile::tempdir().unwrap();
    let path = temp.path().join("lxmd.toml");
    std::fs::write(
        &path,
        r#"
propagation_node = true
announce_interval_secs = 120
service_tick_interval_secs = 2
propagation_target_cost = 20
on_inbound = "echo inbound"
rnsconfig = "/tmp/rns"
"#,
    )
    .unwrap();

    let config = LxmdConfig::load_from_path(&path).unwrap();
    assert!(config.propagation_node);
    assert_eq!(config.announce_interval_secs, 120);
    assert_eq!(config.service_tick_interval_secs, 2);
    assert_eq!(config.propagation_target_cost, 20);
    assert_eq!(config.on_inbound.as_deref(), Some("echo inbound"));
    assert_eq!(config.rnsconfig.as_deref(), Some("/tmp/rns"));
}

#[test]
fn lxmd_serve_tick_runs_jobs_announces_and_inbound_hook() {
    let temp = tempfile::tempdir().unwrap();
    let hook_file = temp.path().join("hook.log");

    let config = LxmdConfig {
        propagation_node: true,
        announce_interval_secs: 10,
        storage_path: Some(temp.path().join("store").display().to_string()),
        on_inbound: Some(format!(
            "echo \"$LXMF_MESSAGE_ID:$LXMF_CONTENT\" >> {}",
            hook_file.display()
        )),
        ..LxmdConfig::default()
    };

    let mut runtime = LxmdRuntime::new(config).unwrap();
    assert!(runtime.router().propagation_enabled());

    runtime.queue_inbound(InboundEvent::new([0x11; 16], [0x22; 16], "m-1", "hello"));

    let first = runtime.serve_tick_at(100).unwrap();
    assert!(first.announced);
    assert_eq!(first.inbound_processed, 1);
    assert_eq!(first.jobs_run_total, 1);

    let second = runtime.serve_tick_at(105).unwrap();
    assert!(!second.announced);
    assert_eq!(second.inbound_processed, 0);
    assert_eq!(second.jobs_run_total, 2);

    let third = runtime.serve_tick_at(111).unwrap();
    assert!(third.announced);
    assert_eq!(third.jobs_run_total, 3);

    let hook = std::fs::read_to_string(&hook_file).unwrap();
    assert!(hook.contains("m-1:hello"));
}

#[test]
fn lxmd_sync_unpeer_and_status_flow() {
    let mut runtime = LxmdRuntime::new(LxmdConfig::default()).unwrap();

    let sync_output = execute_with_runtime(
        &mut runtime,
        LxmdCommand::Sync {
            peer: Some("peer-a".into()),
        },
        1_700_000_000,
    )
    .unwrap();
    assert!(sync_output.contains("synced_peers=1"));
    assert!(sync_output.contains("created_transfers=1"));

    let status_output =
        execute_with_runtime(&mut runtime, LxmdCommand::Status, 1_700_000_001).unwrap();
    assert!(status_output.contains("peer_count=1"));
    assert!(status_output.contains("sync_runs=1"));

    let unpeer_output = execute_with_runtime(
        &mut runtime,
        LxmdCommand::Unpeer {
            peer: "peer-a".into(),
        },
        1_700_000_002,
    )
    .unwrap();
    assert!(unpeer_output.contains("removed=true"));

    let status_after = runtime.status();
    assert_eq!(status_after.peer_count, 0);
    assert_eq!(status_after.unpeer_runs, 1);
}
