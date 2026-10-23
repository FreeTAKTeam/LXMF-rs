use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};

struct Stub;
impl rns_transport::iface::Interface for Stub {
    fn mtu() -> usize {
        500
    }
}
struct CountDrop(Arc<AtomicUsize>);
impl Drop for CountDrop {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

#[tokio::test(start_paused = true)]
async fn named_retirement_joins_every_uncooperative_child_before_returning_its_error() {
    let manager = Arc::new(tokio::sync::Mutex::new(InterfaceManager::new(8)));
    let dropped = Arc::new(AtomicUsize::new(0));
    let worker = |dropped: Arc<AtomicUsize>| {
        move |context| async move {
            let _context: rns_transport::iface::InterfaceContext<Stub> = context;
            let _drop = CountDrop(dropped);
            std::future::pending::<()>().await;
        }
    };
    let address = manager.lock().await.spawn(Stub, worker(dropped.clone()));
    for _ in 0..2 {
        manager
            .lock()
            .await
            .spawn_inheriting(address, Stub, worker(dropped.clone()))
            .expect("child");
    }
    let unrelated = manager.lock().await.new_channel(2);
    tokio::task::yield_now().await;
    let mut record = InterfaceRecord {
        kind: "pipe".into(),
        name: Some("stuck".into()),
        enabled: true,
        host: None,
        port: None,
        settings: Some(serde_json::json!({"command":"cat"})),
    };
    mark_interface_startup_status(&mut record, "spawned", None, Some(&address.to_string()));
    let daemon = Arc::new(RpcDaemon::test_instance());
    daemon.replace_interfaces(vec![record.clone()]);
    let mut managed = HashMap::from([(
        configured_key("stuck"),
        ManagedHotApplyInterface::active(record, address, None, ManagedInterfaceOrigin::Configured),
    )]);
    let error = named_management::apply(
        NativeInterfaceContext { iface_manager: &manager, transport: None, reconnect_events: None },
        &mut managed,
        named_management::NamedInterfaceAction::new("detach", "stuck", None),
        &HotApplyRuntimeRefreshes::new(),
        Some(&Arc::downgrade(&daemon)),
    )
    .await
    .expect_err("uncooperative native workers report timeout");
    assert_eq!(error.kind(), io::ErrorKind::TimedOut);
    assert_eq!(dropped.load(Ordering::SeqCst), 3, "all futures settled despite first timeout");
    assert_eq!(
        manager.lock().await.interface_hashes(),
        std::collections::HashSet::from([*unrelated.address()])
    );
    assert!(managed.is_empty());
    let records = daemon.interface_records();
    assert!(!records[0].enabled);
    assert!(
        records[0].settings.as_ref().expect("settings")["_runtime"]["iface"].is_null(),
        "diagnostic failure still publishes native absence"
    );
}

#[tokio::test(start_paused = true)]
async fn auxiliary_failures_are_reported_after_all_auxiliary_tasks_are_joined() {
    let manager = Arc::new(tokio::sync::Mutex::new(InterfaceManager::new(8)));
    let channel = manager.lock().await.new_channel(2);
    let address = *channel.address();
    let dropped = Arc::new(AtomicUsize::new(0));
    let mut native = NativeInterfaceRuntime::new(address, None);
    let counter = dropped.clone();
    native.auxiliary.push(tokio::spawn(async move {
        let _drop = CountDrop(counter);
        panic!("injected auxiliary failure");
    }));
    for _ in 0..2 {
        let counter = dropped.clone();
        native.auxiliary.push(tokio::spawn(async move {
            let _drop = CountDrop(counter);
            std::future::pending::<()>().await;
        }));
    }
    tokio::task::yield_now().await;
    let error = native_shutdown::stop_owned_native(&manager, None, native)
        .await
        .expect_err("auxiliary failure must not become successful shutdown");
    assert_eq!(error.kind(), io::ErrorKind::Other);
    assert!(error.to_string().contains(&address.to_string()));
    assert_eq!(dropped.load(Ordering::SeqCst), 3);
    assert!(manager.lock().await.interface_hashes().is_empty());
}

#[tokio::test(start_paused = true)]
async fn cancelled_shutdown_keeps_the_unique_worker_join_until_completion() {
    let transport = Arc::new(Transport::new(rns_transport::transport::TransportConfig::default()));
    let manager = transport.iface_manager();
    let dropped = Arc::new(AtomicUsize::new(0));
    let counter = dropped.clone();
    let address = manager.lock().await.spawn(Stub, move |context| async move {
        let _context: rns_transport::iface::InterfaceContext<Stub> = context;
        let _drop = CountDrop(counter);
        std::future::pending::<()>().await;
    });
    let record = InterfaceRecord {
        kind: "serial".into(),
        name: Some("cancel-owner".into()),
        enabled: true,
        host: None,
        port: None,
        settings: None,
    };
    let daemon = Arc::new(RpcDaemon::test_instance());
    daemon.replace_interfaces(vec![record.clone()]);
    let mut owner = InterfaceHotApplyBridge::spawn_with_configured_roots(
        transport,
        vec![crate::configured_interface_runtime::ConfiguredInterfaceStartup {
            index: 0,
            record,
            native: Some(NativeInterfaceRuntime::new(address, None)),
        }],
        Arc::downgrade(&daemon),
        None,
        None,
    );
    daemon.set_interface_mutation_bridge(Arc::new(owner.bridge.clone()));
    tokio::task::yield_now().await;
    assert!(tokio::time::timeout(std::time::Duration::from_secs(1), owner.shutdown())
        .await
        .is_err());
    assert!(owner.completion.is_some(), "cancelled await cannot detach the unique original worker");
    let response = daemon
        .handle_rpc(rns_rpc::RpcRequest {
            id: 1,
            method: "set_interfaces".into(),
            params: Some(serde_json::json!({"interfaces":[]})),
        })
        .expect("retiring bridge response");
    assert!(response.error.is_some(), "retiring owner cannot acknowledge new queued work");
    let error = owner.shutdown().await.expect_err("native worker timeout remains observable");
    assert_eq!(error.kind(), io::ErrorKind::TimedOut);
    assert_eq!(dropped.load(Ordering::SeqCst), 1);
    assert!(owner.completion.is_none());
    assert!(manager.lock().await.interface_hashes().is_empty());
    let response = daemon
        .handle_rpc(rns_rpc::RpcRequest {
            id: 1,
            method: "set_interfaces".into(),
            params: Some(serde_json::json!({"interfaces":[]})),
        })
        .expect("closed bridge response");
    assert!(response.error.is_some());
}
