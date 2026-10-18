#[tokio::test]
async fn configured_dispatch_preserves_mixed_order_selection_and_owned_effects() {
    let peer = std::net::TcpListener::bind("127.0.0.1:0").expect("hold TCP peer endpoint");
    let config = DaemonConfig::from_toml(&format!(
        r#"interfaces = [
          {{ type = "FutureDisabled", enabled = false }},
          {{ type = "auto", enabled = true, name = "auto-first" }},
          {{ type = "tcp_server", enabled = true, name = "selected", host = "127.0.0.1", port = 4242 }},
          {{ type = "FutureUnknown", enabled = true }},
          {{ type = "udp", enabled = true, name = "udp", host = "127.0.0.1", port = 0 }},
          {{ type = "pipe", enabled = true, name = "pipe", command = "cat" }},
          {{ type = "auto", enabled = false, name = "auto-disabled" }},
          {{ type = "auto", enabled = true, name = "auto-second" }},
          {{ type = "tcp_client", enabled = true, name = "client", host = "127.0.0.1", port = {} }},
          {{ type = "tcp_server", enabled = true, name = "shadowed", host = "127.0.0.1", port = 4243 }}
        ]"#,
        peer.local_addr().expect("peer address").port()
    ))
    .expect("mixed configured recipes");
    let identity = rns_core::identity::PrivateIdentity::new_from_rand(rand_core::OsRng);
    let private = rns_transport::identity_bridge::to_transport_private_identity(&identity);
    let transport = Transport::new(rns_transport::transport::TransportConfig::new(
        "dispatch-order",
        &private,
        true,
    ));
    let manager = transport.iface_manager();
    // Selection annotation consumes a preinstalled native address; listener construction is separate.
    let selected = *manager.lock().await.new_channel(8).address();
    let selection = TcpServerSelection {
        selected_index: Some(2),
        kind: "tcp_server".to_owned(),
        bind_addr: Some("127.0.0.1:4242".to_owned()),
        ..TcpServerSelection::default()
    };
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
    records[0].settings = Some(serde_json::json!({"disabled_sentinel": true}));
    let disabled = records[0].clone();
    let disabled_auto = records[6].clone();
    let original_order =
        records.iter().map(|row| (row.kind.clone(), row.name.clone())).collect::<Vec<_>>();
    let plan = auto_loopback_plan();
    let discovery_port = plan.config.discovery_port;
    let data_port = plan.config.data_port;
    let mut builder_calls = Vec::new();
    let mut batch = startup_configured_interfaces_with_auto_plan_builder(
        &startup_test_args(),
        &config,
        &selection,
        &transport,
        &manager,
        Some(&selected),
        &mut records,
        std::path::Path::new("."),
        None,
        None,
        |iface| {
            builder_calls.push(iface.name.clone().expect("named Auto recipe"));
            if builder_calls.len() == 1 {
                Ok(plan.clone())
            } else {
                Err("planned second Auto rejection".to_owned())
            }
        },
    )
    .await;
    assert_eq!(builder_calls, ["auto-first", "auto-second"]);
    assert_eq!(records[0], disabled);
    assert_eq!(records[6], disabled_auto);
    assert_eq!(
        records.iter().map(|row| (row.kind.clone(), row.name.clone())).collect::<Vec<_>>(),
        original_order
    );
    assert_eq!(
        batch.startup_successes, 4,
        "selected listener is already accounted outside this dispatcher"
    );
    assert_eq!(
        batch.startup_failures.iter().map(|failure| failure.label.as_str()).collect::<Vec<_>>(),
        ["FutureUnknown[3]", "auto-second"]
    );
    assert_eq!(
        batch.startup_failures[1].error,
        "AutoInterface OS interface discovery failed: planned second Auto rejection"
    );
    assert_eq!(
        records[2].settings.as_ref().expect("selected settings")["_runtime"]["iface"],
        selected.to_string()
    );
    assert_eq!(
        records[9].settings.as_ref().expect("shadowed settings")["_runtime"]["startup_status"],
        "shadowed_by_transport_override"
    );
    assert_eq!(
        batch
            .seeded_hot_apply_interfaces
            .iter()
            .map(|(_, record, _)| record.name.as_deref())
            .collect::<Vec<_>>(),
        [Some("selected"), Some("udp"), Some("client")]
    );
    assert_eq!(batch.seeded_hot_apply_interfaces[0].2, selected);
    assert_eq!(batch.tunnel_synth_ifaces.len(), 1);
    assert_eq!(batch.tunnel_synth_ifaces[0], batch.tcp_runtime_refreshes[0].runtime_iface);
    assert_eq!(batch.auto_runtime_refreshes.len(), 1);
    assert_eq!(batch.auto_runtime_shutdowns.len(), 1);
    assert_eq!(batch.udp_runtime_refreshes.len(), 1);
    assert_eq!(batch.pipe_runtime_refreshes.len(), 1);
    assert_eq!(batch.tcp_runtime_refreshes.len(), 1);
    assert!(batch.rnode_management_bindings.is_empty() && batch.weave_control_bindings.is_empty());
    for shutdown in batch.auto_runtime_shutdowns.drain(..) {
        let workers = manager.lock().await.take_worker_tree(shutdown.host_iface);
        assert!(shutdown.stop().await);
        for worker in workers {
            tokio::time::timeout(Duration::from_secs(2), worker)
                .await
                .expect("Auto worker stops")
                .expect("Auto worker joins");
        }
    }
    let addresses = manager.lock().await.interface_hashes();
    for address in addresses {
        let workers = manager.lock().await.take_worker_tree(address);
        assert!(transport.stop_interface(address).await);
        for worker in workers {
            tokio::time::timeout(Duration::from_secs(2), worker)
                .await
                .expect("owned worker stops")
                .expect("owned worker joins");
        }
    }
    assert!(manager.lock().await.interface_hashes().is_empty());
    for port in [discovery_port, data_port] {
        std::net::UdpSocket::bind((std::net::Ipv4Addr::LOCALHOST, port))
            .expect("Auto owned port released");
    }
}
