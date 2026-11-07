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
        stopping: None,
        wakeup: Default::default(),
        path_requests: Default::default(),
        deferred_dispatch: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        daemon: daemon.clone(),
        transport,
        peer_crypto: Arc::new(Mutex::new(HashMap::new())),
        outbound_propagation_identities: Arc::new(Mutex::new(HashMap::new())),
        receipt_map: Arc::new(Mutex::new(HashMap::new())),
        outbound_resource_map: Arc::new(Mutex::new(HashMap::new())),
        outbound_propagation_link: Arc::new(tokio::sync::Mutex::new(None)),
        direct_backchannel_links: DirectBackchannelLinks::new(),
        receipt_tx: receipt_tx.into(),
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
        stopping: None,
        wakeup: Default::default(),
        path_requests: Default::default(),
        deferred_dispatch: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        daemon: daemon.clone(),
        transport,
        peer_crypto: Arc::new(Mutex::new(HashMap::new())),
        outbound_propagation_identities: Arc::new(Mutex::new(HashMap::new())),
        receipt_map: Arc::new(Mutex::new(HashMap::new())),
        outbound_resource_map: Arc::new(Mutex::new(HashMap::new())),
        outbound_propagation_link: Arc::new(tokio::sync::Mutex::new(None)),
        direct_backchannel_links: DirectBackchannelLinks::new(),
        receipt_tx: receipt_tx.into(),
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
