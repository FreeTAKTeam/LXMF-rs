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
    let first = tokio::spawn(async move { task.resolve_destination_identity().await });
    let second = tokio::spawn(async move { second.resolve_destination_identity().await });
    channel.tx_channel.recv().await.expect("first request");
    for _ in 0..8 {
        tokio::task::yield_now().await;
    }
    assert!(channel.tx_channel.try_recv().is_err(), "duplicate path request");
    first.abort();
    second.abort();
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
    let owner = tokio::spawn(async move { task.resolve_destination_identity().await });
    channel.tx_channel.recv().await.expect("path request dispatched");
    tokio::time::advance(Duration::from_secs(window_seconds - 1)).await;
    tokio::task::yield_now().await;
    assert!(!owner.is_finished(), "discovery must retain its measured medium window");
    tokio::time::advance(Duration::from_secs(1)).await;
    for _ in 0..4 {
        tokio::task::yield_now().await;
    }
    assert!(owner.is_finished(), "resolver must finish at its measured deadline");
    assert!(matches!(owner.await.expect("resolver"),
        Err(crate::bridge::delivery_attempt::DeliveryAttemptOutcome::AwaitPath { .. })));
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
