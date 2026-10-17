#[tokio::test(start_paused = true)]
async fn identity_path_wait_respects_slow_interface_round_trip() {
    assert_identity_path_wait(100, 86).await;
}

#[tokio::test(start_paused = true)]
async fn identity_path_wait_uses_normal_medium_floor() {
    assert_identity_path_wait(100000, 7).await;
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
