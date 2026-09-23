use super::*;

use rns_transport::iface::auto::{
    AutoDataListenerBinding, AutoDiscoveryListenerBinding, AutoInterfaceConfig,
    AutoInterfaceDeviceFilter,
};
use rns_transport::iface::auto_runtime::AutoRuntimePlan;
use rns_transport::iface::InterfaceManager;

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
