#[test]
fn tcp_client_builder_uses_python_fixed_mtu() {
    let iface = InterfaceConfig {
        kind: "tcp_client".to_string(),
        enabled: Some(true),
        host: Some("rmap.world".to_string()),
        port: Some(4242),
        mtu: Some(4096),
        ..InterfaceConfig::default()
    };

    let adapter = build_tcp_client_adapter("rmap.world:4242".to_string(), &iface, None);

    assert_eq!(adapter.addr(), "rmap.world:4242");
    assert_eq!(adapter.mtu_value(), 4096);
    assert!(adapter.socket_tuning().is_empty());
    assert!(!adapter.hdlc_liveness_enabled());
    assert!(!adapter.reconnect_events_enabled());
}

#[tokio::test]
async fn startup_configured_interfaces_marks_unknown_kind_as_failed() {
    let config = DaemonConfig::from_toml(
        r#"
interfaces = [
  { type = "FutureReticulumInterface", enabled = true, name = "future" }
]
"#,
    )
    .expect("unknown interface kinds should parse for runtime reporting");
    let identity = rns_core::identity::PrivateIdentity::new_from_rand(rand_core::OsRng);
    let transport_identity = to_transport_private_identity(&identity);
    let transport = Transport::new(TransportConfig::new("test", &transport_identity, true));
    let manager = transport.iface_manager();
    let mut records = config
        .interfaces
        .iter()
        .map(|iface| InterfaceRecord {
            kind: iface.kind.clone(),
            enabled: iface.enabled(),
            host: iface.host.clone(),
            port: iface.port,
            name: iface.name.clone(),
            settings: iface.settings_json(),
        })
        .collect::<Vec<_>>();

    let batch = startup_configured_interfaces(
        &test_args(),
        &config,
        &super::super::TcpServerSelection::default(),
        &transport,
        &manager,
        None,
        &mut records,
        std::path::Path::new("."),
        None,
        None,
    )
    .await;

    assert_eq!(batch.startup_successes, 0);
    assert_eq!(batch.startup_failures.len(), 1);
    assert_eq!(batch.startup_failures[0].kind, "FutureReticulumInterface");
    assert_eq!(
        batch.startup_failures[0].error,
        "unsupported interface kind 'FutureReticulumInterface'"
    );
    let runtime = records[0]
        .settings
        .as_ref()
        .and_then(|settings| settings.get("_runtime"))
        .expect("runtime metadata");
    assert_eq!(runtime["startup_status"].as_str(), Some("failed"));
    assert_eq!(
        runtime["startup_error"].as_str(),
        Some("unsupported interface kind 'FutureReticulumInterface'")
    );
}

#[tokio::test]
async fn startup_configured_interfaces_routes_python_style_aliases_to_real_branches() {
    let config = DaemonConfig::from_toml(
        r#"
interfaces = [
  { type = "UDPInterface", enabled = true, name = "udp-alias", listen_ip = "127.0.0.1", listen_port = 0, forward_ip = "127.0.0.1", forward_port = 4242 },
  { type = "SerialInterface", enabled = true, name = "serial-alias", port = "/dev/does-not-exist-serial-alias", speed = 9600 },
  { type = "KISSInterface", enabled = true, name = "kiss-alias", port = "/dev/does-not-exist-kiss-alias", speed = 1200 },
  { type = "TCPClientInterface", enabled = true, name = "tcp-client-alias", target_host = "127.0.0.1", target_port = 65535 },
  { type = "BackboneClientInterface", enabled = true, name = "backbone-client-alias", target_host = "127.0.0.1", target_port = 65535 },
  { type = "LocalServerInterface", enabled = true, name = "local-server-alias", listen_ip = "127.0.0.1", listen_port = 0 },
  { type = "Vrn76KissBluetoothInterface", enabled = true, name = "vrn76-alias", device_name_filter = "VR-N76" }
]
"#,
    )
    .expect("python-style aliases should parse");
    let identity = rns_core::identity::PrivateIdentity::new_from_rand(rand_core::OsRng);
    let transport_identity = to_transport_private_identity(&identity);
    let transport = Transport::new(TransportConfig::new("test", &transport_identity, true));
    let manager = transport.iface_manager();
    let mut records = config
        .interfaces
        .iter()
        .map(|iface| InterfaceRecord {
            kind: iface.kind.clone(),
            enabled: iface.enabled(),
            host: iface.host.clone(),
            port: iface.port,
            name: iface.name.clone(),
            settings: iface.settings_json(),
        })
        .collect::<Vec<_>>();

    let batch = startup_configured_interfaces(
        &test_args(),
        &config,
        &super::super::TcpServerSelection::default(),
        &transport,
        &manager,
        None,
        &mut records,
        std::path::Path::new("."),
        None,
        None,
    )
    .await;

    assert!(
        batch
            .startup_failures
            .iter()
            .all(|failure| !failure.error.contains("unsupported interface kind")),
        "aliases should route to real startup branches, failures={:?}",
        batch.startup_failures
    );
    assert!(records.iter().all(|record| {
        record
            .settings
            .as_ref()
            .and_then(|settings| settings.get("_runtime"))
            .and_then(|runtime| runtime.get("startup_error"))
            .and_then(serde_json::Value::as_str)
            .is_none_or(|error| !error.contains("unsupported interface kind"))
    }));
}

#[test]
fn backbone_client_builder_enables_socket_tuning_and_liveness() {
    let iface = InterfaceConfig {
        kind: "backbone_client".to_string(),
        enabled: Some(true),
        host: Some("rmap.world".to_string()),
        port: Some(4242),
        mtu: Some(1_048_576),
        ..InterfaceConfig::default()
    };

    let (reconnect_tx, _reconnect_rx) = tokio::sync::mpsc::channel(32);
    let adapter =
        build_tcp_client_adapter("rmap.world:4242".to_string(), &iface, Some(reconnect_tx));

    assert_eq!(adapter.addr(), "rmap.world:4242");
    assert_eq!(adapter.mtu_value(), 1_048_576);
    assert_eq!(adapter.socket_tuning().nodelay, Some(true));
    assert_eq!(adapter.socket_tuning().keepalive, Some(true));
    assert_eq!(adapter.socket_tuning().tcp_keepalive_idle, Some(Duration::from_secs(5)));
    assert_eq!(adapter.socket_tuning().tcp_keepalive_interval, Some(Duration::from_secs(2)));
    assert_eq!(adapter.socket_tuning().tcp_keepalive_retries, Some(12));
    assert_eq!(adapter.socket_tuning().tcp_user_timeout, Some(Duration::from_secs(24)));
    assert!(adapter.hdlc_liveness_enabled());
    assert!(adapter.reconnect_events_enabled());
}

#[test]
fn tcp_client_builder_applies_reticulum_reconnect_options() {
    let iface = InterfaceConfig {
        kind: "tcp_client".to_string(),
        enabled: Some(true),
        host: Some("rmap.world".to_string()),
        port: Some(4242),
        connect_timeout: Some(7),
        max_reconnect_tries: Some(3),
        prefer_ipv6: Some(true),
        ..InterfaceConfig::default()
    };

    let (reconnect_tx, _reconnect_rx) = tokio::sync::mpsc::channel(32);
    let adapter =
        build_tcp_client_adapter("rmap.world:4242".to_string(), &iface, Some(reconnect_tx));

    assert_eq!(adapter.connect_timeout(), Duration::from_secs(7));
    assert_eq!(adapter.max_reconnect_tries(), Some(3));
    assert!(adapter.prefer_ipv6());
    assert!(adapter.reconnect_events_enabled());
}

#[test]
fn local_client_builder_forces_shared_instance_bitrate() {
    let iface = InterfaceConfig {
        kind: "local_client".to_string(),
        enabled: Some(true),
        host: Some("127.0.0.1".to_string()),
        port: Some(37_428),
        bitrate: Some(1_000_000),
        force_shared_instance_bitrate: Some(1_000_000),
        ..InterfaceConfig::default()
    };

    let adapter = build_tcp_client_adapter("127.0.0.1:37428".to_string(), &iface, None);

    assert_eq!(adapter.forced_bitrate_bps(), Some(1_000_000));
    assert!(adapter.socket_tuning().is_empty());
    assert!(!adapter.hdlc_liveness_enabled());
    assert!(!adapter.reconnect_events_enabled());
}

#[test]
fn tcp_client_builder_applies_i2p_tunneled_socket_profile() {
    let iface = InterfaceConfig {
        kind: "tcp_client".to_string(),
        enabled: Some(true),
        host: Some("rmap.world".to_string()),
        port: Some(4242),
        i2p_tunneled: Some(true),
        ..InterfaceConfig::default()
    };

    let adapter = build_tcp_client_adapter("rmap.world:4242".to_string(), &iface, None);

    assert_eq!(adapter.socket_tuning(), TcpSocketTuning::i2p_tunneled());
    assert!(!adapter.hdlc_liveness_enabled());
    assert!(!adapter.reconnect_events_enabled());
}

#[test]
fn apply_runtime_config_records_reticulum_shared_options() {
    let mut manager = rns_transport::iface::InterfaceManager::new(16);
    let channel = manager.new_channel(16);
    let iface = InterfaceConfig {
        kind: "kiss".to_string(),
        enabled: Some(true),
        outgoing: Some(false),
        bitrate: Some(1200),
        announce_cap: Some(5),
        announce_rate_target: Some(120),
        announce_rate_grace: Some(2),
        announce_rate_penalty: Some(30),
        bootstrap_only: Some(true),
        ifac_size: Some(16),
        networkname: Some("field-net".to_string()),
        pass_phrase: Some("shared-secret".to_string()),
        ingress_control: Some(false),
        egress_control: Some(true),
        ic_burst_hold: Some(1.5),
        ec_pr_freq: Some(0.5),
        discoverable: Some(true),
        announce_interval: Some(360),
        discovery_name: Some("field node".to_string()),
        discovery_encrypt: Some(true),
        reachable_on: Some("lxmf://field".to_string()),
        publish_ifac: Some(true),
        latitude: Some(45.5),
        longitude: Some(-63.5),
        height: Some(42.0),
        discovery_frequency: Some(915_000_000),
        discovery_bandwidth: Some(125_000),
        discovery_modulation: Some(1),
        ..InterfaceConfig::default()
    };

    apply_interface_runtime_config(&mut manager, *channel.address(), &iface)
        .expect("valid runtime policy");

    assert_eq!(manager.outgoing(channel.address()), Some(false));
    assert_eq!(manager.announce_pacing(channel.address()), Some((1200, 5)));
    assert_eq!(
        manager.shared_config(channel.address()),
        Some(&InterfaceSharedConfig {
            announce_rate_target: Some(120),
            announce_rate_grace: Some(2),
            announce_rate_penalty: Some(30),
            bootstrap_only: Some(true),
            ifac_size: Some(16),
            network_name: Some("field-net".to_string()),
            passphrase: Some("shared-secret".to_string()),
            ingress_control: Some(false),
            egress_control: Some(true),
            ic_burst_hold: Some(1.5),
            ec_pr_freq: Some(0.5),
            discoverable: Some(true),
            announce_interval: Some(21_600),
            discovery_name: Some("field node".to_string()),
            discovery_encrypt: Some(true),
            reachable_on: Some("lxmf://field".to_string()),
            publish_ifac: Some(true),
            latitude: Some(45.5),
            longitude: Some(-63.5),
            height: Some(42.0),
            discovery_frequency: Some(915_000_000),
            discovery_bandwidth: Some(125_000),
            discovery_modulation: Some(1),
            ..InterfaceSharedConfig::default()
        })
    );
}

#[tokio::test]
async fn udp_startup_tags_multicast_config_as_multicast_role() {
    let args = test_args();
    let iface = InterfaceConfig {
        kind: "udp".to_string(),
        enabled: Some(true),
        name: Some("auto-style-multicast".to_string()),
        host: Some("239.255.0.1".to_string()),
        port: Some(0),
        target_host: Some("239.255.0.1".to_string()),
        target_port: Some(4242),
        ..InterfaceConfig::default()
    };
    let identity = rns_core::identity::PrivateIdentity::new_from_rand(rand_core::OsRng);
    let transport_identity = to_transport_private_identity(&identity);
    let transport = Transport::new(TransportConfig::new("test", &transport_identity, true));
    let manager = transport.iface_manager();
    let mut record = InterfaceRecord {
        kind: iface.kind.clone(),
        enabled: true,
        host: iface.host.clone(),
        port: iface.port,
        name: iface.name.clone(),
        settings: iface.settings_json(),
    };
    let mut startup_failures = Vec::new();
    let mut udp_runtime_refreshes = Vec::new();
    let mut seeded_hot_apply_interfaces = Vec::new();
    let mut sinks = UdpStartupSinks {
        startup_failures: &mut startup_failures,
        runtime_refreshes: &mut udp_runtime_refreshes,
        seeded_hot_apply_interfaces: &mut seeded_hot_apply_interfaces,
    };

    let started = startup_udp(
        &args,
        &iface,
        "auto-style-multicast",
        &transport,
        &manager,
        &mut record,
        &mut sinks,
    )
    .await;

    assert!(started);
    assert!(startup_failures.is_empty());
    assert_eq!(udp_runtime_refreshes.len(), 1);
    let runtime_iface = record
        .settings
        .as_ref()
        .and_then(|settings| settings.get("_runtime"))
        .and_then(|runtime| runtime.get("iface"))
        .and_then(|iface| iface.as_str())
        .expect("runtime iface");
    let runtime_iface =
        AddressHash::new_from_hex_string(runtime_iface.trim_matches('/')).expect("iface hash");
    assert_eq!(manager.lock().await.role(&runtime_iface), Some(IfaceRole::Multicast));
    let udp_status = &record.settings.as_ref().expect("settings")["_runtime"]["udp"]["status"];
    assert_eq!(udp_status["link_state"].as_str(), Some("configured"));
    assert_eq!(udp_status["role"].as_str(), Some("multicast"));
    assert!(udp_status["bind_addr"].as_str().expect("bind").ends_with(":0"));
    assert_eq!(udp_status["forward_addr"].as_str(), Some("239.255.0.1:4242"));
    assert_eq!(udp_status["iface"].as_str(), Some(runtime_iface.to_string().as_str()));
    assert_eq!(seeded_hot_apply_interfaces.len(), 1);
    assert_eq!(seeded_hot_apply_interfaces[0].0, "auto-style-multicast");
    assert_eq!(seeded_hot_apply_interfaces[0].1.name.as_deref(), Some("auto-style-multicast"));
    assert_eq!(seeded_hot_apply_interfaces[0].1.kind, "udp");
    assert_eq!(seeded_hot_apply_interfaces[0].2, udp_runtime_refreshes[0].runtime_iface);
}

#[tokio::test]
async fn udp_startup_seeds_unicast_config_for_hot_apply() {
    let args = test_args();
    let iface = InterfaceConfig {
        kind: "udp".to_string(),
        enabled: Some(true),
        name: Some("udp-loopback".to_string()),
        host: Some("127.0.0.1".to_string()),
        port: Some(0),
        target_host: Some("127.0.0.1".to_string()),
        target_port: Some(4242),
        ..InterfaceConfig::default()
    };
    let identity = rns_core::identity::PrivateIdentity::new_from_rand(rand_core::OsRng);
    let transport_identity = to_transport_private_identity(&identity);
    let transport = Transport::new(TransportConfig::new("test", &transport_identity, true));
    let manager = transport.iface_manager();
    let mut record = InterfaceRecord {
        kind: iface.kind.clone(),
        enabled: true,
        host: iface.host.clone(),
        port: iface.port,
        name: iface.name.clone(),
        settings: iface.settings_json(),
    };
    let mut startup_failures = Vec::new();
    let mut udp_runtime_refreshes = Vec::new();
    let mut seeded_hot_apply_interfaces = Vec::new();
    let mut sinks = UdpStartupSinks {
        startup_failures: &mut startup_failures,
        runtime_refreshes: &mut udp_runtime_refreshes,
        seeded_hot_apply_interfaces: &mut seeded_hot_apply_interfaces,
    };

    let started =
        startup_udp(&args, &iface, "udp-loopback", &transport, &manager, &mut record, &mut sinks)
            .await;

    assert!(started);
    assert!(startup_failures.is_empty());
    assert_eq!(udp_runtime_refreshes.len(), 1);
    assert_eq!(seeded_hot_apply_interfaces.len(), 1);
    assert_eq!(seeded_hot_apply_interfaces[0].0, "udp-loopback");
    assert_eq!(seeded_hot_apply_interfaces[0].1.name.as_deref(), Some("udp-loopback"));
    assert_eq!(seeded_hot_apply_interfaces[0].1.kind, "udp");
    assert_eq!(seeded_hot_apply_interfaces[0].2, udp_runtime_refreshes[0].runtime_iface);
}

#[test]
fn tcp_server_startup_seeds_selected_loopback_config_for_hot_apply() {
    let iface = InterfaceConfig {
        kind: "tcp_server".to_string(),
        enabled: Some(true),
        name: Some("tcp-loopback".to_string()),
        host: Some("127.0.0.1".to_string()),
        port: Some(4242),
        ..InterfaceConfig::default()
    };
    let selected_tcp_server = super::super::TcpServerSelection {
        bind_addr: Some("127.0.0.1:4242".to_string()),
        selected_index: Some(2),
        kind: "tcp_server".to_string(),
        ..super::super::TcpServerSelection::default()
    };
    let runtime_iface = AddressHash::new([0x42; 16]);
    let mut startup_failures = Vec::new();
    let mut record = InterfaceRecord {
        kind: iface.kind.clone(),
        enabled: true,
        host: iface.host.clone(),
        port: iface.port,
        name: iface.name.clone(),
        settings: iface.settings_json(),
    };

    let seed = startup_tcp_server_record(
        2,
        &iface,
        "tcp-loopback",
        &selected_tcp_server,
        Some(&runtime_iface),
        &mut record,
        &mut startup_failures,
    )
    .expect("selected loopback TCP server should seed hot apply");

    assert!(startup_failures.is_empty());
    assert_eq!(seed.0, "tcp-loopback");
    assert_eq!(seed.1.name.as_deref(), Some("tcp-loopback"));
    assert_eq!(seed.1.kind, "tcp_server");
    assert_eq!(seed.2, runtime_iface);
    assert_eq!(
        record
            .settings
            .as_ref()
            .and_then(|settings| settings.get("_runtime"))
            .and_then(|runtime| runtime.get("startup_status"))
            .and_then(serde_json::Value::as_str),
        Some("active")
    );

    let mut shadowed_record = InterfaceRecord {
        kind: iface.kind.clone(),
        enabled: true,
        host: iface.host.clone(),
        port: iface.port,
        name: Some("shadowed-tcp".to_string()),
        settings: iface.settings_json(),
    };
    let shadowed_seed = startup_tcp_server_record(
        3,
        &iface,
        "shadowed-tcp",
        &selected_tcp_server,
        Some(&runtime_iface),
        &mut shadowed_record,
        &mut startup_failures,
    );

    assert!(shadowed_seed.is_none());
    assert_eq!(
        shadowed_record
            .settings
            .as_ref()
            .and_then(|settings| settings.get("_runtime"))
            .and_then(|runtime| runtime.get("startup_status"))
            .and_then(serde_json::Value::as_str),
        Some("shadowed_by_transport_override")
    );
}

#[tokio::test]
async fn serial_startup_embeds_configured_runtime_status_without_strict_preflight() {
    let cfg = reticulum_daemon::config::DaemonConfig::from_toml(
        r#"
interfaces = [
  { type = "SerialInterface", enabled = true, name = "serial-main", port = "/dev/does-not-exist-serial", speed = 19200, databits = 7, parity = "even", stopbits = 2, flow_control = "hardware", mtu = 1024 }
]
"#,
    )
    .expect("parse serial config");
    let args = test_args();
    let iface = &cfg.interfaces[0];
    let identity = rns_core::identity::PrivateIdentity::new_from_rand(rand_core::OsRng);
    let transport_identity = to_transport_private_identity(&identity);
    let transport = Transport::new(TransportConfig::new("test", &transport_identity, true));
    let manager = transport.iface_manager();
    let mut record = InterfaceRecord {
        kind: iface.kind.clone(),
        enabled: true,
        host: None,
        port: None,
        name: iface.name.clone(),
        settings: iface.settings_json(),
    };
    let mut startup_failures = Vec::new();
    let mut serial_runtime_refreshes = Vec::new();

    let started = startup_serial(
        &args,
        iface,
        "serial-main",
        &manager,
        &mut record,
        &mut startup_failures,
        &mut serial_runtime_refreshes,
    )
    .await;

    assert!(started);
    assert!(startup_failures.is_empty());
    assert_eq!(serial_runtime_refreshes.len(), 1);
    let runtime = record
        .settings
        .as_ref()
        .and_then(|settings| settings.get("_runtime"))
        .expect("runtime settings");
    assert_eq!(runtime["startup_status"].as_str(), Some("spawned"));
    let runtime_iface =
        runtime["iface"].as_str().expect("runtime iface").trim_matches('/').to_string();
    let runtime_iface = AddressHash::new_from_hex_string(&runtime_iface).expect("iface hash");
    assert_eq!(manager.lock().await.role(&runtime_iface), Some(IfaceRole::Unicast));
    let serial_status = &runtime["serial"]["status"];
    assert_eq!(serial_status["link_state"].as_str(), Some("configured"));
    assert_eq!(serial_status["device"].as_str(), Some("/dev/does-not-exist-serial"));
    assert_eq!(serial_status["baud_rate"].as_u64(), Some(19_200));
    assert_eq!(serial_status["data_bits"].as_u64(), Some(7));
    assert_eq!(serial_status["parity"].as_str(), Some("even"));
    assert_eq!(serial_status["stop_bits"].as_u64(), Some(2));
    assert_eq!(serial_status["flow_control"].as_str(), Some("hardware"));
    assert_eq!(serial_status["mtu"].as_u64(), Some(1024));
    assert_eq!(serial_status["iface"].as_str(), Some(runtime_iface.to_string().as_str()));
}

#[tokio::test]
async fn ax25_kiss_startup_marks_spawned_unicast_without_strict_preflight() {
    let cfg = reticulum_daemon::config::DaemonConfig::from_toml(
        r#"
interfaces = [
  { type = "AX25KISSInterface", enabled = true, name = "ax25-main", port = "/dev/does-not-exist-ax25", speed = 1200, callsign = "N0CALL", ssid = 1 }
]
"#,
    )
    .expect("parse ax25 kiss config");
    let args = test_args();
    let iface = &cfg.interfaces[0];
    let identity = rns_core::identity::PrivateIdentity::new_from_rand(rand_core::OsRng);
    let transport_identity = to_transport_private_identity(&identity);
    let transport = Transport::new(TransportConfig::new("test", &transport_identity, true));
    let manager = transport.iface_manager();
    let mut record = InterfaceRecord {
        kind: iface.kind.clone(),
        enabled: true,
        host: None,
        port: None,
        name: iface.name.clone(),
        settings: iface.settings_json(),
    };
    let mut startup_failures = Vec::new();
    let mut kiss_runtime_refreshes = Vec::new();

    let started = startup_kiss(
        &args,
        iface,
        "ax25-main",
        &manager,
        &mut record,
        &mut startup_failures,
        &mut kiss_runtime_refreshes,
    )
    .await;

    assert!(started);
    assert!(startup_failures.is_empty());
    assert_eq!(kiss_runtime_refreshes.len(), 1);
    let runtime = record
        .settings
        .as_ref()
        .and_then(|settings| settings.get("_runtime"))
        .expect("runtime settings");
    assert_eq!(runtime["startup_status"].as_str(), Some("spawned"));
    let runtime_iface =
        runtime["iface"].as_str().expect("runtime iface").trim_matches('/').to_string();
    let runtime_iface = AddressHash::new_from_hex_string(&runtime_iface).expect("iface hash");
    assert_eq!(manager.lock().await.role(&runtime_iface), Some(IfaceRole::Unicast));
    let kiss_status = &runtime["kiss"]["status"];
    assert_eq!(kiss_status["link_state"].as_str(), Some("configured"));
    assert_eq!(kiss_status["bearer"].as_str(), Some("serial"));
    assert_eq!(kiss_status["device"].as_str(), Some("/dev/does-not-exist-ax25"));
    assert_eq!(kiss_status["baud_rate"].as_u64(), Some(1200));
    assert_eq!(kiss_status["ax25"].as_bool(), Some(true));
    assert_eq!(kiss_status["callsign"].as_str(), Some("N0CALL"));
    assert_eq!(kiss_status["ssid"].as_u64(), Some(1));
    assert_eq!(kiss_status["iface"].as_str(), Some(runtime_iface.to_string().as_str()));
    assert_eq!(kiss_runtime_refreshes[0].runtime_iface, runtime_iface);
    assert_eq!(kiss_runtime_refreshes[0].runtime_key, "kiss");
}

#[tokio::test]
async fn kiss_tcp_client_startup_marks_spawned_unicast_without_strict_preflight() {
    let cfg = reticulum_daemon::config::DaemonConfig::from_toml(
        r#"
interfaces = [
  { type = "kiss_tcp_client", enabled = true, name = "kiss-wifi", host = "127.0.0.1", port = 65535, flow_control = true }
]
"#,
    )
    .expect("parse kiss tcp client config");
    let args = test_args();
    let iface = &cfg.interfaces[0];
    let identity = rns_core::identity::PrivateIdentity::new_from_rand(rand_core::OsRng);
    let transport_identity = to_transport_private_identity(&identity);
    let transport = Transport::new(TransportConfig::new("test", &transport_identity, true));
    let manager = transport.iface_manager();
    let mut record = InterfaceRecord {
        kind: iface.kind.clone(),
        enabled: true,
        host: iface.host.clone(),
        port: iface.port,
        name: iface.name.clone(),
        settings: iface.settings_json(),
    };
    let mut startup_failures = Vec::new();
    let mut kiss_runtime_refreshes = Vec::new();

    let started = startup_kiss_tcp_client(
        &args,
        iface,
        "kiss-wifi",
        &manager,
        &mut record,
        &mut startup_failures,
        &mut kiss_runtime_refreshes,
    )
    .await;

    assert!(started);
    assert!(startup_failures.is_empty());
    assert_eq!(kiss_runtime_refreshes.len(), 1);
    let runtime = record
        .settings
        .as_ref()
        .and_then(|settings| settings.get("_runtime"))
        .expect("runtime settings");
    assert_eq!(runtime["startup_status"].as_str(), Some("spawned"));
    let runtime_iface =
        runtime["iface"].as_str().expect("runtime iface").trim_matches('/').to_string();
    let runtime_iface = AddressHash::new_from_hex_string(&runtime_iface).expect("iface hash");
    assert_eq!(manager.lock().await.role(&runtime_iface), Some(IfaceRole::Unicast));
    let kiss_status = &runtime["kiss_tcp"]["status"];
    assert_eq!(kiss_status["link_state"].as_str(), Some("configured"));
    assert_eq!(kiss_status["bearer"].as_str(), Some("tcp"));
    assert_eq!(kiss_status["endpoint"].as_str(), Some("127.0.0.1:65535"));
    assert_eq!(kiss_status["kiss_flow_control"].as_bool(), Some(true));
    assert_eq!(kiss_status["ax25"].as_bool(), Some(false));
    assert_eq!(kiss_status["iface"].as_str(), Some(runtime_iface.to_string().as_str()));
    assert_eq!(kiss_runtime_refreshes[0].runtime_iface, runtime_iface);
    assert_eq!(kiss_runtime_refreshes[0].runtime_key, "kiss_tcp");
}
