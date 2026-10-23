use super::*;
use crate::bridge_rnode_management::{
    DaemonRNodeManagementBinding, DaemonRNodeManagementBridge, DaemonRNodeManagementHandle,
};
use crate::configured_interface_runtime::{ConfiguredInterfaceStartup, NativeRuntimeStatus};
use rns_rpc::RNodeManagementBridge;
use rns_transport::iface::lora::{LoraConfig, LoraInterface, LoraRuntimeStatusHandle};
use rns_transport::iface::weave::WeaveInterface;
use rns_transport::iface::{Interface, InterfaceContext, InterfaceMode};
use serde_json::json;

// Retain real adapter queues without opening a radio or serial device.
async fn retain_control_receiver<T: Interface + Send + 'static>(context: InterfaceContext<T>) {
    context.channel.stop.cancelled().await;
    drop(context);
}

async fn lora_root(
    manager: &Arc<tokio::sync::Mutex<InterfaceManager>>,
    index: usize,
    name: &str,
) -> ConfiguredInterfaceStartup {
    let adapter = LoraInterface::new("unused-control-fixture", 115200, LoraConfig::us915_default());
    let handle = adapter.rnode_management_handle();
    let (address, inner) = manager.lock().await.spawn_as_with_mode_and_handle(
        adapter,
        retain_control_receiver,
        IfaceRole::Unicast,
        InterfaceMode::Full,
    );
    let mut native = NativeInterfaceRuntime::new(
        address,
        Some(NativeRuntimeStatus::Lora(crate::bootstrap::LoraRuntimeStatusSource::Lora(
            LoraRuntimeStatusHandle::new(inner),
        ))),
    );
    native.rnode.push((name.into(), DaemonRNodeManagementHandle::Lora(handle)));
    let mut record = InterfaceRecord {
        kind: "lora".into(),
        name: Some(name.into()),
        enabled: true,
        host: None,
        port: None,
        settings: None,
    };
    mark_interface_startup_status(&mut record, "spawned", None, Some(&address.to_string()));
    ConfiguredInterfaceStartup { index, record, native: Some(native) }
}

fn command(daemon: &RpcDaemon, selector: &str) -> rns_rpc::RpcResponse {
    daemon
        .handle_rpc(rns_rpc::RpcRequest {
            id: 1,
            method: "rnode_management".into(),
            params: Some(json!({"iface":selector,"command":"blink","pattern":1})),
        })
        .expect("control RPC")
}

#[tokio::test]
async fn native_control_projection_retires_old_receivers_and_preserves_other_bindings() {
    let manager = Arc::new(tokio::sync::Mutex::new(InterfaceManager::new(8)));
    let first = lora_root(&manager, 0, "first").await;
    let second = lora_root(&manager, 1, "second").await;
    let native = first.native.as_ref().expect("first root");
    let old_address = native.address;
    let old_bridge =
        Arc::new(DaemonRNodeManagementBridge::new(vec![DaemonRNodeManagementBinding {
            runtime_iface: old_address,
            name: "first".into(),
            handle: native.rnode[0].1.clone(),
        }]));
    let records = vec![first.record.clone(), second.record.clone()];
    let daemon = Arc::new(RpcDaemon::test_instance());
    daemon.replace_interfaces(records);
    let mut managed = mutation_worker::initial_interfaces(Vec::new(), Some(vec![first, second]));
    record_publication::publish_control_bindings(&managed, &daemon);
    assert!(command(&daemon, "first").error.is_none());
    assert!(command(&daemon, "second").error.is_none());
    let first = managed.remove(&ManagedInterfaceKey::Startup(0)).expect("first recipe");
    native_shutdown::stop_owned_native(&manager, None, first.native.expect("native root"))
        .await
        .expect("first retired");
    record_publication::publish_control_bindings(&managed, &daemon);
    assert!(command(&daemon, "first").error.is_some());
    assert!(command(&daemon, &old_address.to_string()).error.is_some());
    assert!(
        old_bridge.dispatch_rnode_management("first", "blink", &json!({"pattern":1})).is_err(),
        "even a retained old bridge cannot queue to the retired receiver"
    );
    assert!(command(&daemon, "second").error.is_none(), "unrelated binding remains usable");
    let second = managed.remove(&ManagedInterfaceKey::Startup(1)).expect("second recipe");
    native_shutdown::stop_owned_native(&manager, None, second.native.expect("native root"))
        .await
        .expect("second retired");
    record_publication::publish_control_bindings(&managed, &daemon);
    assert!(
        command(&daemon, "second").error.is_some(),
        "empty projection replaces the last native bridge"
    );
}

#[tokio::test]
async fn initial_native_controls_are_installed_before_the_worker_can_run() {
    let identity = rns_transport::identity::PrivateIdentity::new_from_name("initial-controls");
    let transport = Arc::new(Transport::new(rns_transport::transport::TransportConfig::new(
        "initial-controls",
        &identity,
        true,
    )));
    let manager = transport.iface_manager();
    let radio = lora_root(&manager, 0, "initial-radio").await;
    let adapter = WeaveInterface::new("unused-control-fixture", manager.clone());
    let status = adapter.runtime_status_handle();
    let handle = adapter.weave_management_handle();
    let (address, _inner) = manager.lock().await.spawn_as_with_mode_and_handle(
        adapter,
        retain_control_receiver,
        IfaceRole::Unicast,
        InterfaceMode::Full,
    );
    drop(_inner);
    let mut native = NativeInterfaceRuntime::new(address, Some(NativeRuntimeStatus::Weave(status)));
    native.weave.push(("initial-weave".into(), handle));
    let weave = ConfiguredInterfaceStartup {
        index: 1,
        record: InterfaceRecord {
            kind: "weave".into(),
            name: Some("initial-weave".into()),
            enabled: true,
            host: None,
            port: None,
            settings: None,
        },
        native: Some(native),
    };
    let daemon = Arc::new(RpcDaemon::test_instance());
    daemon.replace_interfaces(vec![radio.record.clone(), weave.record.clone()]);
    let mut owner = InterfaceHotApplyBridge::spawn_with_configured_roots(
        transport,
        vec![radio, weave],
        Arc::downgrade(&daemon),
        None,
        None,
    );
    // Current-thread execution has not yielded to the newly spawned mutation worker.
    assert!(command(&daemon, "initial-radio").error.is_none());
    let response = daemon
        .handle_rpc(rns_rpc::RpcRequest {
            id: 2,
            method: "weave_remote_display_control".into(),
            params: Some(
                json!({"iface":"initial-weave","enable":true,"remote_switch_id_hex":"10203040"}),
            ),
        })
        .expect("Weave RPC");
    assert!(response.error.is_none(), "{response:?}");
    owner.shutdown().await.expect("all native controls retired");
    assert!(command(&daemon, "initial-radio").error.is_some());
}
