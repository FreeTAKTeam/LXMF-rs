use super::*;
use crate::configured_interface_runtime::{ConfiguredInterfaceStartup, NativeRuntimeStatus};
use serde_json::json;

#[tokio::test(start_paused = true)]
async fn owner_publication_is_immediate_then_two_seconds_with_delay_and_preserves_degraded_state() {
    let identity = rns_transport::identity::PrivateIdentity::new_from_name("status-owner");
    let transport = Arc::new(Transport::new(rns_transport::transport::TransportConfig::new(
        "status-owner",
        &identity,
        true,
    )));
    let manager = transport.iface_manager();
    let channel = manager.lock().await.new_channel(2);
    let address = *channel.address();
    let adapter = UdpInterface::new("127.0.0.1:0", None);
    let status = adapter.runtime_status_handle();
    status.update(|status| status.bytes_rx = 1);
    let mut record = InterfaceRecord {
        kind: "udp".into(),
        name: Some("retained-status".into()),
        enabled: true,
        host: Some("127.0.0.1".into()),
        port: Some(0),
        settings: None,
    };
    mark_interface_startup_status(&mut record, "spawned", None, Some(&address.to_string()));
    mark_interface_runtime_fields(&mut record, "degraded", 0);
    let daemon = Arc::new(RpcDaemon::test_instance());
    daemon.replace_interfaces(vec![record.clone()]);
    let mut owner = InterfaceHotApplyBridge::spawn_with_configured_roots(
        transport,
        vec![ConfiguredInterfaceStartup {
            index: 0,
            record,
            native: Some(NativeInterfaceRuntime::new(
                address,
                Some(NativeRuntimeStatus::Udp(status.clone())),
            )),
        }],
        Arc::downgrade(&daemon),
        None,
        None,
    );
    let value = || {
        daemon.interface_records()[0].settings.as_ref().expect("settings")["_runtime"]["udp"]
            ["status"]["bytes_rx"]
            .clone()
    };
    for _ in 0..20 {
        tokio::task::yield_now().await;
    }
    assert_eq!(value(), 1, "first publication needs no clock advance or discovery");
    status.update(|status| status.bytes_rx = 2);
    tokio::time::advance(std::time::Duration::from_millis(1999)).await;
    for _ in 0..20 {
        tokio::task::yield_now().await;
    }
    assert_eq!(value(), 1);
    tokio::time::advance(std::time::Duration::from_millis(1)).await;
    for _ in 0..20 {
        tokio::task::yield_now().await;
    }
    assert_eq!(value(), 2);
    status.update(|status| status.bytes_rx = 3);
    tokio::time::advance(std::time::Duration::from_secs(7)).await;
    for _ in 0..20 {
        tokio::task::yield_now().await;
    }
    assert_eq!(value(), 3);
    status.update(|status| status.bytes_rx = 4);
    tokio::time::advance(std::time::Duration::from_secs(1)).await;
    for _ in 0..20 {
        tokio::task::yield_now().await;
    }
    assert_eq!(value(), 3, "missed publication delays the next tick instead of catching up");
    tokio::time::advance(std::time::Duration::from_secs(1)).await;
    for _ in 0..20 {
        tokio::task::yield_now().await;
    }
    assert_eq!(value(), 4);
    assert_eq!(
        daemon.interface_records()[0].settings.as_ref().expect("settings")["_runtime"]
            ["runtime_status"],
        "degraded"
    );
    owner.shutdown().await.expect("joined owner");
}

#[tokio::test]
async fn projection_rebinds_identical_failed_and_active_startup_occurrences() {
    let manager = Arc::new(tokio::sync::Mutex::new(InterfaceManager::new(8)));
    let channel = manager.lock().await.new_channel(2);
    let address = *channel.address();
    let mut first = InterfaceRecord {
        kind: "serial".into(),
        name: None,
        enabled: true,
        host: None,
        port: None,
        settings: Some(json!({"device":"same-private-test-device"})),
    };
    let mut second = first.clone();
    mark_interface_startup_status(&mut first, "failed", Some("first occurrence rejected"), None);
    mark_interface_startup_status(&mut second, "spawned", None, Some(&address.to_string()));
    let disabled = InterfaceRecord {
        kind: "tcp_client".into(),
        name: Some("old".into()),
        enabled: false,
        host: Some("127.0.0.1".into()),
        port: Some(4242),
        settings: None,
    };
    let records = vec![
        disabled.clone(),
        first.clone(),
        InterfaceRecord { name: Some("middle".into()), ..disabled.clone() },
        second.clone(),
    ];
    let daemon = Arc::new(RpcDaemon::test_instance());
    daemon.replace_interfaces(records);
    let mut managed = mutation_worker::initial_interfaces(
        Vec::new(),
        Some(vec![
            ConfiguredInterfaceStartup { index: 1, record: first.clone(), native: None },
            ConfiguredInterfaceStartup {
                index: 3,
                record: second.clone(),
                native: Some(NativeInterfaceRuntime::new(address, None)),
            },
        ]),
    );
    let readonly = InterfaceRecord {
        kind: "FutureDisplayOnly".into(),
        name: Some("readonly".into()),
        ..disabled.clone()
    };
    let desired = vec![readonly.clone(), disabled];
    let projected =
        record_publication::preserve_unmanaged_records(desired, &mut managed, Some(&daemon));
    daemon.replace_interfaces(projected);
    discovery_runtime::publish_live_runtime_records(&managed, &daemon);
    let after = daemon.interface_records();
    assert_eq!(after.len(), 4);
    assert_eq!(after[1], first);
    assert_eq!(after[3], second);
    assert!(
        !managed.values().any(|entry| entry.record.kind == readonly.kind),
        "display-only submission acquires no native authority"
    );
    let mut active = managed.remove(&ManagedInterfaceKey::Startup(3)).expect("exact second recipe");
    native_shutdown::stop_owned_native(
        &manager,
        None,
        active.native.take().expect("real registered native root"),
    )
    .await
    .expect("native absence");
    active.record.enabled = false;
    record_publication::clear_inactive_runtime(&mut active.record, "detached", "stopped", None);
    managed.insert(ManagedInterfaceKey::Startup(3), active);
    daemon.replace_interfaces(record_publication::preserve_unmanaged_records(
        Vec::new(),
        &mut managed,
        Some(&daemon),
    ));
    discovery_runtime::publish_live_runtime_records(&managed, &daemon);
    let after = daemon.interface_records();
    assert_eq!(after.iter().filter(|record| record.kind == "serial").count(), 2);
    let serial = after.iter().filter(|record| record.kind == "serial").collect::<Vec<_>>();
    assert_eq!(*serial[0], first);
    assert!(!serial[1].enabled);
    assert!(serial[1].settings.as_ref().expect("settings")["_runtime"]["iface"].is_null());
}
