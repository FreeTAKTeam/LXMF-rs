impl RemoteControlBridge for RemoteTransferErrorBridge {
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
        if self.fail_download {
            Err(std::io::Error::new(self.kind, self.message))
        } else {
            Ok(json!({
                "remote": remote,
                "messages": [],
            }))
        }
    }

    fn propagation_remote_fetch(
        &self,
        remote: &str,
        _identity_private_key_hex: Option<&str>,
        _timeout_secs: f64,
        _transfer_limit_kb: Option<f64>,
    ) -> Result<JsonValue, std::io::Error> {
        if self.fail_fetch {
            Err(std::io::Error::new(self.kind, self.message))
        } else {
            Ok(json!({
                "remote": remote,
                "messages": [],
            }))
        }
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

impl RemoteControlBridge for TransferLimitResultRemoteControlBridge {
    fn propagation_remote_status(
        &self,
        _remote: &str,
        _identity_private_key_hex: Option<&str>,
        _timeout_secs: f64,
    ) -> Result<JsonValue, std::io::Error> {
        Ok(json!({"status": "ok"}))
    }

    fn propagation_remote_sync(
        &self,
        remote: &str,
        peer: &str,
        _identity_private_key_hex: Option<&str>,
        _timeout_secs: f64,
        transfer_limit_kb: Option<f64>,
    ) -> Result<JsonValue, std::io::Error> {
        assert_eq!(transfer_limit_kb, self.expected_sync_transfer_limit_kb);
        let mut result = self.result.clone();
        result["remote"] = json!(remote);
        result["peer"] = json!(peer);
        Ok(result)
    }

    fn propagation_remote_download(
        &self,
        remote: &str,
        _identity_private_key_hex: Option<&str>,
        _timeout_secs: f64,
        _transfer_limit_kb: Option<f64>,
    ) -> Result<JsonValue, std::io::Error> {
        let mut result = self.result.clone();
        result["remote"] = json!(remote);
        Ok(result)
    }

    fn propagation_remote_fetch(
        &self,
        remote: &str,
        _identity_private_key_hex: Option<&str>,
        _timeout_secs: f64,
        _transfer_limit_kb: Option<f64>,
    ) -> Result<JsonValue, std::io::Error> {
        let mut result = self.result.clone();
        result["remote"] = json!(remote);
        Ok(result)
    }

    fn propagation_remote_unpeer(
        &self,
        remote: &str,
        peer: &str,
        _identity_private_key_hex: Option<&str>,
        _timeout_secs: f64,
    ) -> Result<JsonValue, std::io::Error> {
        let mut result = self.result.clone();
        result["remote"] = json!(remote);
        result["peer"] = json!(peer);
        result["unpeered"] = json!(true);
        Ok(result)
    }
}

impl RemoteControlBridge for FailingTransferLimitRemoteControlBridge {
    fn propagation_remote_status(
        &self,
        _remote: &str,
        _identity_private_key_hex: Option<&str>,
        _timeout_secs: f64,
    ) -> Result<JsonValue, std::io::Error> {
        Ok(json!({"status": "ok"}))
    }

    fn propagation_remote_sync(
        &self,
        _remote: &str,
        _peer: &str,
        _identity_private_key_hex: Option<&str>,
        _timeout_secs: f64,
        transfer_limit_kb: Option<f64>,
    ) -> Result<JsonValue, std::io::Error> {
        assert_eq!(transfer_limit_kb, self.expected_sync_transfer_limit_kb);
        Err(std::io::Error::new(self.kind, "remote sync failed"))
    }

    fn propagation_remote_download(
        &self,
        _remote: &str,
        _identity_private_key_hex: Option<&str>,
        _timeout_secs: f64,
        _transfer_limit_kb: Option<f64>,
    ) -> Result<JsonValue, std::io::Error> {
        Err(std::io::Error::new(self.kind, "remote download failed"))
    }

    fn propagation_remote_fetch(
        &self,
        _remote: &str,
        _identity_private_key_hex: Option<&str>,
        _timeout_secs: f64,
        _transfer_limit_kb: Option<f64>,
    ) -> Result<JsonValue, std::io::Error> {
        Err(std::io::Error::new(self.kind, "remote fetch failed"))
    }

    fn propagation_remote_unpeer(
        &self,
        _remote: &str,
        _peer: &str,
        _identity_private_key_hex: Option<&str>,
        _timeout_secs: f64,
    ) -> Result<JsonValue, std::io::Error> {
        Err(std::io::Error::new(self.kind, "remote unpeer failed"))
    }
}

struct TransferLimitRemoteControlBridge;

impl RemoteControlBridge for TransferLimitRemoteControlBridge {
    fn propagation_remote_status(
        &self,
        _remote: &str,
        _identity_private_key_hex: Option<&str>,
        _timeout_secs: f64,
    ) -> Result<JsonValue, std::io::Error> {
        Ok(json!({"status": "ok"}))
    }

    fn propagation_remote_sync(
        &self,
        _remote: &str,
        _peer: &str,
        _identity_private_key_hex: Option<&str>,
        _timeout_secs: f64,
        transfer_limit_kb: Option<f64>,
    ) -> Result<JsonValue, std::io::Error> {
        assert_eq!(transfer_limit_kb, Some(42.5));
        Ok(json!({"synced": true}))
    }

    fn propagation_remote_download(
        &self,
        _remote: &str,
        _identity_private_key_hex: Option<&str>,
        _timeout_secs: f64,
        transfer_limit_kb: Option<f64>,
    ) -> Result<JsonValue, std::io::Error> {
        assert_eq!(transfer_limit_kb, Some(42.5));
        Ok(json!({
            "downloaded_count": 0,
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
        Ok(json!({"unpeered": true}))
    }

    fn propagation_remote_fetch(
        &self,
        _remote: &str,
        _identity_private_key_hex: Option<&str>,
        _timeout_secs: f64,
        _transfer_limit_kb: Option<f64>,
    ) -> Result<JsonValue, std::io::Error> {
        Ok(json!({"messages": []}))
    }
}

#[test]
fn selected_propagation_node_updates_status_snapshot() {
    let daemon = RpcDaemon::test_instance();

    daemon
        .handle_rpc(rpc_request(
            67,
            "set_outbound_propagation_node",
            json!({
                "peer": "  peer-propagation-node  ",
            }),
        ))
        .expect("set propagation node");

    let propagation_status = daemon
        .handle_rpc(RpcRequest { id: 68, method: "propagation_status".to_string(), params: None })
        .expect("propagation status")
        .result
        .expect("propagation status result");
    assert_eq!(
        propagation_status["propagation"]["selected_node"].as_str(),
        Some("peer-propagation-node")
    );

    let daemon_status = daemon
        .handle_rpc(RpcRequest { id: 69, method: "daemon_status_ex".to_string(), params: None })
        .expect("daemon status")
        .result
        .expect("daemon status result");
    assert_eq!(
        daemon_status["propagation"]["selected_node"].as_str(),
        Some("peer-propagation-node")
    );

    let nodes = daemon
        .handle_rpc(RpcRequest {
            id: 72,
            method: "list_propagation_nodes".to_string(),
            params: None,
        })
        .expect("list propagation nodes")
        .result
        .expect("list propagation nodes result");
    let node = nodes["nodes"].as_array().and_then(|rows| rows.first()).expect("node row");
    assert_eq!(node["peer"].as_str(), Some("peer-propagation-node"));
    assert_eq!(node["selected"].as_bool(), Some(true));
    assert_eq!(node["capabilities"], json!(["propagation"]));

    daemon
        .handle_rpc(rpc_request(70, "set_outbound_propagation_node", json!({ "peer": " " })))
        .expect("clear propagation node");
    let cleared = daemon
        .handle_rpc(RpcRequest { id: 71, method: "propagation_status".to_string(), params: None })
        .expect("propagation status")
        .result
        .expect("propagation status result");
    assert_eq!(cleared["propagation"]["selected_node"], JsonValue::Null);
}

#[test]
fn selected_propagation_node_queues_existing_entries_for_peer_sync() {
    let daemon = RpcDaemon::test_instance();
    let entry = PropagationEntryRecord {
        transient_id: "ad".repeat(32),
        destination: "12".repeat(16),
        payload_hex: "34".repeat(24),
        received_at: 1_700_000_607,
        size_bytes: 24,
        stamp_value: None,
    };
    daemon.store.upsert_propagation_entry(&entry).expect("store propagation entry");

    let result = daemon
        .handle_rpc(rpc_request(
            73,
            "set_outbound_propagation_node",
            json!({ "peer": "peer-selected-queue" }),
        ))
        .expect("set propagation node")
        .result
        .expect("set propagation node result");
    assert_eq!(result["peer"].as_str(), Some("peer-selected-queue"));

    let peers = daemon
        .handle_rpc(RpcRequest { id: 74, method: "list_peers".to_string(), params: None })
        .expect("list peers")
        .result
        .expect("list peers result");
    let row = peers["peers"]
        .as_array()
        .expect("peer rows")
        .iter()
        .find(|row| row["peer"].as_str() == Some("peer-selected-queue"))
        .expect("selected peer row");
    assert_eq!(row["peer_type"].as_str(), Some("manual"));
    assert_eq!(row["messages"]["offered"].as_u64(), Some(0));
    assert_eq!(row["messages"]["unhandled"].as_u64(), Some(1));
    assert_eq!(
        row["messages"]["unhandled_ids"].as_array().expect("message unhandled ids"),
        &[json!(entry.transient_id.as_str())]
    );
    assert_eq!(
        daemon
            .store
            .list_peer_unhandled_propagation("peer-selected-queue")
            .expect("list selected peer unhandled")
            .len(),
        1
    );
}
