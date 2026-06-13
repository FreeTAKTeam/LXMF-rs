#[test]
fn parses_lora_python_rnode_arbitrary_valid_bandwidth() {
    let input = r#"
interfaces = [
  { type = "RNodeInterface", enabled = true, name = "rnode-custom-bandwidth", region = "US915", state_path = "/tmp/lora-state.json", port = "tcp://192.0.2.10:8001", frequency = 2400000000, bandwidth = 1000000, spreadingfactor = 5, codingrate = 5, txpower = 17 }
]
"#;
    let cfg = DaemonConfig::from_toml(input).expect("parse Python-valid Reticulum RNode bandwidth");
    let iface = &cfg.interfaces[0];

    assert_eq!(iface.kind, "lora");
    assert_eq!(iface.bandwidth_hz, Some(1_000_000));

    let settings = iface.settings_json().expect("settings");
    assert_eq!(settings["bandwidth_hz"], 1_000_000);
}

#[test]
fn rejects_reticulum_rnode_interface_missing_python_radio_parameters() {
    let input = r#"
interfaces = [
  { type = "RNodeInterface", enabled = true, name = "rnode-main", region = "US915", state_path = "/tmp/lora-state.json", port = "/dev/ttyACM0" }
]
"#;
    let err = DaemonConfig::from_toml(input)
        .expect_err("Reticulum RNodeInterface must require Python radio parameters");
    let message = err.to_string();
    assert!(
        message.contains("frequency is required for RNodeInterface"),
        "unexpected parse error: {message}"
    );
}

#[test]
fn parses_reticulum_rnode_interface_type_alias() {
    let input = r#"
interfaces = [
  { type = "RNodeInterface", enabled = true, name = "rnode-main", region = "US915", state_path = "/tmp/lora-state.json", port = "/dev/ttyACM0", baud_rate = 115200, frequency = 915000000, bandwidth = 125000, spreadingfactor = 9, codingrate = 5, txpower = 17 }
]
"#;
    let cfg = DaemonConfig::from_toml(input).expect("parse Reticulum RNodeInterface config");
    let iface = &cfg.interfaces[0];
    assert_eq!(iface.kind, "lora");
    assert_eq!(iface.device.as_deref(), Some("/dev/ttyACM0"));
    assert_eq!(iface.frequency_hz, Some(915_000_000));
    assert_eq!(iface.bandwidth_hz, Some(125_000));
    assert_eq!(iface.spreading_factor, Some(9));
    assert_eq!(iface.coding_rate.as_deref(), Some("5"));
    assert_eq!(iface.tx_power_dbm, Some(17));
}

#[test]
fn rejects_lora_invalid_airtime_limit() {
    let input = r#"
interfaces = [
  { type = "lora", enabled = true, region = "US915", state_path = "/tmp/lora-state.json", airtime_limit_short = 100.5 }
]
"#;
    let err = DaemonConfig::from_toml(input).expect_err("invalid airtime limit must fail");
    let message = err.to_string();
    assert!(
        message.contains("airtime_limit_short must be between 0 and 100"),
        "unexpected parse error: {message}"
    );
}

#[test]
fn rejects_lora_invalid_frequency_range_like_python_rnode() {
    let input = r#"
interfaces = [
  { type = "RNodeInterface", enabled = true, region = "US915", state_path = "/tmp/lora-state.json", port = "tcp://192.0.2.10:8001", frequency = 136999999, bandwidth = 125000, spreadingfactor = 9, codingrate = 5, txpower = 17 }
]
"#;
    let err = DaemonConfig::from_toml(input).expect_err("invalid RNode frequency must fail");
    let message = err.to_string();
    assert!(
        message.contains("frequency_hz must be between 137000000 and 3000000000"),
        "unexpected parse error: {message}"
    );
}

#[test]
fn rejects_lora_invalid_tx_power_range_like_python_rnode() {
    let input = r#"
interfaces = [
  { type = "RNodeInterface", enabled = true, region = "US915", state_path = "/tmp/lora-state.json", port = "tcp://192.0.2.10:8001", frequency = 915000000, bandwidth = 125000, spreadingfactor = 9, codingrate = 5, txpower = 38 }
]
"#;
    let err = DaemonConfig::from_toml(input).expect_err("invalid RNode TX power must fail");
    let message = err.to_string();
    assert!(
        message.contains("tx_power_dbm must be between 0 and 37"),
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
fn rejects_known_unsupported_python_interface_families_with_specific_error() {
    for kind in
        ["PipeInterface", "LocalInterface", "I2PInterface", "WeaveInterface", "BackboneInterface"]
    {
        let input = format!(
            r#"
interfaces = [
  {{ type = "{kind}", enabled = true, name = "unsupported" }}
]
"#
        );
        let err = match DaemonConfig::from_toml(&input) {
            Ok(_) => panic!("{kind} should be rejected as known unsupported"),
            Err(err) => err,
        };
        let message = err.to_string();
        assert!(
            message.contains("known unsupported Reticulum interface family"),
            "unexpected parse error for {kind}: {message}"
        );
        assert!(message.contains(kind), "error should name {kind}: {message}");
    }
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
