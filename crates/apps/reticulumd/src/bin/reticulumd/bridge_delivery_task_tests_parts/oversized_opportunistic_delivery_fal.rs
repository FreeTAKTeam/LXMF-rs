#[tokio::test]
async fn oversized_opportunistic_delivery_falls_back_to_link_delivery() {
    let message_id = "oversized-opportunistic-fallback";
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
    let daemon = Arc::new(RpcDaemon::with_store(store, "opportunistic-fallback-node".to_string()));
    let signer = PrivateIdentity::new_from_name("oversized-opportunistic-fallback");
    let transport_identity = rns_transport::identity_bridge::to_transport_private_identity(&signer);
    let transport = Arc::new(Transport::new(TransportConfig::new(
        "oversized-opportunistic-fallback",
        &transport_identity,
        true,
    )));
    let mut channel = transport
        .iface_manager()
        .lock()
        .await
        .new_channel_with_role(8, rns_transport::iface::IfaceRole::Unicast);
    let iface = *channel.address();

    let remote_signer = rns_transport::identity::PrivateIdentity::new_from_rand(OsRng);
    let remote_identity = *remote_signer.as_identity();
    let destination_desc = DestinationDesc {
        identity: remote_identity,
        address_hash: remote_identity.address_hash,
        name: DestinationName::new("lxmf", "delivery"),
    };
    let mut destination = [0u8; 16];
    destination.copy_from_slice(remote_identity.address_hash.as_slice());
    let (receipt_tx, mut receipt_rx) = tokio::sync::mpsc::channel(16);
    let task_transport = transport.clone();
    let outbound_resource_map = Arc::new(Mutex::new(HashMap::new()));
    let task = DeliveryTask {
        daemon,
        transport,
        peer_crypto: Arc::new(Mutex::new(HashMap::new())),
        outbound_propagation_identities: Arc::new(Mutex::new(HashMap::new())),
        receipt_map: Arc::new(Mutex::new(HashMap::new())),
        outbound_resource_map: outbound_resource_map.clone(),
        outbound_propagation_link: Arc::new(tokio::sync::Mutex::new(None)),
        direct_backchannel_links: DirectBackchannelLinks::new(),
        receipt_tx,
        message_id: message_id.to_string(),
        source_hash: [1u8; 16],
        destination,
        destination_hash: remote_identity.address_hash,
        destination_hex: hex::encode(destination),
        title: String::new(),
        content: "x".repeat(8192),
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

    let task_handle = tokio::spawn(task.run());
    let link_request = tokio::select! {
        receipt = receipt_rx.recv() => {
            let status = receipt.expect("receipt before link request").status;
            assert!(
                !status.starts_with("failed: opportunistic"),
                "oversized opportunistic delivery should escalate to link delivery, got {status}"
            );
            panic!("expected link request before terminal receipt, got {status}");
        }
        request = channel.tx_channel.recv() => request.expect("link request message"),
    };
    let (tx, _) = tokio::sync::broadcast::channel(8);
    let mut inbound = Link::new_from_request(
        &link_request.packet,
        remote_signer.sign_key().clone(),
        destination_desc,
        tx,
    )
    .expect("link request should parse");
    let link_id = *inbound.id();
    let outbound_link = task_transport.find_out_link(&link_id).await.expect("outbound link");
    assert!(matches!(
        outbound_link.lock().await.handle_packet(&inbound.prove(), iface),
        rns_transport::destination::link::LinkHandleResult::Activated
    ));

    let identify = tokio::time::timeout(Duration::from_secs(2), channel.tx_channel.recv())
        .await
        .expect("backchannel identify")
        .expect("backchannel identify");
    assert_eq!(identify.packet.context, PacketContext::LinkIdentify);
    assert_eq!(identify.packet.destination, link_id);

    let advertisement = tokio::time::timeout(Duration::from_secs(2), channel.tx_channel.recv())
        .await
        .expect("resource advertisement")
        .expect("resource advertisement");
    assert_eq!(advertisement.packet.context, PacketContext::ResourceAdvrtisement);

    let receipt = tokio::time::timeout(Duration::from_secs(2), receipt_rx.recv())
        .await
        .expect("delivery receipt")
        .expect("delivery receipt");
    assert!(
        receipt.status == "sent: link" || receipt.status == "sending: link resource",
        "oversized opportunistic delivery should escalate to link delivery, got {}",
        receipt.status
    );
    {
        let tracked = outbound_resource_map.lock().expect("map");
        let resource_tracking = tracked.values().next().expect("tracked outbound resource");
        assert_eq!(resource_tracking.message_id, message_id);
        assert_eq!(resource_tracking.peer, hex::encode(remote_identity.address_hash.as_slice()));
        assert_eq!(resource_tracking.sent_status, OUTBOUND_RESOURCE_SENT_STATUS);
    }
    task_handle.await.expect("delivery task join");
}

#[tokio::test]
async fn propagated_link_send_tracks_resource_with_propagated_status() {
    let message_id = "propagated-resource-tracking";
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
            receipt_status: Some("sending: propagated resource".to_string()),
        })
        .expect("insert message");
    let daemon = Arc::new(RpcDaemon::with_store(store, "propagated-resource-node".to_string()));
    let signer = PrivateIdentity::new_from_name("propagated-resource-tracking");
    let transport_identity = rns_transport::identity_bridge::to_transport_private_identity(&signer);
    let transport = Arc::new(Transport::new(TransportConfig::new(
        "propagated-resource-tracking",
        &transport_identity,
        true,
    )));
    let mut channel = transport
        .iface_manager()
        .lock()
        .await
        .new_channel_with_role(8, rns_transport::iface::IfaceRole::Unicast);
    let iface = *channel.address();

    let propagation_signer = rns_transport::identity::PrivateIdentity::new_from_rand(OsRng);
    let propagation_identity = *propagation_signer.as_identity();
    let propagation_destination = DestinationDesc {
        identity: propagation_identity,
        address_hash: propagation_identity.address_hash,
        name: DestinationName::new("lxmf", "propagation"),
    };
    let propagation_link = transport.link(propagation_destination).await;
    let request_message =
        tokio::time::timeout(Duration::from_millis(200), channel.tx_channel.recv())
            .await
            .expect("link request tx")
            .expect("link request message");
    let (tx, _) = tokio::sync::broadcast::channel(8);
    let mut inbound = Link::new_from_request(
        &request_message.packet,
        propagation_signer.sign_key().clone(),
        propagation_destination,
        tx,
    )
    .expect("link request should parse");
    assert!(matches!(
        propagation_link.lock().await.handle_packet(&inbound.prove(), iface),
        rns_transport::destination::link::LinkHandleResult::Activated
    ));

    let (receipt_tx, mut receipt_rx) = tokio::sync::mpsc::channel(16);
    let outbound_resource_map = Arc::new(Mutex::new(HashMap::new()));
    let mut final_destination = [0u8; 16];
    final_destination.copy_from_slice(&[0x55; 16]);
    let propagation_node_hex = hex::encode(propagation_identity.address_hash.as_slice());
    let task = DeliveryTask {
        daemon,
        transport,
        peer_crypto: Arc::new(Mutex::new(HashMap::new())),
        outbound_propagation_identities: Arc::new(Mutex::new(HashMap::new())),
        receipt_map: Arc::new(Mutex::new(HashMap::new())),
        outbound_resource_map: outbound_resource_map.clone(),
        outbound_propagation_link: Arc::new(tokio::sync::Mutex::new(None)),
        direct_backchannel_links: DirectBackchannelLinks::new(),
        receipt_tx,
        message_id: message_id.to_string(),
        source_hash: [1u8; 16],
        destination: final_destination,
        destination_hash: AddressHash::new(final_destination),
        destination_hex: hex::encode(final_destination),
        title: String::new(),
        content: String::new(),
        fields: None,
        signer,
        stamp_cost: None,
        outbound_ticket: None,
        include_ticket: None,
        peer_identity: None,
        propagation_node_identity: Some(propagation_identity),
        requested_method: RequestedDeliveryMethod::Propagated,
        try_propagation_on_fail: false,
        propagation_node_hex: Some(propagation_node_hex.clone()),
    };

    task.send_via_existing_link_mode(
        "propagation",
        propagation_node_hex.as_str(),
        propagation_link,
        b"propagated payload",
        LinkModeStatuses {
            packet: "sent: propagated",
            resource: "sending: propagated resource",
            resource_sent: "sent: propagated resource",
        },
    )
    .await
    .expect("propagated resource send");

    let advertisement = tokio::time::timeout(Duration::from_millis(200), channel.tx_channel.recv())
        .await
        .expect("resource advertisement")
        .expect("resource advertisement");
    assert_eq!(advertisement.packet.context, PacketContext::ResourceAdvrtisement);

    let receipt = tokio::time::timeout(Duration::from_millis(200), receipt_rx.recv())
        .await
        .expect("receipt")
        .expect("receipt");
    assert_eq!(receipt.message_id, message_id);
    assert_eq!(receipt.status, "sending: propagated resource");

    let tracked = outbound_resource_map.lock().expect("map");
    let resource_tracking = tracked.values().next().expect("tracked outbound resource");
    assert_eq!(resource_tracking.message_id, message_id);
    assert_eq!(resource_tracking.peer, propagation_node_hex);
    assert_eq!(resource_tracking.bytes, b"propagated payload".len());
    assert_eq!(resource_tracking.sent_status, "sent: propagated resource");
}

#[tokio::test]
async fn build_payload_records_normal_stamp_lifecycle_metadata() {
    let message_id = "stamped-delivery-task";
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
            fields: Some(json!({
                "app": "value",
                "_lxmf": {
                    "stamp_error": "previous failed stamp attempt"
                }
            })),
            receipt_status: Some("queued".to_string()),
        })
        .expect("insert message");
    let daemon = Arc::new(RpcDaemon::with_store(store, "stamp-lifecycle-node".to_string()));
    let signer = PrivateIdentity::new_from_name("stamped-delivery-task");
    let transport_identity = rns_transport::identity_bridge::to_transport_private_identity(&signer);
    let transport = Arc::new(Transport::new(TransportConfig::new(
        "stamped-delivery-task",
        &transport_identity,
        true,
    )));
    let (receipt_tx, _receipt_rx) = tokio::sync::mpsc::channel(16);
    let destination = [0u8; 16];
    let mut source_hash = [0u8; 16];
    source_hash.copy_from_slice(signer.address_hash().as_slice());
    let task = DeliveryTask {
        daemon: daemon.clone(),
        transport,
        peer_crypto: Arc::new(Mutex::new(HashMap::new())),
        outbound_propagation_identities: Arc::new(Mutex::new(HashMap::new())),
        receipt_map: Arc::new(Mutex::new(HashMap::new())),
        outbound_resource_map: Arc::new(Mutex::new(HashMap::new())),
        outbound_propagation_link: Arc::new(tokio::sync::Mutex::new(None)),
        direct_backchannel_links: DirectBackchannelLinks::new(),
        receipt_tx,
        message_id: message_id.to_string(),
        source_hash,
        destination,
        destination_hash: AddressHash::new(destination),
        destination_hex: hex::encode(destination),
        title: "title".to_string(),
        content: "content".to_string(),
        fields: None,
        signer,
        stamp_cost: Some(1),
        outbound_ticket: None,
        include_ticket: None,
        peer_identity: None,
        propagation_node_identity: None,
        requested_method: RequestedDeliveryMethod::Direct,
        try_propagation_on_fail: false,
        propagation_node_hex: None,
    };

    let payload = task.build_payload().await.expect("payload");
    assert!(!payload.is_empty());

    let result = daemon
        .handle_rpc(RpcRequest { id: 77, method: "list_messages".to_string(), params: None })
        .expect("list messages")
        .result
        .expect("result");
    let message = result["messages"]
        .as_array()
        .expect("messages")
        .iter()
        .find(|message| message["id"].as_str() == Some(message_id))
        .expect("message");
    assert_eq!(message["fields"]["app"], json!("value"));
    assert_eq!(message["fields"]["_lxmf"]["stamp_state"], json!("ready"));
    assert_eq!(message["fields"]["_lxmf"]["stamp_kind"], json!("pow"));
    assert_eq!(message["fields"]["_lxmf"]["stamp_target_cost"], json!(1));
    assert_eq!(message["fields"]["_lxmf"]["stamp_error"], JsonValue::Null);
}

#[tokio::test]
async fn build_payload_records_ticket_stamp_lifecycle_metadata() {
    let message_id = "ticket-stamped-delivery-task";
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
    let daemon = Arc::new(RpcDaemon::with_store(store, "ticket-stamp-lifecycle-node".to_string()));
    let signer = PrivateIdentity::new_from_name("ticket-stamped-delivery-task");
    let transport_identity = rns_transport::identity_bridge::to_transport_private_identity(&signer);
    let transport = Arc::new(Transport::new(TransportConfig::new(
        "ticket-stamped-delivery-task",
        &transport_identity,
        true,
    )));
    let (receipt_tx, _receipt_rx) = tokio::sync::mpsc::channel(16);
    let destination = [0u8; 16];
    let mut source_hash = [0u8; 16];
    source_hash.copy_from_slice(signer.address_hash().as_slice());
    let task = DeliveryTask {
        daemon: daemon.clone(),
        transport,
        peer_crypto: Arc::new(Mutex::new(HashMap::new())),
        outbound_propagation_identities: Arc::new(Mutex::new(HashMap::new())),
        receipt_map: Arc::new(Mutex::new(HashMap::new())),
        outbound_resource_map: Arc::new(Mutex::new(HashMap::new())),
        outbound_propagation_link: Arc::new(tokio::sync::Mutex::new(None)),
        direct_backchannel_links: DirectBackchannelLinks::new(),
        receipt_tx,
        message_id: message_id.to_string(),
        source_hash,
        destination,
        destination_hash: AddressHash::new(destination),
        destination_hex: hex::encode(destination),
        title: "title".to_string(),
        content: "content".to_string(),
        fields: None,
        signer,
        stamp_cost: None,
        outbound_ticket: Some("000102030405060708090a0b0c0d0e0f".to_string()),
        include_ticket: None,
        peer_identity: None,
        propagation_node_identity: None,
        requested_method: RequestedDeliveryMethod::Direct,
        try_propagation_on_fail: false,
        propagation_node_hex: None,
    };

    let payload = task.build_payload().await.expect("payload");
    assert!(!payload.is_empty());

    let result = daemon
        .handle_rpc(RpcRequest { id: 79, method: "list_messages".to_string(), params: None })
        .expect("list messages")
        .result
        .expect("result");
    let message = result["messages"]
        .as_array()
        .expect("messages")
        .iter()
        .find(|message| message["id"].as_str() == Some(message_id))
        .expect("message");
    assert_eq!(message["fields"]["_lxmf"]["stamp_state"], json!("ready"));
    assert_eq!(message["fields"]["_lxmf"]["stamp_kind"], json!("ticket"));
    assert_eq!(message["fields"]["_lxmf"]["stamp_target_cost"], json!(256));
    assert_eq!(
        message["fields"]["_lxmf"]["stamp_ticket_source"],
        json!("000102030405060708090a0b0c0d0e0f")
    );
}
