use super::{build_selected_tcp_server_adapter, reticulum_transport_enabled, TcpServerSelection};
use rns_transport::iface::InterfaceManager;
use std::sync::Arc;
use std::time::Duration;

#[test]
fn reticulum_enable_transport_controls_daemon_forwarding() {
    assert!(!reticulum_transport_enabled(None));

    for (config, expected) in [
        ("", false),
        ("[reticulum]\nenable_transport = false\n", false),
        ("[reticulum]\nenable_transport = true\n", true),
    ] {
        let daemon_config =
            reticulum_daemon::config::DaemonConfig::from_toml(config).expect("parse daemon config");

        assert_eq!(reticulum_transport_enabled(Some(&daemon_config)), expected);
    }
}

#[test]
fn selected_backbone_server_adapter_enables_socket_tuning_and_liveness() {
    let manager = Arc::new(tokio::sync::Mutex::new(InterfaceManager::new(8)));
    let tcp = TcpServerSelection {
        bind_addr: Some("127.0.0.1:0".to_string()),
        kind: "tcp_server".to_string(),
        ..TcpServerSelection::default()
    };
    let tcp_server =
        build_selected_tcp_server_adapter("127.0.0.1:0".to_string(), manager.clone(), &tcp);

    assert!(tcp_server.client_socket_tuning().is_empty());
    assert!(!tcp_server.client_hdlc_liveness_enabled());
    assert_eq!(tcp_server.client_forced_bitrate_bps(), None);
    assert!(!tcp_server.prefer_ipv6());

    let backbone = TcpServerSelection {
        bind_addr: Some("127.0.0.1:0".to_string()),
        kind: "backbone".to_string(),
        client_mtu: Some(1_048_576),
        prefer_ipv6: true,
        ..TcpServerSelection::default()
    };
    let backbone_server =
        build_selected_tcp_server_adapter("127.0.0.1:0".to_string(), manager, &backbone);

    assert_eq!(backbone_server.client_socket_tuning().nodelay, Some(true));
    assert_eq!(backbone_server.client_socket_tuning().keepalive, Some(true));
    assert!(backbone_server.client_hdlc_liveness_enabled());
    assert!(backbone_server.prefer_ipv6());
}

#[test]
fn selected_local_server_adapter_forces_shared_instance_bitrate() {
    let manager = Arc::new(tokio::sync::Mutex::new(InterfaceManager::new(8)));
    let local = TcpServerSelection {
        bind_addr: Some("127.0.0.1:37428".to_string()),
        kind: "local".to_string(),
        client_forced_bitrate_bps: Some(1_000_000),
        ..TcpServerSelection::default()
    };

    let server = build_selected_tcp_server_adapter("127.0.0.1:37428".to_string(), manager, &local);

    assert_eq!(server.client_forced_bitrate_bps(), Some(1_000_000));
    assert!(server.client_socket_tuning().is_empty());
    assert!(!server.client_hdlc_liveness_enabled());
}

#[test]
fn selected_i2p_tunneled_tcp_server_adapter_applies_client_socket_profile() {
    let manager = Arc::new(tokio::sync::Mutex::new(InterfaceManager::new(8)));
    let tcp = TcpServerSelection {
        bind_addr: Some("127.0.0.1:0".to_string()),
        kind: "tcp_server".to_string(),
        i2p_tunneled: true,
        ..TcpServerSelection::default()
    };

    let tcp_server = build_selected_tcp_server_adapter("127.0.0.1:0".to_string(), manager, &tcp);

    assert_eq!(tcp_server.client_socket_tuning().nodelay, Some(true));
    assert_eq!(tcp_server.client_socket_tuning().keepalive, Some(true));
    assert_eq!(tcp_server.client_socket_tuning().tcp_keepalive_idle, Some(Duration::from_secs(10)));
    assert_eq!(
        tcp_server.client_socket_tuning().tcp_keepalive_interval,
        Some(Duration::from_secs(9))
    );
    assert_eq!(tcp_server.client_socket_tuning().tcp_keepalive_retries, Some(5));
    assert_eq!(tcp_server.client_socket_tuning().tcp_user_timeout, Some(Duration::from_secs(45)));
    assert!(!tcp_server.client_hdlc_liveness_enabled());
}
