#[test]
fn bootstrap_reports_auto_interface_as_spawned_runtime() {
    let temp = TempDir::new().expect("temp dir");
    let db_path = temp.path().join("reticulum.db");
    let config_path = temp.path().join("daemon.toml");
    fs::write(
        &config_path,
        r#"
interfaces = [
  { type = "AutoInterface", enabled = true, name = "auto-main", devices = ["codex-nonexistent-auto-test"] }
]
"#,
    )
    .expect("write config");

    let runtime =
        tokio::runtime::Builder::new_current_thread().enable_all().build().expect("runtime");
    let context = runtime.block_on(async {
        bootstrap::bootstrap(test_args(
            db_path.clone(),
            Some(config_path.clone()),
            Some("127.0.0.1:0".to_string()),
            false,
        ))
        .await
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
        .cloned()
        .expect("interfaces array");

    let auto = interfaces
        .iter()
        .find(|entry| entry.get("type").and_then(|value| value.as_str()) == Some("auto"))
        .expect("auto entry");
    let runtime =
        auto.get("settings").and_then(|value| value.get("_runtime")).expect("runtime settings");
    assert_eq!(
        auto.get("settings")
            .and_then(|value| value.get("discovery_multicast_address"))
            .and_then(|value| value.as_str()),
        Some("ff12:0:d70b:fb1c:16e4:5e39:485e:31e1")
    );
    assert_eq!(runtime.get("startup_status").and_then(|value| value.as_str()), Some("spawned"));
    assert_eq!(runtime.get("runtime_status").and_then(|value| value.as_str()), Some("running"));
    assert_eq!(runtime.get("startup_error"), None);
    assert!(runtime.get("iface").and_then(|value| value.as_str()).is_some());
    let auto_runtime = runtime.get("auto").expect("auto runtime plan metadata");
    assert_eq!(
        auto_runtime.get("auto_runtime_status").and_then(|value| value.as_str()),
        Some("complete")
    );
    assert!(auto_runtime.get("startup_plan").is_some(), "auto startup plan missing: {runtime:?}");
    assert!(
        auto_runtime.get("initial_peer_announces").is_some(),
        "auto initial peer-announce plan missing: {runtime:?}"
    );
    assert!(
        auto_runtime
            .get("planned_discovery_socket_binds")
            .and_then(|value| value.as_array())
            .is_some(),
        "auto discovery socket bind plan missing: {runtime:?}"
    );
    assert!(
        auto_runtime.get("planned_data_socket_binds").and_then(|value| value.as_array()).is_some(),
        "auto peer data socket bind plan missing: {runtime:?}"
    );
    let discovery_runtime =
        runtime.get("auto_discovery_runtime").expect("auto discovery runtime metadata");
    assert_eq!(
        discovery_runtime.get("bound_socket_count").and_then(|value| value.as_u64()),
        Some(0)
    );
    assert_eq!(
        discovery_runtime.get("receive_loop_count").and_then(|value| value.as_u64()),
        Some(0)
    );
    assert_eq!(
        discovery_runtime.get("initial_peer_announce_count").and_then(|value| value.as_u64()),
        Some(0)
    );
    assert_eq!(
        discovery_runtime
            .get("repeat_peer_announce_scheduler_count")
            .and_then(|value| value.as_u64()),
        Some(1)
    );
    assert_eq!(
        discovery_runtime.get("peer_job_scheduler_count").and_then(|value| value.as_u64()),
        Some(1)
    );
    assert_eq!(
        discovery_runtime
            .get("adopted_interface_reconciler_count")
            .and_then(|value| value.as_u64()),
        Some(1)
    );
    assert_eq!(
        discovery_runtime.get("data_socket_count").and_then(|value| value.as_u64()),
        Some(0)
    );
    assert_eq!(
        discovery_runtime.get("data_receive_loop_count").and_then(|value| value.as_u64()),
        Some(0)
    );
}

#[test]
fn bootstrap_selects_local_propagation_node_when_enabled_by_config() {
    let temp = TempDir::new().expect("temp dir");
    let db_path = temp.path().join("reticulum.db");
    let config_path = temp.path().join("daemon.toml");
    fs::write(
        &config_path,
        r#"
[propagation_node]
enabled = true
node_announce_at_start = false
peer_announce_at_start = false
stamp_cost = 16
stamp_cost_flexibility = 3
peering_cost = 18
"#,
    )
    .expect("write config");

    let runtime =
        tokio::runtime::Builder::new_current_thread().enable_all().build().expect("runtime");
    let context = runtime.block_on(async {
        bootstrap::bootstrap(test_args(
            db_path.clone(),
            Some(config_path.clone()),
            Some("127.0.0.1:0".to_string()),
            false,
        ))
        .await
    });

    let selected = context
        .daemon
        .handle_rpc(RpcRequest {
            id: 1,
            method: "get_outbound_propagation_node".to_string(),
            params: None,
        })
        .expect("get selected propagation node")
        .result
        .expect("selected node result");
    let peer = selected
        .get("peer")
        .and_then(|value| value.as_str())
        .expect("config-enabled local propagation node should be selected");

    let status = context
        .daemon
        .handle_rpc(RpcRequest { id: 2, method: "propagation_status".to_string(), params: None })
        .expect("propagation status")
        .result
        .expect("status result");
    assert_eq!(status["propagation"]["selected_node"].as_str(), Some(peer));
    assert_eq!(status["propagation"]["propagation_node_enabled"].as_bool(), Some(true));
}

#[test]
fn bootstrap_strict_mode_rejects_unbindable_udp_interface() {
    let runtime =
        tokio::runtime::Builder::new_current_thread().enable_all().build().expect("runtime");
    runtime.block_on(async {
        let occupied = tokio::net::UdpSocket::bind("127.0.0.1:0").await.expect("bind port");
        let occupied_addr = occupied.local_addr().expect("local addr");

        let temp = TempDir::new().expect("temp dir");
        let db_path = temp.path().join("reticulum.db");
        let config_path = temp.path().join("daemon.toml");
        fs::write(
            &config_path,
            format!(
                "interfaces = [\n  {{ type = \"udp\", enabled = true, name = \"udp-main\", host = \"127.0.0.1\", port = {}, target_host = \"127.0.0.1\", target_port = 4242 }}\n]\n",
                occupied_addr.port()
            ),
        )
        .expect("write config");

        let result = std::panic::AssertUnwindSafe(bootstrap::bootstrap(test_args(
            db_path.clone(),
            Some(config_path.clone()),
            None,
            true,
        )))
        .catch_unwind()
        .await;

        let panic_payload = match result {
            Ok(_) => panic!("strict startup should panic on occupied udp port"),
            Err(panic_payload) => panic_payload,
        };
        let panic_message = if let Some(message) = panic_payload.downcast_ref::<String>() {
            message.clone()
        } else if let Some(message) = panic_payload.downcast_ref::<&str>() {
            (*message).to_string()
        } else {
            String::new()
        };
        assert!(panic_message.contains("strict interface startup policy rejected"));
        assert!(panic_message.contains("udp-main"));
    });
}

#[test]
fn bootstrap_strict_mode_panics_when_transport_is_disabled_for_enabled_interfaces() {
    let temp = TempDir::new().expect("temp dir");
    let db_path = temp.path().join("reticulum.db");
    let config_path = temp.path().join("daemon.toml");
    fs::write(
        &config_path,
        r#"
interfaces = [
  { type = "serial", enabled = true, name = "serial-main", device = "__definitely_not_a_device__", baud_rate = 115200 }
]
"#,
    )
    .expect("write config");

    let runtime =
        tokio::runtime::Builder::new_current_thread().enable_all().build().expect("runtime");
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        runtime.block_on(async {
            bootstrap::bootstrap(test_args(db_path.clone(), Some(config_path.clone()), None, true))
                .await;
        });
    }));
    assert!(result.is_err(), "strict mode should panic on startup failures");
}

#[test]
fn bootstrap_strict_mode_panics_on_serial_preflight_open_failure() {
    let temp = TempDir::new().expect("temp dir");
    let db_path = temp.path().join("reticulum.db");
    let config_path = temp.path().join("daemon.toml");
    fs::write(
        &config_path,
        r#"
interfaces = [
  { type = "serial", enabled = true, name = "serial-main", device = "__definitely_not_a_device__", baud_rate = 115200 }
]
"#,
    )
    .expect("write config");

    let runtime =
        tokio::runtime::Builder::new_current_thread().enable_all().build().expect("runtime");
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        runtime.block_on(async {
            bootstrap::bootstrap(test_args(
                db_path.clone(),
                Some(config_path.clone()),
                Some("127.0.0.1:0".to_string()),
                true,
            ))
            .await;
        });
    }));
    assert!(result.is_err(), "strict mode should panic when serial preflight open fails");
}

#[test]
fn bootstrap_strict_mode_panics_on_rnode_multi_preflight_open_failure() {
    let temp = TempDir::new().expect("temp dir");
    let db_path = temp.path().join("reticulum.db");
    let config_path = temp.path().join("daemon.toml");
    fs::write(
        &config_path,
        r#"
interfaces = [
  { type = "RNodeMultiInterface", enabled = true, name = "rnode-multi", port = "__definitely_not_a_device__", radio0 = { vport = 0, frequency = 915000000, bandwidth = 125000, spreadingfactor = 9, codingrate = 5, txpower = 17 } }
]
"#,
    )
    .expect("write config");

    let runtime =
        tokio::runtime::Builder::new_current_thread().enable_all().build().expect("runtime");
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        runtime.block_on(async {
            bootstrap::bootstrap(test_args(
                db_path.clone(),
                Some(config_path.clone()),
                Some("127.0.0.1:0".to_string()),
                true,
            ))
            .await;
        });
    }));
    assert!(
        result.is_err(),
        "strict mode should panic when RNodeMulti preflight open fails"
    );
}

#[test]
fn bootstrap_strict_mode_panics_on_tcp_client_preflight_connect_failure() {
    let temp = TempDir::new().expect("temp dir");
    let db_path = temp.path().join("reticulum.db");
    let config_path = temp.path().join("daemon.toml");
    fs::write(
        &config_path,
        r#"
interfaces = [
  { type = "tcp_client", enabled = true, name = "tcp-main", host = "203.0.113.1", port = 65535 }
]
"#,
    )
    .expect("write config");

    let runtime =
        tokio::runtime::Builder::new_current_thread().enable_all().build().expect("runtime");
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        runtime.block_on(async {
            bootstrap::bootstrap(test_args(
                db_path.clone(),
                Some(config_path.clone()),
                Some("127.0.0.1:0".to_string()),
                true,
            ))
            .await;
        });
    }));
    assert!(result.is_err(), "strict mode should panic when tcp_client preflight connect fails");
}

#[test]
fn bootstrap_best_effort_marks_ble_validation_failure_as_failed() {
    let temp = TempDir::new().expect("temp dir");
    let db_path = temp.path().join("reticulum.db");
    let config_path = temp.path().join("daemon.toml");
    fs::write(
        &config_path,
        r#"
interfaces = [
  { type = "ble_gatt", enabled = true, name = "ble-main", adapter = "disabled", peripheral_id = "AA:BB:CC:DD:EE:FF", service_uuid = "12345678-1234-1234-1234-1234567890ab", write_char_uuid = "2A37", notify_char_uuid = "2A38" }
]
"#,
    )
    .expect("write config");

    let runtime =
        tokio::runtime::Builder::new_current_thread().enable_all().build().expect("runtime");
    let local = tokio::task::LocalSet::new();
    let context = runtime.block_on(local.run_until(async {
        bootstrap::bootstrap(test_args(
            db_path.clone(),
            Some(config_path.clone()),
            Some("127.0.0.1:0".to_string()),
            false,
        ))
        .await
    }));
    let response = context
        .daemon
        .handle_rpc(RpcRequest { id: 1, method: "list_interfaces".to_string(), params: None })
        .expect("list_interfaces");
    let interfaces = response
        .result
        .expect("result")
        .get("interfaces")
        .and_then(|value| value.as_array())
        .cloned()
        .expect("interfaces array");
    let ble_interface = interfaces
        .iter()
        .find(|entry| {
            entry
                .get("settings")
                .and_then(|value| value.get("_runtime"))
                .and_then(|value| value.get("startup_status"))
                .and_then(|value| value.as_str())
                == Some("failed")
        })
        .expect("failed interface should be present in snapshot");
    assert_eq!(
        ble_interface
            .get("settings")
            .and_then(|value| value.get("_runtime"))
            .and_then(|value| value.get("startup_status"))
            .and_then(|value| value.as_str()),
        Some("failed")
    );
    assert!(
        ble_interface
            .get("settings")
            .and_then(|value| value.get("_runtime"))
            .and_then(|value| value.get("startup_error"))
            .and_then(|value| value.as_str())
            .is_some(),
        "startup error should be populated for failed BLE startup"
    );
}

#[test]
fn bootstrap_strict_mode_panics_on_ble_validation_failure() {
    let temp = TempDir::new().expect("temp dir");
    let db_path = temp.path().join("reticulum.db");
    let config_path = temp.path().join("daemon.toml");
    fs::write(
        &config_path,
        r#"
interfaces = [
  { type = "ble_gatt", enabled = true, name = "ble-main", adapter = "disabled", peripheral_id = "AA:BB:CC:DD:EE:FF", service_uuid = "12345678-1234-1234-1234-1234567890ab", write_char_uuid = "2A37", notify_char_uuid = "2A38" }
]
"#,
    )
    .expect("write config");

    let runtime =
        tokio::runtime::Builder::new_current_thread().enable_all().build().expect("runtime");
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        runtime.block_on(async {
            bootstrap::bootstrap(test_args(
                db_path.clone(),
                Some(config_path.clone()),
                Some("127.0.0.1:0".to_string()),
                true,
            ))
            .await;
        });
    }));
    assert!(result.is_err(), "strict mode should panic when BLE startup validation fails");
}

#[test]
fn bootstrap_best_effort_marks_lora_stale_state_as_failed() {
    let temp = TempDir::new().expect("temp dir");
    let db_path = temp.path().join("reticulum.db");
    let config_path = temp.path().join("daemon.toml");
    let state_path = temp.path().join("lora-state.json");
    let stale_last_updated_unix_ms =
        now_unix_ms_for_test().saturating_sub(31 * 24 * 60 * 60 * 1000);
    fs::write(
        &state_path,
        serde_json::to_vec_pretty(&json!({
            "version": 1,
            "duty_cycle_debt_ms": 5000,
            "last_updated_unix_ms": stale_last_updated_unix_ms,
            "uncertain": false,
            "uncertainty_reason": null
        }))
        .expect("serialize lora state"),
    )
    .expect("write lora state");
    fs::write(
        &config_path,
        format!(
            r#"
interfaces = [
  {{ type = "lora", enabled = true, name = "lora-main", region = "US915", state_path = "{}" }}
]
"#,
            state_path.to_string_lossy().replace('\\', "\\\\")
        ),
    )
    .expect("write config");

    let runtime =
        tokio::runtime::Builder::new_current_thread().enable_all().build().expect("runtime");
    let local = tokio::task::LocalSet::new();
    let context = runtime.block_on(local.run_until(async {
        bootstrap::bootstrap(test_args(
            db_path.clone(),
            Some(config_path.clone()),
            Some("127.0.0.1:0".to_string()),
            false,
        ))
        .await
    }));
    let response = context
        .daemon
        .handle_rpc(RpcRequest { id: 1, method: "list_interfaces".to_string(), params: None })
        .expect("list_interfaces");
    let interfaces = response
        .result
        .expect("result")
        .get("interfaces")
        .and_then(|value| value.as_array())
        .cloned()
        .expect("interfaces array");
    let lora_interface = interfaces
        .iter()
        .find(|entry| {
            entry
                .get("settings")
                .and_then(|value| value.get("_runtime"))
                .and_then(|value| value.get("startup_status"))
                .and_then(|value| value.as_str())
                == Some("failed")
                && entry
                    .get("settings")
                    .and_then(|value| value.get("_runtime"))
                    .and_then(|value| value.get("startup_error"))
                    .and_then(|value| value.as_str())
                    .is_some_and(|error| error.contains("timestamp too old"))
        })
        .expect("lora interface should be present in snapshot");
    assert_eq!(
        lora_interface
            .get("settings")
            .and_then(|value| value.get("_runtime"))
            .and_then(|value| value.get("startup_status"))
            .and_then(|value| value.as_str()),
        Some("failed")
    );
    assert!(
        lora_interface
            .get("settings")
            .and_then(|value| value.get("_runtime"))
            .and_then(|value| value.get("startup_error"))
            .and_then(|value| value.as_str())
            .is_some_and(|error| error.contains("timestamp too old")),
        "startup_error should include stale timestamp fail-closed reason"
    );
}
