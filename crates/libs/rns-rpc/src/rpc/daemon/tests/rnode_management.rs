struct RecordingRNodeManagementBridge {
    calls: std::sync::Mutex<Vec<(String, String, JsonValue)>>,
}

impl RecordingRNodeManagementBridge {
    fn new() -> Self {
        Self { calls: std::sync::Mutex::new(Vec::new()) }
    }

    fn calls(&self) -> Vec<(String, String, JsonValue)> {
        self.calls.lock().expect("calls mutex poisoned").clone()
    }
}

impl RNodeManagementBridge for RecordingRNodeManagementBridge {
    fn dispatch_rnode_management(
        &self,
        iface: &str,
        command: &str,
        params: &JsonValue,
    ) -> Result<JsonValue, std::io::Error> {
        self.calls.lock().expect("calls mutex poisoned").push((
            iface.to_string(),
            command.to_string(),
            params.clone(),
        ));
        Ok(json!({
            "iface": iface,
            "command": command,
            "queued": true,
            "pattern": params.get("pattern").cloned().unwrap_or(JsonValue::Null),
        }))
    }
}

struct FailingRNodeManagementBridge;

impl RNodeManagementBridge for FailingRNodeManagementBridge {
    fn dispatch_rnode_management(
        &self,
        _iface: &str,
        _command: &str,
        _params: &JsonValue,
    ) -> Result<JsonValue, std::io::Error> {
        Err(std::io::Error::new(std::io::ErrorKind::NotFound, "unknown RNode interface"))
    }
}

#[test]
fn rnode_management_rpc_delegates_to_bridge() {
    let daemon = RpcDaemon::test_instance();
    let bridge = Arc::new(RecordingRNodeManagementBridge::new());
    daemon.set_rnode_management_bridge(bridge.clone());

    let response = daemon
        .handle_rpc(rpc_request(
            1,
            "rnode_management",
            json!({
                "iface": "rnode-main",
                "command": "blink",
                "vport": 2,
                "pattern": 3
            }),
        ))
        .expect("rnode management response");

    assert!(response.error.is_none());
    let result = response.result.expect("rnode management result");
    assert_eq!(result["queued"].as_bool(), Some(true));
    assert_eq!(result["iface"].as_str(), Some("rnode-main"));
    assert_eq!(result["command"].as_str(), Some("blink"));
    assert_eq!(result["pattern"].as_u64(), Some(3));
    assert_eq!(
        bridge.calls(),
        vec![(
            "rnode-main".to_string(),
            "blink".to_string(),
            json!({
                "iface": "rnode-main",
                "command": "blink",
                "vport": 2,
                "pattern": 3
            })
        )]
    );
}

#[test]
fn rnode_management_rpc_preserves_guard_params_for_bridge() {
    let daemon = RpcDaemon::test_instance();
    let bridge = Arc::new(RecordingRNodeManagementBridge::new());
    daemon.set_rnode_management_bridge(bridge.clone());

    let response = daemon
        .handle_rpc(rpc_request(
            5,
            "rnode_management",
            json!({
                "iface": "rnode-main",
                "command": "save_config",
                "vport": 2,
                "confirm_persistent": true,
                "confirm_command": "ignored_by_safe_bridge"
            }),
        ))
        .expect("rnode management response");

    assert!(response.error.is_none());
    assert_eq!(
        bridge.calls(),
        vec![(
            "rnode-main".to_string(),
            "save_config".to_string(),
            json!({
                "iface": "rnode-main",
                "command": "save_config",
                "vport": 2,
                "confirm_persistent": true,
                "confirm_command": "ignored_by_safe_bridge"
            })
        )]
    );
}

#[test]
fn rnode_management_rpc_reports_missing_bridge() {
    let daemon = RpcDaemon::test_instance();

    let response = daemon
        .handle_rpc(rpc_request(
            2,
            "rnode_management",
            json!({
                "iface": "rnode-main",
                "command": "radio_state_query"
            }),
        ))
        .expect("rnode management response");

    let error = response.error.expect("missing bridge error");
    assert_eq!(error.code, "RNODE_MANAGEMENT_UNAVAILABLE");
    assert!(response.result.is_none());
}

#[test]
fn rnode_management_rpc_reports_bridge_failure() {
    let daemon = RpcDaemon::test_instance();
    daemon.set_rnode_management_bridge(Arc::new(FailingRNodeManagementBridge));

    let response = daemon
        .handle_rpc(rpc_request(
            3,
            "rnode_management",
            json!({
                "iface": "missing",
                "command": "blink",
                "pattern": 1
            }),
        ))
        .expect("rnode management response");

    let error = response.error.expect("bridge failure error");
    assert_eq!(error.code, "RNODE_MANAGEMENT_FAILED");
    assert!(error.message.contains("unknown RNode interface"));
}

#[test]
fn rnode_management_rpc_rejects_blank_iface_before_bridge() {
    let daemon = RpcDaemon::test_instance();
    daemon.set_rnode_management_bridge(Arc::new(RecordingRNodeManagementBridge::new()));

    let err = daemon
        .handle_rpc(rpc_request(
            4,
            "rnode_management",
            json!({
                "iface": " ",
                "command": "blink",
                "pattern": 1
            }),
        ))
        .expect_err("blank iface should be invalid input");

    assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
    assert_eq!(err.to_string(), "iface is required");
}
