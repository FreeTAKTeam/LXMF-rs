#[test]
fn propagation_remote_status_trims_remote_before_bridge_and_response() {
    let daemon = RpcDaemon::test_instance();
    daemon.set_remote_control_bridge(Arc::new(TestRemoteControlBridge {
        result: Ok(json!({})),
    }));

    let result = daemon
        .handle_rpc(rpc_request(
            71,
            "propagation_remote_status",
            json!({
                "remote": "  remote-status-trimmed  ",
            }),
        ))
        .expect("remote status with padded remote")
        .result
        .expect("remote status result");

    assert_eq!(result["remote"].as_str(), Some("remote-status-trimmed"));
    assert_eq!(result["status"]["remote"].as_str(), Some("remote-status-trimmed"));
}

#[test]
fn propagation_remote_status_rejects_blank_remote_before_bridge_call() {
    let daemon = RpcDaemon::test_instance();
    let status_calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    daemon.set_remote_control_bridge(Arc::new(CountingRemoteControlBridge {
        status_calls: Arc::clone(&status_calls),
        download_calls: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        fetch_calls: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        sync_calls: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        unpeer_calls: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
    }));

    let rejected = daemon
        .handle_rpc(rpc_request(
            72,
            "propagation_remote_status",
            json!({
                "remote": "   ",
            }),
        ))
        .expect_err("blank remote status node should be rejected");
    assert!(
        rejected.to_string().contains("remote is required"),
        "unexpected rejection error: {rejected}"
    );
    assert_eq!(status_calls.load(std::sync::atomic::Ordering::SeqCst), 0);
}

struct TestRemoteControlBridge {
    result: Result<JsonValue, std::io::ErrorKind>,
}

struct TransferLimitResultRemoteControlBridge {
    result: JsonValue,
    expected_sync_transfer_limit_kb: Option<f64>,
}

struct FailingTransferLimitRemoteControlBridge {
    kind: std::io::ErrorKind,
    expected_sync_transfer_limit_kb: Option<f64>,
}

struct RemoteSyncErrorBridge {
    kind: std::io::ErrorKind,
    message: &'static str,
}

struct RemoteUnpeerErrorBridge {
    kind: std::io::ErrorKind,
    message: &'static str,
}

struct RemoteTransferErrorBridge {
    kind: std::io::ErrorKind,
    message: &'static str,
    fail_download: bool,
    fail_fetch: bool,
}

struct CountingRemoteControlBridge {
    status_calls: Arc<std::sync::atomic::AtomicUsize>,
    download_calls: Arc<std::sync::atomic::AtomicUsize>,
    fetch_calls: Arc<std::sync::atomic::AtomicUsize>,
    sync_calls: Arc<std::sync::atomic::AtomicUsize>,
    unpeer_calls: Arc<std::sync::atomic::AtomicUsize>,
}

impl RemoteControlBridge for TestRemoteControlBridge {
    fn propagation_remote_status(
        &self,
        remote: &str,
        _identity_private_key_hex: Option<&str>,
        _timeout_secs: f64,
    ) -> Result<JsonValue, std::io::Error> {
        Ok(json!({
            "remote": remote,
            "status": "ok",
        }))
    }

    fn propagation_remote_sync(
        &self,
        remote: &str,
        peer: &str,
        _identity_private_key_hex: Option<&str>,
        _timeout_secs: f64,
        transfer_limit_kb: Option<f64>,
    ) -> Result<JsonValue, std::io::Error> {
        assert_eq!(transfer_limit_kb, None);
        self.result.clone().map(|mut result| {
            result["remote"] = json!(remote);
            result["peer"] = json!(peer);
            result
        }).map_err(|kind| std::io::Error::new(kind, "remote sync failed"))
    }

    fn propagation_remote_download(
        &self,
        remote: &str,
        _identity_private_key_hex: Option<&str>,
        _timeout_secs: f64,
        transfer_limit_kb: Option<f64>,
    ) -> Result<JsonValue, std::io::Error> {
        assert_eq!(transfer_limit_kb, None);
        self.result.clone().map(|mut result| {
            result["remote"] = json!(remote);
            result
        }).map_err(|kind| std::io::Error::new(kind, "remote download failed"))
    }

    fn propagation_remote_unpeer(
        &self,
        remote: &str,
        peer: &str,
        _identity_private_key_hex: Option<&str>,
        _timeout_secs: f64,
    ) -> Result<JsonValue, std::io::Error> {
        self.result
            .clone()
            .map(|mut result| {
                result["remote"] = json!(remote);
                result["peer"] = json!(peer);
                result["unpeered"] = json!(true);
                result
            })
            .map_err(|kind| std::io::Error::new(kind, "remote unpeer failed"))
    }

    fn propagation_remote_fetch(
        &self,
        remote: &str,
        _identity_private_key_hex: Option<&str>,
        _timeout_secs: f64,
        _transfer_limit_kb: Option<f64>,
    ) -> Result<JsonValue, std::io::Error> {
        self.result
            .clone()
            .map(|mut result| {
                result["remote"] = json!(remote);
                result
            })
            .map_err(|kind| std::io::Error::new(kind, "remote fetch failed"))
    }
}

struct RemoteAccessDeniedBridge;

impl RemoteAccessDeniedBridge {
    fn denied() -> std::io::Error {
        std::io::Error::new(std::io::ErrorKind::PermissionDenied, "propagation node denied access")
    }
}

impl RemoteControlBridge for RemoteAccessDeniedBridge {
    fn propagation_remote_status(
        &self,
        _remote: &str,
        _identity_private_key_hex: Option<&str>,
        _timeout_secs: f64,
    ) -> Result<JsonValue, std::io::Error> {
        Err(Self::denied())
    }

    fn propagation_remote_sync(
        &self,
        _remote: &str,
        _peer: &str,
        _identity_private_key_hex: Option<&str>,
        _timeout_secs: f64,
        _transfer_limit_kb: Option<f64>,
    ) -> Result<JsonValue, std::io::Error> {
        Err(Self::denied())
    }

    fn propagation_remote_download(
        &self,
        _remote: &str,
        _identity_private_key_hex: Option<&str>,
        _timeout_secs: f64,
        _transfer_limit_kb: Option<f64>,
    ) -> Result<JsonValue, std::io::Error> {
        Err(Self::denied())
    }

    fn propagation_remote_unpeer(
        &self,
        _remote: &str,
        _peer: &str,
        _identity_private_key_hex: Option<&str>,
        _timeout_secs: f64,
    ) -> Result<JsonValue, std::io::Error> {
        Err(Self::denied())
    }

    fn propagation_remote_fetch(
        &self,
        _remote: &str,
        _identity_private_key_hex: Option<&str>,
        _timeout_secs: f64,
        _transfer_limit_kb: Option<f64>,
    ) -> Result<JsonValue, std::io::Error> {
        Err(Self::denied())
    }
}

impl RemoteControlBridge for CountingRemoteControlBridge {
    fn propagation_remote_status(
        &self,
        _remote: &str,
        _identity_private_key_hex: Option<&str>,
        _timeout_secs: f64,
    ) -> Result<JsonValue, std::io::Error> {
        self.status_calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(json!({"status": "ok"}))
    }

    fn propagation_remote_sync(
        &self,
        remote: &str,
        peer: &str,
        _identity_private_key_hex: Option<&str>,
        _timeout_secs: f64,
        _transfer_limit_kb: Option<f64>,
    ) -> Result<JsonValue, std::io::Error> {
        self.sync_calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(json!({
            "remote": remote,
            "peer": peer,
            "synced": true,
        }))
    }

    fn propagation_remote_download(
        &self,
        remote: &str,
        _identity_private_key_hex: Option<&str>,
        _timeout_secs: f64,
        _transfer_limit_kb: Option<f64>,
    ) -> Result<JsonValue, std::io::Error> {
        self.download_calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(json!({
            "remote": remote,
            "downloaded_count": 0,
            "messages": [],
        }))
    }

    fn propagation_remote_fetch(
        &self,
        remote: &str,
        _identity_private_key_hex: Option<&str>,
        _timeout_secs: f64,
        _transfer_limit_kb: Option<f64>,
    ) -> Result<JsonValue, std::io::Error> {
        self.fetch_calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(json!({
            "remote": remote,
            "messages": [],
        }))
    }

    fn propagation_remote_unpeer(
        &self,
        remote: &str,
        peer: &str,
        _identity_private_key_hex: Option<&str>,
        _timeout_secs: f64,
    ) -> Result<JsonValue, std::io::Error> {
        self.unpeer_calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Ok(json!({
            "remote": remote,
            "peer": peer,
            "unpeered": true,
        }))
    }
}

impl RemoteControlBridge for RemoteSyncErrorBridge {
    fn propagation_remote_status(
        &self,
        remote: &str,
        _identity_private_key_hex: Option<&str>,
        _timeout_secs: f64,
    ) -> Result<JsonValue, std::io::Error> {
        Ok(json!({
            "remote": remote,
            "status": "ok",
        }))
    }

    fn propagation_remote_sync(
        &self,
        _remote: &str,
        _peer: &str,
        _identity_private_key_hex: Option<&str>,
        _timeout_secs: f64,
        _transfer_limit_kb: Option<f64>,
    ) -> Result<JsonValue, std::io::Error> {
        Err(std::io::Error::new(self.kind, self.message))
    }

    fn propagation_remote_download(
        &self,
        remote: &str,
        _identity_private_key_hex: Option<&str>,
        _timeout_secs: f64,
        _transfer_limit_kb: Option<f64>,
    ) -> Result<JsonValue, std::io::Error> {
        Ok(json!({
            "remote": remote,
            "synced": true,
        }))
    }

    fn propagation_remote_fetch(
        &self,
        remote: &str,
        _identity_private_key_hex: Option<&str>,
        _timeout_secs: f64,
        _transfer_limit_kb: Option<f64>,
    ) -> Result<JsonValue, std::io::Error> {
        Ok(json!({
            "remote": remote,
            "messages": [],
        }))
    }

    fn propagation_remote_unpeer(
        &self,
        remote: &str,
        peer: &str,
        _identity_private_key_hex: Option<&str>,
        _timeout_secs: f64,
    ) -> Result<JsonValue, std::io::Error> {
        Ok(json!({
            "remote": remote,
            "peer": peer,
        }))
    }
}

impl RemoteControlBridge for RemoteUnpeerErrorBridge {
    fn propagation_remote_status(
        &self,
        remote: &str,
        _identity_private_key_hex: Option<&str>,
        _timeout_secs: f64,
    ) -> Result<JsonValue, std::io::Error> {
        Ok(json!({
            "remote": remote,
            "status": "ok",
        }))
    }

    fn propagation_remote_sync(
        &self,
        remote: &str,
        peer: &str,
        _identity_private_key_hex: Option<&str>,
        _timeout_secs: f64,
        _transfer_limit_kb: Option<f64>,
    ) -> Result<JsonValue, std::io::Error> {
        Ok(json!({
            "remote": remote,
            "peer": peer,
            "synced": true,
        }))
    }

    fn propagation_remote_download(
        &self,
        remote: &str,
        _identity_private_key_hex: Option<&str>,
        _timeout_secs: f64,
        _transfer_limit_kb: Option<f64>,
    ) -> Result<JsonValue, std::io::Error> {
        Ok(json!({
            "remote": remote,
            "messages": [],
        }))
    }

    fn propagation_remote_fetch(
        &self,
        remote: &str,
        _identity_private_key_hex: Option<&str>,
        _timeout_secs: f64,
        _transfer_limit_kb: Option<f64>,
    ) -> Result<JsonValue, std::io::Error> {
        Ok(json!({
            "remote": remote,
            "messages": [],
        }))
    }

    fn propagation_remote_unpeer(
        &self,
        _remote: &str,
        _peer: &str,
        _identity_private_key_hex: Option<&str>,
        _timeout_secs: f64,
    ) -> Result<JsonValue, std::io::Error> {
        Err(std::io::Error::new(self.kind, self.message))
    }
}
