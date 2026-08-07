use reticulum_daemon::config::{DaemonConfig, InterfaceConfig};

use rns_transport::iface::InterfaceMode;

use std::fs;

use tempfile::NamedTempFile;

fn expected_i2p_sam_default() -> (String, u16) {
    std::env::var("I2P_SAM_ADDRESS")
        .ok()
        .and_then(|value| {
            let (host, port) = value.trim().split_once(':')?;
            let host = host.trim();
            if host.is_empty() {
                return None;
            }
            Some((host.to_string(), port.trim().parse().ok()?))
        })
        .unwrap_or_else(|| ("127.0.0.1".to_string(), 7656))
}

#[test]
fn parses_interfaces_reference_example_config() {
    let input = include_str!("../../examples/interfaces-reference.toml");

    let cfg = DaemonConfig::from_toml(input).expect("parse interfaces reference config");

    assert_eq!(cfg.interfaces.len(), 20);
    assert!(cfg.interfaces.iter().all(|iface| !iface.enabled()));
}

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
fn parses_reticulum_interface_enabled_alias() {
    let input = r#"
interfaces = [
  { type = "TCPClientInterface", interface_enabled = true, name = "python-tcp-client", target_host = "rmap.world", target_port = 4242 },
  { type = "TCPClientInterface", enabled = false, interface_enabled = false, name = "disabled", target_host = "example.org", target_port = 4242 }
]
"#;
    let cfg = DaemonConfig::from_toml(input).expect("parse interface_enabled alias");

    assert!(cfg.interfaces[0].enabled());
    assert!(!cfg.interfaces[1].enabled());
    assert_eq!(cfg.enabled_tcp_clients().len(), 1);
    assert_eq!(cfg.tcp_client_endpoints(), vec![("rmap.world".to_string(), 4242)]);
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
fn rejects_reticulum_tcp_client_fixed_mtu_below_reticulum_mtu() {
    let input = r#"
interfaces = [
  { type = "TCPClientInterface", enabled = true, name = "python-tcp-client", target_host = "rmap.world", target_port = 4242, fixed_mtu = 499 }
]
"#;
    let err = DaemonConfig::from_toml(input)
        .expect_err("reject Python TCPClientInterface fixed_mtu below Reticulum MTU");
    assert!(
        err.to_string().contains("fixed_mtu must be 0 or at least 500"),
        "unexpected error: {err}"
    );
}

#[test]
fn treats_reticulum_tcp_client_fixed_mtu_zero_as_default() {
    let input = r#"
interfaces = [
  { type = "TCPClientInterface", enabled = true, name = "python-tcp-client", target_host = "rmap.world", target_port = 4242, fixed_mtu = 0 }
]
"#;
    let cfg = DaemonConfig::from_toml(input)
        .expect("parse Python TCPClientInterface fixed_mtu zero config");
    let iface = &cfg.interfaces[0];
    assert_eq!(iface.kind, "tcp_client");
    assert_eq!(iface.host.as_deref(), Some("rmap.world"));
    assert_eq!(iface.port, Some(4242));
    assert_eq!(iface.mtu, None);

    let settings = iface.settings_json().expect("settings");
    assert!(settings.get("mtu").is_none(), "settings should keep TCP default MTU");
}

#[test]
fn parses_reticulum_tcp_client_reconnect_options() {
    let input = r#"
interfaces = [
  { type = "TCPClientInterface", enabled = true, name = "python-tcp-client", target_host = "rmap.world", target_port = 4242, prefer_ipv6 = true, i2p_tunneled = true, connect_timeout = 7, max_reconnect_tries = 3 }
]
"#;
    let cfg = DaemonConfig::from_toml(input).expect("parse Python TCPClientInterface options");
    let iface = &cfg.interfaces[0];
    assert_eq!(iface.kind, "tcp_client");
    assert_eq!(iface.prefer_ipv6, Some(true));
    assert_eq!(iface.i2p_tunneled, Some(true));
    assert_eq!(iface.connect_timeout, Some(7));
    assert_eq!(iface.max_reconnect_tries, Some(3));

    let settings = iface.settings_json().expect("settings");
    assert_eq!(settings["prefer_ipv6"], true);
    assert_eq!(settings["i2p_tunneled"], true);
    assert_eq!(settings["connect_timeout"], 7);
    assert_eq!(settings["max_reconnect_tries"], 3);
}

#[test]
fn parses_reticulum_tcp_client_kiss_framing_as_kiss_tcp_client() {
    let input = r#"
interfaces = [
  { type = "TCPClientInterface", enabled = true, name = "python-kiss-tcp", target_host = "192.0.2.10", target_port = 8001, kiss_framing = true, fixed_mtu = 512, flow_control = true }
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
    assert_eq!(settings["kiss_flow_control"], true);
}

#[test]
fn rejects_enabled_tcp_client_missing_endpoint() {
    let input = r#"
interfaces = [
  { type = "TCPClientInterface", enabled = true, name = "broken-client" }
]
"#;
    let err = DaemonConfig::from_toml(input).expect_err("tcp client endpoint should be required");
    let message = err.to_string();
    assert!(
        message.contains("host or target_host is required for tcp_client"),
        "unexpected parse error: {message}"
    );
}

#[test]
fn parses_reticulum_tcp_server_interface_aliases() {
    let input = r#"
interfaces = [
  { type = "TCPServerInterface", enabled = true, name = "python-tcp-server", listen_ip = "127.0.0.1", listen_port = 4242, i2p_tunneled = true }
]
"#;
    let cfg = DaemonConfig::from_toml(input).expect("parse Python TCPServerInterface config");
    let iface = &cfg.interfaces[0];
    assert_eq!(iface.kind, "tcp_server");
    assert_eq!(iface.host.as_deref(), Some("127.0.0.1"));
    assert_eq!(iface.port, Some(4242));
    assert_eq!(iface.i2p_tunneled, Some(true));
    assert_eq!(cfg.tcp_server_endpoints(), vec![("127.0.0.1".to_string(), 4242)]);

    let settings = iface.settings_json().expect("settings");
    assert_eq!(settings["i2p_tunneled"], true);
}

#[test]
fn parses_reticulum_tcp_server_device_listener() {
    let input = r#"
interfaces = [
  { type = "TCPServerInterface", enabled = true, name = "python-tcp-server", device = "eth0", port = 4242, prefer_ipv6 = true }
]
"#;
    let cfg = DaemonConfig::from_toml(input).expect("parse Python TCPServerInterface device config");
    let iface = &cfg.interfaces[0];

    assert_eq!(iface.kind, "tcp_server");
    assert_eq!(iface.device.as_deref(), Some("eth0"));
    assert_eq!(iface.port, Some(4242));
    assert_eq!(iface.prefer_ipv6, Some(true));

    let settings = iface.settings_json().expect("settings");
    assert_eq!(settings["device"], "eth0");
    assert_eq!(settings["prefer_ipv6"], true);
}

#[test]
fn parses_reticulum_ble_aliases_with_columba_defaults() {
    for alias in ["AndroidBLE", "AndroidBLEInterface", "BLEInterface", "reticulum_ble"] {
        let input = format!(
            r#"
interfaces = [
  {{ type = "{alias}", enabled = true, name = "columba-ble" }}
]
"#
        );
        let cfg = DaemonConfig::from_toml(&input).expect("parse reticulum BLE alias");
        let iface = &cfg.interfaces[0];
        assert_eq!(iface.kind, "reticulum_ble");
        assert_eq!(iface.service_uuid.as_deref(), Some("37145b00-442d-4a94-917f-8f42c5da28e3"));
        assert_eq!(iface.notify_char_uuid.as_deref(), Some("37145b00-442d-4a94-917f-8f42c5da28e4"));
        assert_eq!(iface.write_char_uuid.as_deref(), Some("37145b00-442d-4a94-917f-8f42c5da28e5"));
        assert_eq!(iface.identity_char_uuid.as_deref(), Some("37145b00-442d-4a94-917f-8f42c5da28e6"));
        assert_eq!(iface.mtu, Some(185));
        assert_eq!(iface.max_connections, Some(7));
        assert_eq!(iface.scan_duration_ms, Some(10_000));
        assert_eq!(iface.discovery_interval_ms, Some(5_000));
        assert_eq!(iface.discovery_interval_idle_ms, Some(30_000));
        assert_eq!(iface.min_rssi_dbm, Some(-85));
        assert_eq!(iface.enable_central, Some(true));
        assert_eq!(iface.enable_peripheral, Some(true));

        let settings = iface.settings_json().expect("settings");
        assert_eq!(settings["service_uuid"], "37145b00-442d-4a94-917f-8f42c5da28e3");
        assert_eq!(settings["tx_char_uuid"], "37145b00-442d-4a94-917f-8f42c5da28e4");
        assert_eq!(settings["rx_char_uuid"], "37145b00-442d-4a94-917f-8f42c5da28e5");
        assert_eq!(settings["identity_char_uuid"], "37145b00-442d-4a94-917f-8f42c5da28e6");
        assert_eq!(settings["mtu"], 185);
    }
}

#[test]
fn parses_reticulum_ble_custom_peer_settings() {
    let cfg = DaemonConfig::from_toml(
        r#"
interfaces = [
  { type = "reticulum_ble", enabled = true, name = "columba-ble", adapter = "hci0", tx_char_uuid = "2A37", rx_char_uuid = "2A38", identity_char_uuid = "2A39", mtu = 247, max_connections = 4, scan_duration_ms = 7000, discovery_interval_ms = 11000, discovery_interval_idle_ms = 31000, advertising_refresh_interval_ms = 17000, min_rssi_dbm = -95, enable_central = true, enable_peripheral = false }
]
"#,
    )
    .expect("parse reticulum BLE settings");
    let iface = &cfg.interfaces[0];
    assert_eq!(iface.notify_char_uuid.as_deref(), Some("2A37"));
    assert_eq!(iface.write_char_uuid.as_deref(), Some("2A38"));
    assert_eq!(iface.identity_char_uuid.as_deref(), Some("2A39"));
    assert_eq!(iface.mtu, Some(247));
    assert_eq!(iface.max_connections, Some(4));
    assert_eq!(iface.min_rssi_dbm, Some(-95));

    let settings = iface.settings_json().expect("settings");
    assert_eq!(settings["adapter"], "hci0");
    assert_eq!(settings["tx_char_uuid"], "2A37");
    assert_eq!(settings["rx_char_uuid"], "2A38");
    assert_eq!(settings["mtu"], 247);
    assert_eq!(settings["max_connections"], 4);
    assert_eq!(settings["min_rssi_dbm"], -95);
    assert_eq!(settings["enable_peripheral"], false);
}

#[test]
fn rejects_invalid_reticulum_ble_settings() {
    for (toml, expected) in [
        (
            r#"[{ type = "reticulum_ble", enabled = true, service_uuid = "bad" }]"#,
            "service_uuid must be a 16-, 32-, or 128-bit UUID",
        ),
        (
            r#"[{ type = "reticulum_ble", enabled = true, mtu = 22 }]"#,
            "mtu must be between 23 and 517",
        ),
        (
            r#"[{ type = "reticulum_ble", enabled = true, discovery_interval_ms = 0 }]"#,
            "discovery_interval_ms must be > 0",
        ),
        (
            r#"[{ type = "reticulum_ble", enabled = true, enable_central = false, enable_peripheral = false }]"#,
            "enable_central and enable_peripheral cannot both be false",
        ),
    ] {
        let input = format!("interfaces = {toml}");
        let err = DaemonConfig::from_toml(&input).expect_err("reject invalid reticulum BLE");
        assert!(err.to_string().contains(expected), "unexpected error: {err}");
    }
}

#[test]
fn rejects_enabled_tcp_server_empty_host() {
    let input = r#"
interfaces = [
  { type = "TCPServerInterface", enabled = true, name = "broken-server", listen_ip = "   ", listen_port = 0 }
]
"#;
    let err = DaemonConfig::from_toml(input).expect_err("tcp server empty host should fail");
    let message = err.to_string();
    assert!(
        message.contains("host or listen_ip cannot be empty for tcp_server"),
        "unexpected parse error: {message}"
    );
}

#[test]
fn parses_reticulum_backbone_listener_aliases() {
    let input = r#"
interfaces = [
  { type = "BackboneInterface", enabled = true, name = "python-backbone", listen_on = "127.0.0.1", port = 4242, prefer_ipv6 = true, i2p_tunneled = true }
]
"#;
    let cfg = DaemonConfig::from_toml(input).expect("parse Python BackboneInterface config");
    let iface = &cfg.interfaces[0];
    assert_eq!(iface.kind, "backbone");
    assert_eq!(iface.host.as_deref(), Some("127.0.0.1"));
    assert_eq!(iface.port, Some(4242));
    assert_eq!(iface.mtu, Some(1_048_576));
    assert_eq!(iface.prefer_ipv6, Some(true));
    assert_eq!(iface.i2p_tunneled, Some(true));

    let settings = iface.settings_json().expect("settings");
    assert_eq!(settings["prefer_ipv6"], true);
    assert_eq!(settings["i2p_tunneled"], true);
}

#[test]
fn parses_reticulum_backbone_device_listener() {
    let input = r#"
interfaces = [
  { type = "BackboneInterface", enabled = true, name = "python-backbone", device = "eth0", port = 4242, prefer_ipv6 = true }
]
"#;
    let cfg = DaemonConfig::from_toml(input).expect("parse Python BackboneInterface device config");
    let iface = &cfg.interfaces[0];

    assert_eq!(iface.kind, "backbone");
    assert_eq!(iface.device.as_deref(), Some("eth0"));
    assert_eq!(iface.port, Some(4242));
    assert_eq!(iface.mtu, Some(1_048_576));

    let settings = iface.settings_json().expect("settings");
    assert_eq!(settings["device"], "eth0");
    assert_eq!(settings["prefer_ipv6"], true);
}

#[test]
fn parses_reticulum_backbone_remote_as_client() {
    let input = r#"
interfaces = [
  { type = "BackboneInterface", enabled = true, name = "python-backbone-client", remote = "rmap.world", port = 4242 }
]
"#;
    let cfg =
        DaemonConfig::from_toml(input).expect("parse Python BackboneInterface remote config");
    let iface = &cfg.interfaces[0];
    assert_eq!(iface.kind, "backbone_client");
    assert_eq!(iface.target_host.as_deref(), Some("rmap.world"));
    assert_eq!(iface.target_port, Some(4242));
    assert_eq!(iface.host.as_deref(), Some("rmap.world"));
    assert_eq!(iface.port, Some(4242));
    assert_eq!(iface.mtu, Some(1_048_576));
}

#[test]
fn parses_reticulum_backbone_client_interface_aliases() {
    let input = r#"
interfaces = [
  { type = "BackboneClientInterface", enabled = true, name = "python-backbone-client", target_host = "rmap.world", target_port = 4242 }
]
"#;
    let cfg = DaemonConfig::from_toml(input)
        .expect("parse Python BackboneClientInterface config");
    let iface = &cfg.interfaces[0];
    assert_eq!(iface.kind, "backbone_client");
    assert_eq!(iface.host.as_deref(), Some("rmap.world"));
    assert_eq!(iface.port, Some(4242));
    assert_eq!(iface.mtu, Some(1_048_576));
}

#[test]
fn parses_reticulum_local_interface_defaults_and_aliases() {
    let input = r#"
interfaces = [
  { type = "LocalInterface", enabled = true, name = "local-main", shared_instance_type = "tcp", shared_instance_port = 37428, fixed_mtu = 4096, force_shared_instance_bitrate = 1000000 }
]
"#;
    let cfg = DaemonConfig::from_toml(input).expect("parse Python LocalInterface config");
    let iface = &cfg.interfaces[0];
    assert_eq!(iface.kind, "local");
    assert_eq!(iface.host.as_deref(), Some("127.0.0.1"));
    assert_eq!(iface.port, Some(37_428));
    assert_eq!(iface.mtu, Some(4096));
    assert_eq!(iface.bitrate, Some(1_000_000));
    assert_eq!(iface.force_shared_instance_bitrate, Some(1_000_000));

    let settings = iface.settings_json().expect("local settings");
    assert_eq!(settings["host"], "127.0.0.1");
    assert_eq!(settings["port"], 37_428);
    assert_eq!(settings["mtu"], 4096);
    assert_eq!(settings["bitrate"], 1_000_000);
}

#[test]
fn parses_reticulum_global_share_instance_as_implicit_local() {
    let input = r#"
[reticulum]
share_instance = true
shared_instance_type = "tcp"
shared_instance_port = 0
instance_name = "mesh"
force_shared_instance_bitrate = 1000000
"#;
    let cfg = DaemonConfig::from_toml(input).expect("parse global shared instance config");
    assert_eq!(cfg.interfaces.len(), 1);
    let iface = &cfg.interfaces[0];
    assert_eq!(iface.kind, "local");
    assert_eq!(iface.name.as_deref(), Some("shared-instance"));
    assert!(iface.synthetic_shared_instance);
    assert_eq!(iface.shared_instance_type.as_deref(), Some("tcp"));
    assert_eq!(iface.host.as_deref(), Some("127.0.0.1"));
    assert_eq!(iface.port, Some(0));
    assert_eq!(iface.instance_name.as_deref(), Some("mesh"));
    assert_eq!(iface.force_shared_instance_bitrate, Some(1_000_000));
    assert_eq!(iface.bitrate, Some(1_000_000));
}

#[test]
fn parses_reticulum_global_share_instance_false_without_implicit_local() {
    let input = r#"
[reticulum]
share_instance = false
shared_instance_type = "tcp"
shared_instance_port = 37428
"#;
    let cfg = DaemonConfig::from_toml(input).expect("parse disabled global shared instance config");
    assert!(cfg.interfaces.is_empty());
}

#[test]
fn explicit_local_interface_suppresses_reticulum_global_implicit_local() {
    let input = r#"
interfaces = [
  { type = "LocalInterface", enabled = true, name = "explicit-local", shared_instance_type = "tcp", shared_instance_port = 0 }
]

[reticulum]
share_instance = true
shared_instance_type = "tcp"
shared_instance_port = 37428
"#;
    let cfg = DaemonConfig::from_toml(input).expect("parse explicit local plus global config");
    assert_eq!(cfg.interfaces.len(), 1);
    assert_eq!(cfg.interfaces[0].name.as_deref(), Some("explicit-local"));
    assert!(!cfg.interfaces[0].synthetic_shared_instance);
}

#[test]
fn parses_reticulum_local_server_interface_alias() {
    let input = r#"
interfaces = [
  { type = "LocalServerInterface", enabled = true, name = "local-server", shared_instance_type = "tcp", shared_instance_port = 37428 }
]
"#;
    let cfg = DaemonConfig::from_toml(input).expect("parse Python LocalServerInterface config");
    let iface = &cfg.interfaces[0];
    assert_eq!(iface.kind, "local");
    assert_eq!(iface.host.as_deref(), Some("127.0.0.1"));
    assert_eq!(iface.port, Some(37_428));
    assert_eq!(iface.mtu, Some(262_144));
}

#[test]
fn parses_reticulum_local_client_interface_alias() {
    let input = r#"
interfaces = [
  { type = "LocalClientInterface", enabled = true, name = "local-client", shared_instance_type = "tcp", shared_instance_port = 37428, fixed_mtu = 4096 }
]
"#;
    let cfg = DaemonConfig::from_toml(input).expect("parse Python LocalClientInterface config");
    let iface = &cfg.interfaces[0];
    assert_eq!(iface.kind, "local_client");
    assert_eq!(iface.host.as_deref(), Some("127.0.0.1"));
    assert_eq!(iface.port, Some(37_428));
    assert_eq!(iface.mtu, Some(4096));

    let settings = iface.settings_json().expect("local client settings");
    assert_eq!(settings["shared_instance_type"], "tcp");
    assert_eq!(settings["host"], "127.0.0.1");
    assert_eq!(settings["port"], 37_428);
    assert_eq!(settings["mtu"], 4096);
}

#[test]
fn parses_native_local_interface_with_python_default_port_and_mtu() {
    let input = r#"
interfaces = [
  { type = "local", enabled = true, name = "local-main" }
]
"#;
    let cfg = DaemonConfig::from_toml(input).expect("parse local config");
    let iface = &cfg.interfaces[0];
    assert_eq!(iface.kind, "local");
    assert_eq!(iface.host.as_deref(), Some("127.0.0.1"));
    assert_eq!(iface.port, Some(37_428));
    assert_eq!(iface.mtu, Some(262_144));
    assert_eq!(iface.bitrate, Some(1_000_000_000));
}

#[test]
fn rejects_local_interface_non_loopback_host() {
    let input = r#"
interfaces = [
  { type = "local", enabled = true, host = "0.0.0.0", port = 37428 }
]
"#;
    let err = DaemonConfig::from_toml(input).expect_err("non-loopback local bind must fail");
    let message = err.to_string();
    assert!(message.contains("host must be loopback for local"), "unexpected parse error: {message}");
}

#[test]
fn parses_local_interface_unix_shared_instance_type() {
    let input = r#"
interfaces = [
  { type = "LocalInterface", enabled = true, name = "local-main", shared_instance_type = "unix", instance_name = "mesh" }
]
"#;
    let cfg = DaemonConfig::from_toml(input).expect("parse unix local config");
    let iface = &cfg.interfaces[0];
    assert_eq!(iface.kind, "local");
    assert_eq!(iface.shared_instance_type.as_deref(), Some("unix"));
    assert_eq!(iface.instance_name.as_deref(), Some("mesh"));
    #[cfg(any(target_os = "linux", target_os = "android"))]
    assert_eq!(iface.socket_path.as_deref(), Some("@rns/mesh"));
    #[cfg(not(any(target_os = "linux", target_os = "android")))]
    assert!(
        iface.socket_path.as_deref().unwrap_or_default().ends_with("rns-mesh.sock"),
        "unexpected socket path: {:?}",
        iface.socket_path
    );
    assert_eq!(iface.host, None);
    assert_eq!(iface.port, None);
    assert_eq!(iface.mtu, Some(262_144));

    let settings = iface.settings_json().expect("local settings");
    assert_eq!(settings["shared_instance_type"], "unix");
    assert_eq!(settings["instance_name"], "mesh");
    #[cfg(any(target_os = "linux", target_os = "android"))]
    assert_eq!(settings["socket_path"].as_str(), Some("@rns/mesh"));
    #[cfg(not(any(target_os = "linux", target_os = "android")))]
    assert!(
        settings["socket_path"].as_str().unwrap_or_default().ends_with("rns-mesh.sock"),
        "unexpected socket path setting: {:?}",
        settings["socket_path"]
    );
}

#[test]
fn parses_reticulum_pipe_interface() {
    let input = r#"
interfaces = [
  { type = "PipeInterface", enabled = true, name = "pipe-main", command = "cat", respawn_delay = 0.25 }
]
"#;
    let cfg = DaemonConfig::from_toml(input).expect("parse Python PipeInterface config");
    let iface = &cfg.interfaces[0];
    assert_eq!(iface.kind, "pipe");
    assert_eq!(iface.command.as_deref(), Some("cat"));
    assert_eq!(iface.respawn_delay, Some(0.25));
    assert_eq!(iface.mtu, Some(1_064));

    let settings = iface.settings_json().expect("pipe settings");
    assert_eq!(settings["command"], "cat");
    assert_eq!(settings["respawn_delay"], 0.25);
    assert_eq!(settings["mtu"], 1_064);
}

#[test]
fn parses_reticulum_ax25_kiss_interface_aliases() {
    let input = r#"
interfaces = [
  { type = "AX25KISSInterface", enabled = true, name = "ax25-main", port = "/dev/ttyUSB0", speed = 1200, callsign = "n0call", ssid = 1, flow_control = true, preamble = 300, txtail = 40, slottime = 30, id_callsign = "MYCALL-0", id_interval = 600 }
]
"#;
    let cfg = DaemonConfig::from_toml(input).expect("parse Python AX25KISSInterface config");
    let iface = &cfg.interfaces[0];
    assert_eq!(iface.kind, "ax25_kiss");
    assert_eq!(iface.device.as_deref(), Some("/dev/ttyUSB0"));
    assert_eq!(iface.baud_rate, Some(1200));
    assert_eq!(iface.callsign.as_deref(), Some("n0call"));
    assert_eq!(iface.ssid, Some(1));
    assert_eq!(iface.kiss_flow_control, Some(true));
    assert_eq!(iface.preamble_ms, Some(300));
    assert_eq!(iface.tx_tail_ms, Some(40));
    assert_eq!(iface.slot_time_ms, Some(30));
    assert_eq!(iface.id_callsign.as_deref(), Some("MYCALL-0"));
    assert_eq!(iface.id_interval, Some(600));
    assert_eq!(iface.mtu, Some(564));

    let settings = iface.settings_json().expect("ax25 settings");
    assert_eq!(settings["device"], "/dev/ttyUSB0");
    assert_eq!(settings["baud_rate"], 1200);
    assert_eq!(settings["callsign"], "n0call");
    assert_eq!(settings["ssid"], 1);
    assert_eq!(settings["kiss_flow_control"], true);
    assert_eq!(settings["id_callsign"], "MYCALL-0");
    assert_eq!(settings["id_interval"], 600);
    assert_eq!(settings["mtu"], 564);
}

#[test]
fn rejects_invalid_reticulum_ax25_kiss_callsign_and_ssid() {
    let input = r#"
interfaces = [
  { type = "AX25KISSInterface", enabled = true, name = "ax25-main", port = "/dev/ttyUSB0", speed = 1200, callsign = "NO-CALL", ssid = 16 }
]
"#;
    let err = DaemonConfig::from_toml(input).expect_err("invalid AX.25 config must fail");
    let message = err.to_string();
    assert!(
        message.contains("callsign") || message.contains("ssid"),
        "unexpected parse error: {message}"
    );
}

#[test]
fn rejects_invalid_reticulum_ax25_kiss_id_beacon() {
    let input = r#"
interfaces = [
  { type = "AX25KISSInterface", enabled = true, name = "ax25-main", port = "/dev/ttyUSB0", speed = 1200, callsign = "N0CALL", ssid = 1, id_interval = 0 }
]
"#;
    let err = DaemonConfig::from_toml(input).expect_err("invalid AX.25 ID beacon must fail");
    let message = err.to_string();
    assert!(
        message.contains("id_interval must be > 0"),
        "unexpected parse error: {message}"
    );
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
fn parses_python_interface_gravity_and_exposes_runtime_setting() {
    let input = r#"
interfaces = [
  { type = "tcp_client", enabled = true, host = "rmap.world", port = 4242, gravity = -7 }
]
"#;
    let cfg = DaemonConfig::from_toml(input).expect("parse interface gravity");
    let iface = &cfg.interfaces[0];

    assert_eq!(iface.gravity, Some(-7));
    assert_eq!(iface.interface_gravity(), -7);
    assert_eq!(iface.settings_json().expect("settings")["gravity"], -7);
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
fn parses_common_reticulum_ifac_and_rate_control_fields() {
    let input = r#"
interfaces = [
  { type = "KISSInterface", enabled = true, name = "kiss-main", port = "/dev/ttyACM0", speed = 19200, ifac_size = 16, networkname = "field-net", pass_phrase = "shared-secret", announce_rate_target = 12, ingress_control = false, egress_control = true, ic_burst_hold = 1.5, ic_pr_burst_freq = 0.25, ec_pr_freq = 0.5, bootstrap_only = true }
]
"#;
    let cfg = DaemonConfig::from_toml(input).expect("parse common IFAC and rate control fields");
    let iface = &cfg.interfaces[0];
    assert_eq!(iface.ifac_size, Some(16));
    assert_eq!(iface.networkname.as_deref(), Some("field-net"));
    assert_eq!(iface.pass_phrase.as_deref(), Some("shared-secret"));
    assert_eq!(iface.announce_rate_target, Some(12));
    assert_eq!(iface.announce_rate_grace, None);
    assert_eq!(iface.announce_rate_penalty, None);
    assert_eq!(iface.ingress_control, Some(false));
    assert_eq!(iface.egress_control, Some(true));
    assert_eq!(iface.bootstrap_only, Some(true));

    let settings = iface.settings_json().expect("kiss settings");
    assert_eq!(settings["ifac_size"], 16);
    assert_eq!(settings["network_name"], "field-net");
    assert_eq!(settings["passphrase"], "shared-secret");
    assert_eq!(settings["announce_rate_target"], 12);
    assert_eq!(settings["announce_rate_grace"], 0);
    assert_eq!(settings["announce_rate_penalty"], 0);
    assert_eq!(settings["ingress_control"], false);
    assert_eq!(settings["egress_control"], true);
    assert_eq!(settings["ic_burst_hold"], 1.5);
    assert_eq!(settings["ic_pr_burst_freq"], 0.25);
    assert_eq!(settings["ec_pr_freq"], 0.5);
    assert_eq!(settings["bootstrap_only"], true);
}

#[test]
fn parses_common_reticulum_discovery_metadata_fields() {
    let input = r#"
interfaces = [
  { type = "RNodeInterface", enabled = true, name = "rnode-main", region = "US915", state_path = "/tmp/lora-state.json", port = "tcp://192.0.2.10:8001", frequency = 915000000, bandwidth = 125000, spreadingfactor = 9, codingrate = 5, txpower = 17, discoverable = true, announce_interval = 360, discovery_stamp_value = 8, discovery_name = "field node", discovery_encrypt = true, reachable_on = "lxmf://field", publish_ifac = true, latitude = 45.5, longitude = -63.5, height = 42.0, discovery_frequency = 915000000, discovery_bandwidth = 125000, discovery_modulation = 1, ignore_config_warnings = true }
]
"#;
    let cfg = DaemonConfig::from_toml(input).expect("parse common discovery metadata fields");
    let iface = &cfg.interfaces[0];
    assert_eq!(iface.discoverable, Some(true));
    assert_eq!(iface.announce_interval, Some(360));
    assert_eq!(iface.discovery_stamp_value, Some(8));
    assert_eq!(iface.discovery_name.as_deref(), Some("field node"));
    assert_eq!(iface.discovery_encrypt, Some(true));
    assert_eq!(iface.reachable_on.as_deref(), Some("lxmf://field"));
    assert_eq!(iface.publish_ifac, Some(true));
    assert_eq!(iface.latitude, Some(45.5));
    assert_eq!(iface.longitude, Some(-63.5));
    assert_eq!(iface.height, Some(42.0));
    assert_eq!(iface.discovery_frequency, Some(915_000_000));
    assert_eq!(iface.discovery_bandwidth, Some(125_000));
    assert_eq!(iface.discovery_modulation, Some(1));
    assert_eq!(iface.ignore_config_warnings, Some(true));

    let settings = iface.settings_json().expect("rnode settings");
    assert_eq!(settings["discoverable"], true);
    assert_eq!(settings["announce_interval"], 21_600);
    assert_eq!(settings["discovery_stamp_value"], 8);
    assert_eq!(settings["discovery_name"], "field node");
    assert_eq!(settings["discovery_encrypt"], true);
    assert_eq!(settings["reachable_on"], "lxmf://field");
    assert_eq!(settings["publish_ifac"], true);
    assert_eq!(settings["latitude"], 45.5);
    assert_eq!(settings["longitude"], -63.5);
    assert_eq!(settings["height"], 42.0);
    assert_eq!(settings["discovery_frequency"], 915_000_000);
    assert_eq!(settings["discovery_bandwidth"], 125_000);
    assert_eq!(settings["discovery_modulation"], 1);
    assert_eq!(settings["ignore_config_warnings"], true);
}

#[test]
fn discoverable_interfaces_select_reticulum_gateway_or_ap_modes() {
    let input = r#"
interfaces = [
  { type = "TCPClientInterface", enabled = true, name = "tcp-discovery", target_host = "rmap.world", target_port = 4242, discoverable = true },
  { type = "RNodeInterface", enabled = true, name = "rnode-discovery", region = "US915", state_path = "/tmp/lora-state.json", port = "tcp://192.0.2.10:8001", frequency = 915000000, bandwidth = 125000, spreadingfactor = 9, codingrate = 5, txpower = 17, discoverable = true },
  { type = "UDPInterface", enabled = true, name = "udp-ignored", listen_ip = "127.0.0.1", listen_port = 4242, mode = "boundary", discoverable = true, ignore_config_warnings = true }
]
"#;
    let cfg = DaemonConfig::from_toml(input).expect("parse discoverable mode config");

    assert_eq!(cfg.interfaces[0].interface_mode().unwrap(), InterfaceMode::Gateway);
    assert_eq!(cfg.interfaces[1].interface_mode().unwrap(), InterfaceMode::AccessPoint);
    assert_eq!(cfg.interfaces[2].interface_mode().unwrap(), InterfaceMode::Boundary);

    assert_eq!(
        cfg.interfaces[0].settings_json().expect("tcp settings")["interface_mode"],
        "gateway"
    );
    assert_eq!(
        cfg.interfaces[1].settings_json().expect("rnode settings")["interface_mode"],
        "access_point"
    );
    assert_eq!(
        cfg.interfaces[2].settings_json().expect("udp settings")["interface_mode"],
        "boundary"
    );
}

#[test]
fn discoverable_announce_interval_matches_reticulum_seconds() {
    let input = r#"
interfaces = [
  { type = "TCPClientInterface", enabled = true, name = "default-interval", target_host = "rmap.world", target_port = 4242, discoverable = true },
  { type = "TCPClientInterface", enabled = true, name = "minimum-interval", target_host = "rmap.world", target_port = 4243, discoverable = true, announce_interval = 1 }
]
"#;
    let cfg = DaemonConfig::from_toml(input).expect("parse discoverable intervals");

    assert_eq!(cfg.interfaces[0].discovery_announce_interval_secs(), Some(21_600));
    assert_eq!(cfg.interfaces[1].discovery_announce_interval_secs(), Some(300));
    assert_eq!(
        cfg.interfaces[0].settings_json().expect("default settings")["announce_interval"],
        21_600
    );
    assert_eq!(
        cfg.interfaces[1].settings_json().expect("minimum settings")["announce_interval"],
        300
    );
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
        panic_on_interface_error: false,
        reticulum_enable_transport: false,
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
        panic_on_interface_error: false,
        reticulum_enable_transport: false,
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
fn reticulum_udp_listen_port_does_not_default_forward_port() {
    let input = r#"
interfaces = [
  { type = "UDPInterface", enabled = true, name = "python-udp", listen_ip = "127.0.0.1", listen_port = 4242, forward_ip = "127.0.0.1" }
]
"#;
    let cfg = DaemonConfig::from_toml(input).expect("parse partial Python UDPInterface config");
    let iface = &cfg.interfaces[0];

    assert_eq!(iface.host.as_deref(), Some("127.0.0.1"));
    assert_eq!(iface.port, Some(4242));
    assert!(iface.target_host.is_none());
    assert!(iface.target_port.is_none());
}

#[test]
fn reticulum_udp_forward_port_without_forward_ip_is_receive_only() {
    let input = r#"
interfaces = [
  { type = "UDPInterface", enabled = true, name = "python-udp", listen_ip = "127.0.0.1", listen_port = 4242, forward_port = 4243 }
]
"#;
    let cfg = DaemonConfig::from_toml(input).expect("parse partial Python UDPInterface config");
    let iface = &cfg.interfaces[0];

    assert_eq!(iface.host.as_deref(), Some("127.0.0.1"));
    assert_eq!(iface.port, Some(4242));
    assert!(iface.target_host.is_none());
    assert!(iface.target_port.is_none());
}

#[test]
fn reticulum_udp_shared_port_defaults_forward_port_when_forward_ip_is_present() {
    let input = r#"
interfaces = [
  { type = "UDPInterface", enabled = true, name = "python-udp", listen_ip = "127.0.0.1", port = 4242, forward_ip = "127.0.0.2" }
]
"#;
    let cfg = DaemonConfig::from_toml(input).expect("parse Python UDPInterface shared port config");
    let iface = &cfg.interfaces[0];

    assert_eq!(iface.host.as_deref(), Some("127.0.0.1"));
    assert_eq!(iface.port, Some(4242));
    assert_eq!(iface.target_host.as_deref(), Some("127.0.0.2"));
    assert_eq!(iface.target_port, Some(4242));
}

#[test]
fn parses_reticulum_udp_interface_device_defaults() {
    let input = r#"
interfaces = [
  { type = "UDPInterface", enabled = true, name = "python-udp-device", device = "eth0", port = 4242 }
]
"#;
    let cfg = DaemonConfig::from_toml(input).expect("parse Python UDPInterface device config");
    let iface = &cfg.interfaces[0];
    assert_eq!(iface.kind, "udp");
    assert_eq!(iface.device.as_deref(), Some("eth0"));
    assert_eq!(iface.port, Some(4242));
    assert!(iface.host.is_none());
    assert!(iface.target_host.is_none());
    assert!(iface.target_port.is_none());

    let settings = iface.settings_json().expect("settings");
    assert_eq!(settings["device"], "eth0");
    assert_eq!(settings["port"], 4242);
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
fn reticulum_auto_invalid_multicast_address_type_falls_back_to_temporary() {
    let input = r#"
interfaces = [
  { type = "AutoInterface", enabled = true, name = "python-auto", multicast_address_type = "nonsense" }
]
"#;
    let cfg = DaemonConfig::from_toml(input)
        .expect("parse AutoInterface unknown multicast_address_type fallback");
    let iface = &cfg.interfaces[0];

    assert_eq!(iface.kind, "auto");
    assert_eq!(iface.multicast_address_type.as_deref(), Some("temporary"));

    let settings = iface.settings_json().expect("settings");
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
fn udp_target_host_uses_shared_port_when_target_port_is_absent() {
    let input = r#"
interfaces = [
  { type = "udp", enabled = true, host = "127.0.0.1", port = 4242, target_host = "127.0.0.1" }
]
"#;
    let cfg = DaemonConfig::from_toml(input).expect("parse udp shared port fallback config");
    let iface = &cfg.interfaces[0];

    assert_eq!(iface.host.as_deref(), Some("127.0.0.1"));
    assert_eq!(iface.port, Some(4242));
    assert_eq!(iface.target_host.as_deref(), Some("127.0.0.1"));
    assert_eq!(iface.target_port, Some(4242));
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
fn parses_reticulum_weave_interface_defaults() {
    let input = r#"
interfaces = [
  { type = "WeaveInterface", enabled = true, name = "weave-main", port = "/dev/ttyACM0", configured_bitrate = 250000 }
]
"#;
    let cfg = DaemonConfig::from_toml(input).expect("parse Python WeaveInterface config");
    let iface = &cfg.interfaces[0];
    assert_eq!(iface.kind, "weave");
    assert_eq!(iface.device.as_deref(), Some("/dev/ttyACM0"));
    assert_eq!(iface.baud_rate, Some(3_000_000));
    assert_eq!(iface.mtu, Some(1024));
    assert_eq!(iface.bitrate, Some(250_000));

    let settings = iface.settings_json().expect("settings");
    assert_eq!(settings["device"], "/dev/ttyACM0");
    assert_eq!(settings["baud_rate"], 3_000_000);
    assert_eq!(settings["mtu"], 1024);
}

#[test]
fn parses_reticulum_i2p_interface_defaults() {
    let input = r#"
interfaces = [
  { type = "I2PInterface", enabled = true, name = "i2p-main", peers = "peer-one.b32.i2p, peer-two.b32.i2p", storagepath = "/tmp/rns", configured_bitrate = 128000, reconnect_backoff_ms = 100, ifac_netname = "i2p-field", ifac_netkey = "i2p-secret" }
]
"#;
    let cfg = DaemonConfig::from_toml(input).expect("parse Python I2PInterface config");
    let iface = &cfg.interfaces[0];
    let (expected_sam_host, expected_sam_port) = expected_i2p_sam_default();
    assert_eq!(iface.kind, "i2p");
    assert_eq!(
        iface.peers.as_deref(),
        Some(&["peer-one.b32.i2p".to_string(), "peer-two.b32.i2p".to_string()][..])
    );
    assert_eq!(iface.sam_host.as_deref(), Some(expected_sam_host.as_str()));
    assert_eq!(iface.sam_port, Some(expected_sam_port));
    assert_eq!(iface.mtu, Some(1064));
    assert_eq!(iface.bitrate, Some(128_000));
    assert_eq!(iface.reconnect_backoff_ms, Some(100));
    assert_eq!(iface.state_path.as_deref(), Some("/tmp/rns"));
    assert_eq!(iface.network_name.as_deref(), Some("i2p-field"));
    assert_eq!(iface.passphrase.as_deref(), Some("i2p-secret"));

    let settings = iface.settings_json().expect("settings");
    assert_eq!(settings["peers"], serde_json::json!(["peer-one.b32.i2p", "peer-two.b32.i2p"]));
    assert_eq!(settings["sam_host"], expected_sam_host);
    assert_eq!(settings["sam_port"], expected_sam_port);
    assert_eq!(settings["mtu"], 1064);
    assert_eq!(settings["reconnect_backoff_ms"], 100);
    assert_eq!(settings["state_path"], "/tmp/rns");
    assert_eq!(settings["network_name"], "i2p-field");
    assert_eq!(settings["passphrase"], "i2p-secret");
}

#[test]
fn i2p_ifac_net_aliases_do_not_override_canonical_fields() {
    let input = r#"
interfaces = [
  { type = "I2PInterface", enabled = true, name = "i2p-main", connectable = true, network_name = "canonical-net", passphrase = "canonical-secret", ifac_netname = "i2p-field", ifac_netkey = "i2p-secret" }
]
"#;
    let cfg = DaemonConfig::from_toml(input).expect("parse Python I2PInterface IFAC aliases");
    let iface = &cfg.interfaces[0];

    assert_eq!(iface.network_name.as_deref(), Some("canonical-net"));
    assert_eq!(iface.passphrase.as_deref(), Some("canonical-secret"));

    let settings = iface.settings_json().expect("settings");
    assert_eq!(settings["network_name"], "canonical-net");
    assert_eq!(settings["passphrase"], "canonical-secret");
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
fn parses_reticulum_serial_interface_default_speed() {
    let input = r#"
interfaces = [
  { type = "SerialInterface", interface_enabled = true, name = "python-serial", port = "/dev/ttyUSB0" }
]
"#;
    let cfg = DaemonConfig::from_toml(input).expect("parse Python SerialInterface default speed");
    let iface = &cfg.interfaces[0];

    assert!(iface.enabled());
    assert_eq!(iface.kind, "serial");
    assert_eq!(iface.device.as_deref(), Some("/dev/ttyUSB0"));
    assert_eq!(iface.baud_rate, Some(9600));
    assert_eq!(iface.settings_json().expect("settings")["baud_rate"], 9600);
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
fn rejects_reticulum_kiss_flow_control_non_boolean() {
    let input = r#"
interfaces = [
  { type = "KISSInterface", enabled = true, name = "kiss-main", port = "/dev/ttyACM0", flow_control = "yes" }
]
"#;
    let err = DaemonConfig::from_toml(input).expect_err("kiss flow_control must be boolean");
    let message = err.to_string();
    assert!(
        message.contains("flow_control must be a boolean for kiss"),
        "unexpected parse error: {message}"
    );

    let input = r#"
interfaces = [
  { type = "kiss_tcp_client", enabled = true, name = "kiss-wifi", host = "192.0.2.10", port = 8001, flow_control = "yes" }
]
"#;
    let err =
        DaemonConfig::from_toml(input).expect_err("kiss_tcp_client flow_control must be boolean");
    let message = err.to_string();
    assert!(
        message.contains("flow_control must be a boolean for kiss_tcp_client"),
        "unexpected parse error: {message}"
    );
}
