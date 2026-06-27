#[test]
fn parses_reticulum_kiss_interface_type_and_field_aliases() {
    let input = r#"
interfaces = [
  { type = "KISSInterface", enabled = true, name = "kiss-main", port = "/dev/ttyACM0", speed = 19200, databits = 8, parity = "N", stopbits = 1, preamble = 350, txtail = 20, persistence = 64, slottime = 20, flow_control = true, id_callsign = "MYCALL-0", id_interval = 600 }
]
"#;
    let cfg = DaemonConfig::from_toml(input).expect("parse Reticulum KISSInterface config");
    let iface = &cfg.interfaces[0];
    assert_eq!(iface.kind, "kiss");
    assert_eq!(iface.device.as_deref(), Some("/dev/ttyACM0"));
    assert_eq!(iface.baud_rate, Some(19200));
    assert_eq!(iface.data_bits, Some(8));
    assert_eq!(iface.parity.as_deref(), Some("N"));
    assert_eq!(iface.stop_bits, Some(1));
    assert_eq!(iface.preamble_ms, Some(350));
    assert_eq!(iface.tx_tail_ms, Some(20));
    assert_eq!(iface.slot_time_ms, Some(20));
    assert_eq!(iface.kiss_flow_control, Some(true));
    assert_eq!(iface.id_callsign.as_deref(), Some("MYCALL-0"));
    assert_eq!(iface.id_interval, Some(600));

    let settings = iface.settings_json().expect("settings");
    assert_eq!(settings["device"], "/dev/ttyACM0");
    assert_eq!(settings["baud_rate"], 19200);
    assert_eq!(settings["preamble_ms"], 350);
    assert_eq!(settings["tx_tail_ms"], 20);
    assert_eq!(settings["slot_time_ms"], 20);
    assert_eq!(settings["kiss_flow_control"], true);
    assert_eq!(settings["id_callsign"], "MYCALL-0");
    assert_eq!(settings["id_interval"], 600);
}

#[test]
fn parses_android_kiss_beacon_aliases_as_python_id_beacon() {
    let input = r#"
interfaces = [
  { type = "KISSInterface", enabled = true, name = "android-kiss", port = "/dev/ttyACM0", beacon_interval = 900, beacon_data = "ANDROID-1" }
]
"#;
    let cfg = DaemonConfig::from_toml(input).expect("parse Android KISS beacon aliases");
    let iface = &cfg.interfaces[0];

    assert_eq!(iface.kind, "kiss");
    assert_eq!(iface.id_interval, Some(900));
    assert_eq!(iface.id_callsign.as_deref(), Some("ANDROID-1"));

    let settings = iface.settings_json().expect("settings");
    assert_eq!(settings["id_interval"], 900);
    assert_eq!(settings["id_callsign"], "ANDROID-1");
}

#[test]
fn android_kiss_beacon_aliases_preserve_canonical_id_beacon_precedence() {
    let input = r#"
interfaces = [
  { type = "KISSInterface", enabled = true, name = "android-kiss", port = "/dev/ttyACM0", id_interval = 600, id_callsign = "PYTHON-0", beacon_interval = 900, beacon_data = "ANDROID-1" }
]
"#;
    let cfg = DaemonConfig::from_toml(input).expect("parse mixed KISS beacon aliases");
    let iface = &cfg.interfaces[0];

    assert_eq!(iface.id_interval, Some(600));
    assert_eq!(iface.id_callsign.as_deref(), Some("PYTHON-0"));
    assert!(iface.extra.is_empty());
}

#[test]
fn parses_reticulum_kiss_interface_default_speed() {
    let input = r#"
interfaces = [
  { type = "KISSInterface", enabled = true, name = "kiss-main", port = "/dev/ttyACM0" }
]
"#;
    let cfg = DaemonConfig::from_toml(input).expect("parse Reticulum KISSInterface default speed");
    let iface = &cfg.interfaces[0];
    assert_eq!(iface.kind, "kiss");
    assert_eq!(iface.device.as_deref(), Some("/dev/ttyACM0"));
    assert_eq!(iface.baud_rate, Some(9600));

    let settings = iface.settings_json().expect("settings");
    assert_eq!(settings["baud_rate"], 9600);
}

#[test]
fn rejects_enabled_kiss_interface_missing_required_fields() {
    let input = r#"
interfaces = [
  { type = "kiss", enabled = true, baud_rate = 9600 }
]
"#;
    let err = DaemonConfig::from_toml(input).expect_err("kiss should require device");
    let message = err.to_string();
    assert!(message.contains("device is required for kiss"), "unexpected parse error: {message}");
}

#[test]
fn parses_enabled_kiss_tcp_client_interface_with_modem_settings() {
    let input = r#"
interfaces = [
  { type = "kiss_tcp_client", enabled = true, name = "kiss-wifi", host = "192.0.2.10", port = 8001, mtu = 512, preamble_ms = 350, tx_tail_ms = 20, persistence = 64, slot_time_ms = 20, kiss_flow_control = true }
]
"#;
    let cfg = DaemonConfig::from_toml(input).expect("parse kiss tcp client config");
    let iface = &cfg.interfaces[0];
    assert_eq!(iface.kind, "kiss_tcp_client");
    assert_eq!(iface.host.as_deref(), Some("192.0.2.10"));
    assert_eq!(iface.port, Some(8001));

    let settings = iface.settings_json().expect("settings");
    assert_eq!(settings["host"], "192.0.2.10");
    assert_eq!(settings["port"], 8001);
    assert_eq!(settings["mtu"], 512);
    assert_eq!(settings["preamble_ms"], 350);
    assert_eq!(settings["tx_tail_ms"], 20);
    assert_eq!(settings["persistence"], 64);
    assert_eq!(settings["slot_time_ms"], 20);
    assert_eq!(settings["kiss_flow_control"], true);
}

#[test]
fn rejects_enabled_kiss_tcp_client_missing_required_fields() {
    let input = r#"
interfaces = [
  { type = "kiss_tcp_client", enabled = true, port = 8001 }
]
"#;
    let err =
        DaemonConfig::from_toml(input).expect_err("kiss_tcp_client should require host and port");
    let message = err.to_string();
    assert!(
        message.contains("host is required for kiss_tcp_client"),
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
fn parses_vrn76_kiss_ble_interface_with_profile_defaults() {
    let input = r#"
interfaces = [
  { type = "vrn76_kiss_ble", enabled = true, name = "vrn76-main", peripheral_id = "VR-N76", adapter = "Bluetooth", mtu = 564, max_write_len = 128, preamble_ms = 350, tx_tail_ms = 20, persistence = 64, slot_time_ms = 20, kiss_flow_control = true, scan_timeout_ms = 10000, connect_timeout_ms = 3000 }
]
"#;
    let cfg = DaemonConfig::from_toml(input).expect("parse vrn76 kiss ble config");
    let iface = &cfg.interfaces[0];
    assert_eq!(iface.kind, "vrn76_kiss_ble");
    assert_eq!(iface.peripheral_id.as_deref(), Some("VR-N76"));
    assert_eq!(iface.adapter.as_deref(), Some("Bluetooth"));

    let settings = iface.settings_json().expect("settings");
    assert_eq!(settings["adapter"], "Bluetooth");
    assert_eq!(settings["peripheral_id"], "VR-N76");
    assert_eq!(settings["mtu"], 564);
    assert_eq!(settings["max_write_len"], 128);
    assert_eq!(settings["preamble_ms"], 350);
    assert_eq!(settings["tx_tail_ms"], 20);
    assert_eq!(settings["persistence"], 64);
    assert_eq!(settings["slot_time_ms"], 20);
    assert_eq!(settings["kiss_flow_control"], true);
    assert_eq!(settings["scan_timeout_ms"], 10000);
    assert_eq!(settings["connect_timeout_ms"], 3000);
}

#[test]
fn parses_vrn76_kiss_ble_issue_197_config_aliases() {
    let input = r#"
interfaces = [
  { type = "vrn76_kiss_ble", enabled = true, name = "vrn76-main", device_name_filter = "VR-N76", device_address = "", ble_scan_timeout_ms = 10000, command_timeout_ms = 3000, mtu = 564, preamble_ms = 350, tx_tail_ms = 20, persistence = 64, slot_time_ms = 20, kiss_flow_control = false, mode = "full", outgoing = true }
]
"#;
    let cfg = DaemonConfig::from_toml(input).expect("parse vrn76 issue config aliases");
    let iface = &cfg.interfaces[0];
    assert_eq!(iface.kind, "vrn76_kiss_ble");
    assert_eq!(iface.peripheral_id.as_deref(), Some("VR-N76"));
    assert_eq!(iface.scan_timeout_ms, Some(10_000));
    assert_eq!(iface.connect_timeout_ms, Some(3_000));

    let settings = iface.settings_json().expect("settings");
    assert_eq!(settings["peripheral_id"], "VR-N76");
    assert_eq!(settings["scan_timeout_ms"], 10_000);
    assert_eq!(settings["connect_timeout_ms"], 3_000);
    assert_eq!(settings["outgoing"], true);
}

#[test]
fn parses_vrn76_kiss_ble_reticulum_style_type_and_modem_aliases() {
    let input = r#"
interfaces = [
  { type = "Vrn76KissBluetoothInterface", enabled = true, name = "vrn76-main", device_name_filter = "VR-N76", ble_scan_timeout_ms = 10000, command_timeout_ms = 3000, mtu = 564, preamble = 350, txtail = 20, persistence = 64, slottime = 20, flow_control = false, mode = "full", outgoing = true }
]
"#;
    let cfg = DaemonConfig::from_toml(input).expect("parse reticulum-style vrn76 config");
    let iface = &cfg.interfaces[0];
    assert_eq!(iface.kind, "vrn76_kiss_ble");
    assert_eq!(iface.peripheral_id.as_deref(), Some("VR-N76"));
    assert_eq!(iface.preamble_ms, Some(350));
    assert_eq!(iface.tx_tail_ms, Some(20));
    assert_eq!(iface.slot_time_ms, Some(20));
    assert_eq!(iface.kiss_flow_control, Some(false));

    let settings = iface.settings_json().expect("settings");
    assert_eq!(settings["preamble_ms"], 350);
    assert_eq!(settings["tx_tail_ms"], 20);
    assert_eq!(settings["slot_time_ms"], 20);
    assert_eq!(settings["kiss_flow_control"], false);
}

#[test]
fn parses_vrn76_kiss_ble_short_reticulum_style_type_alias() {
    let input = r#"
interfaces = [
  { type = "Vrn76KissBleInterface", enabled = true, name = "vrn76-main", device_name_filter = "VR-N76" }
]
"#;
    let cfg = DaemonConfig::from_toml(input).expect("parse short Reticulum-style vrn76 config");
    let iface = &cfg.interfaces[0];
    assert_eq!(iface.kind, "vrn76_kiss_ble");
    assert_eq!(iface.peripheral_id.as_deref(), Some("VR-N76"));

    let settings = iface.settings_json().expect("settings");
    assert_eq!(settings["peripheral_id"], "VR-N76");
}

#[test]
fn parses_vrn76_kiss_ble_python_kiss_id_beacon_settings() {
    let input = r#"
interfaces = [
  { type = "Vrn76KissBluetoothInterface", enabled = true, name = "vrn76-main", device_name_filter = "VR-N76", id_callsign = "MYCALL-0", id_interval = 600 }
]
"#;
    let cfg = DaemonConfig::from_toml(input).expect("parse vrn76 id beacon config");
    let iface = &cfg.interfaces[0];
    assert_eq!(iface.kind, "vrn76_kiss_ble");
    assert_eq!(iface.id_callsign.as_deref(), Some("MYCALL-0"));
    assert_eq!(iface.id_interval, Some(600));
    let settings = iface.settings_json().expect("vrn76 settings");
    assert_eq!(settings["id_callsign"], "MYCALL-0");
    assert_eq!(settings["id_interval"], 600);
}

#[test]
fn parses_vrn76_kiss_ble_raw_kiss_frame_mode() {
    let input = r#"
interfaces = [
  { type = "Vrn76KissBluetoothInterface", enabled = true, name = "vrn76-raw", device_name_filter = "VR-N76", frame_mode = "raw_kiss" }
]
"#;
    let cfg = DaemonConfig::from_toml(input).expect("parse raw KISS VR-N76 config");
    let iface = &cfg.interfaces[0];
    assert_eq!(iface.kind, "vrn76_kiss_ble");
    assert_eq!(iface.frame_mode.as_deref(), Some("raw_kiss"));

    let settings = iface.settings_json().expect("settings");
    assert_eq!(settings["frame_mode"], "raw_kiss");
}

#[test]
fn parses_vrn76_kiss_ble_table_style_interface_config() {
    let input = r#"
[interfaces.vrn76_kiss_ble]
type = "Vrn76KissBluetoothInterface"
enabled = true
device_name_filter = "VR-N76"
ble_scan_timeout_ms = 10000
command_timeout_ms = 3000
mtu = 564
preamble = 350
txtail = 20
persistence = 64
slottime = 20
flow_control = false
mode = "full"
outgoing = true
"#;
    let cfg = DaemonConfig::from_toml(input).expect("parse table-style vrn76 config");
    assert_eq!(cfg.interfaces.len(), 1);
    let iface = &cfg.interfaces[0];
    assert_eq!(iface.kind, "vrn76_kiss_ble");
    assert_eq!(iface.name.as_deref(), Some("vrn76_kiss_ble"));
    assert_eq!(iface.peripheral_id.as_deref(), Some("VR-N76"));
    assert_eq!(iface.scan_timeout_ms, Some(10_000));
    assert_eq!(iface.connect_timeout_ms, Some(3_000));
    assert_eq!(iface.kiss_flow_control, Some(false));
}

#[test]
fn rejects_enabled_vrn76_kiss_ble_missing_peripheral_id() {
    let input = r#"
interfaces = [
  { type = "vrn76_kiss_ble", enabled = true }
]
"#;
    let err = DaemonConfig::from_toml(input).expect_err("vrn76 should require peripheral_id");
    let message = err.to_string();
    assert!(
        message.contains("peripheral_id is required for vrn76_kiss_ble"),
        "unexpected parse error: {message}"
    );
}

#[test]
fn rejects_vrn76_kiss_ble_too_small_max_write_len() {
    let input = r#"
interfaces = [
  { type = "vrn76_kiss_ble", enabled = true, peripheral_id = "VR-N76", max_write_len = 5 }
]
"#;
    let err = DaemonConfig::from_toml(input).expect_err("max_write_len below Benshi minimum");
    let message = err.to_string();
    assert!(
        message.contains("max_write_len must be between 6 and 65535 for vrn76_kiss_ble"),
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
fn parses_active_lora_interface_with_serial_device_settings() {
    let input = r#"
interfaces = [
  { type = "lora", enabled = true, name = "lora-main", region = "US915", state_path = "/tmp/lora-state.json", device = "/dev/ttyACM1", baud_rate = 115200, frequency_hz = 915000000, bandwidth_hz = 125000, spreading_factor = 9, coding_rate = "4/5", tx_power_dbm = 17, airtime_limit_short = 33.0, airtime_limit_long = 1.5, max_payload_bytes = 220 }
]
"#;
    let cfg = DaemonConfig::from_toml(input).expect("parse active lora config");
    let iface = &cfg.interfaces[0];
    assert_eq!(iface.kind, "lora");
    assert_eq!(iface.device.as_deref(), Some("/dev/ttyACM1"));
    assert_eq!(iface.baud_rate, Some(115200));

    let settings = iface.settings_json().expect("settings");
    assert_eq!(settings["device"], "/dev/ttyACM1");
    assert_eq!(settings["baud_rate"], 115200);
    assert_eq!(settings["frequency_hz"], 915000000);
    assert_eq!(settings["airtime_limit_short"], 33.0);
    assert_eq!(settings["airtime_limit_long"], 1.5);
}

#[test]
fn parses_lora_python_rnode_config_aliases() {
    let input = r#"
interfaces = [
  { type = "lora", enabled = true, name = "rnode-main", region = "US915", state_path = "/tmp/lora-state.json", port = "/dev/ttyACM0", baud_rate = 115200, frequency = 915000000, bandwidth = 125000, spreadingfactor = 9, codingrate = 5, txpower = 17, airtime_limit_short = 33.0, airtime_limit_long = 1.5 }
]
"#;
    let cfg = DaemonConfig::from_toml(input).expect("parse python-style rnode config aliases");
    let iface = &cfg.interfaces[0];
    assert_eq!(iface.kind, "lora");
    assert_eq!(iface.device.as_deref(), Some("/dev/ttyACM0"));
    assert_eq!(iface.frequency_hz, Some(915_000_000));
    assert_eq!(iface.bandwidth_hz, Some(125_000));
    assert_eq!(iface.spreading_factor, Some(9));
    assert_eq!(iface.coding_rate.as_deref(), Some("5"));
    assert_eq!(iface.tx_power_dbm, Some(17));

    let settings = iface.settings_json().expect("settings");
    assert_eq!(settings["device"], "/dev/ttyACM0");
    assert_eq!(settings["frequency_hz"], 915000000);
    assert_eq!(settings["bandwidth_hz"], 125000);
    assert_eq!(settings["spreading_factor"], 9);
    assert_eq!(settings["coding_rate"], "5");
    assert_eq!(settings["tx_power_dbm"], 17);
}

#[test]
fn parses_lora_python_rnode_flow_control() {
    let input = r#"
interfaces = [
  { type = "RNodeInterface", enabled = true, name = "rnode-main", region = "US915", state_path = "/tmp/lora-state.json", port = "/dev/ttyACM0", baud_rate = 115200, frequency = 915000000, bandwidth = 125000, spreadingfactor = 9, codingrate = 5, txpower = 17, flow_control = true }
]
"#;
    let cfg = DaemonConfig::from_toml(input).expect("parse Reticulum RNode flow_control config");
    let iface = &cfg.interfaces[0];
    assert_eq!(iface.kind, "lora");

    let settings = iface.settings_json().expect("settings");
    assert_eq!(settings["flow_control"], true);
}

#[test]
fn parses_lora_python_rnode_command_timeout_alias() {
    let input = r#"
interfaces = [
  { type = "RNodeInterface", enabled = true, name = "rnode-main", region = "US915", state_path = "/tmp/lora-state.json", port = "/dev/ttyACM0", baud_rate = 115200, frequency = 915000000, bandwidth = 125000, spreadingfactor = 9, codingrate = 5, txpower = 17, command_timeout_ms = 2750 }
]
"#;
    let cfg = DaemonConfig::from_toml(input).expect("parse Reticulum RNode command_timeout config");
    let iface = &cfg.interfaces[0];
    assert_eq!(iface.kind, "lora");
    assert_eq!(iface.connect_timeout_ms, Some(2_750));

    let settings = iface.settings_json().expect("settings");
    assert_eq!(settings["connect_timeout_ms"], 2_750);
}

#[test]
fn parses_lora_python_rnode_tcp_port_without_serial_baud_rate() {
    let input = r#"
interfaces = [
  { type = "RNodeInterface", enabled = true, name = "rnode-wifi", region = "US915", state_path = "/tmp/lora-state.json", port = "tcp://192.0.2.10:8001", frequency = 915000000, bandwidth = 125000, spreadingfactor = 9, codingrate = 5, txpower = 17 }
]
"#;
    let cfg = DaemonConfig::from_toml(input).expect("parse Reticulum RNode tcp port config");
    let iface = &cfg.interfaces[0];
    assert_eq!(iface.kind, "lora");
    assert_eq!(iface.device.as_deref(), Some("tcp://192.0.2.10:8001"));
    assert_eq!(iface.baud_rate, None);

    let settings = iface.settings_json().expect("settings");
    assert_eq!(settings["device"], "tcp://192.0.2.10:8001");
}

#[test]
fn parses_reticulum_rnode_ble_port_for_native_backend_startup() {
    let input = r#"
interfaces = [
  { type = "RNodeInterface", enabled = true, name = "rnode-ble", region = "US915", state_path = "/tmp/lora-state.json", port = "ble://RNode 1234", frequency = 915000000, bandwidth = 125000, spreadingfactor = 9, codingrate = 5, txpower = 17, command_timeout_ms = 1500, ble_connect_timeout_ms = 5000 }
]
"#;
    let cfg = DaemonConfig::from_toml(input).expect("parse Reticulum RNode BLE port config");
    let iface = &cfg.interfaces[0];
    assert_eq!(iface.kind, "lora");
    assert_eq!(iface.device.as_deref(), Some("ble://RNode 1234"));
    assert_eq!(iface.baud_rate, None);
    assert_eq!(iface.connect_timeout_ms, Some(1_500));
    assert_eq!(iface.ble_connect_timeout_ms, Some(5_000));

    let settings = iface.settings_json().expect("settings");
    assert_eq!(settings["device"], "ble://RNode 1234");
    assert_eq!(settings["connect_timeout_ms"], 1_500);
    assert_eq!(settings["ble_connect_timeout_ms"], 5_000);
}

#[test]
fn parses_android_rnode_tcp_host_selector_alias() {
    let input = r#"
interfaces = [
  { type = "RNodeInterface", enabled = true, name = "rnode-android-tcp", region = "US915", state_path = "/tmp/lora-state.json", tcp_host = "192.0.2.10", force_tcp = true, frequency = 915000000, bandwidth = 125000, spreadingfactor = 9, codingrate = 5, txpower = 17 }
]
"#;
    let cfg = DaemonConfig::from_toml(input).expect("parse Android RNode tcp_host config");
    let iface = &cfg.interfaces[0];
    assert_eq!(iface.kind, "lora");
    assert_eq!(iface.device.as_deref(), Some("tcp://192.0.2.10:7633"));
    assert_eq!(iface.baud_rate, None);

    let settings = iface.settings_json().expect("settings");
    assert_eq!(settings["device"], "tcp://192.0.2.10:7633");
}

#[test]
fn parses_android_rnode_ble_selector_aliases() {
    let input = r#"
interfaces = [
  { type = "RNodeInterface", enabled = true, name = "rnode-android-ble", region = "US915", state_path = "/tmp/lora-state.json", ble_addr = "AA:BB:CC:DD:EE:FF", force_ble = true, frequency = 915000000, bandwidth = 125000, spreadingfactor = 9, codingrate = 5, txpower = 17 }
]
"#;
    let cfg = DaemonConfig::from_toml(input).expect("parse Android RNode BLE selector config");
    let iface = &cfg.interfaces[0];
    assert_eq!(iface.kind, "lora");
    assert_eq!(iface.device.as_deref(), Some("ble://AA:BB:CC:DD:EE:FF"));
    assert_eq!(iface.baud_rate, None);

    let settings = iface.settings_json().expect("settings");
    assert_eq!(settings["device"], "ble://AA:BB:CC:DD:EE:FF");
}

#[test]
fn parses_android_rnode_bluetooth_target_selector_aliases() {
    let input = r#"
interfaces = [
  { type = "RNodeInterface", enabled = true, name = "rnode-android-bt", region = "US915", state_path = "/tmp/lora-state.json", allow_bluetooth = true, target_device_name = "RNode Classic", frequency = 915000000, bandwidth = 125000, spreadingfactor = 9, codingrate = 5, txpower = 17 }
]
"#;
    let cfg = DaemonConfig::from_toml(input)
        .expect("parse Android RNode Bluetooth target selector config");
    let iface = &cfg.interfaces[0];
    assert_eq!(iface.kind, "lora");
    assert_eq!(iface.device.as_deref(), Some("ble://RNode Classic"));
    assert_eq!(iface.baud_rate, None);

    let settings = iface.settings_json().expect("settings");
    assert_eq!(settings["device"], "ble://RNode Classic");
}

#[test]
fn parses_android_rnode_bluetooth_address_before_name() {
    let input = r#"
interfaces = [
  { type = "RNodeInterface", enabled = true, name = "rnode-android-bt", region = "US915", state_path = "/tmp/lora-state.json", allow_bluetooth = true, target_device_name = "RNode Classic", target_device_address = "AA:BB:CC:DD:EE:FF", frequency = 915000000, bandwidth = 125000, spreadingfactor = 9, codingrate = 5, txpower = 17 }
]
"#;
    let cfg = DaemonConfig::from_toml(input)
        .expect("parse Android RNode Bluetooth address selector config");
    let iface = &cfg.interfaces[0];
    assert_eq!(iface.kind, "lora");
    assert_eq!(iface.device.as_deref(), Some("ble://AA:BB:CC:DD:EE:FF"));
    assert_eq!(iface.baud_rate, None);

    let settings = iface.settings_json().expect("settings");
    assert_eq!(settings["device"], "ble://AA:BB:CC:DD:EE:FF");
}

#[test]
fn rejects_android_rnode_bluetooth_without_target() {
    let input = r#"
interfaces = [
  { type = "RNodeInterface", enabled = true, name = "rnode-android-bt", region = "US915", state_path = "/tmp/lora-state.json", allow_bluetooth = true, frequency = 915000000, bandwidth = 125000, spreadingfactor = 9, codingrate = 5, txpower = 17 }
]
"#;
    let err = DaemonConfig::from_toml(input).expect_err("Bluetooth selector target must fail");
    let message = err.to_string();
    assert!(
        message.contains("target_device_name"),
        "unexpected parse error: {message}"
    );
}

#[test]
fn parses_lora_python_rnode_serial_default_speed() {
    let input = r#"
interfaces = [
  { type = "RNodeInterface", enabled = true, name = "rnode-main", region = "US915", state_path = "/tmp/lora-state.json", port = "/dev/ttyACM0", frequency = 915000000, bandwidth = 125000, spreadingfactor = 9, codingrate = 5, txpower = 17 }
]
"#;
    let cfg = DaemonConfig::from_toml(input).expect("parse Reticulum RNode default speed");
    let iface = &cfg.interfaces[0];
    assert_eq!(iface.kind, "lora");
    assert_eq!(iface.device.as_deref(), Some("/dev/ttyACM0"));
    assert_eq!(iface.baud_rate, Some(115_200));

    let settings = iface.settings_json().expect("settings");
    assert_eq!(settings["baud_rate"], 115_200);
}

#[test]
fn parses_python_rnode_multi_interface_with_vport_subinterfaces() {
    let input = r#"
interfaces = [
  { type = "RNodeMultiInterface", enabled = true, name = "rnode-multi", port = "/dev/ttyACM0", id_callsign = "MYCALL-0", id_interval = 600, flow_control = true, airtime_limit_short = 25, radio0 = { vport = 0, frequency = 915000000, bandwidth = 125000, spreadingfactor = 9, codingrate = 5, txpower = 17 }, radio1 = { vport = 3, frequency = 920000000, bandwidth = 125000, spreadingfactor = 10, codingrate = "4/6", txpower = 14, flow_control = false, airtime_limit_long = 75.5, outgoing = false } }
]
"#;
    let cfg = DaemonConfig::from_toml(input).expect("parse Reticulum RNodeMultiInterface");
    let iface = &cfg.interfaces[0];
    assert_eq!(iface.kind, "rnode_multi");
    assert_eq!(iface.device.as_deref(), Some("/dev/ttyACM0"));
    assert_eq!(iface.baud_rate, Some(115_200));
    assert_eq!(iface.mtu, Some(508));
    assert_eq!(iface.id_callsign.as_deref(), Some("MYCALL-0"));
    assert_eq!(iface.id_interval, Some(600));

    let settings = iface.settings_json().expect("settings");
    assert_eq!(settings["device"], "/dev/ttyACM0");
    assert_eq!(settings["baud_rate"], 115_200);
    assert_eq!(settings["id_callsign"], "MYCALL-0");
    assert_eq!(settings["id_interval"], 600);
    assert_eq!(settings["subinterfaces"][0]["vport"], 0);
    assert_eq!(settings["subinterfaces"][0]["frequency_hz"], 915_000_000);
    assert_eq!(settings["subinterfaces"][0]["bandwidth_hz"], 125_000);
    assert_eq!(settings["subinterfaces"][0]["spreading_factor"], 9);
    assert_eq!(settings["subinterfaces"][0]["coding_rate"], 5);
    assert_eq!(settings["subinterfaces"][0]["tx_power_dbm"], 17);
    assert_eq!(settings["subinterfaces"][0]["flow_control"], true);
    assert_eq!(settings["subinterfaces"][0]["airtime_limit_short"], 25.0);
    assert_eq!(settings["subinterfaces"][1]["vport"], 3);
    assert_eq!(settings["subinterfaces"][1]["flow_control"], false);
    assert_eq!(settings["subinterfaces"][1]["airtime_limit_short"], 25.0);
    assert_eq!(settings["subinterfaces"][1]["airtime_limit_long"], 75.5);
    assert_eq!(settings["subinterfaces"][1]["outgoing"], false);
}

#[test]
fn parses_python_rnode_multi_tcp_interface() {
    let input = r#"
interfaces = [
  { type = "RNodeMultiInterface", enabled = true, name = "rnode-multi-tcp", port = "tcp://192.0.2.10:8001", radio0 = { vport = 0, frequency = 915000000, bandwidth = 125000, spreadingfactor = 9, codingrate = 5, txpower = 17 } }
]
"#;
    let cfg = DaemonConfig::from_toml(input).expect("parse Reticulum TCP RNodeMultiInterface");
    let iface = &cfg.interfaces[0];

    assert_eq!(iface.kind, "rnode_multi");
    assert_eq!(iface.device.as_deref(), Some("tcp://192.0.2.10:8001"));
    assert_eq!(iface.mtu, Some(508));

    let settings = iface.settings_json().expect("settings");
    assert_eq!(settings["device"], "tcp://192.0.2.10:8001");
    assert_eq!(settings["subinterfaces"][0]["vport"], 0);
}

#[test]
fn parses_python_rnode_multi_child_enabled_semantics_and_negative_txpower() {
    let input = r#"
interfaces = [
  { type = "RNodeMultiInterface", interface_enabled = true, name = "rnode-multi", port = "/dev/ttyACM0", radio0 = { enabled = false, vport = 0, frequency = 915000000, bandwidth = 125000, spreadingfactor = 9, codingrate = 5, txpower = -9 }, radio1 = { enabled = true, vport = 1, frequency = 920000000, bandwidth = 125000, spreadingfactor = 10, codingrate = 5, txpower = 14 } }
]
"#;
    let cfg = DaemonConfig::from_toml(input)
        .expect("parse RNodeMultiInterface child enable semantics");
    let iface = &cfg.interfaces[0];

    assert!(iface.enabled());
    let settings = iface.settings_json().expect("settings");
    let subinterfaces =
        settings["subinterfaces"].as_array().expect("subinterfaces array");
    assert_eq!(subinterfaces.len(), 1);
    assert_eq!(subinterfaces[0]["vport"], 1);
    assert_eq!(subinterfaces[0]["tx_power_dbm"], 14);
}

#[test]
fn rejects_python_rnode_multi_duplicate_vports() {
    let input = r#"
interfaces = [
  { type = "RNodeMultiInterface", enabled = true, name = "rnode-multi", port = "/dev/ttyACM0", radio0 = { vport = 1, frequency = 915000000, bandwidth = 125000, spreadingfactor = 9, codingrate = 5, txpower = 17 }, radio1 = { vport = 1, frequency = 920000000, bandwidth = 125000, spreadingfactor = 10, codingrate = 5, txpower = 14 } }
]
"#;
    let err = DaemonConfig::from_toml(input).expect_err("duplicate vports should fail");
    let message = err.to_string();
    assert!(message.contains("duplicated"), "unexpected parse error: {message}");
}

#[test]
fn rejects_python_rnode_multi_invalid_subinterface_options() {
    let input = r#"
interfaces = [
  { type = "RNodeMultiInterface", enabled = true, name = "rnode-multi", port = "/dev/ttyACM0", radio0 = { vport = 1, frequency = 915000000, bandwidth = 125000, spreadingfactor = 9, codingrate = 5, txpower = 17, flow_control = "yes" } }
]
"#;
    let err = DaemonConfig::from_toml(input).expect_err("flow_control must be boolean");
    let message = err.to_string();
    assert!(
        message.contains("radio0.flow_control must be a boolean"),
        "unexpected parse error: {message}"
    );

    let input = r#"
interfaces = [
  { type = "RNodeMultiInterface", enabled = true, name = "rnode-multi", port = "/dev/ttyACM0", radio0 = { vport = 1, frequency = 915000000, bandwidth = 125000, spreadingfactor = 9, codingrate = 5, txpower = 17, airtime_limit_short = 125 } }
]
"#;
    let err = DaemonConfig::from_toml(input).expect_err("airtime limit must be in range");
    let message = err.to_string();
    assert!(
        message.contains("radio0.airtime_limit_short must be between 0 and 100"),
        "unexpected parse error: {message}"
    );
}

#[test]
fn parses_lora_python_rnode_high_bandwidth_preset() {
    let input = r#"
interfaces = [
  { type = "RNodeInterface", enabled = true, name = "rnode-high-rate", region = "US915", state_path = "/tmp/lora-state.json", port = "tcp://192.0.2.10:8001", frequency = 2400000000, bandwidth = 1625000, spreadingfactor = 5, codingrate = 5, txpower = 17 }
]
"#;
    let cfg = DaemonConfig::from_toml(input).expect("parse high-bandwidth Reticulum RNode config");
    let iface = &cfg.interfaces[0];

    assert_eq!(iface.kind, "lora");
    assert_eq!(iface.frequency_hz, Some(2_400_000_000));
    assert_eq!(iface.bandwidth_hz, Some(1_625_000));
    assert_eq!(iface.spreading_factor, Some(5));

    let settings = iface.settings_json().expect("settings");
    assert_eq!(settings["frequency_hz"], 2_400_000_000_u64);
    assert_eq!(settings["bandwidth_hz"], 1_625_000);
    assert_eq!(settings["spreading_factor"], 5);
}
