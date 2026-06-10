use reticulum_daemon::config::{DaemonConfig, InterfaceConfig};
use rns_transport::iface::InterfaceMode;
use std::fs;
use tempfile::NamedTempFile;

#[test]
fn parses_tcp_client_interface() {
    let input = r#"
display_name = "RCH Rust Stress Hub"

interfaces = [
  { type = "tcp_client", enabled = true, host = "rmap.world", port = 4242, name = "Public RMap" }
]
"#;
    let cfg = DaemonConfig::from_toml(input).expect("parse");
    assert_eq!(cfg.display_name.as_deref(), Some("RCH Rust Stress Hub"));
    assert_eq!(cfg.interfaces.len(), 1);
    let iface = &cfg.interfaces[0];
    assert_eq!(iface.name.as_deref(), Some("Public RMap"));
    assert_eq!(iface.host.as_deref(), Some("rmap.world"));
    assert_eq!(iface.port, Some(4242));
    assert!(iface.enabled.unwrap_or(false));
}

#[test]
fn parses_reticulum_tcp_client_interface_aliases() {
    let input = r#"
interfaces = [
  { type = "TCPClientInterface", enabled = true, name = "python-tcp-client", target_host = "rmap.world", target_port = 4242 }
]
"#;
    let cfg = DaemonConfig::from_toml(input).expect("parse Python TCPClientInterface config");
    let iface = &cfg.interfaces[0];
    assert_eq!(iface.kind, "tcp_client");
    assert_eq!(iface.host.as_deref(), Some("rmap.world"));
    assert_eq!(iface.port, Some(4242));
    assert_eq!(cfg.tcp_client_endpoints(), vec![("rmap.world".to_string(), 4242)]);
}

#[test]
fn parses_reticulum_tcp_client_fixed_mtu_alias() {
    let input = r#"
interfaces = [
  { type = "TCPClientInterface", enabled = true, name = "python-tcp-client", target_host = "rmap.world", target_port = 4242, fixed_mtu = 4096 }
]
"#;
    let cfg =
        DaemonConfig::from_toml(input).expect("parse Python TCPClientInterface fixed_mtu config");
    let iface = &cfg.interfaces[0];
    assert_eq!(iface.kind, "tcp_client");
    assert_eq!(iface.host.as_deref(), Some("rmap.world"));
    assert_eq!(iface.port, Some(4242));
    assert_eq!(iface.mtu, Some(4096));

    let settings = iface.settings_json().expect("settings");
    assert_eq!(settings["host"], "rmap.world");
    assert_eq!(settings["port"], 4242);
    assert_eq!(settings["mtu"], 4096);
}

#[test]
fn parses_reticulum_tcp_client_kiss_framing_as_kiss_tcp_client() {
    let input = r#"
interfaces = [
  { type = "TCPClientInterface", enabled = true, name = "python-kiss-tcp", target_host = "192.0.2.10", target_port = 8001, kiss_framing = true, fixed_mtu = 512 }
]
"#;
    let cfg = DaemonConfig::from_toml(input)
        .expect("parse Python TCPClientInterface KISS framing config");
    let iface = &cfg.interfaces[0];
    assert_eq!(iface.kind, "kiss_tcp_client");
    assert_eq!(iface.host.as_deref(), Some("192.0.2.10"));
    assert_eq!(iface.port, Some(8001));
    assert_eq!(iface.mtu, Some(512));
    assert!(cfg.tcp_client_endpoints().is_empty());

    let settings = iface.settings_json().expect("settings");
    assert_eq!(settings["host"], "192.0.2.10");
    assert_eq!(settings["port"], 8001);
    assert_eq!(settings["mtu"], 512);
}

#[test]
fn parses_reticulum_tcp_server_interface_aliases() {
    let input = r#"
interfaces = [
  { type = "TCPServerInterface", enabled = true, name = "python-tcp-server", listen_ip = "127.0.0.1", listen_port = 4242 }
]
"#;
    let cfg = DaemonConfig::from_toml(input).expect("parse Python TCPServerInterface config");
    let iface = &cfg.interfaces[0];
    assert_eq!(iface.kind, "tcp_server");
    assert_eq!(iface.host.as_deref(), Some("127.0.0.1"));
    assert_eq!(iface.port, Some(4242));
    assert_eq!(cfg.tcp_server_endpoints(), vec![("127.0.0.1".to_string(), 4242)]);
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
fn parses_common_reticulum_outgoing_flag() {
    let input = r#"
interfaces = [
  { type = "KISSInterface", enabled = true, name = "kiss-main", port = "/dev/ttyACM0", speed = 19200, outgoing = false },
  { type = "RNodeInterface", enabled = true, name = "rnode-main", region = "US915", state_path = "/tmp/lora-state.json", port = "tcp://192.0.2.10:8001", frequency = 915000000, bandwidth = 125000, spreadingfactor = 9, codingrate = 5, txpower = 17, outgoing = true }
]
"#;
    let cfg = DaemonConfig::from_toml(input).expect("parse common outgoing flags");
    assert_eq!(cfg.interfaces[0].outgoing, Some(false));
    assert_eq!(cfg.interfaces[1].outgoing, Some(true));

    let kiss_settings = cfg.interfaces[0].settings_json().expect("kiss settings");
    assert_eq!(kiss_settings["outgoing"], false);
    let lora_settings = cfg.interfaces[1].settings_json().expect("lora settings");
    assert_eq!(lora_settings["outgoing"], true);
}

#[test]
fn parses_common_reticulum_announce_pacing_fields() {
    let input = r#"
interfaces = [
  { type = "KISSInterface", enabled = true, name = "kiss-main", port = "/dev/ttyACM0", speed = 19200, bitrate = 1200, announce_cap = 5 },
  { type = "RNodeInterface", enabled = true, name = "rnode-main", region = "US915", state_path = "/tmp/lora-state.json", port = "tcp://192.0.2.10:8001", frequency = 915000000, bandwidth = 125000, spreadingfactor = 9, codingrate = 5, txpower = 17, bitrate = 9600, announce_cap = 2 }
]
"#;
    let cfg = DaemonConfig::from_toml(input).expect("parse common announce pacing fields");
    assert_eq!(cfg.interfaces[0].bitrate, Some(1200));
    assert_eq!(cfg.interfaces[0].announce_cap, Some(5));
    assert_eq!(cfg.interfaces[1].bitrate, Some(9600));
    assert_eq!(cfg.interfaces[1].announce_cap, Some(2));

    let kiss_settings = cfg.interfaces[0].settings_json().expect("kiss settings");
    assert_eq!(kiss_settings["bitrate"], 1200);
    assert_eq!(kiss_settings["announce_cap"], 5);
    let lora_settings = cfg.interfaces[1].settings_json().expect("lora settings");
    assert_eq!(lora_settings["bitrate"], 9600);
    assert_eq!(lora_settings["announce_cap"], 2);
}

#[test]
fn rejects_invalid_common_announce_pacing_fields() {
    let input = r#"
interfaces = [
  { type = "KISSInterface", enabled = true, name = "kiss-main", port = "/dev/ttyACM0", speed = 19200, bitrate = 0, announce_cap = 101 }
]
"#;
    let err = DaemonConfig::from_toml(input).expect_err("invalid announce pacing must fail");
    let message = err.to_string();
    assert!(message.contains("bitrate must be > 0"), "unexpected parse error: {message}");
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
        display_name: None,
        announce_capabilities: Vec::new(),
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
        display_name: None,
        announce_capabilities: Vec::new(),
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
fn parses_reticulum_udp_interface_aliases() {
    let input = r#"
interfaces = [
  { type = "UDPInterface", enabled = true, name = "python-udp", listen_ip = "127.0.0.1", listen_port = 4242, forward_ip = "127.0.0.1", forward_port = 4243 }
]
"#;
    let cfg = DaemonConfig::from_toml(input).expect("parse Python UDPInterface config");
    let iface = &cfg.interfaces[0];
    assert_eq!(iface.kind, "udp");
    assert_eq!(iface.host.as_deref(), Some("127.0.0.1"));
    assert_eq!(iface.port, Some(4242));
    assert_eq!(iface.target_host.as_deref(), Some("127.0.0.1"));
    assert_eq!(iface.target_port, Some(4243));

    let settings = iface.settings_json().expect("settings");
    assert_eq!(settings["target_host"], "127.0.0.1");
    assert_eq!(settings["target_port"], 4243);
}

#[test]
fn parses_reticulum_auto_interface_defaults() {
    let input = r#"
interfaces = [
  { type = "AutoInterface", enabled = true, name = "python-auto" }
]
"#;
    let cfg = DaemonConfig::from_toml(input).expect("parse Python AutoInterface config");
    let iface = &cfg.interfaces[0];
    assert_eq!(iface.kind, "auto");
    assert_eq!(iface.group_id.as_deref(), Some("reticulum"));
    assert_eq!(iface.discovery_scope.as_deref(), Some("link"));
    assert_eq!(iface.discovery_port, Some(29716));
    assert_eq!(iface.data_port, Some(42671));
    assert_eq!(iface.multicast_address_type.as_deref(), Some("temporary"));

    let settings = iface.settings_json().expect("settings");
    assert_eq!(settings["group_id"], "reticulum");
    assert_eq!(settings["discovery_scope"], "link");
    assert_eq!(settings["discovery_port"], 29716);
    assert_eq!(settings["data_port"], 42671);
    assert_eq!(settings["multicast_address_type"], "temporary");
    assert_eq!(settings["discovery_multicast_address"], "ff12:0:d70b:fb1c:16e4:5e39:485e:31e1");
}

#[test]
fn parses_reticulum_auto_interface_options() {
    let input = r#"
interfaces = [
  { type = "AutoInterface", enabled = true, name = "python-auto", group_id = "field-net", discovery_scope = "global", discovery_port = 48555, data_port = 49555, multicast_address_type = "permanent", devices = ["wlan0", "eth1"], ignored_devices = "tun0,eth0", configured_bitrate = 10000000 }
]
"#;
    let cfg = DaemonConfig::from_toml(input).expect("parse configured Python AutoInterface");
    let iface = &cfg.interfaces[0];
    assert_eq!(iface.kind, "auto");
    assert_eq!(iface.devices.as_deref(), Some(&["wlan0".to_string(), "eth1".to_string()][..]));
    assert_eq!(
        iface.ignored_devices.as_deref(),
        Some(&["tun0".to_string(), "eth0".to_string()][..])
    );
    assert_eq!(iface.bitrate, Some(10_000_000));

    let settings = iface.settings_json().expect("settings");
    assert_eq!(settings["group_id"], "field-net");
    assert_eq!(settings["discovery_scope"], "global");
    assert_eq!(settings["discovery_port"], 48555);
    assert_eq!(settings["data_port"], 49555);
    assert_eq!(settings["multicast_address_type"], "permanent");
    assert_eq!(settings["discovery_multicast_address"], "ff0e:0:77b9:4bfd:9488:364b:4bbe:119d");
    assert_eq!(settings["devices"], serde_json::json!(["wlan0", "eth1"]));
    assert_eq!(settings["ignored_devices"], serde_json::json!(["tun0", "eth0"]));
    assert_eq!(settings["bitrate"], 10_000_000);
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
fn parses_reticulum_serial_interface_type_and_field_aliases() {
    let input = r#"
interfaces = [
  { type = "SerialInterface", enabled = true, name = "python-serial", port = "/dev/ttyUSB0", speed = 19200, databits = 7, parity = "N", stopbits = 2 }
]
"#;
    let cfg = DaemonConfig::from_toml(input).expect("parse Python SerialInterface config");
    let iface = &cfg.interfaces[0];
    assert_eq!(iface.kind, "serial");
    assert_eq!(iface.device.as_deref(), Some("/dev/ttyUSB0"));
    assert_eq!(iface.baud_rate, Some(19200));
    assert_eq!(iface.data_bits, Some(7));
    assert_eq!(iface.parity.as_deref(), Some("N"));
    assert_eq!(iface.stop_bits, Some(2));

    let settings = iface.settings_json().expect("settings");
    assert_eq!(settings["device"], "/dev/ttyUSB0");
    assert_eq!(settings["baud_rate"], 19200);
    assert_eq!(settings["data_bits"], 7);
    assert_eq!(settings["parity"], "N");
    assert_eq!(settings["stop_bits"], 2);
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
fn parses_enabled_kiss_interface_with_modem_settings() {
    let input = r#"
interfaces = [
  { type = "kiss", enabled = true, name = "kiss-main", device = "/dev/ttyACM0", baud_rate = 9600, preamble_ms = 350, tx_tail_ms = 20, persistence = 64, slot_time_ms = 20, kiss_flow_control = true }
]
"#;
    let cfg = DaemonConfig::from_toml(input).expect("parse kiss config");
    let iface = &cfg.interfaces[0];
    assert_eq!(iface.kind, "kiss");
    assert_eq!(iface.device.as_deref(), Some("/dev/ttyACM0"));
    assert_eq!(iface.baud_rate, Some(9600));

    let settings = iface.settings_json().expect("settings");
    assert_eq!(settings["preamble_ms"], 350);
    assert_eq!(settings["tx_tail_ms"], 20);
    assert_eq!(settings["persistence"], 64);
    assert_eq!(settings["slot_time_ms"], 20);
    assert_eq!(settings["kiss_flow_control"], true);
}

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
