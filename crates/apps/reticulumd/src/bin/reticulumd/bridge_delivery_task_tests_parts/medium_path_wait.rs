#[tokio::test(start_paused = true)]
async fn identity_path_wait_respects_slow_interface_round_trip() {
    assert_identity_path_wait(100, 86).await;
}

#[tokio::test(start_paused = true)]
async fn identity_path_wait_uses_normal_medium_floor() {
    assert_identity_path_wait(100000, 7).await;
}

#[tokio::test(start_paused = true)]
async fn concurrent_identity_lookups_dispatch_one_path_request() {
    let mut task = delivery_task_for_propagation_cost_lookup(Arc::new(RpcDaemon::test_instance()));
    task.requested_method = RequestedDeliveryMethod::Direct;
    let mut channel = task.transport.iface_manager().lock().await.new_channel(16);
    let mut second = delivery_task_for_propagation_cost_lookup(task.daemon.clone());
    second.message_id = "second-discovery-owner".into();
    second.requested_method = RequestedDeliveryMethod::Direct;
    second.transport = task.transport.clone();
    second.path_requests = task.path_requests.clone();
    let (stop1, first) = start_discovery(task);
    let (stop2, second) = start_discovery(second);
    channel.tx_channel.recv().await.expect("first request");
    for _ in 0..8 {
        tokio::task::yield_now().await;
    }
    assert!(channel.tx_channel.try_recv().is_err(), "duplicate path request");
    stop1.send_replace(true);
    stop2.send_replace(true);
    first.await.expect("first shutdown");
    second.await.expect("second shutdown");
}

async fn assert_identity_path_wait(bitrate: u64, window_seconds: u64) {
    let daemon = Arc::new(RpcDaemon::with_store(
        MessagesStore::in_memory().expect("store"),
        "slow-path-wait".into(),
    ));
    let mut task = delivery_task_for_propagation_cost_lookup(daemon);
    task.requested_method = RequestedDeliveryMethod::Direct;
    let mut channel = task.transport.iface_manager().lock().await.new_channel(16);
    assert!(task.transport.iface_manager().lock().await.set_announce_pacing(
        *channel.address(),
        bitrate,
        100
    ));
    let (stop, owner) = start_discovery(task);
    channel.tx_channel.recv().await.expect("path request dispatched");
    tokio::time::advance(Duration::from_secs(window_seconds - 1)).await;
    tokio::task::yield_now().await;
    assert!(!owner.is_finished(), "discovery must retain its measured medium window");
    tokio::time::advance(Duration::from_secs(1)).await;
    for _ in 0..4 {
        tokio::task::yield_now().await;
    }
    assert!(!owner.is_finished(), "the admitted owner retains the unresolved delivery");
    assert!(channel.tx_channel.try_recv().is_err(), "strict deadline is not due at equality");
    tokio::time::advance(Duration::from_millis(1)).await;
    for _ in 0..4 { tokio::task::yield_now().await; }
    channel.tx_channel.try_recv().expect("next measured discovery request");
    stop.send_replace(true);
    owner.await.expect("discovery shutdown");
}

#[tokio::test(start_paused = true)]
async fn propagation_cost_discovery_uses_measured_path_window() {
    let task = delivery_task_for_propagation_cost_lookup(Arc::new(RpcDaemon::test_instance()));
    let mut channel = task.transport.iface_manager().lock().await.new_channel(16);
    assert!(task.transport.iface_manager().lock().await.set_announce_pacing(
        *channel.address(), 100, 100
    ));
    let node = AddressHash::new([7; 16]);
    let owner = tokio::spawn(async move {
        task.propagation_target_cost_reference_style(&hex::encode(node.as_slice()), node).await
    });
    channel.tx_channel.recv().await.expect("cost path request");
    tokio::time::advance(Duration::from_secs(85)).await;
    tokio::task::yield_now().await;
    assert!(!owner.is_finished(), "slow medium exceeds the former fixed twelve seconds");
    tokio::time::advance(Duration::from_secs(1)).await;
    for _ in 0..4 { tokio::task::yield_now().await; }
    assert!(owner.is_finished(), "measured 86-second deadline");
    assert_eq!(owner.await.expect("owner").expect("lookup"), (None, "unavailable"));
}

#[tokio::test(start_paused = true)]
async fn slow_propagation_cost_discovery_stops_promptly_on_cancellation() {
    let store = MessagesStore::in_memory().expect("store");
    let message_id = "propagation-cost-lookup-message";
    store.insert_message(&MessageRecord {
        id: message_id.into(), source: "source".into(), destination: hex::encode([0; 16]),
        title: String::new(), content: String::new(), timestamp: 0, direction: "out".into(),
        fields: None, receipt_status: Some("queued".into()),
    }).expect("message");
    let daemon = Arc::new(RpcDaemon::with_store(store, "cancel-cost".into()));
    let task = Arc::new(delivery_task_for_propagation_cost_lookup(daemon.clone()));
    let mut channel = task.transport.iface_manager().lock().await.new_channel(16);
    assert!(task.transport.iface_manager().lock().await.set_announce_pacing(
        *channel.address(), 100, 100
    ));
    let node = AddressHash::new([7; 16]);
    let attempt = task.clone();
    let owner = tokio::spawn(async move {
        attempt.propagation_target_cost_reference_style(&hex::encode(node.as_slice()), node).await
    });
    channel.tx_channel.recv().await.expect("initial path request");
    daemon.record_network_receipt(message_id, "cancelled", serde_json::Map::new()).expect("cancel");
    tokio::time::advance(Duration::from_millis(250)).await;
    for _ in 0..4 { tokio::task::yield_now().await; }
    assert!(owner.is_finished(), "cancellation must not retain an 86-second discovery wait");
    assert_eq!(owner.await.expect("owner"), Err(crate::bridge::delivery_attempt::DeliveryAttemptOutcome::Stopped));
    assert_eq!(task.propagation_target_cost_reference_style(&hex::encode(node.as_slice()), node).await,
        Err(crate::bridge::delivery_attempt::DeliveryAttemptOutcome::Stopped));
    assert!(channel.tx_channel.try_recv().is_err(), "cancelled task must not request again");
}

#[tokio::test(start_paused = true)]
async fn direct_and_propagated_discovery_fail_at_exact_reference_budget() {
    for method in [RequestedDeliveryMethod::Direct, RequestedDeliveryMethod::Propagated] {
        for (bitrate, seconds, requests) in [(100000, 7, 4), (100, 86, 2)] {
            let mut task = delivery_task_for_propagation_cost_lookup(Arc::new(RpcDaemon::test_instance()));
            task.requested_method = method;
            let mut channel = task.transport.iface_manager().lock().await.new_channel(16);
            assert!(task.transport.iface_manager().lock().await.set_announce_pacing(*channel.address(), bitrate, 100));
            let (tx, mut receipts) = tokio::sync::mpsc::channel(16);
            task.receipt_tx = tx.into();
            let (_stop, owner) = start_discovery(task);
            channel.tx_channel.recv().await.expect("initial request");
            for request in 1..=requests {
                tokio::time::advance(Duration::from_secs(seconds)).await;
                for _ in 0..8 { tokio::task::yield_now().await; }
                assert!(!owner.is_finished(), "strict deadline equality");
                assert!(channel.tx_channel.try_recv().is_err());
                tokio::time::advance(Duration::from_millis(1)).await;
                for _ in 0..8 { tokio::task::yield_now().await; }
                if request < requests {
                    channel.tx_channel.try_recv().expect("next permitted discovery request");
                    assert!(!owner.is_finished());
                } else {
                    assert!(owner.is_finished(), "reference attempt budget exhausted");
                    assert!(channel.tx_channel.try_recv().is_err(), "exhaustion must not send one extra request");
                }
            }
            owner.await.expect("owner");
            let terminal = receipts.recv().await.expect("terminal publication");
            assert!(terminal.status.starts_with("failed: Link delivery attempts exhausted"));
        }
    }
}

fn start_discovery(mut task: DeliveryTask) -> (tokio::sync::watch::Sender<bool>, tokio::task::JoinHandle<()>) {
    if task.requested_method == RequestedDeliveryMethod::Propagated {
        task.propagation_node_hex = Some(task.destination_hex.clone());
    }
    start_prepared(task, PreparedDeliveryPayload { lxmf_payload: vec![0; 128], propagation: None })
}
