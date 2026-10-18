use super::*;

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
        "discoverable": true,
        "announce_interval": 21600
    }));

    let refreshes = runtime_refreshes();
    apply_hot_apply_interface_records(
        NativeInterfaceContext {
            iface_manager: &iface_manager,
            transport: None,
            reconnect_events: None,
        },
        &mut managed,
        vec![record],
        &refreshes,
        None,
    )
    .await;

    let address = managed
        .get(&super::super::configured_key("loopback"))
        .expect("managed tcp client")
        .native
        .as_ref()
        .expect("active native root")
        .address;
    let manager = iface_manager.lock().await;
    let path_table_identity = manager
        .tcp_client_path_table_metadata(&address)
        .expect("hot-applied TCP client path-table identity");
    assert_eq!(path_table_identity.name, "loopback");
    assert_eq!(path_table_identity.target_host, "127.0.0.1");
    assert_eq!(path_table_identity.target_port, 1);
    assert_eq!(manager.display_name(&address), None);
    assert_eq!(manager.mode(&address), Some(InterfaceMode::Gateway));
    assert_eq!(manager.outgoing(&address), Some(false));
    assert_eq!(manager.announce_pacing(&address), Some((1200, 5)));
    assert_eq!(
        manager.shared_config(&address),
        Some(&InterfaceSharedConfig {
            announce_rate_target: Some(120),
            announce_rate_grace: Some(2),
            announce_rate_penalty: Some(30),
            discoverable: Some(true),
            announce_interval: Some(21_600),
            ..InterfaceSharedConfig::default()
        })
    );
    drop(manager);

    apply_hot_apply_interface_records(
        NativeInterfaceContext {
            iface_manager: &iface_manager,
            transport: None,
            reconnect_events: None,
        },
        &mut managed,
        vec![tcp_record("loopback", "127.0.0.2", 2)],
        &refreshes,
        None,
    )
    .await;

    let updated_address = managed
        .get(&super::super::configured_key("loopback"))
        .expect("replaced TCP client")
        .native
        .as_ref()
        .expect("active native root")
        .address;
    assert_ne!(updated_address, address);
    let manager = iface_manager.lock().await;
    let updated_identity = manager
        .tcp_client_path_table_metadata(&updated_address)
        .expect("updated hot-applied TCP client path-table identity");
    assert_eq!(updated_identity.name, "loopback");
    assert_eq!(updated_identity.target_host, "127.0.0.2");
    assert_eq!(updated_identity.target_port, 2);
    assert_eq!(manager.display_name(&updated_address), None);
}

#[tokio::test(flavor = "current_thread")]
async fn hot_apply_updates_existing_tcp_client_runtime_settings() {
    let iface_manager = Arc::new(tokio::sync::Mutex::new(InterfaceManager::new(8)));
    let address = {
        let mut manager = iface_manager.lock().await;
        *manager.new_channel(8).address()
    };
    let mut managed = HashMap::from([(
        "loopback".to_string().into(),
        ManagedHotApplyInterface::active(
            tcp_record("loopback", "127.0.0.1", 1),
            address,
            None,
            super::super::ManagedInterfaceOrigin::Configured,
        ),
    )]);
    let mut record = tcp_record("loopback", "127.0.0.1", 1);
    record.settings = Some(json!({
        "interface_mode": "access_point",
        "outgoing": false,
        "publish_ifac": false
    }));

    let refreshes = runtime_refreshes();
    apply_hot_apply_interface_records(
        NativeInterfaceContext {
            iface_manager: &iface_manager,
            transport: None,
            reconnect_events: None,
        },
        &mut managed,
        vec![record],
        &refreshes,
        None,
    )
    .await;

    let manager = iface_manager.lock().await;
    assert_eq!(manager.mode(&address), Some(InterfaceMode::AccessPoint));
    assert_eq!(manager.outgoing(&address), Some(false));
    assert_eq!(
        manager.shared_config(&address),
        Some(&InterfaceSharedConfig {
            publish_ifac: Some(false),
            ..InterfaceSharedConfig::default()
        })
    );
}
