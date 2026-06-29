struct RecordingPathLookupBridge {
    known: std::sync::Mutex<bool>,
    discover_on_request: bool,
    requests: std::sync::Mutex<Vec<String>>,
}

impl RecordingPathLookupBridge {
    fn new(known: bool) -> Self {
        Self {
            known: std::sync::Mutex::new(known),
            discover_on_request: false,
            requests: std::sync::Mutex::new(Vec::new()),
        }
    }

    fn discover_on_request() -> Self {
        Self {
            known: std::sync::Mutex::new(false),
            discover_on_request: true,
            requests: std::sync::Mutex::new(Vec::new()),
        }
    }

    fn requests(&self) -> Vec<String> {
        self.requests.lock().expect("requests mutex poisoned").clone()
    }
}

impl PathLookupBridge for RecordingPathLookupBridge {
    fn has_path(&self, _destination: &str) -> Result<bool, std::io::Error> {
        Ok(*self.known.lock().expect("known mutex poisoned"))
    }

    fn request_path(&self, destination: &str) -> Result<(), std::io::Error> {
        self.requests
            .lock()
            .expect("requests mutex poisoned")
            .push(destination.to_string());
        if self.discover_on_request {
            *self.known.lock().expect("known mutex poisoned") = true;
        }
        Ok(())
    }
}

struct FailingPathLookupBridge;

impl PathLookupBridge for FailingPathLookupBridge {
    fn has_path(&self, _destination: &str) -> Result<bool, std::io::Error> {
        Err(std::io::Error::other("path table unavailable"))
    }

    fn request_path(&self, _destination: &str) -> Result<(), std::io::Error> {
        Err(std::io::Error::other("path request dispatch unavailable"))
    }
}

#[test]
fn path_status_reports_known_path() {
    let daemon = RpcDaemon::test_instance();
    daemon.set_path_lookup_bridge(Arc::new(RecordingPathLookupBridge::new(true)));

    let response = daemon
        .handle_rpc(rpc_request(
            1,
            "path_status",
            json!({ "destination": "AABBCCDDEEFF00112233445566778899" }),
        ))
        .expect("path status response");

    assert!(response.error.is_none());
    let result = response.result.expect("path status result");
    assert_eq!(result["destination"].as_str(), Some("aabbccddeeff00112233445566778899"));
    assert_eq!(result["destination_hash"].as_str(), Some("aabbccddeeff00112233445566778899"));
    assert_eq!(result["known"].as_bool(), Some(true));
    assert_eq!(result["path_found"].as_bool(), Some(true));
    assert_eq!(result["status"].as_str(), Some("found"));
}

#[test]
fn request_path_times_out_when_unknown_path_stays_unknown() {
    let daemon = RpcDaemon::test_instance();
    let bridge = Arc::new(RecordingPathLookupBridge::new(false));
    daemon.set_path_lookup_bridge(bridge.clone());

    let response = daemon
        .handle_rpc(rpc_request(
            2,
            "request_path",
            json!({ "destination_hash": "00112233445566778899aabbccddeeff" }),
        ))
        .expect("request path response");

    assert!(response.error.is_none());
    let result = response.result.expect("request path result");
    assert_eq!(result["destination"].as_str(), Some("00112233445566778899aabbccddeeff"));
    assert_eq!(result["destination_hash"].as_str(), Some("00112233445566778899aabbccddeeff"));
    assert_eq!(result["known"].as_bool(), Some(false));
    assert_eq!(result["path_found"].as_bool(), Some(false));
    assert_eq!(result["requested"].as_bool(), Some(true));
    assert_eq!(result["status"].as_str(), Some("timeout"));
    assert_eq!(bridge.requests(), vec!["00112233445566778899aabbccddeeff".to_string()]);
}

#[test]
fn request_path_reports_found_when_path_appears_after_request() {
    let daemon = RpcDaemon::test_instance();
    let bridge = Arc::new(RecordingPathLookupBridge::discover_on_request());
    daemon.set_path_lookup_bridge(bridge.clone());

    let response = daemon
        .handle_rpc(rpc_request(
            7,
            "request_path",
            json!({
                "destination": "00112233445566778899aabbccddeeff",
                "timeout_secs": 1
            }),
        ))
        .expect("request path response");

    assert!(response.error.is_none());
    let result = response.result.expect("request path result");
    assert_eq!(result["known"].as_bool(), Some(true));
    assert_eq!(result["path_found"].as_bool(), Some(true));
    assert_eq!(result["requested"].as_bool(), Some(true));
    assert_eq!(result["status"].as_str(), Some("found"));
    assert_eq!(bridge.requests(), vec!["00112233445566778899aabbccddeeff".to_string()]);
}

#[test]
fn request_path_skips_dispatch_when_already_known() {
    let daemon = RpcDaemon::test_instance();
    let bridge = Arc::new(RecordingPathLookupBridge::new(true));
    daemon.set_path_lookup_bridge(bridge.clone());

    let response = daemon
        .handle_rpc(rpc_request(
            3,
            "request_path",
            json!({ "destination": "00112233445566778899aabbccddeeff" }),
        ))
        .expect("request path response");

    assert!(response.error.is_none());
    let result = response.result.expect("request path result");
    assert_eq!(result["known"].as_bool(), Some(true));
    assert_eq!(result["path_found"].as_bool(), Some(true));
    assert_eq!(result["requested"].as_bool(), Some(false));
    assert_eq!(result["status"].as_str(), Some("found"));
    assert!(bridge.requests().is_empty());
}

#[test]
fn path_rpc_reports_missing_bridge() {
    let daemon = RpcDaemon::test_instance();

    let response = daemon
        .handle_rpc(rpc_request(
            4,
            "path_status",
            json!({ "destination": "00112233445566778899aabbccddeeff" }),
        ))
        .expect("path status response");

    let error = response.error.expect("missing bridge error");
    assert_eq!(error.code, "PATH_LOOKUP_UNAVAILABLE");
    assert!(response.result.is_none());
}

#[test]
fn path_rpc_rejects_invalid_destination_before_bridge() {
    let daemon = RpcDaemon::test_instance();
    daemon.set_path_lookup_bridge(Arc::new(RecordingPathLookupBridge::new(false)));

    let err = daemon
        .handle_rpc(rpc_request(5, "request_path", json!({ "destination": "abcd" })))
        .expect_err("short destination should be invalid input");

    assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
    assert_eq!(err.to_string(), "destination must decode to a 16-byte RNS destination hash");
}

#[test]
fn path_status_reports_bridge_failure() {
    let daemon = RpcDaemon::test_instance();
    daemon.set_path_lookup_bridge(Arc::new(FailingPathLookupBridge));

    let response = daemon
        .handle_rpc(rpc_request(
            6,
            "path_status",
            json!({ "destination": "00112233445566778899aabbccddeeff" }),
        ))
        .expect("path status response");

    let error = response.error.expect("bridge failure error");
    assert_eq!(error.code, "PATH_LOOKUP_FAILED");
    assert!(error.message.contains("path table unavailable"));
}

#[test]
fn request_path_reports_lookup_failure_as_rpc_error() {
    let daemon = RpcDaemon::test_instance();
    daemon.set_path_lookup_bridge(Arc::new(FailingPathLookupBridge));

    let response = daemon
        .handle_rpc(rpc_request(
            7,
            "request_path",
            json!({ "destination_hash": "00112233445566778899aabbccddeeff" }),
        ))
        .expect("request path response");

    let error = response.error.expect("lookup failure error");
    assert_eq!(error.code, "PATH_LOOKUP_FAILED");
    assert!(error.message.contains("path table unavailable"));
}
