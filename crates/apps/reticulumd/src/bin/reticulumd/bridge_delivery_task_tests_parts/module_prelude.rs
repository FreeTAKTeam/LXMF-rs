use crate::bridge::delivery_scheduler::start_prepared;
use super::*;

use crate::bridge::delivery_task::PreparedDeliveryPayload;

use rand_core::OsRng;

use rns_rpc::{MessageRecord, MessagesStore};

use rns_transport::transport::TransportConfig;

pub(in crate::bridge) fn delivery_task_for_propagation_cost_lookup(daemon: Arc<RpcDaemon>) -> DeliveryTask {
    let signer = PrivateIdentity::new_from_name("propagation-cost-lookup-task");
    let transport_identity = rns_transport::identity_bridge::to_transport_private_identity(&signer);
    let transport = Arc::new(Transport::new(TransportConfig::new(
        "propagation-cost-lookup-task",
        &transport_identity,
        true,
    )));
    let (receipt_tx, _receipt_rx) = tokio::sync::mpsc::channel(16);
    let destination = [0u8; 16];
    DeliveryTask {
        stopping: None,
        wakeup: Default::default(),
        path_requests: Default::default(),
        deferred_dispatch: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        daemon,
        transport,
        peer_crypto: Arc::new(Mutex::new(HashMap::new())),
        outbound_propagation_identities: Arc::new(Mutex::new(HashMap::new())),
        receipt_map: Arc::new(Mutex::new(HashMap::new())),
        outbound_resource_map: Arc::new(Mutex::new(HashMap::new())),
        outbound_propagation_link: Arc::new(tokio::sync::Mutex::new(None)),
        direct_backchannel_links: DirectBackchannelLinks::new(),
        receipt_tx: receipt_tx.into(),
        message_id: "propagation-cost-lookup-message".to_string(),
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
        propagation_node_hex: None,
    }
}

#[test]
fn cancelled_status_detection_is_case_and_space_tolerant() {
    assert!(DeliveryTask::is_cancelled_status(Some("cancelled")));
    assert!(DeliveryTask::is_cancelled_status(Some("  CANCELLED  ")));
    assert!(!DeliveryTask::is_cancelled_status(Some("sending")));
    assert!(!DeliveryTask::is_cancelled_status(Some("sent: link")));
    assert!(!DeliveryTask::is_cancelled_status(None));
}

#[tokio::test]
async fn terminal_observation_reads_persisted_daemon_status() {
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
        stopping: None,
        wakeup: Default::default(),
        path_requests: Default::default(),
        deferred_dispatch: Arc::new(std::sync::atomic::AtomicBool::new(false)),
        daemon,
        transport,
        peer_crypto: Arc::new(Mutex::new(HashMap::new())),
        outbound_propagation_identities: Arc::new(Mutex::new(HashMap::new())),
        receipt_map: Arc::new(Mutex::new(HashMap::new())),
        outbound_resource_map: Arc::new(Mutex::new(HashMap::new())),
        outbound_propagation_link: Arc::new(tokio::sync::Mutex::new(None)),
        direct_backchannel_links: DirectBackchannelLinks::new(),
        receipt_tx: receipt_tx.into(),
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

    assert!(task.observe_delivery_terminal().expect("terminal status"));
}
