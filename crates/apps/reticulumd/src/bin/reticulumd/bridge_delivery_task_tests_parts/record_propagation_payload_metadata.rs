#[tokio::test]
async fn record_propagation_payload_metadata_persists_packed_bytes() {
    let message_id = "propagation-packed-metadata";
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
    let daemon = Arc::new(RpcDaemon::with_store(store, "propagation-packed-node".to_string()));
    let signer = PrivateIdentity::new_from_name("propagation-packed-metadata");
    let transport_identity = rns_transport::identity_bridge::to_transport_private_identity(&signer);
    let transport = Arc::new(Transport::new(TransportConfig::new(
        "propagation-packed-metadata",
        &transport_identity,
        true,
    )));
    let (receipt_tx, _receipt_rx) = tokio::sync::mpsc::channel(16);
    let destination = [0u8; 16];
    let task = DeliveryTask {
        daemon: daemon.clone(),
        transport,
        peer_crypto: Arc::new(Mutex::new(HashMap::new())),
        outbound_propagation_identities: Arc::new(Mutex::new(HashMap::new())),
        receipt_map: Arc::new(Mutex::new(HashMap::new())),
        outbound_resource_map: Arc::new(Mutex::new(HashMap::new())),
        outbound_propagation_link: Arc::new(tokio::sync::Mutex::new(None)),
        receipt_tx,
        message_id: message_id.to_string(),
        source_hash: [1u8; 16],
        destination,
        destination_hash: AddressHash::new(destination),
        destination_hex: hex::encode(destination),
        title: String::new(),
        content: String::new(),
        fields: None,
        signer,
        stamp_cost: None,
        outbound_ticket: None,
        include_ticket: None,
        peer_identity: None,
        propagation_node_identity: None,
        requested_method: RequestedDeliveryMethod::Propagated,
        try_propagation_on_fail: false,
        propagation_node_hex: Some(hex::encode([2u8; 16])),
    };
    let payload = propagation::PropagationPayload {
        bytes: b"packed-propagation-payload".to_vec(),
        transient_id: [3u8; 32],
        stamp_value: 17,
    };

    task.record_propagation_payload_metadata(&payload, 5);

    let result = daemon
        .handle_rpc(RpcRequest { id: 78, method: "list_messages".to_string(), params: None })
        .expect("list messages")
        .result
        .expect("result");
    let message = result["messages"]
        .as_array()
        .expect("messages")
        .iter()
        .find(|message| message["id"].as_str() == Some(message_id))
        .expect("message");
    assert_eq!(message["fields"]["_lxmf"]["propagation_packed"], json!(true));
    assert_eq!(
        message["fields"]["_lxmf"]["propagation_packed_base64"],
        json!("cGFja2VkLXByb3BhZ2F0aW9uLXBheWxvYWQ=")
    );
    assert_eq!(message["fields"]["_lxmf"]["propagation_packed_size"], json!(26));
    assert_eq!(message["fields"]["_lxmf"]["propagation_stamp_value"], json!(17));
}

#[tokio::test]
async fn propagation_stamp_retry_clears_stale_error_metadata() {
    let message_id = "propagation-stamp-retry-clears-error";
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
                "_lxmf": {
                    "propagation_stamp_error": "previous propagation stamp failure"
                }
            })),
            receipt_status: Some("queued".to_string()),
        })
        .expect("insert message");
    let daemon = Arc::new(RpcDaemon::with_store(store, "propagation-stamp-retry-node".to_string()));
    let mut task = delivery_task_for_propagation_cost_lookup(daemon.clone());
    task.message_id = message_id.to_string();

    task.record_propagation_stamp_work_metadata("generating", 5, None);

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
    assert_eq!(message["fields"]["_lxmf"]["propagation_stamp_state"], json!("generating"));
    assert_eq!(message["fields"]["_lxmf"]["propagation_stamp_error"], JsonValue::Null);
}

#[tokio::test]
async fn propagation_target_cost_matches_selected_node_case_insensitively() {
    let daemon = Arc::new(RpcDaemon::test_instance());
    daemon
        .handle_rpc(RpcRequest {
            id: 701,
            method: "propagation_enable".to_string(),
            params: Some(json!({
                "enabled": true,
                "autopeer": true,
            })),
        })
        .expect("enable propagation");
    let peer = "aabbccddeeff00112233445566778899";
    let app_data = rmp_serde::to_vec_named(&rmpv::Value::Array(vec![
        rmpv::Value::Boolean(false),
        rmpv::Value::from(1_700_000_021i64),
        rmpv::Value::Boolean(true),
        rmpv::Value::from(333),
        rmpv::Value::from(999),
        rmpv::Value::Array(vec![rmpv::Value::from(23), rmpv::Value::from(2), rmpv::Value::from(5)]),
        rmpv::Value::Map(Vec::new()),
    ]))
    .expect("encode propagation app data");
    let announce = daemon
        .handle_rpc(RpcRequest {
            id: 702,
            method: "announce_received".to_string(),
            params: Some(json!({
                "peer": peer,
                "timestamp": 1_700_000_021i64,
                "app_data_hex": hex::encode(app_data),
                "aspect": "lxmf.propagation",
                "hops": 1,
            })),
        })
        .expect("announce received");
    assert!(announce.error.is_none(), "unexpected announce error: {announce:?}");

    let task = delivery_task_for_propagation_cost_lookup(daemon);

    assert_eq!(task.propagation_target_cost(&peer.to_ascii_uppercase()), Some(23));
}
