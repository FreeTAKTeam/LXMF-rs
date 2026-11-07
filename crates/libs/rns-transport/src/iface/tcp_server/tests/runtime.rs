use std::sync::Arc;
use std::time::Duration;

use super::{
    backbone_hdlc_watchdog, bind_tcp_listener, AcceptedClientOptions, FastFlapPolicy,
    FastFlapTracker, TcpClient, TcpServer, TcpSocketTuning,
};
use crate::iface::InterfaceManager;
use tokio::net::{TcpListener, TcpStream};

#[test]
fn tcp_server_exposes_client_socket_tuning() {
    let manager = Arc::new(tokio::sync::Mutex::new(InterfaceManager::new(8)));
    let server = TcpServer::new("127.0.0.1:0", manager.clone());
    assert!(server.client_socket_tuning().is_empty());
    assert!(!server.client_hdlc_liveness_enabled());
    assert!(!server.prefer_ipv6());

    let tuned = TcpServer::new("127.0.0.1:0", manager)
        .with_client_socket_tuning(TcpSocketTuning::backbone())
        .with_prefer_ipv6(true);
    assert_eq!(tuned.client_socket_tuning().nodelay, Some(true));
    assert_eq!(tuned.client_socket_tuning().keepalive, Some(true));
    assert_eq!(tuned.client_socket_tuning().tcp_keepalive_idle, Some(Duration::from_secs(5)));
    assert_eq!(tuned.client_socket_tuning().tcp_keepalive_interval, Some(Duration::from_secs(2)));
    assert_eq!(tuned.client_socket_tuning().tcp_keepalive_retries, Some(12));
    assert_eq!(tuned.client_socket_tuning().tcp_user_timeout, Some(Duration::from_secs(24)));
    assert!(!tuned.client_hdlc_liveness_enabled());
    assert!(tuned.prefer_ipv6());
}

#[test]
fn tcp_server_backbone_client_liveness_is_exposed() {
    let manager = Arc::new(tokio::sync::Mutex::new(InterfaceManager::new(8)));
    let server = TcpServer::new("127.0.0.1:0", manager).with_backbone_client_liveness();

    assert!(server.client_hdlc_liveness_enabled());
}

#[test]
fn fast_flapping_blocks_after_grace_and_reports_live_ip_state() {
    let tracker = FastFlapTracker::default();
    let policy = FastFlapPolicy {
        enabled: true,
        threshold: Duration::from_secs(2),
        grace: 2,
        expiry: Duration::from_secs(60),
    };

    tracker.record_short_connection("192.0.2.7", Duration::from_millis(1), policy);
    tracker.record_short_connection("192.0.2.7", Duration::from_millis(1), policy);
    assert_eq!(tracker.blocked_ip_count(policy), 0);
    assert!(tracker.blocked_ip_list(policy).is_empty());

    tracker.record_short_connection("192.0.2.7", Duration::from_millis(1), policy);
    assert_eq!(tracker.blocked_ip_count(policy), 1);
    assert_eq!(tracker.blocked_ip_list(policy), vec!["192.0.2.7"]);
    assert!(tracker.is_blocked("192.0.2.7", policy));

    let disabled = FastFlapPolicy { enabled: false, ..policy };
    assert_eq!(tracker.blocked_ip_count(disabled), 0);
    assert!(tracker.blocked_ip_list(disabled).is_empty());
    assert!(!tracker.is_blocked("192.0.2.7", disabled));
}

#[tokio::test]
async fn tcp_server_forwards_configured_liveness_to_accepted_clients() {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind listener");
    let addr = listener.local_addr().expect("listener addr");
    let _peer = TcpStream::connect(addr).await.expect("connect peer");
    let (stream, peer_addr) = listener.accept().await.expect("accept stream");

    let ordinary = TcpServer::accepted_client(
        peer_addr.to_string(),
        stream,
        AcceptedClientOptions {
            client_mtu: TcpClient::DEFAULT_MTU,
            client_socket_tuning: TcpSocketTuning::default(),
            client_hdlc_watchdog: None,
            client_forced_bitrate_bps: None,
            fast_flap_tracker: None,
            fast_flap_policy: FastFlapPolicy::default(),
            peer_ip: None,
        },
    );
    assert!(!ordinary.hdlc_liveness_enabled());
    assert_eq!(ordinary.forced_bitrate_bps(), None);

    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind listener");
    let addr = listener.local_addr().expect("listener addr");
    let _peer = TcpStream::connect(addr).await.expect("connect peer");
    let (stream, peer_addr) = listener.accept().await.expect("accept stream");

    let backbone = TcpServer::accepted_client(
        peer_addr.to_string(),
        stream,
        AcceptedClientOptions {
            client_mtu: 1_048_576,
            client_socket_tuning: TcpSocketTuning::backbone(),
            client_hdlc_watchdog: Some(backbone_hdlc_watchdog()),
            client_forced_bitrate_bps: Some(9_600),
            fast_flap_tracker: None,
            fast_flap_policy: FastFlapPolicy::default(),
            peer_ip: None,
        },
    );

    assert_eq!(backbone.mtu_value(), 1_048_576);
    assert_eq!(backbone.socket_tuning().nodelay, Some(true));
    assert!(backbone.hdlc_liveness_enabled());
    assert_eq!(backbone.forced_bitrate_bps(), Some(9_600));
}

#[tokio::test]
async fn tcp_server_runtime_status_tracks_accepted_client() {
    let probe = TcpListener::bind("127.0.0.1:0").await.expect("bind probe listener");
    let addr = probe.local_addr().expect("probe listener addr");
    drop(probe);

    let manager = Arc::new(tokio::sync::Mutex::new(InterfaceManager::new(8)));
    let server = TcpServer::new(addr.to_string(), manager.clone())
        .with_backbone_client_liveness()
        .with_client_forced_bitrate(9_600);
    let status = server.runtime_status_handle();
    let parent_iface = manager.lock().await.spawn(server, TcpServer::spawn);

    wait_for_status(&status, |status| {
        status.get("listener_state").and_then(serde_json::Value::as_str) == Some("listening")
    })
    .await;
    let _peer = TcpStream::connect(addr).await.expect("connect peer");
    wait_for_status(&status, |status| {
        status.get("accepted_connections").and_then(serde_json::Value::as_u64) == Some(1)
    })
    .await;

    let snapshot = status.to_json();
    assert_eq!(snapshot["bind_addr"].as_str(), Some(addr.to_string().as_str()));
    assert_eq!(snapshot["client_liveness_enabled"].as_bool(), Some(true));
    assert_eq!(snapshot["client_forced_bitrate_bps"].as_u64(), Some(9_600));
    assert!(snapshot["latest_client_endpoint"].as_str().is_some());
    assert!(snapshot["latest_client_iface"].as_str().is_some());
    assert_eq!(snapshot["latest_stream_status"]["liveness_enabled"].as_bool(), Some(true));
    assert_eq!(snapshot["latest_stream_status"]["forced_bitrate_bps"].as_u64(), Some(9_600));

    manager.lock().await.stop_interface(parent_iface);
}

#[tokio::test]
async fn tcp_listener_sets_reuse_address_for_ipv4() {
    let listener =
        bind_tcp_listener("127.0.0.1:0".to_string(), false).await.expect("bind listener");
    let std_listener = listener.into_std().expect("std listener");
    let socket: socket2::Socket = std_listener.into();

    assert!(socket.reuse_address().expect("reuse_address"));
}

#[tokio::test]
async fn tcp_listener_sets_reuse_address_for_ipv6() {
    let listener = match bind_tcp_listener("[::1]:0".to_string(), true).await {
        Ok(listener) => listener,
        Err(err)
            if matches!(
                err.kind(),
                std::io::ErrorKind::AddrNotAvailable | std::io::ErrorKind::Unsupported
            ) =>
        {
            return;
        }
        Err(err) => panic!("bind IPv6 listener: {err}"),
    };
    let std_listener = listener.into_std().expect("std listener");
    let socket: socket2::Socket = std_listener.into();

    assert!(socket.reuse_address().expect("reuse_address"));
}

async fn wait_for_status(
    status: &super::TcpListenerRuntimeStatusHandle,
    predicate: impl Fn(&serde_json::Value) -> bool,
) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
    loop {
        let snapshot = status.to_json();
        if predicate(&snapshot) {
            return;
        }
        assert!(tokio::time::Instant::now() < deadline, "timed out waiting for {snapshot:?}");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

#[path = "issue_614_accepted_client_lifecycle.rs"]
mod issue_614_accepted_client_lifecycle;
