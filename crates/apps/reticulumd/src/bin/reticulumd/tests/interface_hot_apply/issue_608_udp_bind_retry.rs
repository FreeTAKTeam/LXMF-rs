use super::*;
use tokio::net::UdpSocket;

#[tokio::test(flavor = "current_thread")]
async fn hot_apply_ifac_udp_bind_retry_keeps_reconfigured_policy_fail_closed() {
    let daemon = Arc::new(RpcDaemon::test_instance());
    let iface_manager = Arc::new(tokio::sync::Mutex::new(InterfaceManager::new(8)));
    let bridge = InterfaceHotApplyBridge::spawn_with_daemon(
        iface_manager.clone(),
        Vec::new(),
        Arc::downgrade(&daemon),
    );

    let mut initial = udp_record("ifac-retry", "127.0.0.1", 0);
    initial.settings = Some(json!({
        "ifac_size": 128,
        "network_name": "ifac-retry-network",
        "passphrase": "ifac-retry-initial-secret"
    }));
    let applied = bridge.apply_interfaces(vec![initial]).expect("apply initial IFAC UDP");
    daemon.replace_interfaces(applied);
    let (initial_iface, initial_refresh) = wait_for_hot_apply_udp_refresh(&bridge).await;
    timeout(Duration::from_secs(2), async {
        while initial_refresh.status.snapshot().link_state != "bound" {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("initial IFAC UDP listener should bind");

    let reservation = UdpSocket::bind("127.0.0.1:0").await.expect("reserve replacement port");
    let replacement_port = reservation.local_addr().expect("reserved address").port();
    let mut replacement = udp_record("ifac-retry", "127.0.0.1", replacement_port);
    replacement.settings = Some(json!({
        "ifac_size": 128,
        "network_name": "ifac-retry-network",
        "passphrase": "ifac-retry-replacement-secret"
    }));
    let applied = bridge.apply_interfaces(vec![replacement]).expect("queue IFAC reconfiguration");
    daemon.replace_interfaces(applied);

    let (replacement_iface, replacement_refresh) = timeout(Duration::from_secs(2), async {
        loop {
            let refresh = bridge
                .udp_refreshes
                .lock()
                .expect("udp refresh mutex poisoned")
                .get("ifac-retry")
                .cloned();
            if let Some(refresh) = refresh.filter(|entry| {
                entry.record.port == Some(replacement_port) && entry.runtime_iface != initial_iface
            }) {
                break (refresh.runtime_iface, refresh);
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("replacement UDP interface should be installed");

    timeout(Duration::from_secs(2), async {
        while replacement_refresh.status.snapshot().link_state != "bind_failed" {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("occupied replacement port should report bind failure");
    let failed = replacement_refresh.status.snapshot();
    assert!(failed.socket_errors >= 1);
    assert_eq!(failed.packets_rx, 0);
    assert_eq!(failed.packets_tx, 0);
    let bind_error = failed.last_error.as_deref().expect("bind failure diagnostic");
    assert!(!bind_error.contains("ifac-retry-initial-secret"));
    assert!(!bind_error.contains("ifac-retry-replacement-secret"));
    refresh_hot_apply_udp_runtime_status_once(&daemon, &bridge.udp_refreshes);
    let failed_report = daemon
        .handle_rpc(RpcRequest { id: 772, method: "daemon_status_ex".to_string(), params: None })
        .expect("daemon status during failed reconfiguration")
        .result
        .expect("daemon status result");
    let failed_report = &failed_report["interfaces"][0]["settings"]["_runtime"]["udp"]["status"];
    assert_eq!(failed_report["link_state"].as_str(), Some("bind_failed"));
    assert!(failed_report["socket_errors"].as_u64().unwrap_or_default() >= 1);
    let reported_bind_error = failed_report["last_error"].as_str().expect("reported bind error");
    assert!(!reported_bind_error.contains("ifac-retry-initial-secret"));
    assert!(!reported_bind_error.contains("ifac-retry-replacement-secret"));

    {
        let manager = iface_manager.lock().await;
        assert_eq!(manager.role(&initial_iface), None, "old listener must be stopped");
        assert_eq!(manager.role(&replacement_iface), Some(IfaceRole::Unicast));
        let config = manager.shared_config(&replacement_iface).expect("replacement IFAC policy");
        assert_eq!(config.ifac_size, Some(128));
        assert_eq!(config.network_name.as_deref(), Some("ifac-retry-network"));
        assert_eq!(config.passphrase.as_deref(), Some("ifac-retry-replacement-secret"));
    }

    drop(reservation);
    timeout(Duration::from_secs(7), async {
        while replacement_refresh.status.snapshot().link_state != "bound" {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("UDP worker should retry binding without clearing IFAC");

    let probe = UdpSocket::bind("127.0.0.1:0").await.expect("bind plaintext probe");
    probe
        .send_to(&[0_u8; 64], ("127.0.0.1", replacement_port))
        .await
        .expect("send plaintext UDP probe");
    timeout(Duration::from_secs(2), async {
        loop {
            let mut manager = iface_manager.lock().await;
            let violations = manager
                .traffic_snapshots()
                .into_iter()
                .find(|snapshot| snapshot.address == replacement_iface)
                .map(|snapshot| snapshot.ifac_violations)
                .unwrap_or_default();
            if violations > 0 && replacement_refresh.status.snapshot().decode_errors > 0 {
                assert_eq!(violations, 1);
                break;
            }
            drop(manager);
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("rebound IFAC listener must reject and count plaintext");

    let final_status = replacement_refresh.status.snapshot();
    assert_eq!(final_status.link_state, "bound");
    assert_eq!(final_status.decode_errors, 1);
    assert_eq!(final_status.packets_rx, 0);
    assert_eq!(final_status.packets_tx, 0);
    assert_eq!(final_status.last_error.as_deref(), Some("couldn't decode packet"));
    refresh_hot_apply_udp_runtime_status_once(&daemon, &bridge.udp_refreshes);
    let recovered_report = daemon
        .handle_rpc(RpcRequest { id: 773, method: "daemon_status_ex".to_string(), params: None })
        .expect("daemon status after bind retry")
        .result
        .expect("daemon status result");
    let recovered_report =
        &recovered_report["interfaces"][0]["settings"]["_runtime"]["udp"]["status"];
    assert_eq!(recovered_report["link_state"].as_str(), Some("bound"));
    assert_eq!(recovered_report["decode_errors"].as_u64(), Some(1));
    assert_eq!(recovered_report["packets_rx"].as_u64(), Some(0));
}
