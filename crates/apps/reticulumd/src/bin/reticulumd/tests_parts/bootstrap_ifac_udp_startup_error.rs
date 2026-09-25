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
fn bootstrap_uses_python_default_ifac_size_for_negative_size_on_real_udp_carrier() {
    let temp = TempDir::new().expect("temp dir");
    let db_path = temp.path().join("reticulum.db");
    let config_path = temp.path().join("daemon.toml");
    fs::write(
        &config_path,
        r#"
interfaces = [
  { type = "udp", enabled = true, name = "negative-ifac-size", host = "127.0.0.1", port = 0, target_host = "127.0.0.1", target_port = 42421, ifac_size = -1, network_name = "below-minimum-test" }
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
        .find(|entry| entry.get("name").and_then(|value| value.as_str()) == Some("negative-ifac-size"))
        .expect("configured IFAC UDP carrier");
    let runtime = interface
        .get("settings")
        .and_then(|value| value.get("_runtime"))
        .expect("runtime status");
    assert_eq!(runtime.get("startup_status").and_then(|value| value.as_str()), Some("spawned"));
    assert!(runtime.get("startup_error").is_none());
}

#[test]
fn bootstrap_floors_non_byte_aligned_ifac_bits_on_real_udp_carrier() {
    let temp = TempDir::new().expect("temp dir");
    let db_path = temp.path().join("reticulum.db");
    let config_path = temp.path().join("daemon.toml");
    let port_reservation = std::net::UdpSocket::bind(("127.0.0.1", 0)).expect("reserve UDP port");
    let port = port_reservation.local_addr().expect("reserved UDP address").port();
    fs::write(
        &config_path,
        format!(
            r#"
interfaces = [
  {{ type = "udp", enabled = true, name = "non-byte-ifac-size", host = "127.0.0.1", port = {port}, target_host = "127.0.0.1", target_port = 42421, ifac_size = 9, passphrase = "python-floor-test" }}
]
"#,
        ),
    )
    .expect("write non-byte-aligned IFAC config");

    let runtime =
        tokio::runtime::Builder::new_current_thread().enable_all().build().expect("runtime");
    drop(port_reservation);
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
            .find(|entry| entry.get("name").and_then(|value| value.as_str()) == Some("non-byte-ifac-size"))
            .expect("configured IFAC UDP interface");
        let settings = interface.get("settings").expect("interface settings");
        let runtime_status = settings.get("_runtime").expect("interface runtime status");
        assert_eq!(runtime_status["startup_status"].as_str(), Some("spawned"));
        let udp_status = &runtime_status["udp"]["status"];
        if udp_status["link_state"].as_str() == Some("bound") {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "IFAC UDP carrier did not bind: {udp_status}"
        );
        runtime.block_on(async { tokio::time::sleep(Duration::from_millis(10)).await });
    }

    let ifac = rns_transport::transport::IfacContext::from_network_credentials(
        1,
        None,
        Some("python-floor-test"),
    )
    .expect("derive pinned-Python one-byte IFAC context");
    let packet = rns_transport::packet::Packet {
        destination: rns_transport::hash::AddressHash::new([0x47; 16]),
        data: rns_transport::packet::PacketDataBuffer::new_from_slice(b"floor-to-byte"),
        ..rns_transport::packet::Packet::default()
    };
    let frame = ifac.encode(&packet.to_bytes().expect("serialize test packet"))
        .expect("encode Python-compatible one-byte IFAC frame");
    let sender = std::net::UdpSocket::bind(("127.0.0.1", 0)).expect("bind UDP sender");
    sender.send_to(&frame, ("127.0.0.1", port)).expect("send one-byte IFAC packet");

    let deadline = std::time::Instant::now() + Duration::from_secs(2);
    loop {
        let response = context
            .daemon
            .handle_rpc(RpcRequest { id: 2, method: "daemon_status_ex".to_string(), params: None })
            .expect("daemon_status_ex");
        let result = response.result.expect("status result");
        let interfaces = result
            .pointer("/reticulum/transport/interfaces")
            .and_then(|value| value.as_array())
            .expect("transport interface snapshots");
        if let Some(interface) = interfaces
            .iter()
            .find(|interface| interface["rx_bytes"].as_u64().is_some_and(|bytes| bytes > 0))
        {
            assert_eq!(interface["violations"]["ifac"].as_u64(), Some(0));
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "production UDP worker did not authenticate the 9-bit-configured frame: {interfaces:?}"
        );
        runtime.block_on(async { tokio::time::sleep(Duration::from_millis(10)).await });
    }

    drop(context);
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
