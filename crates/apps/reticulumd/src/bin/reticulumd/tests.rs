use crate::bootstrap::{
    enforce_startup_policy, mark_interface_runtime_fields, mark_interface_startup_status,
    InterfaceStartupFailure,
};
use crate::bridge_helpers::opportunistic_payload;
use crate::interfaces::{lora, serial};
use crate::{bootstrap, Args};
use reticulum_daemon::config::InterfaceConfig;
use rns_rpc::{InterfaceRecord, RpcRequest};
use rns_transport::delivery::send_outcome_status;
use rns_transport::destination_hash::parse_destination_hash_required;
use rns_transport::transport::SendPacketOutcome;
use serde_json::json;
use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};
use tempfile::TempDir;

#[test]
fn opportunistic_payload_strips_destination_prefix() {
    let destination = [0xAA; 16];
    let mut payload = destination.to_vec();
    payload.extend_from_slice(&[1, 2, 3, 4]);
    assert_eq!(opportunistic_payload(&payload, &destination), &[1, 2, 3, 4]);
}

#[test]
fn opportunistic_payload_keeps_payload_without_prefix() {
    let destination = [0xAA; 16];
    let payload = vec![0xBB; 24];
    assert_eq!(opportunistic_payload(&payload, &destination), payload.as_slice());
}

#[test]
fn send_outcome_status_maps_success() {
    assert_eq!(
        send_outcome_status("opportunistic", SendPacketOutcome::SentDirect),
        "sent: opportunistic"
    );
}

#[test]
fn send_outcome_status_maps_failures() {
    assert_eq!(
        send_outcome_status("opportunistic", SendPacketOutcome::DroppedMissingDestinationIdentity),
        "failed: opportunistic missing destination identity"
    );
    assert_eq!(
        send_outcome_status("opportunistic", SendPacketOutcome::DroppedNoRoute),
        "failed: opportunistic no route"
    );
}

#[test]
fn parse_destination_hex_required_rejects_invalid_hashes() {
    let err = parse_destination_hash_required("not-hex").expect_err("invalid hash");
    assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
}

#[test]
fn serial_builder_rejects_missing_required_fields() {
    let iface = InterfaceConfig {
        kind: "serial".to_string(),
        enabled: Some(true),
        ..InterfaceConfig::default()
    };
    let result = serial::build_adapter(&iface);
    assert!(result.is_err(), "missing device/baud should fail");
    let err = result.err().unwrap_or_default();
    assert!(err.contains("serial.device"));
}

#[test]
fn lora_startup_persists_state_file() {
    let temp = TempDir::new().expect("temp dir");
    let state_path = temp.path().join("lora-state.json");

    let iface = InterfaceConfig {
        kind: "lora".to_string(),
        enabled: Some(true),
        name: Some("lora-main".to_string()),
        region: Some("US915".to_string()),
        state_path: Some(state_path.to_string_lossy().to_string()),
        ..InterfaceConfig::default()
    };

    lora::startup(&iface).expect("lora startup");
    let state = fs::read_to_string(&state_path).expect("state file exists");
    assert!(state.contains("\"version\": 1"));
}

#[test]
fn startup_status_metadata_is_embedded_in_interface_settings() {
    let mut record = InterfaceRecord {
        kind: "serial".to_string(),
        enabled: true,
        host: None,
        port: None,
        name: Some("serial-main".to_string()),
        settings: Some(json!({
            "device": "/dev/ttyUSB0",
            "baud_rate": 115200
        })),
    };

    mark_interface_startup_status(
        &mut record,
        "failed",
        Some("permission denied"),
        Some("deadbeef"),
    );

    let settings = record.settings.expect("settings should be present");
    let runtime = settings
        .get("_runtime")
        .and_then(|value| value.as_object())
        .expect("runtime metadata should be present");
    assert_eq!(runtime.get("startup_status").and_then(|value| value.as_str()), Some("failed"));
    assert_eq!(
        runtime.get("startup_error").and_then(|value| value.as_str()),
        Some("permission denied")
    );
    assert_eq!(runtime.get("iface").and_then(|value| value.as_str()), Some("deadbeef"));
}

#[test]
fn runtime_status_metadata_is_embedded_in_interface_settings() {
    let mut record = InterfaceRecord {
        kind: "ble_gatt".to_string(),
        enabled: true,
        host: None,
        port: None,
        name: Some("ble-main".to_string()),
        settings: Some(json!({
            "peripheral_id": "AA:BB:CC:DD:EE:FF"
        })),
    };

    mark_interface_startup_status(&mut record, "spawned", None, Some("beefcafe"));
    mark_interface_runtime_fields(&mut record, "running", 0);

    let settings = record.settings.expect("settings should be present");
    let runtime = settings
        .get("_runtime")
        .and_then(|value| value.as_object())
        .expect("runtime metadata should be present");
    assert_eq!(runtime.get("startup_status").and_then(|value| value.as_str()), Some("spawned"));
    assert_eq!(runtime.get("runtime_status").and_then(|value| value.as_str()), Some("running"));
    assert_eq!(runtime.get("reconnect_attempts").and_then(|value| value.as_u64()), Some(0));
    assert_eq!(runtime.get("iface").and_then(|value| value.as_str()), Some("beefcafe"));
}

#[test]
fn best_effort_startup_policy_allows_partial_failures() {
    let failures = vec![InterfaceStartupFailure {
        label: "lora-main".to_string(),
        kind: "lora".to_string(),
        error: "state marked uncertain".to_string(),
    }];
    enforce_startup_policy(false, &failures).expect("best-effort policy should not fail");
}

#[test]
fn strict_startup_policy_rejects_interface_failures() {
    let failures = vec![InterfaceStartupFailure {
        label: "lora-main".to_string(),
        kind: "lora".to_string(),
        error: "state marked uncertain".to_string(),
    }];
    let err = enforce_startup_policy(true, &failures).expect_err("strict policy should fail");
    assert!(err.contains("strict interface startup policy rejected"));
    assert!(err.contains("lora-main"));
}

#[test]
fn bootstrap_best_effort_marks_interfaces_inactive_when_transport_is_disabled() {
    let temp = TempDir::new().expect("temp dir");
    let db_path = temp.path().join("reticulum.db");
    let config_path = temp.path().join("daemon.toml");
    fs::write(
        &config_path,
        r#"
interfaces = [
  { type = "serial", enabled = true, name = "serial-main", device = "/dev/ttyUSB0", baud_rate = 115200 }
]
"#,
    )
    .expect("write config");

    let runtime =
        tokio::runtime::Builder::new_current_thread().enable_all().build().expect("runtime");
    let context = runtime.block_on(async {
        bootstrap::bootstrap(test_args(db_path.clone(), Some(config_path.clone()), None, false))
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
    assert_eq!(interfaces.len(), 1);
    assert_eq!(
        interfaces[0]
            .get("settings")
            .and_then(|value| value.get("_runtime"))
            .and_then(|value| value.get("startup_status"))
            .and_then(|value| value.as_str()),
        Some("inactive_transport_disabled")
    );
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
  { type = "serial", enabled = true, name = "serial-main", device = "/dev/ttyUSB0", baud_rate = 115200 }
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
            state_path.display()
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

#[test]
fn bootstrap_strict_mode_panics_on_lora_debt_overflow_fail_closed() {
    let temp = TempDir::new().expect("temp dir");
    let db_path = temp.path().join("reticulum.db");
    let config_path = temp.path().join("daemon.toml");
    let state_path = temp.path().join("lora-state.json");
    fs::write(
        &state_path,
        serde_json::to_vec_pretty(&json!({
            "version": 1,
            "duty_cycle_debt_ms": 86_400_001,
            "last_updated_unix_ms": now_unix_ms_for_test(),
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
            state_path.display()
        ),
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
        "strict mode should panic when lora state debt exceeds compliance bounds"
    );
}

fn now_unix_ms_for_test() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

fn test_args(
    db: PathBuf,
    config: Option<PathBuf>,
    transport: Option<String>,
    strict_interface_startup: bool,
) -> Args {
    Args {
        rpc: "127.0.0.1:0".to_string(),
        db,
        config,
        identity: None,
        announce_interval_secs: 0,
        transport,
        strict_interface_startup,
        rpc_tls_cert: None,
        rpc_tls_key: None,
        rpc_tls_client_ca: None,
    }
}
