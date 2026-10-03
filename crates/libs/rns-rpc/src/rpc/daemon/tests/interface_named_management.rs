struct RecordingNamedInterfaceBridge {
    calls: std::sync::Mutex<Vec<(String, String)>>,
}

impl RecordingNamedInterfaceBridge {
    fn new() -> Self {
        Self { calls: std::sync::Mutex::new(Vec::new()) }
    }
}

impl InterfaceMutationBridge for RecordingNamedInterfaceBridge {
    fn apply_interfaces(
        &self,
        interfaces: Vec<InterfaceRecord>,
    ) -> Result<Vec<InterfaceRecord>, std::io::Error> {
        Ok(interfaces)
    }

    fn manage_named_interface(
        &self,
        operation: &str,
        name: &str,
    ) -> Result<JsonValue, std::io::Error> {
        self.calls
            .lock()
            .expect("calls mutex poisoned")
            .push((operation.to_string(), name.to_string()));
        Ok(json!({ "name": name, "operation": operation, "complete": true }))
    }
}

#[test]
fn rns_1_5_5_named_management_rejects_invalid_operation_and_name() {
    let daemon = RpcDaemon::test_instance();
    for params in [
        json!({ "operation": "blink", "name": "uplink" }),
        json!({ "operation": "attach", "name": "  " }),
        json!({ "operation": "detach" }),
    ] {
        let response = daemon
            .handle_rpc(rpc_request(101, "manage_interface", params))
            .expect("management response");
        assert_eq!(response.error.as_ref().map(|error| error.code.as_str()), Some("CONFIG_INVALID_INTERFACE"));
    }
}

#[test]
fn rns_1_5_5_named_management_requires_bridge_and_returns_completed_result() {
    let daemon = RpcDaemon::test_instance();
    let request = || rpc_request(102, "manage_interface", json!({ "operation": "attach", "name": "uplink" }));
    let unavailable = daemon.handle_rpc(request()).expect("unavailable management response");
    assert!(unavailable.error.is_some());

    let bridge = Arc::new(RecordingNamedInterfaceBridge::new());
    daemon.set_interface_mutation_bridge(bridge.clone());
    let response = daemon.handle_rpc(request()).expect("completed management response");
    assert!(response.error.is_none());
    assert_eq!(response.result.as_ref().and_then(|result| result.get("complete")).and_then(JsonValue::as_bool), Some(true));
    assert_eq!(
        *bridge.calls.lock().expect("calls mutex poisoned"),
        vec![("attach".to_string(), "uplink".to_string())]
    );
}
