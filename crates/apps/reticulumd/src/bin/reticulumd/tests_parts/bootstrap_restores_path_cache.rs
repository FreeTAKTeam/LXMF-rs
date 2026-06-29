#[test]
fn bootstrap_restores_python_path_table_for_path_lookup_rpc() {
    let temp = TempDir::new().expect("temp dir");
    let db_path = temp.path().join("reticulum.db");
    let mut identity_path = db_path.clone();
    identity_path.set_extension("identity");
    let local_identity =
        reticulum_daemon::identity_store::load_or_create_identity(&identity_path)
            .expect("seed daemon identity");
    let runtime =
        tokio::runtime::Builder::new_current_thread().enable_all().build().expect("runtime");

    let (destination_hex, expected_iface_hex) = runtime.block_on(async {
        let transport_identity =
            rns_transport::identity_bridge::to_transport_private_identity(&local_identity);
        let mut config =
            TransportConfig::new("bootstrap-path-restore-seed", &transport_identity, true);
        config.set_retransmit(true);
        let seed_transport = Transport::new(config);
        let iface_channel = seed_transport.iface_manager().lock().await.new_channel(16);
        let iface = *iface_channel.address();

        let remote_identity =
            rns_transport::identity::PrivateIdentity::new_from_rand(rand_core::OsRng);
        let mut remote_destination = rns_transport::destination::SingleInputDestination::new(
            remote_identity,
            DestinationName::new("lxmf", "delivery"),
        );
        let announce = remote_destination
            .announce(rand_core::OsRng, None)
            .expect("valid announce packet");
        let destination = announce.destination;
        let packet_hash_hex = hex::encode(announce.hash().as_slice());

        iface_channel
            .rx_channel
            .send(rns_transport::iface::RxMessage {
                address: iface,
                packet: announce,
                source: rns_transport::iface::IfaceSource::None,
            })
            .await
            .expect("seed announce");

        for _ in 0..20 {
            if seed_transport.has_path(&destination).await {
                break;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        assert!(
            seed_transport.has_path(&destination).await,
            "seed transport should learn remote path before saving"
        );
        assert_eq!(
            seed_transport
                .save_reticulum_path_table(temp.path())
                .await
                .expect("save path cache"),
            1
        );
        assert!(
            temp.path().join("destination_table").exists(),
            "seed should write Reticulum-compatible destination_table"
        );
        assert!(
            temp.path()
                .join("cache")
                .join("announces")
                .join(packet_hash_hex)
                .exists(),
            "seed should write Reticulum-compatible announce cache"
        );

        (hex::encode(destination.as_slice()), hex::encode(iface.as_slice()))
    });

    let context = runtime.block_on(async {
        bootstrap::bootstrap(test_args(
            db_path.clone(),
            None,
            Some("127.0.0.1:0".to_string()),
            false,
        ))
        .await
    });

    let status = context
        .daemon
        .handle_rpc(RpcRequest {
            id: 501,
            method: "path_status".to_string(),
            params: Some(json!({ "destination": destination_hex })),
        })
        .expect("path_status rpc");
    assert!(status.error.is_none(), "path_status should succeed: {:?}", status.error);
    let result = status.result.expect("path_status result");
    assert_eq!(result["destination"].as_str(), Some(destination_hex.as_str()));
    assert_eq!(result["destination_hash"].as_str(), Some(destination_hex.as_str()));
    assert_eq!(result["known"].as_bool(), Some(true));
    assert_eq!(result["path_found"].as_bool(), Some(true));
    assert_eq!(result["status"].as_str(), Some("found"));
    let expected_next_hop = format!("/{destination_hex}/");
    let expected_iface = format!("/{expected_iface_hex}/");
    assert_eq!(result["next_hop"].as_str(), Some(expected_next_hop.as_str()));
    assert_eq!(result["interface"].as_str(), Some(expected_iface.as_str()));
    assert_eq!(result["hops"].as_u64(), Some(0));

    let request = context
        .daemon
        .handle_rpc(RpcRequest {
            id: 502,
            method: "request_path".to_string(),
            params: Some(json!({ "destination": destination_hex, "timeout_secs": 0 })),
        })
        .expect("request_path rpc");
    assert!(request.error.is_none(), "request_path should succeed: {:?}", request.error);
    let result = request.result.expect("request_path result");
    assert_eq!(result["status"].as_str(), Some("found"));
    assert_eq!(result["known"].as_bool(), Some(true));
    assert_eq!(result["path_found"].as_bool(), Some(true));
    assert_eq!(result["requested"].as_bool(), Some(false));
}
