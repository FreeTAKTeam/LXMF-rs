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
        propagation_node: None,
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
        propagation_node: None,
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
