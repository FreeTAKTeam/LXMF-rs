#[test]
fn bootstrap_reports_bind_failure_for_ifac_enabled_udp_interface() {
    let temp = TempDir::new().expect("temp dir");
    let db_path = temp.path().join("reticulum.db");
    let config_path = temp.path().join("daemon.toml");
    let occupied = std::net::UdpSocket::bind("127.0.0.1:0").expect("reserve UDP port");
    let port = occupied.local_addr().expect("reserved UDP address").port();
    fs::write(
        &config_path,
        format!(
            r#"
interfaces = [
  {{ type = "udp", enabled = true, name = "ifac-startup", host = "127.0.0.1", port = {port}, target_host = "127.0.0.1", target_port = 4242, ifac_size = 128, network_name = "startup-test", passphrase = "non-secret-test-value" }}
]
"#
        ),
    )
    .expect("write IFAC UDP config");

    let runtime =
        tokio::runtime::Builder::new_current_thread().enable_all().build().expect("runtime");
    let context = runtime.block_on(async {
        bootstrap::bootstrap(test_args(db_path, Some(config_path), None, false)).await
    });

    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    loop {
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
            .find(|entry| entry.get("name").and_then(|value| value.as_str()) == Some("ifac-startup"))
            .expect("IFAC UDP interface");
        let runtime_status = interface
            .get("settings")
            .and_then(|value| value.get("_runtime"))
            .expect("runtime status");
        let udp_status = runtime_status
            .get("udp")
            .and_then(|value| value.get("status"))
            .expect("UDP status");

        if udp_status.get("link_state").and_then(|value| value.as_str()) == Some("bind_failed") {
            assert_eq!(
                runtime_status.get("startup_status").and_then(|value| value.as_str()),
                Some("spawned"),
                "daemon should expose worker-level failure separately from interface creation"
            );
            let error = udp_status
                .get("last_error")
                .and_then(|value| value.as_str())
                .expect("reported UDP bind error");
            assert!(!error.is_empty());
            assert!(
                !error.contains("non-secret-test-value"),
                "runtime error must not expose the configured IFAC passphrase"
            );
            break;
        }

        assert!(std::time::Instant::now() < deadline, "UDP worker did not report bind failure: {udp_status}");
        runtime.block_on(async { tokio::time::sleep(Duration::from_millis(10)).await });
    }

    drop(context);
    drop(occupied);
}

#[test]
fn bootstrap_strict_startup_rejects_bind_failure_for_ifac_enabled_udp_interface() {
    let runtime =
        tokio::runtime::Builder::new_current_thread().enable_all().build().expect("runtime");
    runtime.block_on(async {
        let occupied = tokio::net::UdpSocket::bind("127.0.0.1:0").await.expect("reserve UDP port");
        let port = occupied.local_addr().expect("reserved UDP address").port();
        let temp = TempDir::new().expect("temp dir");
        let db_path = temp.path().join("reticulum.db");
        let config_path = temp.path().join("daemon.toml");
        fs::write(
            &config_path,
            format!(
                r#"
interfaces = [
  {{ type = "udp", enabled = true, name = "ifac-strict-startup", host = "127.0.0.1", port = {port}, target_host = "127.0.0.1", target_port = 4242, ifac_size = 128, network_name = "startup-test", passphrase = "strict-startup-secret" }}
]
"#
            ),
        )
        .expect("write IFAC UDP config");

        let result = std::panic::AssertUnwindSafe(bootstrap::bootstrap(test_args(
            db_path,
            Some(config_path),
            None,
            true,
        )))
        .catch_unwind()
        .await;
        let panic_payload = match result {
            Ok(_) => panic!("strict startup should reject the IFAC UDP bind failure"),
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
        assert!(panic_message.contains("ifac-strict-startup (udp)"));
        assert!(!panic_message.contains("strict-startup-secret"));
        drop(occupied);
    });
}

#[test]
fn bootstrap_reports_invalid_ifac_config_without_creating_interface() {
    let temp = TempDir::new().expect("temp dir");
    let db_path = temp.path().join("reticulum.db");
    let config_path = temp.path().join("daemon.toml");
    fs::write(
        &config_path,
        r#"
interfaces = [
  { type = "udp", enabled = true, name = "invalid-ifac-startup", host = "127.0.0.1", port = 42420, target_host = "127.0.0.1", target_port = 42421, ifac_size = 128, secret = "credential-must-not-appear" }
]
"#,
    )
    .expect("write invalid IFAC config");

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

    let rejected = interfaces
        .iter()
        .find(|entry| entry.get("name").and_then(|value| value.as_str()) == Some("invalid-ifac-startup"))
        .expect("rejected config should remain visible as a management diagnostic");
    assert_eq!(rejected.get("type").and_then(|value| value.as_str()), Some("udp"));
    assert_eq!(rejected.get("enabled").and_then(|value| value.as_bool()), Some(true));
    let runtime = rejected.get("settings").and_then(|value| value.get("_runtime"))
        .expect("runtime startup diagnostic");
    assert_eq!(runtime.get("startup_status").and_then(|value| value.as_str()), Some("failed"));
    let error = runtime.get("startup_error").and_then(|value| value.as_str()).expect("safe error");
    assert!(error.contains("IFAC configuration rejected"));
    assert!(!error.contains("credential-must-not-appear"));
    assert_eq!(interfaces.iter().filter(|entry| entry.get("name").and_then(|value| value.as_str()) == Some("invalid-ifac-startup")).count(), 1);
}

#[test]
fn bootstrap_uses_python_default_ifac_size_below_minimum_on_real_udp_carrier() {
    let temp = TempDir::new().expect("temp dir");
    let db_path = temp.path().join("reticulum.db");
    let config_path = temp.path().join("daemon.toml");
    fs::write(
        &config_path,
        r#"
interfaces = [
  { type = "udp", enabled = true, name = "below-minimum-ifac-size", host = "127.0.0.1", port = 0, target_host = "127.0.0.1", target_port = 42421, ifac_size = 7, network_name = "below-minimum-test" }
]
"#,
    )
    .expect("write invalid IFAC size config");

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
        .find(|entry| entry.get("name").and_then(|value| value.as_str()) == Some("below-minimum-ifac-size"))
        .expect("configured IFAC UDP carrier");
    let runtime = interface
        .get("settings")
        .and_then(|value| value.get("_runtime"))
        .expect("runtime status");
    assert_eq!(runtime.get("startup_status").and_then(|value| value.as_str()), Some("spawned"));
    assert!(runtime.get("startup_error").is_none());
}

#[test]
fn bootstrap_reports_nonnumeric_ifac_size_without_exposing_credentials() {
    let temp = TempDir::new().expect("temp dir");
    let db_path = temp.path().join("reticulum.db");
    let config_path = temp.path().join("daemon.toml");
    fs::write(
        &config_path,
        r#"
interfaces = [
  { type = "udp", enabled = true, name = "nonnumeric-ifac-size", host = "127.0.0.1", port = 42420, target_host = "127.0.0.1", target_port = 42421, ifac_size = "not-a-number", passphrase = "credential-must-not-appear" }
]
"#,
    )
    .expect("write mistyped IFAC size config");

    let runtime =
        tokio::runtime::Builder::new_current_thread().enable_all().build().expect("runtime");
    let context = runtime.block_on(async {
        bootstrap::bootstrap(test_args(db_path, Some(config_path), None, false)).await
    });
    let response = context
        .daemon
        .handle_rpc(RpcRequest { id: 1, method: "list_interfaces".to_string(), params: None })
        .expect("list_interfaces");
    let interfaces = response
        .result
        .expect("result")
        .get("interfaces")
        .and_then(|value| value.as_array())
        .expect("interfaces array")
        .clone();

    let rejected = interfaces
        .iter()
        .find(|entry| entry.get("name").and_then(|value| value.as_str()) == Some("nonnumeric-ifac-size"))
        .expect("mistyped IFAC config should remain visible as a management diagnostic");
    assert_eq!(rejected.get("type").and_then(|value| value.as_str()), Some("udp"));
    let runtime = rejected
        .get("settings")
        .and_then(|value| value.get("_runtime"))
        .expect("runtime startup diagnostic");
    assert_eq!(runtime.get("startup_status").and_then(|value| value.as_str()), Some("failed"));
    let error = runtime.get("startup_error").and_then(|value| value.as_str()).expect("safe error");
    assert!(error.contains("IFAC configuration rejected"));
    assert!(!error.contains("credential-must-not-appear"));
}

#[test]
fn bootstrap_prefers_pinned_python_ifac_credential_alias_on_conflict() {
    let temp = TempDir::new().expect("temp dir");
    let db_path = temp.path().join("reticulum.db");
    let config_path = temp.path().join("daemon.toml");
    fs::write(
        &config_path,
        r#"
interfaces = [
  { type = "udp", enabled = true, name = "ifac-alias-precedence", host = "127.0.0.1", port = 0, target_host = "127.0.0.1", target_port = 4242, ifac_size = 128, networkname = "legacy-network", network_name = "current-network", passphrase = "legacy-passphrase", pass_phrase = "current-passphrase" }
]
"#,
    )
    .expect("write conflicting IFAC aliases");

    let runtime =
        tokio::runtime::Builder::new_current_thread().enable_all().build().expect("runtime");
    let context = runtime.block_on(async {
        bootstrap::bootstrap(test_args(db_path, Some(config_path), None, false)).await
    });
    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    loop {
        let response = context
            .daemon
            .handle_rpc(RpcRequest { id: 1, method: "list_interfaces".to_string(), params: None })
            .expect("list_interfaces");
        let result = response.result.expect("result");
        let interfaces = result["interfaces"].as_array().expect("interfaces array");
        let interface = interfaces
            .iter()
            .find(|entry| {
                entry.get("name").and_then(|value| value.as_str()) == Some("ifac-alias-precedence")
            })
            .expect("started IFAC UDP interface");
        let settings = interface.get("settings").expect("interface settings");
        assert_eq!(settings["network_name"].as_str(), Some("current-network"));
        assert_eq!(settings["passphrase"].as_str(), Some("current-passphrase"));
        let udp_status = &settings["_runtime"]["udp"]["status"];
        let link_state = udp_status["link_state"].as_str();
        if link_state == Some("bound") {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "production UDP interface did not reach bound state: {udp_status}"
        );
        runtime.block_on(async { tokio::time::sleep(Duration::from_millis(10)).await });
    }

    drop(context);
}
