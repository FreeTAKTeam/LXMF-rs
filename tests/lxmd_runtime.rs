use lxmf::lxmd::config::LxmdConfig;
use lxmf::lxmd::runtime::{execute, LxmdCommand};

#[test]
fn lxmd_config_defaults_are_stable() {
    let config = LxmdConfig::default();
    assert!(!config.propagation_node);
    assert_eq!(config.announce_interval_secs, 3600);
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
on_inbound = "echo inbound"
rnsconfig = "/tmp/rns"
"#,
    )
    .unwrap();

    let config = LxmdConfig::load_from_path(&path).unwrap();
    assert!(config.propagation_node);
    assert_eq!(config.announce_interval_secs, 120);
    assert_eq!(config.on_inbound.as_deref(), Some("echo inbound"));
    assert_eq!(config.rnsconfig.as_deref(), Some("/tmp/rns"));
}

#[test]
fn lxmd_runtime_executes_core_commands() {
    let config = LxmdConfig {
        propagation_node: true,
        announce_interval_secs: 30,
        ..LxmdConfig::default()
    };

    let serve = execute(LxmdCommand::Serve, &config).unwrap();
    assert!(serve.contains("lxmd serve"));
    assert!(serve.contains("propagation_node=true"));

    let sync = execute(
        LxmdCommand::Sync {
            peer: Some("peer-a".into()),
        },
        &config,
    )
    .unwrap();
    assert!(sync.contains("lxmd sync"));
    assert!(sync.contains("peer=peer-a"));

    let unpeer = execute(
        LxmdCommand::Unpeer {
            peer: "peer-b".into(),
        },
        &config,
    )
    .unwrap();
    assert!(unpeer.contains("lxmd unpeer peer=peer-b"));

    let status = execute(LxmdCommand::Status, &config).unwrap();
    assert!(status.contains("lxmd status"));
    assert!(status.contains("announce_interval_secs=30"));
}
