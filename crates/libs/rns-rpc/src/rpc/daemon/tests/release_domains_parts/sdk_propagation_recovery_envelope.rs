struct ReleasePropagationRemoteFailureBridge;

impl RemoteControlBridge for ReleasePropagationRemoteFailureBridge {
    fn propagation_remote_status(
        &self,
        _remote: &str,
        _identity_private_key_hex: Option<&str>,
        _timeout_secs: f64,
    ) -> Result<JsonValue, std::io::Error> {
        Ok(json!({ "state": "online" }))
    }

    fn propagation_remote_sync(
        &self,
        _remote: &str,
        _peer: &str,
        _identity_private_key_hex: Option<&str>,
        _timeout_secs: f64,
        _transfer_limit_kb: Option<f64>,
    ) -> Result<JsonValue, std::io::Error> {
        Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "remote access denied",
        ))
    }

    fn propagation_remote_fetch(
        &self,
        _remote: &str,
        _identity_private_key_hex: Option<&str>,
        _timeout_secs: f64,
        _transfer_limit_kb: Option<f64>,
    ) -> Result<JsonValue, std::io::Error> {
        Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "remote access denied",
        ))
    }

    fn propagation_remote_download(
        &self,
        _remote: &str,
        _identity_private_key_hex: Option<&str>,
        _timeout_secs: f64,
        _transfer_limit_kb: Option<f64>,
    ) -> Result<JsonValue, std::io::Error> {
        Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "remote access denied",
        ))
    }

    fn propagation_remote_unpeer(
        &self,
        _remote: &str,
        _peer: &str,
        _identity_private_key_hex: Option<&str>,
        _timeout_secs: f64,
    ) -> Result<JsonValue, std::io::Error> {
        Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "remote access denied",
        ))
    }
}

#[test]
fn sdk_propagation_payload_envelopes_include_daemon_recovery_state() {
    let daemon = RpcDaemon::test_instance();

    let ingest = daemon
        .handle_rpc(rpc_request(
            1227,
            "sdk_envelope_execute_v2",
            json!({
                "operation_id": "app.propagation.ingest",
                "kind": "command",
                "payload": {
                    "payload_hex": "70726f7061676174696f6e"
                },
            }),
        ))
        .expect("propagation ingest envelope");
    assert!(ingest.error.is_none());
    let ingest_result = ingest.result.expect("ingest result");
    let ingest_payload = &ingest_result["response"]["payload"];
    assert_eq!(
        ingest_payload["propagation"]["client_propagation_messages_received"],
        json!(1)
    );
    assert_eq!(ingest_payload["propagation"]["last_ingest_count"], json!(1));
    let transient_id = ingest_payload["transient_id"].as_str().expect("transient id");

    let fetch = daemon
        .handle_rpc(rpc_request(
            1228,
            "sdk_envelope_execute_v2",
            json!({
                "operation_id": "app.propagation.fetch",
                "kind": "command",
                "payload": {
                    "transient_id": transient_id
                },
            }),
        ))
        .expect("propagation fetch envelope");
    assert!(fetch.error.is_none());
    let fetch_result = fetch.result.expect("fetch result");
    let fetch_payload = &fetch_result["response"]["payload"];
    assert_eq!(fetch_payload["payload_hex"], json!("70726f7061676174696f6e"));
    assert_eq!(
        fetch_payload["propagation"]["client_propagation_messages_served"],
        json!(1)
    );
}

#[test]
fn sdk_propagation_remote_failure_envelope_preserves_recovery_payload() {
    let daemon = RpcDaemon::test_instance();
    daemon.set_remote_control_bridge(std::sync::Arc::new(ReleasePropagationRemoteFailureBridge));

    let fetch = daemon
        .handle_rpc(rpc_request(
            1229,
            "sdk_envelope_execute_v2",
            json!({
                "operation_id": "app.propagation.remote_fetch",
                "kind": "command",
                "payload": {
                    "remote": "remote-denied"
                },
            }),
        ))
        .expect("remote fetch envelope");
    assert!(fetch.error.is_none());
    let fetch_result = fetch.result.expect("fetch result");
    let fetch_payload = &fetch_result["response"]["payload"];
    assert_eq!(fetch_payload["remote"], json!("remote-denied"));
    assert_eq!(fetch_payload["result"]["failure_kind"], json!("no_access"));
    assert_eq!(fetch_payload["propagation"]["state_name"], json!("failed"));
    assert_eq!(fetch_payload["propagation"]["last_sync_error"], json!("remote access denied"));

    let sync = daemon
        .handle_rpc(rpc_request(
            1230,
            "sdk_envelope_execute_v2",
            json!({
                "operation_id": "app.propagation.remote_sync",
                "kind": "command",
                "payload": {
                    "remote": "remote-denied",
                    "peer": "peer-denied"
                },
            }),
        ))
        .expect("remote sync envelope");
    assert!(sync.error.is_none());
    let sync_result = sync.result.expect("sync result");
    let sync_payload = &sync_result["response"]["payload"];
    assert_eq!(sync_payload["remote"], json!("remote-denied"));
    assert_eq!(sync_payload["peer"], json!("peer-denied"));
    assert_eq!(sync_payload["result"]["failure_kind"], json!("no_access"));
    assert_eq!(sync_payload["result"]["error"], json!("remote access denied"));
}
