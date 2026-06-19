use super::super::*;
use super::*;
use rand_core::OsRng;
use rns_core::identity::PrivateIdentity;
use rns_rpc::{MessageRecord, MessagesStore, RpcDaemon, RpcRequest};
use rns_transport::identity_bridge::to_transport_private_identity;
use rns_transport::transport::{Transport, TransportConfig};

#[test]
fn scheduler_metrics_record_admission_and_queue_pressure() {
    let metrics = DeliverySchedulerMetrics::default();

    metrics.record_admitted_for_peer("peer-a");
    metrics.record_admitted_for_peer("peer-a");
    metrics.record_queue_full();

    let snapshot = metrics.snapshot();
    assert_eq!(snapshot.accepted_total, 2);
    assert_eq!(snapshot.rejected_queue_full_total, 1);
    assert_eq!(snapshot.queued_current, 2);
}

#[test]
fn scheduler_metrics_track_deferred_stamp_worker_ownership() {
    let metrics = DeliverySchedulerMetrics::default();

    metrics.record_stamp_queued_for_peer("peer-a");
    metrics.record_stamp_started_for_peer("peer-a");
    metrics.record_stamp_retry_for_peer("peer-a");
    metrics.record_stamp_completed_for_peer("peer-a");
    metrics.record_stamp_started_for_peer("peer-a");
    metrics.record_stamp_completed_for_peer("peer-a");
    metrics.record_stamp_queued_for_peer("peer-a");
    metrics.record_stamp_unqueued_for_peer("peer-a");

    let snapshot = metrics.snapshot();
    assert_eq!(snapshot.stamp_queued_current, 0);
    assert_eq!(snapshot.stamp_in_flight_current, 0);
    assert_eq!(snapshot.stamp_retried_total, 1);
    assert_eq!(snapshot.stamp_completed_total, 2);
}

#[tokio::test]
async fn prepare_payload_builds_propagation_stamp_before_delivery_lane() {
    let message_id = "deferred-propagation-prepare";
    let store = MessagesStore::in_memory().expect("store");
    let destination_identity = rns_transport::identity::PrivateIdentity::new_from_rand(OsRng);
    let destination_hash = destination_identity.as_identity().address_hash;
    let destination_hex = hex::encode(destination_hash.as_slice());
    store
        .insert_message(&MessageRecord {
            id: message_id.to_string(),
            source: "source".to_string(),
            destination: destination_hex.clone(),
            title: String::new(),
            content: "propagated before delivery".to_string(),
            timestamp: 0,
            direction: "out".to_string(),
            fields: None,
            receipt_status: Some("queued".to_string()),
        })
        .expect("insert message");
    let daemon = Arc::new(RpcDaemon::with_store(store, "deferred-propagation-node".to_string()));
    let local_propagation_hash = hex::encode([0x77u8; 16]);
    daemon.set_propagation_destination_hash(Some(local_propagation_hash.clone()));
    daemon
        .handle_rpc(RpcRequest {
            id: 901,
            method: "propagation_enable".to_string(),
            params: Some(json!({
                "enabled": true,
                "target_cost": 0,
                "autopeer": true,
            })),
        })
        .expect("enable propagation");
    let app_data = rmp_serde::to_vec_named(&rmpv::Value::Array(vec![
        rmpv::Value::Boolean(false),
        rmpv::Value::from(1_700_000_901i64),
        rmpv::Value::Boolean(true),
        rmpv::Value::from(256),
        rmpv::Value::from(2048),
        rmpv::Value::Array(vec![rmpv::Value::from(0), rmpv::Value::from(0), rmpv::Value::from(0)]),
        rmpv::Value::Map(Vec::new()),
    ]))
    .expect("encode app data");
    daemon
        .handle_rpc(RpcRequest {
            id: 902,
            method: "announce_received".to_string(),
            params: Some(json!({
                "peer": local_propagation_hash,
                "timestamp": 1_700_000_901i64,
                "app_data_hex": hex::encode(app_data),
                "aspect": "lxmf.propagation",
                "hops": 0,
            })),
        })
        .expect("local propagation announce");
    daemon
        .handle_rpc(RpcRequest {
            id: 903,
            method: "set_outbound_propagation_node".to_string(),
            params: Some(json!({ "peer": local_propagation_hash.clone() })),
        })
        .expect("select local propagation node");

    let signer = PrivateIdentity::new_from_name("deferred-propagation-prepare");
    let transport_identity = to_transport_private_identity(&signer);
    let transport = Arc::new(Transport::new(TransportConfig::new(
        "deferred-propagation-prepare",
        &transport_identity,
        true,
    )));
    let (receipt_tx, _receipt_rx) = tokio::sync::mpsc::channel(16);
    let mut destination = [0u8; 16];
    destination.copy_from_slice(destination_hash.as_slice());
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
        destination_hash,
        destination_hex: destination_hex.clone(),
        title: String::new(),
        content: "propagated before delivery".to_string(),
        fields: None,
        signer,
        stamp_cost: None,
        outbound_ticket: None,
        include_ticket: None,
        peer_identity: Some(*destination_identity.as_identity()),
        propagation_node_identity: Some(*transport_identity.as_identity()),
        requested_method: RequestedDeliveryMethod::Propagated,
        try_propagation_on_fail: false,
        propagation_node_hex: Some(local_propagation_hash),
    };
    let metrics = Arc::new(DeliverySchedulerMetrics::default());
    let stamp_limit = Arc::new(Semaphore::new(1));
    task.record_deferred_stamp_queued_metadata();
    metrics.record_stamp_queued_for_peer(&destination_hex);

    let _payload = prepare_payload(&task, &stamp_limit, &metrics, &destination_hex)
        .await
        .expect("prepared propagated payload");

    let result = daemon
        .handle_rpc(RpcRequest { id: 904, method: "list_messages".to_string(), params: None })
        .expect("list messages")
        .result
        .expect("result");
    let message = result["messages"]
        .as_array()
        .expect("messages")
        .iter()
        .find(|message| message["id"].as_str() == Some(message_id))
        .expect("message");
    let lxmf = &message["fields"]["_lxmf"];
    assert_eq!(lxmf["propagation_stamp_state"], json!("ready"));
    assert_eq!(lxmf["propagation_packed"], json!(true));
    assert!(lxmf["propagation_packed_size"].as_u64().unwrap_or_default() > 0);
    let snapshot = metrics.snapshot();
    assert_eq!(snapshot.stamp_queued_current, 0);
    assert_eq!(snapshot.stamp_in_flight_current, 0);
    assert_eq!(snapshot.in_flight_current, 0);
}
