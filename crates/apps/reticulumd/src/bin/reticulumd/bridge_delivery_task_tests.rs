use super::*;
use rand_core::OsRng;
use rns_rpc::{MessageRecord, MessagesStore};
use rns_transport::transport::TransportConfig;

#[test]
fn cancelled_status_detection_is_case_and_space_tolerant() {
    assert!(DeliveryTask::is_cancelled_status(Some("cancelled")));
    assert!(DeliveryTask::is_cancelled_status(Some("  CANCELLED  ")));
    assert!(!DeliveryTask::is_cancelled_status(Some("sending")));
    assert!(!DeliveryTask::is_cancelled_status(Some("sent: link")));
    assert!(!DeliveryTask::is_cancelled_status(None));
}

#[tokio::test]
async fn abort_if_cancelled_reads_persisted_daemon_status() {
    let message_id = "cancelled-delivery-task";
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
            receipt_status: Some("cancelled".to_string()),
        })
        .expect("insert message");
    let daemon = Arc::new(RpcDaemon::with_store(store, "cancel-test-node".to_string()));
    let signer = PrivateIdentity::new_from_name("cancelled-delivery-task");
    let transport_identity = rns_transport::identity_bridge::to_transport_private_identity(&signer);
    let transport = Arc::new(Transport::new(TransportConfig::new(
        "cancelled-delivery-task",
        &transport_identity,
        true,
    )));
    let (receipt_tx, _receipt_rx) = tokio::sync::mpsc::channel(16);
    let destination = [0u8; 16];
    let task = DeliveryTask {
        daemon,
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
        requested_method: RequestedDeliveryMethod::Direct,
        try_propagation_on_fail: false,
        propagation_node_hex: None,
    };

    assert!(task.abort_if_cancelled("test"));
}

#[tokio::test]
async fn tracked_outbound_resource_cancel_sends_resource_cancel_frame() {
    let message_id = "cancel-active-resource";
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
            receipt_status: Some("cancelled".to_string()),
        })
        .expect("insert message");
    let daemon = RpcDaemon::with_store(store, "cancel-resource-node".to_string());
    let local_signer = PrivateIdentity::new_from_name("cancel-resource-node");
    let transport_identity =
        rns_transport::identity_bridge::to_transport_private_identity(&local_signer);
    let transport =
        Transport::new(TransportConfig::new("cancel-resource-node", &transport_identity, true));
    let mut channel = transport
        .iface_manager()
        .lock()
        .await
        .new_channel_with_role(8, rns_transport::iface::IfaceRole::Unicast);
    let iface = *channel.address();

    let remote_signer = rns_transport::identity::PrivateIdentity::new_from_rand(OsRng);
    let remote_identity = *remote_signer.as_identity();
    let destination = DestinationDesc {
        identity: remote_identity,
        address_hash: remote_identity.address_hash,
        name: DestinationName::new("lxmf", "delivery"),
    };
    let link = transport.link(destination).await;
    let request_message =
        tokio::time::timeout(Duration::from_millis(200), channel.tx_channel.recv())
            .await
            .expect("link request tx")
            .expect("link request message");
    let (tx, _) = tokio::sync::broadcast::channel(8);
    let mut inbound = Link::new_from_request(
        &request_message.packet,
        remote_signer.sign_key().clone(),
        destination,
        tx,
    )
    .expect("link request should parse");
    assert!(matches!(
        link.lock().await.handle_packet(&inbound.prove(), iface),
        rns_transport::destination::link::LinkHandleResult::Activated
    ));
    let link_id = *link.lock().await.id();

    let resource_hash = transport
        .send_resource(&link_id, b"active resource".to_vec(), None)
        .await
        .expect("resource send");
    let advertisement = tokio::time::timeout(Duration::from_millis(200), channel.tx_channel.recv())
        .await
        .expect("advertisement tx")
        .expect("advertisement message");
    assert_eq!(advertisement.packet.context, PacketContext::ResourceAdvrtisement);

    let outbound_resource_map = Arc::new(Mutex::new(HashMap::new()));
    let resource_hash_hex = hex::encode(resource_hash.as_slice());
    track_outbound_resource(
        &outbound_resource_map,
        resource_hash_hex.clone(),
        OutboundResourceTracking {
            message_id: message_id.to_string(),
            peer: hex::encode(destination.address_hash.as_slice()),
            bytes: 15,
            sent_status: OUTBOUND_RESOURCE_SENT_STATUS.to_string(),
        },
    );

    let cancelled = link_send::cancel_tracked_resource_if_message_cancelled(
        &daemon,
        &transport,
        &outbound_resource_map,
        message_id,
        link_id,
        resource_hash,
    )
    .await
    .expect("cancel check");
    assert!(cancelled);
    assert!(outbound_resource_map.lock().expect("map").get(&resource_hash_hex).is_none());

    let cancel = tokio::time::timeout(Duration::from_millis(200), channel.tx_channel.recv())
        .await
        .expect("cancel tx")
        .expect("cancel message");
    assert_eq!(cancel.packet.destination, link_id);
    assert_eq!(cancel.packet.context, PacketContext::ResourceInitiatorCancel);
}

#[tokio::test]
async fn resource_cancel_monitor_aborts_resource_after_late_cancel() {
    let message_id = "late-cancel-active-resource";
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
            receipt_status: Some("sending: link resource".to_string()),
        })
        .expect("insert message");
    let daemon = Arc::new(RpcDaemon::with_store(store, "late-cancel-resource-node".to_string()));
    let local_signer = PrivateIdentity::new_from_name("late-cancel-resource-node");
    let transport_identity =
        rns_transport::identity_bridge::to_transport_private_identity(&local_signer);
    let transport = Arc::new(Transport::new(TransportConfig::new(
        "late-cancel-resource-node",
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
    let destination = DestinationDesc {
        identity: remote_identity,
        address_hash: remote_identity.address_hash,
        name: DestinationName::new("lxmf", "delivery"),
    };
    let link = transport.link(destination).await;
    let request_message =
        tokio::time::timeout(Duration::from_millis(200), channel.tx_channel.recv())
            .await
            .expect("link request tx")
            .expect("link request message");
    let (tx, _) = tokio::sync::broadcast::channel(8);
    let mut inbound = Link::new_from_request(
        &request_message.packet,
        remote_signer.sign_key().clone(),
        destination,
        tx,
    )
    .expect("link request should parse");
    assert!(matches!(
        link.lock().await.handle_packet(&inbound.prove(), iface),
        rns_transport::destination::link::LinkHandleResult::Activated
    ));
    let link_id = *link.lock().await.id();

    let resource_hash = transport
        .send_resource(&link_id, b"late cancel resource".to_vec(), None)
        .await
        .expect("resource send");
    let _advertisement =
        tokio::time::timeout(Duration::from_millis(200), channel.tx_channel.recv())
            .await
            .expect("advertisement tx")
            .expect("advertisement message");

    let outbound_resource_map = Arc::new(Mutex::new(HashMap::new()));
    let resource_hash_hex = hex::encode(resource_hash.as_slice());
    track_outbound_resource(
        &outbound_resource_map,
        resource_hash_hex.clone(),
        OutboundResourceTracking {
            message_id: message_id.to_string(),
            peer: hex::encode(destination.address_hash.as_slice()),
            bytes: 20,
            sent_status: OUTBOUND_RESOURCE_SENT_STATUS.to_string(),
        },
    );
    link_send::spawn_tracked_resource_cancel_monitor(link_send::ResourceCancelMonitor {
        daemon: daemon.clone(),
        transport: transport.clone(),
        outbound_resource_map: outbound_resource_map.clone(),
        message_id: message_id.to_string(),
        destination_hex: hex::encode(destination.address_hash.as_slice()),
        trace_stage: "link".to_string(),
        link_id,
        resource_hash,
    });

    let cancel_result = daemon
        .handle_rpc(RpcRequest {
            id: 902,
            method: "sdk_cancel_message_v2".to_string(),
            params: Some(json!({ "message_id": message_id })),
        })
        .expect("cancel message");
    assert_eq!(cancel_result.result.expect("cancel result")["result"], json!("Accepted"));

    let cancel = tokio::time::timeout(Duration::from_secs(1), channel.tx_channel.recv())
        .await
        .expect("cancel tx")
        .expect("cancel message");
    assert_eq!(cancel.packet.context, PacketContext::ResourceInitiatorCancel);
    assert!(outbound_resource_map.lock().expect("map").get(&resource_hash_hex).is_none());
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
            fields: Some(json!({"app": "value"})),
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
