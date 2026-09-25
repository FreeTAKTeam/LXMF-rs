use super::*;

use rns_transport::iface::auto::{
    AutoDataListenerBinding, AutoDiscoveryListenerBinding, AutoInterfaceConfig,
    AutoInterfaceDeviceFilter,
};
use rns_transport::iface::auto_runtime::AutoRuntimePlan;
use rns_transport::iface::InterfaceManager;

fn startup_test_args() -> Args {
    Args {
        rpc: None,
        db: std::path::PathBuf::from("reticulum.db"),
        config: None,
        identity: None,
        announce_interval_secs: 0,
        transport: Some("127.0.0.1:0".to_string()),
        strict_interface_startup: false,
        rpc_tls_cert: None,
        rpc_tls_key: None,
        rpc_tls_client_ca: None,
        rpc_token_issuer: None,
        rpc_token_audience: None,
        rpc_token_secret_env: None,
        rpc_token_jti_ttl_ms: 60_000,
        rpc_token_clock_skew_ms: 5_000,
        rpc_unix: None,
        #[cfg(feature = "zmq-pipeline-rpc")]
        zmq_rpc_command: None,
        #[cfg(feature = "zmq-pipeline-rpc")]
        zmq_rpc_endpoint: None,
    }
}

fn auto_loopback_plan() -> AutoRuntimePlan {
    let discovery_reservation = std::net::UdpSocket::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .expect("reserve daemon AutoInterface discovery port");
    let data_reservation = std::net::UdpSocket::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .expect("reserve daemon AutoInterface data port");
    let discovery_port =
        discovery_reservation.local_addr().expect("read reserved discovery address").port();
    let data_port = data_reservation.local_addr().expect("read reserved data address").port();
    drop((discovery_reservation, data_reservation));

    let config =
        AutoInterfaceConfig { discovery_port, data_port, ..AutoInterfaceConfig::default() };
    let mut plan =
        AutoRuntimePlan::from_candidates(config, AutoInterfaceDeviceFilter::default(), vec![]);
    plan.startup_plan.discovery_listeners.push(AutoDiscoveryListenerBinding {
        ifname: "lo".to_string(),
        link_local_address: std::net::Ipv4Addr::LOCALHOST.to_string(),
        unicast_bind_address: std::net::Ipv4Addr::LOCALHOST.to_string(),
        unicast_bind_port: discovery_port,
        multicast_group_address: "239.255.0.1".to_string(),
        multicast_bind_address: "239.255.0.1".to_string(),
        multicast_bind_port: discovery_port,
    });
    plan.startup_plan.data_listeners.push(AutoDataListenerBinding {
        ifname: "lo".to_string(),
        link_local_address: std::net::Ipv4Addr::LOCALHOST.to_string(),
        bind_address: std::net::Ipv4Addr::LOCALHOST.to_string(),
        bind_port: data_port,
    });
    plan
}

fn auto_plan_for_config(config: AutoInterfaceConfig) -> AutoRuntimePlan {
    let mut plan =
        AutoRuntimePlan::from_candidates(config, AutoInterfaceDeviceFilter::default(), vec![]);
    let discovery_port = plan.config.discovery_port;
    let data_port = plan.config.data_port;
    plan.startup_plan.discovery_listeners.push(AutoDiscoveryListenerBinding {
        ifname: "lo".to_string(),
        link_local_address: std::net::Ipv4Addr::LOCALHOST.to_string(),
        unicast_bind_address: std::net::Ipv4Addr::LOCALHOST.to_string(),
        unicast_bind_port: discovery_port,
        multicast_group_address: "239.255.0.1".to_string(),
        multicast_bind_address: "239.255.0.1".to_string(),
        multicast_bind_port: discovery_port,
    });
    plan.startup_plan.data_listeners.push(AutoDataListenerBinding {
        ifname: "lo".to_string(),
        link_local_address: std::net::Ipv4Addr::LOCALHOST.to_string(),
        bind_address: std::net::Ipv4Addr::LOCALHOST.to_string(),
        bind_port: data_port,
    });
    plan
}

async fn start_configured_auto_for_test(
    plan: AutoRuntimePlan,
    config: &reticulum_daemon::config::DaemonConfig,
    transport: &rns_transport::transport::Transport,
    iface_manager: &Arc<tokio::sync::Mutex<InterfaceManager>>,
) -> (InterfaceStartupBatch, Vec<InterfaceRecord>) {
    let mut records = config
        .interfaces
        .iter()
        .map(|iface| InterfaceRecord {
            kind: iface.kind.clone(),
            enabled: iface.enabled(),
            host: iface.host.clone(),
            port: iface.port,
            name: iface.name.clone(),
            settings: iface.settings_json(),
        })
        .collect::<Vec<_>>();
    let batch = startup_configured_interfaces_with_auto_plan_builder(
        &startup_test_args(),
        config,
        &TcpServerSelection::default(),
        transport,
        iface_manager,
        None,
        &mut records,
        std::path::Path::new("."),
        None,
        None,
        move |_| Ok(plan.clone()),
    )
    .await;
    (batch, records)
}

#[tokio::test]
async fn configured_daemon_auto_shutdown_releases_sockets_and_allows_restart() {
    let discovery_reservation = std::net::UdpSocket::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .expect("reserve configured discovery port");
    let data_reservation = std::net::UdpSocket::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .expect("reserve configured data port");
    let discovery_port = discovery_reservation.local_addr().expect("discovery address").port();
    let data_port = data_reservation.local_addr().expect("data address").port();
    drop((discovery_reservation, data_reservation));

    let config = reticulum_daemon::config::DaemonConfig::from_toml(&format!(
        r#"interfaces = [{{ type = "AutoInterface", enabled = true, name = "configured-loopback", discovery_scope = "global", discovery_port = {discovery_port}, data_port = {data_port}, multicast_address_type = "permanent" }}]"#
    ))
    .expect("parse configured daemon interface");
    let iface = &config.interfaces[0];
    let plan =
        auto_plan_for_config(auto::auto_config(iface).expect("map configured AutoInterface"));
    let identity = rns_core::identity::PrivateIdentity::new_from_rand(rand_core::OsRng);
    let transport_identity =
        rns_transport::identity_bridge::to_transport_private_identity(&identity);
    let transport =
        rns_transport::transport::Transport::new(rns_transport::transport::TransportConfig::new(
            "configured-auto-test",
            &transport_identity,
            true,
        ));
    let iface_manager = transport.iface_manager();

    let (first, first_records) =
        start_configured_auto_for_test(plan.clone(), &config, &transport, &iface_manager).await;
    assert_eq!(first.startup_successes, 1);
    assert!(first.startup_failures.is_empty());
    assert_eq!(first.auto_runtime_refreshes.len(), 1, "status reporting handle is retained");
    assert_eq!(first.auto_runtime_shutdowns.len(), 1, "daemon owns the explicit stop handle");
    let first_iface = first.auto_runtime_refreshes[0].runtime_iface;
    assert!(iface_manager.lock().await.interface_hashes().contains(&first_iface));
    assert_eq!(
        first_records[0].settings.as_ref().unwrap()["_runtime"]["runtime_status"],
        "running"
    );
    crate::bootstrap::shutdown_auto_interfaces(first.auto_runtime_shutdowns).await;
    assert!(!iface_manager.lock().await.interface_hashes().contains(&first_iface));

    let (restarted, _) =
        start_configured_auto_for_test(plan, &config, &transport, &iface_manager).await;
    assert_eq!(restarted.startup_successes, 1);
    assert_eq!(restarted.auto_runtime_refreshes.len(), 1);
    let restarted_iface = restarted.auto_runtime_refreshes[0].runtime_iface;
    assert_ne!(first_iface, restarted_iface);
    crate::bootstrap::shutdown_auto_interfaces(restarted.auto_runtime_shutdowns).await;
    assert!(!iface_manager.lock().await.interface_hashes().contains(&restarted_iface));

    std::net::UdpSocket::bind((std::net::Ipv4Addr::LOCALHOST, discovery_port))
        .expect("configured daemon shutdown releases discovery socket");
    std::net::UdpSocket::bind((std::net::Ipv4Addr::LOCALHOST, data_port))
        .expect("configured daemon shutdown releases data socket");
}

#[tokio::test]
async fn daemon_auto_activation_stops_adapter_and_restarts_on_owned_loopback_ports() {
    let plan = auto_loopback_plan();
    let discovery_port = plan.startup_plan.discovery_listeners[0].unicast_bind_port;
    let data_port = plan.startup_plan.data_listeners[0].bind_port;
    let config =
        InterfaceConfig { kind: "AutoInterface".to_string(), ..InterfaceConfig::default() };
    let iface_manager = Arc::new(tokio::sync::Mutex::new(InterfaceManager::new(8)));

    let first = activate_auto_plan(&plan, &config, &iface_manager)
        .await
        .expect("activate the daemon AutoInterface adapter");
    let first_iface = first.host_iface;
    assert_eq!(first.runtime.summary.bound_socket_count, 2);
    assert_eq!(first.runtime.summary.data_socket_count, 1);
    assert!(iface_manager.lock().await.interface_hashes().contains(&first_iface));
    assert!(first.stop(&iface_manager).await, "stop should unregister the daemon host channel");
    assert!(!iface_manager.lock().await.interface_hashes().contains(&first_iface));

    let restarted = activate_auto_plan(&plan, &config, &iface_manager)
        .await
        .expect("restart the daemon AutoInterface on the same discovery and data ports");
    let restarted_iface = restarted.host_iface;
    assert_ne!(first_iface, restarted_iface);
    assert_eq!(restarted.runtime.summary.bound_socket_count, 2);
    assert_eq!(restarted.runtime.summary.data_socket_count, 1);
    assert!(iface_manager.lock().await.interface_hashes().contains(&restarted_iface));
    assert!(restarted.stop(&iface_manager).await, "stop should unregister the restarted channel");
    assert!(!iface_manager.lock().await.interface_hashes().contains(&restarted_iface));

    std::net::UdpSocket::bind((std::net::Ipv4Addr::LOCALHOST, discovery_port))
        .expect("daemon shutdown should release its discovery port");
    std::net::UdpSocket::bind((std::net::Ipv4Addr::LOCALHOST, data_port))
        .expect("daemon shutdown should release its data port");
}

#[tokio::test]
async fn daemon_auto_competing_activation_preserves_owner_and_retries_after_stop() {
    let plan = auto_loopback_plan();
    let config =
        InterfaceConfig { kind: "AutoInterface".to_string(), ..InterfaceConfig::default() };
    let iface_manager = Arc::new(tokio::sync::Mutex::new(InterfaceManager::new(8)));

    let owner = activate_auto_plan(&plan, &config, &iface_manager)
        .await
        .expect("activate the daemon AutoInterface socket owner");
    let owner_iface = owner.host_iface;
    assert_eq!(owner.runtime.summary.data_socket_count, 1);
    assert_eq!(iface_manager.lock().await.interface_hashes().len(), 1);

    let competing = activate_auto_plan(&plan, &config, &iface_manager).await;
    assert!(competing.is_err(), "a second runtime must not claim the active data port");
    let registered_ifaces = iface_manager.lock().await.interface_hashes();
    assert_eq!(registered_ifaces.len(), 1, "failed activation must not leak a host channel");
    assert!(
        registered_ifaces.contains(&owner_iface),
        "failed competing activation must preserve the active owner channel"
    );

    assert!(owner.stop(&iface_manager).await, "stop should release the active owner");
    assert!(iface_manager.lock().await.interface_hashes().is_empty());

    let restarted = activate_auto_plan(&plan, &config, &iface_manager)
        .await
        .expect("retry the same daemon plan after its socket owner stops");
    assert_eq!(restarted.runtime.summary.data_socket_count, 1);
    assert_eq!(iface_manager.lock().await.interface_hashes().len(), 1);
    assert!(restarted.stop(&iface_manager).await);
    assert!(iface_manager.lock().await.interface_hashes().is_empty());
}

#[tokio::test]
async fn daemon_auto_toml_ports_reach_production_adapter_and_are_reusable_after_restart() {
    let discovery_reservation = std::net::UdpSocket::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .expect("reserve daemon AutoInterface discovery port");
    let data_reservation = std::net::UdpSocket::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .expect("reserve daemon AutoInterface data port");
    let discovery_port =
        discovery_reservation.local_addr().expect("read reserved discovery address").port();
    let data_port = data_reservation.local_addr().expect("read reserved data address").port();
    drop((discovery_reservation, data_reservation));

    let toml = format!(
        r#"
interfaces = [
  {{ type = "AutoInterface", enabled = true, name = "loopback-auto", group_id = "daemon-restart-test", discovery_scope = "global", discovery_port = {discovery_port}, data_port = {data_port}, multicast_address_type = "permanent" }}
]
"#
    );
    let daemon_config = reticulum_daemon::config::DaemonConfig::from_toml(&toml)
        .expect("parse production daemon AutoInterface stanza");
    let iface = &daemon_config.interfaces[0];
    let runtime_config = auto::auto_config(iface).expect("map daemon settings to runtime config");
    assert_eq!(runtime_config.discovery_port, discovery_port);
    assert_eq!(runtime_config.data_port, data_port);
    assert_eq!(runtime_config.group_id, "daemon-restart-test");

    let mut plan = AutoRuntimePlan::from_candidates(
        runtime_config,
        AutoInterfaceDeviceFilter::default(),
        vec![],
    );
    plan.startup_plan.discovery_listeners.push(AutoDiscoveryListenerBinding {
        ifname: "lo".to_string(),
        link_local_address: std::net::Ipv4Addr::LOCALHOST.to_string(),
        unicast_bind_address: std::net::Ipv4Addr::LOCALHOST.to_string(),
        unicast_bind_port: discovery_port,
        multicast_group_address: "239.255.0.1".to_string(),
        multicast_bind_address: "239.255.0.1".to_string(),
        multicast_bind_port: discovery_port,
    });
    plan.startup_plan.data_listeners.push(AutoDataListenerBinding {
        ifname: "lo".to_string(),
        link_local_address: std::net::Ipv4Addr::LOCALHOST.to_string(),
        bind_address: std::net::Ipv4Addr::LOCALHOST.to_string(),
        bind_port: data_port,
    });

    let iface_manager = Arc::new(tokio::sync::Mutex::new(InterfaceManager::new(8)));
    let first = activate_auto_plan(&plan, iface, &iface_manager)
        .await
        .expect("activate production daemon adapter from configured runtime settings");
    assert_eq!(first.runtime.summary.bound_socket_count, 2);
    assert_eq!(first.runtime.summary.data_socket_count, 1);
    assert!(first.stop(&iface_manager).await, "stop should unregister the daemon host channel");

    let restarted = activate_auto_plan(&plan, iface, &iface_manager)
        .await
        .expect("restart on the configured daemon ports");
    assert_eq!(restarted.runtime.summary.bound_socket_count, 2);
    assert_eq!(restarted.runtime.summary.data_socket_count, 1);
    assert!(restarted.stop(&iface_manager).await, "stop should unregister the restarted channel");

    std::net::UdpSocket::bind((std::net::Ipv4Addr::LOCALHOST, discovery_port))
        .expect("daemon shutdown should release its configured discovery port");
    std::net::UdpSocket::bind((std::net::Ipv4Addr::LOCALHOST, data_port))
        .expect("daemon shutdown should release its configured data port");
}

#[tokio::test]
async fn failed_daemon_auto_activation_unregisters_channel_and_releases_bound_sockets() {
    let mut plan = auto_loopback_plan();
    let discovery_port = plan.startup_plan.discovery_listeners[0].unicast_bind_port;
    plan.startup_plan.data_listeners[0].bind_address = "not-an-ip-address".to_string();
    let iface = InterfaceConfig { kind: "AutoInterface".to_string(), ..InterfaceConfig::default() };
    let iface_manager = Arc::new(tokio::sync::Mutex::new(InterfaceManager::new(8)));

    let error = match activate_auto_plan(&plan, &iface, &iface_manager).await {
        Ok(activation) => {
            activation.stop(&iface_manager).await;
            panic!("invalid data listener unexpectedly activated");
        }
        Err(error) => error,
    };

    assert!(error.contains("invalid"), "unexpected activation error: {error}");
    assert!(
        iface_manager.lock().await.interface_hashes().is_empty(),
        "failed activation unregisters the daemon host interface"
    );
    std::net::UdpSocket::bind((std::net::Ipv4Addr::LOCALHOST, discovery_port))
        .expect("failed activation releases its already-bound discovery socket");
}
