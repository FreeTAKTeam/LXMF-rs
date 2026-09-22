#[derive(Clone, Debug, PartialEq, Eq)]
struct ScopedPathRequest {
    destination: String,
    on_iface: Option<String>,
    tag_hex: Option<String>,
}

struct RecordingPathLookupBridge {
    known: std::sync::Mutex<bool>,
    discover_on_request: bool,
    link_count: usize,
    requests: std::sync::Mutex<Vec<String>>,
    scoped_requests: std::sync::Mutex<Vec<ScopedPathRequest>>,
}

impl RecordingPathLookupBridge {
    fn new(known: bool) -> Self {
        Self {
            known: std::sync::Mutex::new(known),
            discover_on_request: false,
            link_count: 0,
            requests: std::sync::Mutex::new(Vec::new()),
            scoped_requests: std::sync::Mutex::new(Vec::new()),
        }
    }

    fn with_link_count(link_count: usize) -> Self {
        Self { link_count, ..Self::new(false) }
    }

    fn discover_on_request() -> Self {
        Self {
            known: std::sync::Mutex::new(false),
            discover_on_request: true,
            link_count: 0,
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

    fn link_count(&self) -> Result<usize, std::io::Error> {
        Ok(self.link_count)
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

    fn link_count(&self) -> Result<usize, std::io::Error> {
        Err(std::io::Error::other("link table unavailable"))
    }
}

struct MutationPathLookupBridge;

impl PathLookupBridge for MutationPathLookupBridge {
    fn has_path(&self, _destination: &str) -> Result<bool, std::io::Error> {
        Ok(true)
    }

    fn request_path(&self, _destination: &str) -> Result<(), std::io::Error> {
        Ok(())
    }

    fn drop_path(&self, _destination: &str) -> Result<bool, std::io::Error> {
        Ok(true)
    }

    fn drop_all_via(&self, _transport: &str) -> Result<usize, std::io::Error> {
        Ok(3)
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
            "interface_name": "if-test",
            "interface_bitrate": 1_000.0,
            "hops": 2,
        }))
    }
}

#[derive(Clone, Debug, PartialEq)]
struct ProbeRequest {
    destination: String,
    full_name: String,
    size: usize,
    probes: usize,
    timeout_secs: f64,
    wait_secs: f64,
}

struct ProbePathLookupBridge {
    requests: std::sync::Mutex<Vec<ProbeRequest>>,
}

impl PathLookupBridge for ProbePathLookupBridge {
    fn has_path(&self, _destination: &str) -> Result<bool, std::io::Error> {
        Ok(false)
    }

    fn request_path(&self, _destination: &str) -> Result<(), std::io::Error> {
        Ok(())
    }

    fn probe(
        &self,
        destination: &str,
        full_name: &str,
        size: usize,
        probes: usize,
        timeout_secs: f64,
        wait_secs: f64,
    ) -> Result<JsonValue, std::io::Error> {
        self.requests.lock().expect("probe requests mutex poisoned").push(ProbeRequest {
            destination: destination.to_string(),
            full_name: full_name.to_string(),
            size,
            probes,
            timeout_secs,
            wait_secs,
        });
        Ok(json!({
            "destination": destination,
            "full_name": full_name,
            "size": size,
            "probes": probes,
            "replies": probes,
            "packet_loss_percent": 0.0,
        }))
    }
}

struct RuntimeManagementBridge;

impl PathLookupBridge for RuntimeManagementBridge {
    fn has_path(&self, _destination: &str) -> Result<bool, std::io::Error> {
        Ok(false)
    }

    fn request_path(&self, _destination: &str) -> Result<(), std::io::Error> {
        Ok(())
    }

    fn drop_announce_queues(&self) -> Result<usize, std::io::Error> {
        Ok(4)
    }

    fn rate_table(&self) -> Result<JsonValue, std::io::Error> {
        Ok(json!([{
            "hash": "00112233445566778899aabbccddeeff",
            "last": 10.0,
            "rate_violations": 2,
            "blocked_until": 20.0,
            "timestamps": [9.0, 10.0],
        }]))
    }

    fn packet_signal(&self, _packet_hash: &str) -> Result<JsonValue, std::io::Error> {
        Ok(json!({ "rssi": -61.0, "snr": 7.5, "q": 88.0 }))
    }

    fn discovered_interfaces(&self) -> Result<JsonValue, std::io::Error> {
        Ok(json!([{
            "type": "BackboneInterface",
            "status": "available",
            "status_code": 1000,
        }]))
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
fn probe_rpc_forwards_packet_probe_options_and_result() {
    let daemon = RpcDaemon::test_instance();
    let bridge = Arc::new(ProbePathLookupBridge { requests: std::sync::Mutex::new(Vec::new()) });
    daemon.set_path_lookup_bridge(bridge.clone());

    let response = daemon
        .handle_rpc(rpc_request(
            20,
            "probe",
            json!({
                "destination_hash": "AABBCCDDEEFF00112233445566778899",
                "full_name": "rnstransport.probe",
                "size": 32,
                "probes": 4,
                "timeout_secs": 1.5,
                "wait_secs": 0.25,
            }),
        ))
        .expect("probe response");

    assert!(response.error.is_none());
    assert_eq!(response.id, 20);
    let result = response.result.expect("probe result");
    assert_eq!(result["destination"].as_str(), Some("aabbccddeeff00112233445566778899"));
    assert_eq!(result["full_name"].as_str(), Some("rnstransport.probe"));
    assert_eq!(result["replies"].as_u64(), Some(4));
    assert_eq!(
        bridge.requests.lock().expect("probe requests mutex poisoned").as_slice(),
        [ProbeRequest {
            destination: "aabbccddeeff00112233445566778899".to_string(),
            full_name: "rnstransport.probe".to_string(),
            size: 32,
            probes: 4,
            timeout_secs: 1.5,
            wait_secs: 0.25,
        }]
    );
}

#[test]
fn probe_rpc_uses_python_defaults_when_options_are_omitted() {
    let daemon = RpcDaemon::test_instance();
    let bridge = Arc::new(ProbePathLookupBridge { requests: std::sync::Mutex::new(Vec::new()) });
    daemon.set_path_lookup_bridge(bridge.clone());

    daemon
        .handle_rpc(rpc_request(
            21,
            "probe",
            json!({
                "destination": "00112233445566778899aabbccddeeff",
                "full_name": "rnstransport.probe",
            }),
        ))
        .expect("probe response");

    assert_eq!(
        bridge.requests.lock().expect("probe requests mutex poisoned").as_slice(),
        [ProbeRequest {
            destination: "00112233445566778899aabbccddeeff".to_string(),
            full_name: "rnstransport.probe".to_string(),
            size: 16,
            probes: 1,
            timeout_secs: 12.0,
            wait_secs: 0.0,
        }]
    );
}

#[test]
fn probe_rpc_reports_unavailable_without_a_probe_bridge() {
    let daemon = RpcDaemon::test_instance();

    let response = daemon
        .handle_rpc(rpc_request(
            22,
            "probe",
            json!({
                "destination": "00112233445566778899aabbccddeeff",
                "full_name": "rnstransport.probe",
            }),
        ))
        .expect("probe response");

    let error = response.error.expect("probe unavailable error");
    assert_eq!(error.code, "PROBE_UNAVAILABLE");
}

#[test]
fn link_count_reports_bridge_count_as_python_shared_instance_integer() {
    let daemon = RpcDaemon::test_instance();
    daemon.set_path_lookup_bridge(Arc::new(RecordingPathLookupBridge::with_link_count(2)));

    let response = daemon
        .handle_rpc(rpc_request(12, "link_count", json!({})))
        .expect("link count response");

    assert!(response.error.is_none());
    assert_eq!(response.result, Some(json!(2)));
}

#[test]
fn link_count_reports_missing_bridge() {
    let daemon = RpcDaemon::test_instance();

    let response = daemon
        .handle_rpc(rpc_request(13, "link_count", json!({})))
        .expect("link count response");

    let error = response.error.expect("missing bridge error");
    assert_eq!(error.code, "LINK_COUNT_UNAVAILABLE");
    assert!(response.result.is_none());
}

#[test]
fn path_mutation_rpc_matches_python_drop_results() {
    let daemon = RpcDaemon::test_instance();
    daemon.set_path_lookup_bridge(Arc::new(MutationPathLookupBridge));
    let destination = "00112233445566778899aabbccddeeff";

    let dropped = daemon
        .handle_rpc(rpc_request(14, "drop_path", json!({ "destination": destination })))
        .expect("drop path response");
    assert!(dropped.error.is_none());
    assert_eq!(dropped.result, Some(json!({ "dropped": true })));

    let dropped = daemon
        .handle_rpc(rpc_request(15, "drop_all_via", json!({ "destination": destination })))
        .expect("drop all via response");
    assert!(dropped.error.is_none());
    assert_eq!(dropped.result, Some(json!({ "dropped": 3 })));
}

#[test]
fn runtime_management_rpc_matches_python_result_shapes() {
    let daemon = RpcDaemon::test_instance();
    daemon.set_path_lookup_bridge(Arc::new(RuntimeManagementBridge));

    let dropped = daemon
        .handle_rpc(rpc_request(16, "drop_announce_queues", json!({})))
        .expect("drop announce queues response");
    assert_eq!(dropped.result, Some(json!({ "dropped": 4 })));

    let rate_table = daemon
        .handle_rpc(rpc_request(17, "get_rate_table", json!({})))
        .expect("rate table response");
    let rows = rate_table.result.expect("rate table");
    assert_eq!(rows[0]["rate_violations"].as_u64(), Some(2));
    assert_eq!(rows[0]["timestamps"].as_array().map(Vec::len), Some(2));

    let discovered = daemon
        .handle_rpc(rpc_request(19, "discovered_interfaces", json!({})))
        .expect("discovered interfaces response")
        .result
        .expect("discovered interfaces");
    assert_eq!(discovered[0]["status"].as_str(), Some("available"));

    for (method, expected) in
        [("get_packet_rssi", -61.0), ("get_packet_snr", 7.5), ("get_packet_q", 88.0)]
    {
        let response = daemon
            .handle_rpc(rpc_request(
                18,
                method,
                json!({ "packet_hash": "00".repeat(32) }),
            ))
            .expect("packet signal response");
        assert_eq!(response.result.and_then(|value| value.as_f64()), Some(expected));
    }
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
fn next_hop_rpc_reports_python_shared_instance_next_hop() {
    let daemon = RpcDaemon::test_instance();
    daemon.set_path_lookup_bridge(Arc::new(MetadataPathLookupBridge));

    let response = daemon
        .handle_rpc(rpc_request(
            12,
            "next_hop",
            json!({ "destination_hash": "00112233445566778899aabbccddeeff" }),
        ))
        .expect("next hop response");

    assert!(response.error.is_none());
    let result = response.result.expect("next hop result");
    assert_eq!(result["next_hop"].as_str(), Some("8899aabbccddeeff0011223344556677"));
    assert_eq!(result["path_found"].as_bool(), Some(true));
}

#[test]
fn next_hop_if_name_rpc_prefers_bridge_interface_name_like_python() {
    let daemon = RpcDaemon::test_instance();
    daemon.set_path_lookup_bridge(Arc::new(MetadataPathLookupBridge));

    let response = daemon
        .handle_rpc(rpc_request(
            13,
            "next_hop_if_name",
            json!({ "destination_hash": "00112233445566778899aabbccddeeff" }),
        ))
        .expect("next hop interface response");

    assert!(response.error.is_none());
    let result = response.result.expect("next hop interface result");
    assert_eq!(result["next_hop_if_name"].as_str(), Some("if-test"));
}

#[test]
fn first_hop_timeout_rpc_uses_python_default_plus_interface_latency() {
    let daemon = RpcDaemon::test_instance();
    daemon.set_path_lookup_bridge(Arc::new(MetadataPathLookupBridge));

    let response = daemon
        .handle_rpc(rpc_request(
            14,
            "first_hop_timeout",
            json!({ "destination_hash": "00112233445566778899aabbccddeeff" }),
        ))
        .expect("first hop timeout response");

    assert!(response.error.is_none());
    let result = response.result.expect("first hop timeout result");
    assert_eq!(result["first_hop_timeout"].as_f64(), Some(10.0));
}

#[test]
fn next_hop_rpc_reports_null_for_unknown_path_like_python() {
    let daemon = RpcDaemon::test_instance();
    daemon.set_path_lookup_bridge(Arc::new(RecordingPathLookupBridge::new(false)));

    let response = daemon
        .handle_rpc(rpc_request(
            15,
            "next_hop",
            json!({ "destination_hash": "00112233445566778899aabbccddeeff" }),
        ))
        .expect("next hop response");

    assert!(response.error.is_none());
    let result = response.result.expect("next hop result");
    assert_eq!(result["next_hop"], JsonValue::Null);
    assert_eq!(result["path_found"].as_bool(), Some(false));
}

#[test]
fn first_hop_timeout_rpc_returns_python_default_without_latency() {
    let daemon = RpcDaemon::test_instance();
    daemon.set_path_lookup_bridge(Arc::new(RecordingPathLookupBridge::new(false)));

    let response = daemon
        .handle_rpc(rpc_request(
            16,
            "first_hop_timeout",
            json!({ "destination_hash": "00112233445566778899aabbccddeeff" }),
        ))
        .expect("first hop timeout response");

    assert!(response.error.is_none());
    let result = response.result.expect("first hop timeout result");
    assert_eq!(result["first_hop_timeout"].as_f64(), Some(6.0));
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
