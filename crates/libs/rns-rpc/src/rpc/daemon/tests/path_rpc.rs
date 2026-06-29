#[derive(Clone, Debug, PartialEq, Eq)]
struct ScopedPathRequest {
    destination: String,
    on_iface: Option<String>,
    tag_hex: Option<String>,
}

struct RecordingPathLookupBridge {
    known: std::sync::Mutex<bool>,
    discover_on_request: bool,
    requests: std::sync::Mutex<Vec<String>>,
    scoped_requests: std::sync::Mutex<Vec<ScopedPathRequest>>,
}

impl RecordingPathLookupBridge {
    fn new(known: bool) -> Self {
        Self {
            known: std::sync::Mutex::new(known),
            discover_on_request: false,
            requests: std::sync::Mutex::new(Vec::new()),
            scoped_requests: std::sync::Mutex::new(Vec::new()),
        }
    }

    fn discover_on_request() -> Self {
        Self {
            known: std::sync::Mutex::new(false),
            discover_on_request: true,
            requests: std::sync::Mutex::new(Vec::new()),
            scoped_requests: std::sync::Mutex::new(Vec::new()),
        }
    }

    fn requests(&self) -> Vec<String> {
        self.requests.lock().expect("requests mutex poisoned").clone()
    }

    fn scoped_requests(&self) -> Vec<ScopedPathRequest> {
        self.scoped_requests.lock().expect("scoped requests mutex poisoned").clone()
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

    fn request_path_scoped(
        &self,
        destination: &str,
        on_iface: Option<&str>,
        tag: Option<&[u8]>,
    ) -> Result<(), std::io::Error> {
        self.scoped_requests.lock().expect("scoped requests mutex poisoned").push(
            ScopedPathRequest {
                destination: destination.to_string(),
                on_iface: on_iface.map(ToOwned::to_owned),
                tag_hex: tag.map(hex::encode),
            },
        );
        self.request_path(destination)
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

struct MetadataPathLookupBridge;

impl PathLookupBridge for MetadataPathLookupBridge {
    fn has_path(&self, _destination: &str) -> Result<bool, std::io::Error> {
        Ok(true)
    }

    fn request_path(&self, _destination: &str) -> Result<(), std::io::Error> {
        Ok(())
    }

    fn path_status(&self, _destination: &str) -> Result<JsonValue, std::io::Error> {
        Ok(json!({
            "path_found": true,
            "next_hop": "8899aabbccddeeff0011223344556677",
            "interface": "fedcba98765432100123456789abcdef",
            "hops": 2,
        }))
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
fn path_status_preserves_bridge_route_metadata() {
    let daemon = RpcDaemon::test_instance();
    daemon.set_path_lookup_bridge(Arc::new(MetadataPathLookupBridge));

    let response = daemon
        .handle_rpc(rpc_request(
            8,
            "path_status",
            json!({ "destination": "00112233445566778899aabbccddeeff" }),
        ))
        .expect("path status response");

    assert!(response.error.is_none());
    let result = response.result.expect("path status result");
    assert_eq!(result["status"].as_str(), Some("found"));
    assert_eq!(result["next_hop"].as_str(), Some("8899aabbccddeeff0011223344556677"));
    assert_eq!(result["interface"].as_str(), Some("fedcba98765432100123456789abcdef"));
    assert_eq!(result["hops"].as_u64(), Some(2));
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
fn request_path_forwards_scoped_iface_and_tag() {
    let daemon = RpcDaemon::test_instance();
    let bridge = Arc::new(RecordingPathLookupBridge::new(false));
    daemon.set_path_lookup_bridge(bridge.clone());

    let response = daemon
        .handle_rpc(rpc_request(
            9,
            "request_path",
            json!({
                "destination_hash": "00112233445566778899aabbccddeeff",
                "interface": "AABBCCDDEEFF00112233445566778899",
                "tag": "01020304"
            }),
        ))
        .expect("request path response");

    assert!(response.error.is_none());
    let result = response.result.expect("request path result");
    assert_eq!(result["path_found"].as_bool(), Some(false));
    assert_eq!(result["requested"].as_bool(), Some(true));
    assert_eq!(result["on_iface"].as_str(), Some("aabbccddeeff00112233445566778899"));
    assert_eq!(result["interface_scope"].as_str(), Some("aabbccddeeff00112233445566778899"));
    assert_eq!(result["tag_hex"].as_str(), Some("01020304"));
    assert_eq!(
        bridge.scoped_requests(),
        vec![ScopedPathRequest {
            destination: "00112233445566778899aabbccddeeff".to_string(),
            on_iface: Some("aabbccddeeff00112233445566778899".to_string()),
            tag_hex: Some("01020304".to_string()),
        }]
    );
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
fn request_path_dispatches_scoped_refresh_when_already_known() {
    let daemon = RpcDaemon::test_instance();
    let bridge = Arc::new(RecordingPathLookupBridge::new(true));
    daemon.set_path_lookup_bridge(bridge.clone());

    let response = daemon
        .handle_rpc(rpc_request(
            10,
            "request_path",
            json!({
                "destination": "00112233445566778899aabbccddeeff",
                "on_iface": "aabbccddeeff00112233445566778899",
                "tag_hex": "01020304"
            }),
        ))
        .expect("request path response");

    assert!(response.error.is_none());
    let result = response.result.expect("request path result");
    assert_eq!(result["known"].as_bool(), Some(true));
    assert_eq!(result["path_found"].as_bool(), Some(true));
    assert_eq!(result["requested"].as_bool(), Some(true));
    assert_eq!(result["status"].as_str(), Some("found"));
    assert_eq!(
        bridge.scoped_requests(),
        vec![ScopedPathRequest {
            destination: "00112233445566778899aabbccddeeff".to_string(),
            on_iface: Some("aabbccddeeff00112233445566778899".to_string()),
            tag_hex: Some("01020304".to_string()),
        }]
    );
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
fn path_rpc_rejects_invalid_scoped_iface_before_bridge() {
    let daemon = RpcDaemon::test_instance();
    let bridge = Arc::new(RecordingPathLookupBridge::new(false));
    daemon.set_path_lookup_bridge(bridge.clone());

    let err = daemon
        .handle_rpc(rpc_request(
            10,
            "request_path",
            json!({
                "destination_hash": "00112233445566778899aabbccddeeff",
                "on_iface": "abcd"
            }),
        ))
        .expect_err("short interface hash should be invalid input");

    assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
    assert_eq!(
        err.to_string(),
        "on_iface destination must decode to a 16-byte RNS destination hash"
    );
    assert!(bridge.requests().is_empty());
}

#[test]
fn path_rpc_rejects_oversized_tag_before_bridge() {
    let daemon = RpcDaemon::test_instance();
    let bridge = Arc::new(RecordingPathLookupBridge::new(false));
    daemon.set_path_lookup_bridge(bridge.clone());

    let err = daemon
        .handle_rpc(rpc_request(
            11,
            "request_path",
            json!({
                "destination_hash": "00112233445566778899aabbccddeeff",
                "tag_hex": "00112233445566778899aabbccddeeff00"
            }),
        ))
        .expect_err("oversized request tag should be invalid input");

    assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
    assert_eq!(err.to_string(), "tag_hex must decode to 1..=16 bytes");
    assert!(bridge.requests().is_empty());
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
