#[tokio::test]
async fn configured_auto_and_meshtastic_effects_move_to_the_joined_mutation_owner() {
    if auto_lifecycle_runs_in_child() {
        return;
    }
    let plan = auto_loopback_plan();
    let discovery_port = plan.startup_plan.discovery_listeners[0].unicast_bind_port;
    let data_port = plan.startup_plan.data_listeners[0].bind_port;
    let config = DaemonConfig::from_toml("interfaces = [{type='auto', enabled=true}, {type='meshtastic', enabled=true, simulation_loopback=true}]").expect("config");
    let identity = rns_transport::identity::PrivateIdentity::new_from_name("owned-auto-effects");
    let transport = Arc::new(Transport::new(rns_transport::transport::TransportConfig::new(
        "owned-auto-effects",
        &identity,
        true,
    )));
    let manager = transport.iface_manager();
    let (batch, records) =
        start_configured_auto_for_test(plan, &config, &transport, &manager).await;
    assert_eq!(batch.configured_roots.len(), 2);
    let roots = batch.into_owned_roots(&records);
    assert!(roots[0].native.as_ref().expect("Auto native root").auto_shutdown.is_some());
    assert_eq!(roots[1].native.as_ref().expect("Meshtastic native root").auxiliary.len(), 1);
    let unrelated = manager.lock().await.new_channel(2);
    let daemon = Arc::new(rns_rpc::RpcDaemon::test_instance());
    daemon.replace_interfaces(records);
    let mut owner =
        crate::interface_hot_apply::InterfaceHotApplyBridge::spawn_with_configured_roots(
            transport,
            roots,
            Arc::downgrade(&daemon),
            None,
            None,
        );
    daemon.set_interface_mutation_bridge(Arc::new(owner.bridge.clone()));
    tokio::time::timeout(Duration::from_secs(3), owner.shutdown())
        .await
        .expect("bounded joined effect shutdown")
        .expect("effects stopped");
    assert_eq!(
        manager.lock().await.interface_hashes(),
        std::collections::HashSet::from([*unrelated.address()])
    );
    let _discovery = std::net::UdpSocket::bind((std::net::Ipv4Addr::LOCALHOST, discovery_port))
        .expect("Auto producer listener released");
    let _data = std::net::UdpSocket::bind((std::net::Ipv4Addr::LOCALHOST, data_port))
        .expect("Auto data listener released");
}
