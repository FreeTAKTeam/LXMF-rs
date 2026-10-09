#[tokio::test]
async fn opportunistic_packet_delivery_reports_status_hash_and_bytes() {
    let message_id = "opportunistic-packet-status";
    let store = MessagesStore::in_memory().expect("store");
    store
        .insert_message(&MessageRecord {
            id: message_id.to_string(),
            source: "source".to_string(),
            destination: "00000000000000000000000000000000".to_string(),
            title: String::new(),
            content: String::new(),
            timestamp: 0,
            direction: "out".to_string(),
            fields: None,
            receipt_status: Some("queued".to_string()),
        })
        .expect("insert message");
    let daemon = Arc::new(RpcDaemon::with_store(store, "opportunistic-packet-node".to_string()));
    let signer = PrivateIdentity::new_from_name("opportunistic-packet-status");
    let transport_identity = rns_transport::identity_bridge::to_transport_private_identity(&signer);
    let transport = Arc::new(Transport::new(TransportConfig::new(
        "opportunistic-packet-status",
        &transport_identity,
        true,
    )));
    let mut channel = transport
        .iface_manager()
        .lock()
        .await
        .new_channel_with_role(8, rns_transport::iface::IfaceRole::Unicast);

    let remote_signer = rns_transport::identity::PrivateIdentity::new_from_rand(OsRng);
    let remote_identity = *remote_signer.as_identity();
    let mut destination = [0u8; 16];
    destination.copy_from_slice(remote_identity.address_hash.as_slice());
    let receipt_map = Arc::new(Mutex::new(HashMap::new()));
    let (receipt_tx, mut receipt_rx) = tokio::sync::mpsc::channel(16);
    let task = DeliveryTask {
        daemon,
        transport,
        peer_crypto: Arc::new(Mutex::new(HashMap::new())),
        outbound_propagation_identities: Arc::new(Mutex::new(HashMap::new())),
        receipt_map: receipt_map.clone(),
        outbound_resource_map: Arc::new(Mutex::new(HashMap::new())),
        outbound_propagation_link: Arc::new(tokio::sync::Mutex::new(None)),
        direct_backchannel_links: DirectBackchannelLinks::new(),
        receipt_tx: receipt_tx.into(),
        message_id: message_id.to_string(),
        source_hash: [1u8; 16],
        destination,
        destination_hash: remote_identity.address_hash,
        destination_hex: hex::encode(destination),
        title: String::new(),
        content: String::new(),
        fields: None,
        signer,
        stamp_cost: None,
        outbound_ticket: None,
        include_ticket: None,
        peer_identity: Some(remote_identity),
        propagation_node_identity: None,
        requested_method: RequestedDeliveryMethod::Opportunistic,
        try_propagation_on_fail: false,
        propagation_node_hex: None,
    };
    let payload_body = b"opportunistic packet body";
    let mut lxmf_payload = destination.to_vec();
    lxmf_payload.extend_from_slice(payload_body);

    let task_handle = tokio::spawn(task.run_prepared(
        PreparedDeliveryPayload { lxmf_payload, propagation: None },
        Arc::new(tokio::sync::Semaphore::new(1)),
    ));
    let sent = tokio::time::timeout(Duration::from_millis(200), channel.tx_channel.recv())
        .await
        .expect("opportunistic packet")
        .expect("opportunistic packet");
    assert_eq!(sent.packet.destination, remote_identity.address_hash);
    assert_eq!(sent.packet.context, PacketContext::None);
    let packet_hash = hex::encode(sent.packet.hash().to_bytes());

    let receipt = tokio::time::timeout(Duration::from_millis(200), receipt_rx.recv())
        .await
        .expect("delivery receipt")
        .expect("delivery receipt");
    assert_eq!(receipt.message_id, message_id);
    assert_eq!(receipt.status, "sent: opportunistic");
    assert_eq!(receipt.packet_hash.as_deref(), Some(packet_hash.as_str()));
    assert_eq!(receipt.method.as_deref(), Some("opportunistic"));
    assert_eq!(receipt.delivery_kind.as_deref(), Some("opportunistic-packet"));
    assert_eq!(receipt.bytes, Some(payload_body.len()));
    assert_eq!(
        receipt_map.lock().expect("receipt map").get(&packet_hash),
        Some(&message_id.to_string())
    );
    task_handle.await.expect("delivery task join");
}
