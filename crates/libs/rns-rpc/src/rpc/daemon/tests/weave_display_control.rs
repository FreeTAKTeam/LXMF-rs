struct RecordingWeaveDisplayControlBridge {
    calls: std::sync::Mutex<Vec<(String, bool, Option<String>)>>,
}

impl RecordingWeaveDisplayControlBridge {
    fn new() -> Self {
        Self { calls: std::sync::Mutex::new(Vec::new()) }
    }

    fn calls(&self) -> Vec<(String, bool, Option<String>)> {
        self.calls.lock().expect("calls mutex poisoned").clone()
    }
}

impl WeaveDisplayControlBridge for RecordingWeaveDisplayControlBridge {
    fn set_weave_remote_display(
        &self,
        iface: &str,
        enable: bool,
        remote_switch_id_hex: Option<&str>,
    ) -> Result<JsonValue, std::io::Error> {
        self.calls.lock().expect("calls mutex poisoned").push((
            iface.to_string(),
            enable,
            remote_switch_id_hex.map(str::to_string),
        ));
        Ok(json!({
            "queued": true,
            "iface": iface,
            "enable": enable,
            "remote_switch_id_hex": remote_switch_id_hex,
        }))
    }
}

struct FailingWeaveDisplayControlBridge;

impl WeaveDisplayControlBridge for FailingWeaveDisplayControlBridge {
    fn set_weave_remote_display(
        &self,
        _iface: &str,
        _enable: bool,
        _remote_switch_id_hex: Option<&str>,
    ) -> Result<JsonValue, std::io::Error> {
        Err(std::io::Error::new(std::io::ErrorKind::NotFound, "unknown Weave interface"))
    }
}

#[test]
fn weave_display_control_rpc_delegates_to_bridge() {
    let daemon = RpcDaemon::test_instance();
    let bridge = Arc::new(RecordingWeaveDisplayControlBridge::new());
    daemon.set_weave_display_control_bridge(bridge.clone());

    let response = daemon
        .handle_rpc(rpc_request(
            1,
            "weave_remote_display_control",
            json!({
                "iface": "weave-main",
                "enable": true,
                "remote_switch_id_hex": "10203040"
            }),
        ))
        .expect("weave display control response");

    assert!(response.error.is_none());
    let result = response.result.expect("weave display control result");
    assert_eq!(result["queued"].as_bool(), Some(true));
    assert_eq!(result["iface"].as_str(), Some("weave-main"));
    assert_eq!(result["enable"].as_bool(), Some(true));
    assert_eq!(result["remote_switch_id_hex"].as_str(), Some("10203040"));
    assert_eq!(
        bridge.calls(),
        vec![("weave-main".to_string(), true, Some("10203040".to_string()))]
    );
}

#[test]
fn weave_display_control_rpc_trims_blank_remote_switch_override() {
    let daemon = RpcDaemon::test_instance();
    let bridge = Arc::new(RecordingWeaveDisplayControlBridge::new());
    daemon.set_weave_display_control_bridge(bridge.clone());

    let response = daemon
        .handle_rpc(rpc_request(
            2,
            "weave_remote_display_control",
            json!({
                "iface": "weave-main",
                "enable": false,
                "remote_switch_id_hex": " "
            }),
        ))
        .expect("weave display control response");

    assert!(response.error.is_none());
    assert_eq!(bridge.calls(), vec![("weave-main".to_string(), false, None)]);
}

#[test]
fn weave_display_control_rpc_reports_missing_bridge() {
    let daemon = RpcDaemon::test_instance();

    let response = daemon
        .handle_rpc(rpc_request(
            3,
            "weave_remote_display_control",
            json!({
                "iface": "weave-main",
                "enable": true
            }),
        ))
        .expect("weave display control response");

    let error = response.error.expect("missing bridge error");
    assert_eq!(error.code, "WEAVE_DISPLAY_CONTROL_UNAVAILABLE");
    assert!(response.result.is_none());
}

#[test]
fn weave_display_control_rpc_reports_bridge_failure() {
    let daemon = RpcDaemon::test_instance();
    daemon.set_weave_display_control_bridge(Arc::new(FailingWeaveDisplayControlBridge));

    let response = daemon
        .handle_rpc(rpc_request(
            4,
            "weave_remote_display_control",
            json!({
                "iface": "missing",
                "enable": true
            }),
        ))
        .expect("weave display control response");

    let error = response.error.expect("bridge failure error");
    assert_eq!(error.code, "WEAVE_DISPLAY_CONTROL_FAILED");
    assert!(error.message.contains("unknown Weave interface"));
}

#[test]
fn weave_display_control_rpc_rejects_blank_iface_before_bridge() {
    let daemon = RpcDaemon::test_instance();
    daemon.set_weave_display_control_bridge(Arc::new(RecordingWeaveDisplayControlBridge::new()));

    let err = daemon
        .handle_rpc(rpc_request(
            5,
            "weave_remote_display_control",
            json!({
                "iface": " ",
                "enable": true
            }),
        ))
        .expect_err("blank iface should be invalid input");

    assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
    assert_eq!(err.to_string(), "iface is required");
}
