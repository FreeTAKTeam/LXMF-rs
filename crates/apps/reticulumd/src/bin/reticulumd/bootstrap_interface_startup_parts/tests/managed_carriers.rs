#[tokio::test]
async fn ble_gatt_spawn_success_marks_running_unicast_without_hardware() {
    let cfg = reticulum_daemon::config::DaemonConfig::from_toml(
        r#"
interfaces = [
  { type = "ble_gatt", enabled = true, name = "ble-main", peripheral_id = "AA:BB:CC:DD:EE:FF", service_uuid = "12345678-1234-1234-1234-1234567890ab", write_char_uuid = "2A37", notify_char_uuid = "2A38", mode = "ap" }
]
"#,
    )
    .expect("parse ble gatt config");
    let iface = &cfg.interfaces[0];
    let identity = rns_core::identity::PrivateIdentity::new_from_rand(rand_core::OsRng);
    let transport_identity = to_transport_private_identity(&identity);
    let transport = Transport::new(TransportConfig::new("test", &transport_identity, true));
    let manager = transport.iface_manager();
    let ble_iface = *manager.lock().await.new_channel_with_role(8, IfaceRole::Unicast).address();
    let mut record = InterfaceRecord {
        kind: iface.kind.clone(),
        enabled: true,
        host: None,
        port: None,
        name: iface.name.clone(),
        settings: iface.settings_json(),
    };

    apply_interface_runtime_config(&mut *manager.lock().await, ble_iface, iface)
        .expect("BLE fixture policy is prepared before status publication");
    mark_ble_spawn_success(iface, "ble-main", &mut record, ble_iface);

    let runtime = record
        .settings
        .as_ref()
        .and_then(|settings| settings.get("_runtime"))
        .expect("runtime settings");
    assert_eq!(runtime["startup_status"].as_str(), Some("spawned"));
    assert_eq!(runtime["runtime_status"].as_str(), Some("running"));
    assert_eq!(runtime["reconnect_attempts"].as_u64(), Some(0));
    let runtime_iface =
        runtime["iface"].as_str().expect("runtime iface").trim_matches('/').to_string();
    let runtime_iface = AddressHash::new_from_hex_string(&runtime_iface).expect("iface hash");
    let manager = manager.lock().await;
    assert_eq!(runtime_iface, ble_iface);
    let ble_status = &runtime["ble_gatt"]["status"];
    assert_eq!(ble_status["link_state"].as_str(), Some("configured"));
    assert_eq!(ble_status["peripheral_id"].as_str(), Some("AA:BB:CC:DD:EE:FF"));
    assert_eq!(ble_status["service_uuid"].as_str(), Some("12345678-1234-1234-1234-1234567890ab"));
    assert_eq!(ble_status["write_char_uuid"].as_str(), Some("2A37"));
    assert_eq!(ble_status["notify_char_uuid"].as_str(), Some("2A38"));
    assert_eq!(ble_status["iface"].as_str(), Some(runtime_iface.to_string().as_str()));
    assert_eq!(manager.role(&runtime_iface), Some(IfaceRole::Unicast));
    assert_eq!(manager.mode(&runtime_iface), Some(InterfaceMode::AccessPoint));
}

#[tokio::test]
async fn reticulum_ble_startup_marks_columba_runtime_status() {
    let cfg = reticulum_daemon::config::DaemonConfig::from_toml(
        r#"
interfaces = [
  { type = "BLEInterface", enabled = true, name = "columba-ble", mode = "ap" }
]
"#,
    )
    .expect("parse reticulum ble config");
    let iface = &cfg.interfaces[0];
    let identity = rns_core::identity::PrivateIdentity::new_from_rand(rand_core::OsRng);
    let transport_identity = to_transport_private_identity(&identity);
    let transport = Transport::new(TransportConfig::new("test", &transport_identity, true));
    let manager = transport.iface_manager();
    let mut local_identity = [0_u8; 16];
    local_identity.copy_from_slice(identity.address_hash().as_slice());
    let mut record = InterfaceRecord {
        kind: iface.kind.clone(),
        enabled: true,
        host: None,
        port: None,
        name: iface.name.clone(),
        settings: iface.settings_json(),
    };
    let mut startup_failures = Vec::new();
    let mut refreshes = Vec::new();

    let started = startup_reticulum_ble(
        iface,
        "columba-ble",
        &manager,
        &mut record,
        &mut startup_failures,
        &mut refreshes,
        Some(local_identity),
    )
    .await;

    assert!(started);
    assert!(startup_failures.is_empty());
    assert_eq!(refreshes.len(), 1);
    let runtime = record
        .settings
        .as_ref()
        .and_then(|settings| settings.get("_runtime"))
        .expect("runtime settings");
    assert_eq!(runtime["startup_status"].as_str(), Some("spawned"));
    assert_eq!(runtime["runtime_status"].as_str(), Some("degraded"));
    let runtime_iface =
        runtime["iface"].as_str().expect("runtime iface").trim_matches('/').to_string();
    let runtime_iface = AddressHash::new_from_hex_string(&runtime_iface).expect("iface hash");
    let manager = manager.lock().await;
    assert_eq!(manager.role(&runtime_iface), Some(IfaceRole::Unicast));
    assert_eq!(manager.mode(&runtime_iface), Some(InterfaceMode::AccessPoint));
    assert_eq!(refreshes[0].runtime_iface, runtime_iface);
    let ble_status = &runtime["reticulum_ble"]["status"];
    assert_eq!(ble_status["link_state"].as_str(), Some("degraded"));
    assert_eq!(ble_status["scan_state"].as_str(), Some("native_backend_pending"));
    assert_eq!(ble_status["advertising_state"].as_str(), Some("native_backend_pending"));
    assert_eq!(ble_status["service_uuid"].as_str(), Some("37145b00-442d-4a94-917f-8f42c5da28e3"));
    assert_eq!(ble_status["tx_char_uuid"].as_str(), Some("37145b00-442d-4a94-917f-8f42c5da28e4"));
    assert_eq!(ble_status["rx_char_uuid"].as_str(), Some("37145b00-442d-4a94-917f-8f42c5da28e5"));
    assert_eq!(ble_status["mtu"].as_u64(), Some(185));
    assert_eq!(ble_status["max_connections"].as_u64(), Some(7));
    assert_eq!(ble_status["enable_central"].as_bool(), Some(true));
    assert_eq!(ble_status["enable_peripheral"].as_bool(), Some(true));
    assert_eq!(ble_status["iface"].as_str(), Some(runtime_iface.to_string().as_str()));
}

#[tokio::test]
async fn rnode_multi_startup_tags_parent_as_multicast_role() {
    let cfg = reticulum_daemon::config::DaemonConfig::from_toml(
        r#"
interfaces = [
  { type = "RNodeMultiInterface", enabled = true, name = "rnode-multi", port = "/dev/does-not-exist-rnode-multi", radio0 = { vport = 2, frequency = 915000000, bandwidth = 125000, spreadingfactor = 9, codingrate = 5, txpower = 17 }, radio1 = { vport = 3, frequency = 920000000, bandwidth = 125000, spreadingfactor = 10, codingrate = 5, txpower = 14 } }
]
"#,
    )
    .expect("parse rnode multi config");
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
    let mut rnode_management_bindings = Vec::new();
    let args = test_args();

    let started = startup_rnode_multi(
        args.strict_interface_startup,
        iface,
        "rnode-multi",
        &manager,
        &mut record,
        &mut startup_failures,
        &mut rnode_management_bindings,
    )
    .await;

    let refresh = started.expect("rnode multi should start");
    assert!(startup_failures.is_empty());
    assert_eq!(rnode_management_bindings.len(), 1);
    assert_eq!(rnode_management_bindings[0].name, "rnode-multi");
    assert!(matches!(
        &rnode_management_bindings[0].handle,
        DaemonRNodeManagementHandle::RNodeMulti { allowed_vports, .. }
            if allowed_vports == &vec![2, 3]
    ));
    let runtime_iface = record
        .settings
        .as_ref()
        .and_then(|settings| settings.get("_runtime"))
        .and_then(|runtime| runtime.get("iface"))
        .and_then(|iface| iface.as_str())
        .expect("runtime iface");
    let runtime_iface =
        AddressHash::new_from_hex_string(runtime_iface.trim_matches('/')).expect("iface hash");
    assert_eq!(refresh.runtime_iface, runtime_iface);
    assert_eq!(manager.lock().await.role(&runtime_iface), Some(IfaceRole::Multicast));
    assert_eq!(rnode_management_bindings[0].runtime_iface, runtime_iface);
    let rnode_multi_runtime = record
        .settings
        .as_ref()
        .and_then(|settings| settings.get("_runtime"))
        .and_then(|runtime| runtime.get("rnode_multi"))
        .expect("rnode multi runtime metadata");
    assert_eq!(rnode_multi_runtime["radio_status"]["selected_vport"].as_u64(), Some(2));
    assert_eq!(rnode_multi_runtime["radio_status"]["stream_state"].as_str(), Some("configured"));
    assert!(rnode_multi_runtime["radio_status"]["last_error"].is_null());
    assert_eq!(
        rnode_multi_runtime["radio_status"]["vports"]
            .as_array()
            .expect("status vports")
            .iter()
            .filter_map(|value| value.as_u64())
            .collect::<Vec<_>>(),
        vec![2, 3]
    );
    assert_eq!(
        rnode_multi_runtime["radio_status"]["subinterfaces"]["2"]["airtime_short_percent"].as_f64(),
        Some(0.0)
    );
    assert!(rnode_multi_runtime["radio_status"]["subinterfaces"]["3"]["spreading_factor"].is_null());
}

#[tokio::test]
async fn pipe_startup_embeds_runtime_status() {
    let cfg = reticulum_daemon::config::DaemonConfig::from_toml(
        r#"
interfaces = [
  { type = "PipeInterface", enabled = true, name = "pipe-main", command = "cat", respawn_delay = 0.1 }
]
"#,
    )
    .expect("parse pipe config");
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

    let started =
        startup_pipe(iface, "pipe-main", &manager, &mut record, &mut startup_failures).await;

    assert!(started.is_some());
    assert!(startup_failures.is_empty());
    let runtime_iface = record
        .settings
        .as_ref()
        .and_then(|settings| settings.get("_runtime"))
        .and_then(|runtime| runtime.get("iface"))
        .and_then(|iface| iface.as_str())
        .expect("runtime iface");
    let runtime_iface =
        AddressHash::new_from_hex_string(runtime_iface.trim_matches('/')).expect("iface hash");
    assert_eq!(manager.lock().await.role(&runtime_iface), Some(IfaceRole::Unicast));
    let pipe_status = &record.settings.as_ref().expect("settings")["_runtime"]["pipe"]["status"];
    assert_eq!(pipe_status["command"].as_str(), Some("cat"));
    assert_eq!(pipe_status["process_state"].as_str(), Some("configured"));
    assert_eq!(pipe_status["pipe_is_open"].as_bool(), Some(false));
    assert_eq!(pipe_status["respawn_attempts"].as_u64(), Some(0));
    assert!(pipe_status["last_error"].is_null());
}

#[tokio::test]
async fn weave_startup_tags_parent_as_multicast_role() {
    let cfg = reticulum_daemon::config::DaemonConfig::from_toml(
        r#"
interfaces = [
  { type = "WeaveInterface", enabled = true, name = "weave-main", port = "/dev/does-not-exist-weave" }
]
"#,
    )
    .expect("parse weave config");
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

    let started = startup_weave(
        args.strict_interface_startup,
        iface,
        "weave-main",
        &manager,
        &mut record,
        &mut startup_failures,
    )
    .await;

    let refresh = started.expect("weave should start");
    assert!(startup_failures.is_empty());
    let runtime_iface = record
        .settings
        .as_ref()
        .and_then(|settings| settings.get("_runtime"))
        .and_then(|runtime| runtime.get("iface"))
        .and_then(|iface| iface.as_str())
        .expect("runtime iface");
    let runtime_iface =
        AddressHash::new_from_hex_string(runtime_iface.trim_matches('/')).expect("iface hash");
    assert_eq!(refresh.runtime_iface, runtime_iface);
    assert!(refresh.handle.try_set_remote_display(Some([0x10, 0x20, 0x30, 0x40]), true).is_ok());
    assert_eq!(manager.lock().await.role(&runtime_iface), Some(IfaceRole::Multicast));
    let weave_runtime = record
        .settings
        .as_ref()
        .and_then(|settings| settings.get("_runtime"))
        .and_then(|runtime| runtime.get("weave"))
        .expect("weave runtime metadata");
    assert_eq!(weave_runtime["status"]["device"].as_str(), Some("/dev/does-not-exist-weave"));
    assert_eq!(weave_runtime["status"]["link_state"].as_str(), Some("configured"));
    assert_eq!(weave_runtime["status"]["endpoint_count"].as_u64(), Some(0));
    assert!(weave_runtime["status"]["display"].is_null());
    assert!(weave_runtime["status"]["device_stats"].is_null());
}

#[tokio::test]
async fn i2p_startup_tags_parent_as_multicast_role() {
    let cfg = reticulum_daemon::config::DaemonConfig::from_toml(
        r#"
interfaces = [
  { type = "I2PInterface", enabled = true, name = "i2p-main", peers = ["exampledestination.b32.i2p"] }
]
"#,
    )
    .expect("parse i2p config");
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

    let started = startup_i2p(
        args.strict_interface_startup,
        iface,
        "i2p-main",
        &manager,
        &mut record,
        &mut startup_failures,
        std::path::Path::new("."),
        None,
    )
    .await;

    assert!(started.is_some());
    assert!(startup_failures.is_empty());
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
    let tunnel_status = record
        .settings
        .as_ref()
        .and_then(|settings| settings.get("_runtime"))
        .and_then(|runtime| runtime.get("i2p"))
        .and_then(|i2p| i2p.get("tunnel_status"))
        .expect("i2p tunnel status metadata");
    assert_eq!(tunnel_status["sam_endpoint"].as_str(), Some("127.0.0.1:7656"));
    assert_eq!(tunnel_status["configured_peer_count"].as_u64(), Some(1));
    assert_eq!(tunnel_status["accept_state"].as_str(), Some("closed"));
    let peers = tunnel_status["peers"].as_array().expect("peer rows");
    assert_eq!(peers.len(), 1);
    assert_eq!(peers[0]["peer"].as_str(), Some("exampledestination.b32.i2p"));
    assert_eq!(peers[0]["state"].as_str(), Some("configured"));
}

#[tokio::test]
async fn i2p_startup_reports_persisted_reachable_endpoint_metadata() {
    let root =
        std::env::temp_dir().join(format!("lxmfrs-i2p-startup-metadata-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(root.as_path());
    let private_key = fake_i2p_private_key();
    let identity_hash = [0x42_u8; 16];
    let key_path = rns_transport::iface::i2p::i2p_private_key_old_format_path(&root, "i2p-main");
    std::fs::create_dir_all(key_path.parent().expect("key dir")).expect("key dir");
    std::fs::write(key_path.as_path(), private_key.as_bytes()).expect("write key");
    let expected_endpoint =
        rns_transport::iface::i2p::i2p_b32_from_private_destination(&private_key)
            .expect("expected endpoint");

    let cfg = reticulum_daemon::config::DaemonConfig::from_toml(&format!(
        r#"
interfaces = [
  {{ type = "I2PInterface", enabled = true, name = "i2p-main", connectable = true, storagepath = {} }}
]
"#,
        toml::Value::String(root.to_string_lossy().into_owned())
    ))
    .expect("parse i2p config");
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

    let started = startup_i2p(
        args.strict_interface_startup,
        iface,
        "i2p-main",
        &manager,
        &mut record,
        &mut startup_failures,
        std::path::Path::new("."),
        Some(identity_hash),
    )
    .await;

    assert!(started.is_some());
    assert!(startup_failures.is_empty());
    let i2p = record
        .settings
        .as_ref()
        .and_then(|settings| settings.get("_runtime"))
        .and_then(|runtime| runtime.get("i2p"))
        .expect("i2p runtime metadata");
    assert_eq!(i2p.get("connectable").and_then(|value| value.as_bool()), Some(true));
    assert_eq!(i2p.get("private_key_persisted").and_then(|value| value.as_bool()), Some(true));
    assert_eq!(
        i2p.get("reachable_endpoint").and_then(|value| value.as_str()),
        Some(expected_endpoint.as_str())
    );
    assert_eq!(
        i2p.get("private_key_path").and_then(|value| value.as_str()),
        Some(key_path.to_string_lossy().as_ref())
    );
    let _ = std::fs::remove_dir_all(root.as_path());
}

#[tokio::test]
async fn i2p_startup_uses_daemon_storage_path_when_config_omits_storagepath() {
    let root = std::env::temp_dir()
        .join(format!("lxmfrs-i2p-default-storage-metadata-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(root.as_path());
    let private_key = fake_i2p_private_key();
    let identity_hash = [0x42_u8; 16];
    let key_path = rns_transport::iface::i2p::i2p_private_key_old_format_path(&root, "i2p-main");
    std::fs::create_dir_all(key_path.parent().expect("key dir")).expect("key dir");
    std::fs::write(key_path.as_path(), private_key.as_bytes()).expect("write key");
    let expected_endpoint =
        rns_transport::iface::i2p::i2p_b32_from_private_destination(&private_key)
            .expect("expected endpoint");

    let cfg = reticulum_daemon::config::DaemonConfig::from_toml(
        r#"
interfaces = [
  { type = "I2PInterface", enabled = true, name = "i2p-main", connectable = true }
]
"#,
    )
    .expect("parse i2p config");
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

    let started = startup_i2p(
        args.strict_interface_startup,
        iface,
        "i2p-main",
        &manager,
        &mut record,
        &mut startup_failures,
        root.as_path(),
        Some(identity_hash),
    )
    .await;

    assert!(started.is_some());
    assert!(startup_failures.is_empty());
    let i2p = record
        .settings
        .as_ref()
        .and_then(|settings| settings.get("_runtime"))
        .and_then(|runtime| runtime.get("i2p"))
        .expect("i2p runtime metadata");
    assert_eq!(i2p.get("private_key_persisted").and_then(|value| value.as_bool()), Some(true));
    assert_eq!(
        i2p.get("reachable_endpoint").and_then(|value| value.as_str()),
        Some(expected_endpoint.as_str())
    );
    assert_eq!(
        i2p.get("private_key_path").and_then(|value| value.as_str()),
        Some(key_path.to_string_lossy().as_ref())
    );
    let _ = std::fs::remove_dir_all(root.as_path());
}

#[tokio::test]
async fn i2p_startup_reports_generated_reachable_endpoint_metadata() {
    let private_key = fake_i2p_private_key();
    let expected_endpoint =
        rns_transport::iface::i2p::i2p_b32_from_private_destination(&private_key)
            .expect("expected endpoint");
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind fake SAM");
    let sam_addr = listener.local_addr().expect("local addr");
    let private_key_for_server = private_key.clone();
    let server = tokio::spawn(async move {
        let (socket, _) = listener.accept().await.expect("accept destination generation");
        let mut reader = BufReader::new(socket);
        let mut lines = Vec::new();
        let responses = [
            "HELLO REPLY RESULT=OK VERSION=3.3\n".to_string(),
            format!("DEST REPLY PUB=public-destination PRIV={private_key_for_server}\n"),
        ];
        for response in responses {
            let mut line = String::new();
            reader.read_line(&mut line).await.expect("read command");
            lines.push(line.trim_end().to_string());
            reader.get_mut().write_all(response.as_bytes()).await.expect("write response");
        }
        lines
    });

    let root = std::env::temp_dir()
        .join(format!("lxmfrs-i2p-startup-generated-metadata-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(root.as_path());
    let identity_hash = [0x42_u8; 16];
    let expected_key_path = rns_transport::iface::i2p::i2p_private_key_new_format_path(
        &root,
        "i2p-main",
        &identity_hash,
    );
    let cfg = reticulum_daemon::config::DaemonConfig::from_toml(&format!(
        r#"
interfaces = [
  {{ type = "I2PInterface", enabled = true, name = "i2p-main", connectable = true, sam_ip = "{}", sam_port = {}, storagepath = {} }}
]
"#,
        sam_addr.ip(),
        sam_addr.port(),
        toml::Value::String(root.to_string_lossy().into_owned())
    ))
    .expect("parse i2p config");
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

    let started = startup_i2p(
        args.strict_interface_startup,
        iface,
        "i2p-main",
        &manager,
        &mut record,
        &mut startup_failures,
        std::path::Path::new("."),
        Some(identity_hash),
    )
    .await;

    assert!(started.is_some());
    assert!(startup_failures.is_empty());
    let i2p = record
        .settings
        .as_ref()
        .and_then(|settings| settings.get("_runtime"))
        .and_then(|runtime| runtime.get("i2p"))
        .expect("i2p runtime metadata");
    assert_eq!(i2p.get("private_key_persisted").and_then(|value| value.as_bool()), Some(true));
    assert_eq!(
        i2p.get("reachable_endpoint").and_then(|value| value.as_str()),
        Some(expected_endpoint.as_str())
    );
    assert_eq!(
        i2p.get("private_key_path").and_then(|value| value.as_str()),
        Some(expected_key_path.to_string_lossy().as_ref())
    );
    assert_eq!(std::fs::read_to_string(expected_key_path).expect("stored key"), private_key);
    let lines = server.await.expect("server lines");
    assert_eq!(lines[0], "HELLO VERSION MIN=3.0 MAX=3.3");
    assert_eq!(lines[1], "DEST GENERATE SIGNATURE_TYPE=7");
    let _ = std::fs::remove_dir_all(root.as_path());
}

#[tokio::test]
async fn strict_i2p_startup_records_fake_sam_handshake_rejection_without_spawning() {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind fake SAM");
    let sam_addr = listener.local_addr().expect("fake SAM address");
    let server = tokio::spawn(async move {
        let (socket, _) = listener.accept().await.expect("accept preflight");
        let mut reader = BufReader::new(socket);
        let mut command = String::new();
        reader.read_line(&mut command).await.expect("read HELLO");
        reader
            .get_mut()
            .write_all(b"HELLO REPLY RESULT=I2P_ERROR MESSAGE=disabled\n")
            .await
            .expect("reject HELLO");
        command
    });
    let cfg = reticulum_daemon::config::DaemonConfig::from_toml(&format!(
        r#"interfaces = [{{ type = "I2PInterface", enabled = true, name = "i2p-rejected", sam_ip = "{}", sam_port = {} }}]"#,
        sam_addr.ip(), sam_addr.port()
    ))
    .expect("parse I2P config");
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
    let mut failures = Vec::new();
    let mut args = test_args();
    args.strict_interface_startup = true;

    let started = startup_i2p(
        args.strict_interface_startup,
        iface,
        "i2p-rejected",
        &manager,
        &mut record,
        &mut failures,
        std::path::Path::new("."),
        None,
    )
    .await;

    let command = server.await.expect("fake SAM task");
    assert_eq!(command.trim_end(), "HELLO VERSION MIN=3.0 MAX=3.3");
    assert!(started.is_none());
    assert!(manager.lock().await.interface_hashes().is_empty());
    assert_eq!(failures.len(), 1);
    assert!(failures[0].error.contains("preflight hello failed"));
    assert!(failures[0].error.contains("disabled"));
}

fn fake_i2p_private_key() -> String {
    let mut private = vec![0_u8; 500];
    for (index, byte) in private.iter_mut().enumerate() {
        *byte = index as u8;
    }
    private[385] = 0;
    private[386] = 3;
    let engine = base64::engine::GeneralPurpose::new(
        &base64::alphabet::Alphabet::new(
            "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-~",
        )
        .expect("alphabet"),
        base64::engine::general_purpose::PAD,
    );
    engine.encode(private)
}

fn test_args() -> Args {
    Args {
        rpc: None,
        db: PathBuf::from("reticulum.db"),
        config: None,
        identity: None,
        announce_interval_secs: 0,
        transport: Some("127.0.0.1:0".to_string()),
        strict_interface_startup: false,
        rpc_tls_cert: None,
        rpc_tls_key: None,
        rpc_tls_client_ca: None,
        rpc_token_issuer: None,
        rpc_token_audience: None,
        rpc_token_secret_env: None,
        rpc_token_jti_ttl_ms: 60_000,
        rpc_token_clock_skew_ms: 5_000,
        rpc_unix: None,
        #[cfg(feature = "zmq-pipeline-rpc")]
        zmq_rpc_command: None,
        zmq_durable_broker: false,
        zmq_broker_budget_bytes: 512 * 1024 * 1024,
        zmq_broker_restored: false,
        #[cfg(feature = "zmq-pipeline-rpc")]
        zmq_rpc_endpoint: None,
        meshchat_bind: None,
        meshchat_assets: None,
    }
}
