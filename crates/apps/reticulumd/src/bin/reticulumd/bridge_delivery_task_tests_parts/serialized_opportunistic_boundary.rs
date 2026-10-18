#[tokio::test]
async fn daemon_opportunistic_boundary_matches_serialized_python_messages() {
    let fixtures: JsonValue = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../libs/test-support/fixtures/lxmf_1_2_0_sizes.json"
    )))
    .expect("fixtures");
    for case in fixtures["cases"].as_array().expect("cases") {
        let payload = hex::decode(case["wire_hex"].as_str().expect("wire")).expect("hex");
        let wire = lxmf::WireMessage::unpack(&payload).expect("Python message");
        let remote_identity =
            rns_transport::identity::PrivateIdentity::from_private_key_bytes(&[0x22; 64])
                .expect("fixture identity");
        let mut remote = rns_transport::destination::SingleInputDestination::new(
            remote_identity,
            DestinationName::new("lxmf", "delivery"),
        );
        assert_eq!(remote.desc.address_hash.as_slice(), &wire.destination);
        let daemon = Arc::new(RpcDaemon::with_store(
            MessagesStore::in_memory().expect("store"),
            "serialized-boundary".into(),
        ));
        let mut task = delivery_task_for_propagation_cost_lookup(daemon);
        task.destination = wire.destination;
        task.destination_hash = remote.desc.address_hash;
        task.destination_hex = hex::encode(wire.destination);
        task.peer_identity = Some(*remote.identity.as_identity());
        task.requested_method = RequestedDeliveryMethod::Opportunistic;
        let (receipt_tx, mut receipts) = tokio::sync::mpsc::channel(16);
        task.receipt_tx = receipt_tx.into();
        let mut channel = task.transport.iface_manager().lock().await.new_channel(16);
        let expected_payload = payload.clone();
        channel.rx_channel.send(rns_transport::iface::RxMessage {
            address: *channel.address(), packet: remote.announce(OsRng, None).expect("signed announce"),
            source: rns_transport::iface::IfaceSource::None,
        }).await.expect("announce ingress");
        while !task.transport.has_path(&remote.desc.address_hash).await { tokio::task::yield_now().await; }
        let (stop, owner) = start_prepared(task,
            PreparedDeliveryPayload { lxmf_payload: payload, propagation: None });
        let packet = tokio::time::timeout(Duration::from_secs(1), channel.tx_channel.recv())
            .await
            .expect("wire output")
            .expect("packet")
            .packet;
        if case["method"].as_u64() == Some(1) {
            assert_eq!(packet.header.packet_type, PacketType::Data, "{}", case["name"]);
            let (plaintext, _) = remote
                .decrypt_with_ratchets(packet.data.as_slice())
                .expect("recipient decrypts packet");
            assert_eq!(plaintext, expected_payload[16..], "{}", case["name"]);
            let receipt = tokio::time::timeout(Duration::from_secs(1), receipts.recv())
                .await
                .expect("receipt timeout")
                .expect("receipt");
            assert_eq!(receipt.status, "sent: opportunistic");
            assert!(!owner.is_finished(), "SINGLE handoff awaits receipt");
            stop.send_replace(true);
            owner.await.expect("production owner shutdown");
            assert!(channel.tx_channel.try_recv().is_err(), "no Link fallback");
        } else {
            assert_eq!(packet.header.packet_type, PacketType::LinkRequest, "{}", case["name"]);
            // This boundary test ends at the Link request; existing mixed-peer
            // tests prove its handshake and subsequent packet/Resource delivery.
            assert!(!owner.is_finished(), "Pending Link retains owner");
            stop.send_replace(true);
            owner.await.expect("production owner shutdown");
        }
    }
}
