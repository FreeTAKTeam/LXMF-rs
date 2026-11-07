#[cfg(unix)]
#[tokio::test]
async fn auto_runtime_stop_releases_loopback_sockets_for_restart() {
    if auto_lifecycle_runs_in_child() {
        return;
    }
    let discovery_reservation = std::net::UdpSocket::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .expect("reserve discovery loopback UDP port");
    let data_reservation = std::net::UdpSocket::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .expect("reserve data loopback UDP port");
    let discovery_port = discovery_reservation.local_addr().expect("read discovery port").port();
    let data_port = data_reservation.local_addr().expect("read data port").port();
    drop(data_reservation);
    drop(discovery_reservation);
    let mut plan =
        build_startup_plan_from_candidates(&auto_iface(), Vec::new()).expect("startup plan");
    plan.startup_plan.discovery_listeners.push(AutoDiscoveryListenerBinding {
        ifname: "lo".to_string(),
        link_local_address: "127.0.0.1".to_string(),
        unicast_bind_address: "127.0.0.1".to_string(),
        unicast_bind_port: discovery_port,
        multicast_group_address: "239.255.0.1".to_string(),
        multicast_bind_address: "239.255.0.1".to_string(),
        multicast_bind_port: discovery_port,
    });
    plan.startup_plan.data_listeners.push(AutoDataListenerBinding {
        ifname: "lo".to_string(),
        link_local_address: "127.0.0.1".to_string(),
        bind_address: "127.0.0.1".to_string(),
        bind_port: data_port,
    });

    let first = plan
        .spawn_discovery_runtime_with_native_scope_ids()
        .await
        .expect("start loopback AutoInterface runtime");
    assert_eq!(first.summary.bound_socket_count, 2);
    assert_eq!(first.summary.data_socket_count, 1);
    first.stop().await;

    let restarted = plan
        .spawn_discovery_runtime_with_native_scope_ids()
        .await
        .expect("restart after stop released discovery and data sockets");
    assert_eq!(restarted.summary.bound_socket_count, 2);
    assert_eq!(restarted.summary.data_socket_count, 1);
    restarted.stop().await;
}

#[cfg(unix)]
#[tokio::test]
async fn auto_runtime_reports_per_device_echo_loss_and_recovery_and_restarts_cleanly() {
    if auto_lifecycle_runs_in_child() {
        return;
    }
    let discovery_reservation = std::net::UdpSocket::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .expect("reserve discovery loopback UDP port");
    let data_reservation = std::net::UdpSocket::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .expect("reserve data loopback UDP port");
    let discovery_port = discovery_reservation.local_addr().expect("read discovery port").port();
    let data_port = data_reservation.local_addr().expect("read data port").port();
    drop((discovery_reservation, data_reservation));

    let mut plan = AutoRuntimePlan::from_candidates(
        AutoInterfaceConfig { discovery_port, data_port, ..AutoInterfaceConfig::default() },
        AutoInterfaceDeviceFilter {
            allowed: vec!["lo".to_string(), "lo-secondary".to_string()],
            ignored: Vec::new(),
        },
        vec![
            AutoInterfaceDeviceCandidate {
                ifname: "lo".to_string(),
                ipv6_addresses: vec!["fe80::1111".to_string()],
            },
            AutoInterfaceDeviceCandidate {
                ifname: "lo-secondary".to_string(),
                ipv6_addresses: vec!["fe80::2222".to_string()],
            },
        ],
    );
    plan.startup_plan.discovery_listeners = vec![AutoDiscoveryListenerBinding {
        ifname: "lo".to_string(),
        link_local_address: "127.0.0.1".to_string(),
        unicast_bind_address: "127.0.0.1".to_string(),
        unicast_bind_port: discovery_port,
        multicast_group_address: "239.255.0.1".to_string(),
        multicast_bind_address: "239.255.0.1".to_string(),
        multicast_bind_port: discovery_port,
    }];
    plan.startup_plan.data_listeners = vec![AutoDataListenerBinding {
        ifname: "lo".to_string(),
        link_local_address: "127.0.0.1".to_string(),
        bind_address: "127.0.0.1".to_string(),
        bind_port: data_port,
    }];

    let status = AutoRuntimeStatusHandle::from_startup_plan(&plan.startup_plan);
    let state = Arc::new(tokio::sync::Mutex::new(AutoDiscoveryState::from_timing(
        plan.adopted_devices.clone(),
        AutoInterfaceTiming::for_platform(plan.platform),
    )));
    let announce_socket = plan
        .bind_peer_announce_runtime_socket()
        .await
        .expect("bind production peer-job announce socket");
    let no_change = plan
        .send_due_peer_job_with_runtime_socket(
            Arc::clone(&state),
            Arc::clone(&announce_socket),
            None,
            Some(&status),
            core::time::Duration::ZERO,
        )
        .await
        .expect("prime production peer-job carrier state");
    assert!(no_change.carrier_events.is_empty());

    let echo_packet = plan.config.multicast_peering_packet(&plan.adopted_devices[0]);
    let secondary_echo_packet = plan.config.multicast_peering_packet(&plan.adopted_devices[1]);
    let echo_at = plan.startup_plan.initial_peering_wait;
    let mut state_guard = state.lock().await;
    let echo = plan
        .process_discovery_datagram(
            &mut state_guard,
            AutoDiscoveryDatagram {
                kind: AutoDiscoverySocketKind::Unicast,
                ifname: "lo".to_string(),
                bind_addr: (std::net::Ipv4Addr::LOCALHOST, discovery_port).into(),
                multicast_group_addr: None,
                source_addr: std::net::SocketAddr::new(
                    "fe80::1111".parse().expect("parse adopted loopback test address"),
                    discovery_port,
                ),
                payload: echo_packet.token.to_vec(),
            },
            echo_at,
        )
        .expect("authenticate reference-shaped local echo")
        .expect("process echo after initial peering wait");
    assert_eq!(echo.event, AutoDiscoveryEvent::LocalMulticastEcho { ifname: "lo".into() });
    let secondary_echo = plan
        .process_discovery_datagram(
            &mut state_guard,
            AutoDiscoveryDatagram {
                kind: AutoDiscoverySocketKind::Unicast,
                ifname: "lo-secondary".to_string(),
                bind_addr: (std::net::Ipv4Addr::LOCALHOST, discovery_port).into(),
                multicast_group_addr: None,
                source_addr: std::net::SocketAddr::new(
                    "fe80::2222".parse().expect("parse secondary adopted test address"),
                    discovery_port,
                ),
                payload: secondary_echo_packet.token.to_vec(),
            },
            echo_at,
        )
        .expect("authenticate secondary reference-shaped local echo")
        .expect("process secondary echo after initial peering wait");
    assert_eq!(
        secondary_echo.event,
        AutoDiscoveryEvent::LocalMulticastEcho { ifname: "lo-secondary".into() }
    );
    drop(state_guard);
    let _ = plan
        .send_due_peer_job_with_runtime_socket(
            Arc::clone(&state),
            Arc::clone(&announce_socket),
            None,
            Some(&status),
            echo_at,
        )
        .await
        .expect("record initial per-device multicast echo");

    let lost_at = echo_at
        + AutoInterfaceTiming::for_platform(plan.platform).multicast_echo_timeout
        + core::time::Duration::from_millis(1);
    let mut state_guard = state.lock().await;
    let secondary_echo = plan
        .process_discovery_datagram(
            &mut state_guard,
            AutoDiscoveryDatagram {
                kind: AutoDiscoverySocketKind::Unicast,
                ifname: "lo-secondary".to_string(),
                bind_addr: (std::net::Ipv4Addr::LOCALHOST, discovery_port).into(),
                multicast_group_addr: None,
                source_addr: std::net::SocketAddr::new(
                    "fe80::2222".parse().expect("parse secondary adopted test address"),
                    discovery_port,
                ),
                payload: secondary_echo_packet.token.to_vec(),
            },
            lost_at,
        )
        .expect("authenticate continuing secondary echo")
        .expect("process continuing secondary echo");
    assert_eq!(
        secondary_echo.event,
        AutoDiscoveryEvent::LocalMulticastEcho { ifname: "lo-secondary".into() }
    );
    drop(state_guard);
    let lost = plan
        .send_due_peer_job_with_runtime_socket(
            Arc::clone(&state),
            Arc::clone(&announce_socket),
            None,
            Some(&status),
            lost_at,
        )
        .await
        .expect("run production peer job after echo timeout");
    assert_eq!(
        lost.carrier_events,
        vec![AutoMulticastCarrierEvent::CarrierLost { ifname: "lo".to_string() }]
    );
    let lost_status = status.to_json();
    assert_eq!(lost_status["carrier_events"][0]["event"], "carrier_lost");
    assert_eq!(lost_status["carrier_events"][0]["ifname"], "lo");
    assert_eq!(lost_status["carrier_events"].as_array().map(Vec::len), Some(1));

    let mut state_guard = state.lock().await;
    let echo = plan
        .process_discovery_datagram(
            &mut state_guard,
            AutoDiscoveryDatagram {
                kind: AutoDiscoverySocketKind::Unicast,
                ifname: "lo".to_string(),
                bind_addr: (std::net::Ipv4Addr::LOCALHOST, discovery_port).into(),
                multicast_group_addr: None,
                source_addr: std::net::SocketAddr::new(
                    "fe80::1111".parse().expect("parse adopted loopback test address"),
                    discovery_port,
                ),
                payload: echo_packet.token.to_vec(),
            },
            lost_at,
        )
        .expect("authenticate recovered reference-shaped local echo")
        .expect("process echo after initial peering wait");
    assert_eq!(echo.event, AutoDiscoveryEvent::LocalMulticastEcho { ifname: "lo".into() });
    drop(state_guard);
    let recovered_at = lost_at + core::time::Duration::from_millis(1);
    let recovered = plan
        .send_due_peer_job_with_runtime_socket(
            Arc::clone(&state),
            Arc::clone(&announce_socket),
            None,
            Some(&status),
            recovered_at,
        )
        .await
        .expect("run production peer job after per-device echo returns");
    assert_eq!(
        recovered.carrier_events,
        vec![AutoMulticastCarrierEvent::CarrierRecovered { ifname: "lo".to_string() }]
    );
    let recovered_status = status.to_json();
    assert_eq!(recovered_status["carrier_events"][0]["event"], "carrier_recovered");
    assert_eq!(recovered_status["carrier_events"][0]["ifname"], "lo");
    assert_eq!(recovered_status["carrier_events"].as_array().map(Vec::len), Some(1));
    drop(announce_socket);

    let first = plan
        .spawn_discovery_runtime_with_native_scope_ids()
        .await
        .expect("start production AutoInterface runtime on loopback bindings");
    assert_eq!(first.summary.bound_socket_count, 2);
    assert_eq!(first.summary.data_socket_count, 1);
    first.stop().await;
    std::net::UdpSocket::bind((std::net::Ipv4Addr::LOCALHOST, discovery_port))
        .expect("stopping the runtime releases both discovery sockets");
    std::net::UdpSocket::bind((std::net::Ipv4Addr::LOCALHOST, data_port))
        .expect("stopping the runtime releases the per-device data socket");

    let restarted = plan
        .spawn_discovery_runtime_with_native_scope_ids()
        .await
        .expect("restart production runtime after per-device recovery and cleanup");
    assert_eq!(restarted.summary.bound_socket_count, 2);
    assert_eq!(restarted.summary.data_socket_count, 1);
    restarted.stop().await;
    std::net::UdpSocket::bind((std::net::Ipv4Addr::LOCALHOST, discovery_port))
        .expect("stopping restarted runtime releases discovery sockets");
    std::net::UdpSocket::bind((std::net::Ipv4Addr::LOCALHOST, data_port))
        .expect("stopping restarted runtime releases data socket");
}

#[cfg(unix)]
#[tokio::test]
async fn auto_runtime_startup_failure_releases_discovery_socket_before_retry() {
    if auto_lifecycle_runs_in_child() {
        return;
    }
    let discovery_reservation = std::net::UdpSocket::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .expect("reserve discovery loopback UDP port");
    let data_reservation = std::net::UdpSocket::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .expect("reserve data loopback UDP port");
    let discovery_port = discovery_reservation.local_addr().expect("read discovery port").port();
    let data_port = data_reservation.local_addr().expect("read data port").port();
    drop((data_reservation, discovery_reservation));

    let mut plan =
        build_startup_plan_from_candidates(&auto_iface(), Vec::new()).expect("startup plan");
    plan.startup_plan.discovery_listeners.push(AutoDiscoveryListenerBinding {
        ifname: "lo".to_string(),
        link_local_address: "127.0.0.1".to_string(),
        unicast_bind_address: "127.0.0.1".to_string(),
        unicast_bind_port: discovery_port,
        multicast_group_address: "239.255.0.1".to_string(),
        multicast_bind_address: "239.255.0.1".to_string(),
        multicast_bind_port: discovery_port,
    });
    plan.startup_plan.data_listeners.push(AutoDataListenerBinding {
        ifname: "lo".to_string(),
        link_local_address: "127.0.0.1".to_string(),
        bind_address: "not-an-ip-address".to_string(),
        bind_port: data_port,
    });

    let startup_error = match plan.spawn_discovery_runtime_with_native_scope_ids().await {
        Ok(runtime) => {
            runtime.stop().await;
            panic!("invalid data listener unexpectedly started the runtime");
        }
        Err(error) => error,
    };
    assert!(startup_error.contains("invalid"), "unexpected bind error: {startup_error}");

    let released_discovery_port =
        std::net::UdpSocket::bind((std::net::Ipv4Addr::LOCALHOST, discovery_port))
            .expect("failed startup should release its already-bound discovery socket");
    drop(released_discovery_port);

    plan.startup_plan.data_listeners[0].bind_address = "127.0.0.1".to_string();
    let retry = plan
        .spawn_discovery_runtime_with_native_scope_ids()
        .await
        .expect("retry after failed startup released discovery socket");
    assert_eq!(retry.summary.bound_socket_count, 2);
    assert_eq!(retry.summary.data_socket_count, 1);
    retry.stop().await;
}
