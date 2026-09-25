#[test]
fn bootstrap_reports_sanitized_bind_failure_for_ifac_enabled_tcp_listener() {
    let runtime =
        tokio::runtime::Builder::new_current_thread().enable_all().build().expect("runtime");
    runtime.block_on(async {
        let occupied = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("reserve TCP listener port");
        let port = occupied.local_addr().expect("reserved TCP address").port();
        let temp = TempDir::new().expect("temp dir");
        let db_path = temp.path().join("reticulum.db");
        let config_path = temp.path().join("daemon.toml");
        fs::write(
            &config_path,
            format!(
                r#"
interfaces = [
  {{ type = "tcp_server", enabled = true, name = "ifac-tcp-startup", host = "127.0.0.1", port = {port}, ifac_size = 16, network_name = "tcp-startup-network", passphrase = "tcp-startup-secret" }}
]
"#
            ),
        )
        .expect("write IFAC TCP config");

        let context = bootstrap::bootstrap(test_args(db_path, Some(config_path), None, false)).await;
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        loop {
            let response = context
                .daemon
                .handle_rpc(RpcRequest {
                    id: 1,
                    method: "list_interfaces".to_string(),
                    params: None,
                })
                .expect("list_interfaces");
            let result = response.result.expect("result");
            let interfaces = result
                .get("interfaces")
                .and_then(|value| value.as_array())
                .expect("interfaces array");
            let interface = interfaces
                .iter()
                .find(|entry| {
                    entry.get("name").and_then(|value| value.as_str()) == Some("ifac-tcp-startup")
                })
                .expect("IFAC TCP interface");
            let runtime_status = interface
                .get("settings")
                .and_then(|value| value.get("_runtime"))
                .expect("runtime status");
            let tcp_status = runtime_status
                .get("tcp")
                .and_then(|value| value.get("listener_status"))
                .unwrap_or_else(|| panic!("TCP listener status missing: {runtime_status}"));

            if tcp_status.get("listener_state").and_then(|value| value.as_str()) == Some("bind_error") {
                assert_eq!(
                    runtime_status.get("startup_status").and_then(|value| value.as_str()),
                    Some("active"),
                    "the configured daemon listener remains visible with its worker failure"
                );
                assert_eq!(
                    tcp_status.get("accepted_connections").and_then(serde_json::Value::as_u64),
                    Some(0),
                    "the failed IFAC listener must not admit plaintext clients"
                );
                assert!(
                    tcp_status.get("latest_client_iface").is_none_or(serde_json::Value::is_null),
                    "the failed listener must not publish an admitted client"
                );
                let error = tcp_status
                    .get("last_error")
                    .and_then(|value| value.as_str())
                    .expect("reported TCP bind error");
                assert!(!error.is_empty());
                assert!(!error.contains("tcp-startup-secret"));
                assert!(!error.contains("tcp-startup-network"));
                break;
            }

            assert!(
                std::time::Instant::now() < deadline,
                "TCP worker did not report bind failure: {tcp_status}"
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }

        drop(context);
        drop(occupied);
    });
}

#[test]
fn bootstrap_strict_startup_rejects_bind_failure_for_ifac_enabled_tcp_listener() {
    let runtime =
        tokio::runtime::Builder::new_current_thread().enable_all().build().expect("runtime");
    runtime.block_on(async {
        let occupied = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("reserve TCP listener port");
        let port = occupied.local_addr().expect("reserved TCP address").port();
        let temp = TempDir::new().expect("temp dir");
        let db_path = temp.path().join("reticulum.db");
        let config_path = temp.path().join("daemon.toml");
        fs::write(
            &config_path,
            format!(
                r#"
interfaces = [
  {{ type = "tcp_server", enabled = true, name = "ifac-strict-tcp-startup", host = "127.0.0.1", port = {port}, ifac_size = 16, network_name = "strict-tcp-network", passphrase = "strict-tcp-secret" }}
]
"#
            ),
        )
        .expect("write IFAC TCP config");

        let result = std::panic::AssertUnwindSafe(bootstrap::bootstrap(test_args(
            db_path,
            Some(config_path),
            None,
            true,
        )))
        .catch_unwind()
        .await;
        let panic_payload = match result {
            Ok(_) => panic!("strict startup should reject the IFAC TCP bind failure"),
            Err(payload) => payload,
        };
        let panic_message = if let Some(message) = panic_payload.downcast_ref::<String>() {
            message.clone()
        } else if let Some(message) = panic_payload.downcast_ref::<&str>() {
            (*message).to_string()
        } else {
            String::new()
        };

        assert!(panic_message.contains("strict interface startup policy rejected 1 interface(s)"));
        assert!(panic_message.contains("ifac-strict-tcp-startup (tcp_server)"));
        assert!(!panic_message.contains("strict-tcp-secret"));
        drop(occupied);
    });
}

#[test]
fn bootstrap_uses_default_ifac_size_below_minimum_for_tcp_listener() {
    let temp = TempDir::new().expect("temp dir");
    let db_path = temp.path().join("reticulum.db");
    let config_path = temp.path().join("daemon.toml");
    fs::write(
        &config_path,
        r#"
interfaces = [
  { type = "tcp_server", enabled = true, name = "invalid-ifac-tcp-size", host = "127.0.0.1", port = 0, ifac_size = 7, network_name = "credential-must-not-appear" }
]
"#,
    )
    .expect("write invalid TCP IFAC config");

    let runtime =
        tokio::runtime::Builder::new_current_thread().enable_all().build().expect("runtime");
    let context = runtime.block_on(async {
        bootstrap::bootstrap(test_args(db_path, Some(config_path), None, false)).await
    });
    let response = context
        .daemon
        .handle_rpc(RpcRequest { id: 1, method: "list_interfaces".to_string(), params: None })
        .expect("list_interfaces");
    let result = response.result.expect("result");
    let interfaces = result
        .get("interfaces")
        .and_then(|value| value.as_array())
        .expect("interfaces array");
    let interface = interfaces
        .iter()
        .find(|entry| {
            entry.get("name").and_then(|value| value.as_str()) == Some("invalid-ifac-tcp-size")
        })
        .expect("TCP IFAC config");
    assert_eq!(interface.get("type").and_then(|value| value.as_str()), Some("tcp_server"));
    assert_eq!(interface.get("enabled").and_then(|value| value.as_bool()), Some(true));
    let runtime = interface
        .get("settings")
        .and_then(|value| value.get("_runtime"))
        .expect("runtime startup diagnostic");
    assert!(matches!(
        runtime.get("startup_status").and_then(|value| value.as_str()),
        Some("spawned" | "active")
    ));
    assert!(runtime.get("startup_error").is_none());
}
