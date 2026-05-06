use reticulum_daemon::config::{DaemonConfig, InterfaceConfig};
use rns_transport::iface::InterfaceMode;
use std::fs;
use tempfile::NamedTempFile;

#[test]
fn parses_tcp_client_interface() {
    let input = r#"
interfaces = [
  { type = "tcp_client", enabled = true, host = "rmap.world", port = 4242, name = "Public RMap" }
]
"#;
    let cfg = DaemonConfig::from_toml(input).expect("parse");
    assert_eq!(cfg.interfaces.len(), 1);
    let iface = &cfg.interfaces[0];
    assert_eq!(iface.name.as_deref(), Some("Public RMap"));
    assert_eq!(iface.host.as_deref(), Some("rmap.world"));
    assert_eq!(iface.port, Some(4242));
    assert!(iface.enabled.unwrap_or(false));
}

#[test]
fn parses_python_interface_mode_aliases() {
    let input = r#"
interfaces = [
  { type = "tcp_client", enabled = true, host = "rmap.world", port = 4242, interface_mode = "ap" },
  { type = "udp", enabled = false, host = "127.0.0.1", port = 4242, mode = "gw" }
]
"#;
    let cfg = DaemonConfig::from_toml(input).expect("parse interface modes");
    assert_eq!(cfg.interfaces[0].interface_mode().unwrap(), InterfaceMode::AccessPoint);
    assert_eq!(cfg.interfaces[1].interface_mode().unwrap(), InterfaceMode::Gateway);

    let settings = cfg.interfaces[0].settings_json().expect("settings");
    assert_eq!(settings["interface_mode"], "access_point");
}

#[test]
fn rejects_invalid_interface_mode() {
    let input = r#"
interfaces = [
  { type = "tcp_client", enabled = true, host = "rmap.world", port = 4242, interface_mode = "invalid" }
]
"#;
    let err = DaemonConfig::from_toml(input).expect_err("invalid mode must fail");
    let message = err.to_string();
    assert!(message.contains("interface_mode must be one of"), "unexpected parse error: {message}");
}

#[test]
fn filters_enabled_tcp_clients() {
    let cfg = DaemonConfig {
        interfaces: vec![
            InterfaceConfig {
                kind: "tcp_client".into(),
                enabled: Some(true),
                host: Some("rmap.world".into()),
                port: Some(4242),
                ..InterfaceConfig::default()
            },
            InterfaceConfig {
                kind: "tcp_client".into(),
                enabled: Some(false),
                host: Some("example.com".into()),
                port: Some(1),
                ..InterfaceConfig::default()
            },
        ],
    };
    let endpoints = cfg.tcp_client_endpoints();
    assert_eq!(endpoints.len(), 1);
    assert_eq!(endpoints[0].0, "rmap.world");
    assert_eq!(endpoints[0].1, 4242);
}

#[test]
fn filters_enabled_tcp_servers_with_default_host() {
    let cfg = DaemonConfig {
        interfaces: vec![
            InterfaceConfig {
                kind: "tcp_server".into(),
                enabled: Some(true),
                host: None,
                port: Some(4242),
                ..InterfaceConfig::default()
            },
            InterfaceConfig {
                kind: "tcp_server".into(),
                enabled: Some(true),
                host: Some("127.0.0.1".into()),
                port: Some(4243),
                ..InterfaceConfig::default()
            },
            InterfaceConfig {
                kind: "tcp_server".into(),
                enabled: Some(false),
                host: Some("192.0.2.1".into()),
                port: Some(9999),
                ..InterfaceConfig::default()
            },
        ],
    };
    let endpoints = cfg.tcp_server_endpoints();
    assert_eq!(endpoints, vec![("0.0.0.0".to_string(), 4242), ("127.0.0.1".to_string(), 4243)]);
}

#[test]
fn parses_udp_interface_with_target_settings() {
    let input = r#"
interfaces = [
  { type = "udp", enabled = true, host = "127.0.0.1", port = 4242, target_host = "127.0.0.1", target_port = 4243, name = "udp-main" }
]
"#;
    let cfg = DaemonConfig::from_toml(input).expect("parse udp config");
    let iface = &cfg.interfaces[0];
    assert_eq!(iface.kind, "udp");
    assert_eq!(iface.host.as_deref(), Some("127.0.0.1"));
    assert_eq!(iface.port, Some(4242));
    assert_eq!(iface.target_host.as_deref(), Some("127.0.0.1"));
    assert_eq!(iface.target_port, Some(4243));
}

#[test]
fn rejects_udp_target_host_without_target_port() {
    let input = r#"
interfaces = [
  { type = "udp", enabled = true, host = "127.0.0.1", port = 4242, target_host = "127.0.0.1" }
]
"#;
    let err = DaemonConfig::from_toml(input).expect_err("partial udp target settings must fail");
    let message = err.to_string();
    assert!(
        message.contains("target_host and target_port must be provided together for udp"),
        "unexpected parse error: {message}"
    );
}

#[test]
fn parses_enabled_serial_interface_with_settings() {
    let input = r#"
interfaces = [
  { type = "serial", enabled = true, name = "tty-primary", device = "/dev/ttyUSB0", baud_rate = 115200, reconnect_backoff_ms = 250 }
]
"#;
    let cfg = DaemonConfig::from_toml(input).expect("parse serial config");
    assert_eq!(cfg.interfaces.len(), 1);
    let iface = &cfg.interfaces[0];
    assert_eq!(iface.kind, "serial");
    assert_eq!(iface.device.as_deref(), Some("/dev/ttyUSB0"));
    assert_eq!(iface.baud_rate, Some(115200));
}

#[test]
fn rejects_invalid_serial_line_settings() {
    let input = r#"
interfaces = [
  { type = "serial", enabled = true, device = "/dev/ttyUSB0", baud_rate = 115200, data_bits = 9, parity = "mark", flow_control = "xonxoff" }
]
"#;
    let err = DaemonConfig::from_toml(input)
        .expect_err("serial validation should reject invalid line settings");
    let message = err.to_string();
    assert!(
        message.contains("data_bits must be one of 5, 6, 7, 8 for serial"),
        "unexpected parse error: {message}"
    );
}

#[test]
fn rejects_zero_serial_baud_rate() {
    let input = r#"
interfaces = [
  { type = "serial", enabled = true, device = "/dev/ttyUSB0", baud_rate = 0 }
]
"#;
    let err = DaemonConfig::from_toml(input).expect_err("zero baud rate should fail");
    let message = err.to_string();
    assert!(
        message.contains("baud_rate must be > 0 for serial"),
        "unexpected parse error: {message}"
    );
}

#[test]
fn rejects_enabled_ble_interface_missing_required_fields() {
    let input = r#"
interfaces = [
  { type = "ble_gatt", enabled = true, peripheral_id = "ABC" }
]
"#;
    let err = DaemonConfig::from_toml(input).expect_err("ble should require full settings");
    let message = err.to_string();
    assert!(
        message.contains("service_uuid is required for ble_gatt"),
        "unexpected parse error: {message}"
    );
}

#[test]
fn rejects_ble_with_invalid_uuid_format() {
    let input = r#"
interfaces = [
  { type = "ble_gatt", enabled = true, peripheral_id = "ABC", service_uuid = "not-a-uuid", write_char_uuid = "2A37", notify_char_uuid = "2A38" }
]
"#;
    let err = DaemonConfig::from_toml(input).expect_err("invalid BLE UUID should fail");
    let message = err.to_string();
    assert!(
        message.contains("service_uuid must be a 16-, 32-, or 128-bit UUID for ble_gatt"),
        "unexpected parse error: {message}"
    );
}

#[test]
fn rejects_lora_unknown_region() {
    let input = r#"
interfaces = [
  { type = "lora", enabled = true, region = "MARS1", state_path = "/tmp/lora-state.json" }
]
"#;
    let err = DaemonConfig::from_toml(input).expect_err("invalid region must fail");
    let message = err.to_string();
    assert!(message.contains("region must be one of"), "unexpected parse error: {message}");
}

#[test]
fn rejects_unknown_keys_for_new_interface_kinds() {
    let input = r#"
interfaces = [
  { type = "lora", enabled = true, region = "US915", state_path = "/tmp/lora-state.json", unknown_option = true }
]
"#;
    let err = DaemonConfig::from_toml(input).expect_err("unknown keys must fail");
    let message = err.to_string();
    assert!(message.contains("unknown settings key"), "unexpected parse error: {message}");
}

#[test]
fn allows_disabled_new_interface_without_required_fields() {
    let input = r#"
interfaces = [
  { type = "ble_gatt", enabled = false }
]
"#;
    let cfg = DaemonConfig::from_toml(input).expect("disabled ble should parse");
    assert_eq!(cfg.interfaces.len(), 1);
    assert!(!cfg.interfaces[0].enabled());
}

#[test]
fn trims_interface_kind_whitespace() {
    let input = r#"
interfaces = [
  { type = " serial ", enabled = true, device = "/dev/ttyUSB0", baud_rate = 9600 }
]
"#;
    let cfg = DaemonConfig::from_toml(input).expect("serial with whitespace kind should parse");
    assert_eq!(cfg.interfaces[0].kind, "serial");
}

#[test]
fn loads_config_from_file() {
    let input = r#"
interfaces = [
  { type = "tcp_client", enabled = true, host = "rmap.world", port = 4242 }
]
"#;
    let file = NamedTempFile::new().expect("temp file");
    fs::write(file.path(), input).expect("write");

    let cfg = DaemonConfig::from_path(file.path()).expect("load");
    let endpoints = cfg.tcp_client_endpoints();
    assert_eq!(endpoints.len(), 1);
    assert_eq!(endpoints[0].0, "rmap.world");
    assert_eq!(endpoints[0].1, 4242);
}
