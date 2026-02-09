use lxmf::router::TransferPhase;

#[test]
fn peer_sync_batch_respects_config_limit_and_creates_transfers() {
    let mut router = lxmf::router::Router::default();
    let destination = [0xAA; 16];

    router.register_peer(destination);
    router.set_propagation_limits(1, 2);
    router.queue_peer_unhandled(destination, b"id-1");
    router.queue_peer_unhandled(destination, b"id-2");
    router.queue_peer_unhandled(destination, b"id-3");

    let batch = router.build_peer_sync_batch(&destination, 10);
    assert_eq!(batch.len(), 2);
    assert_eq!(router.stats().peer_sync_runs_total, 1);
    assert_eq!(router.stats().peer_sync_items_total, 2);

    for transient_id in &batch {
        let state = router.propagation_transfer_state(transient_id).unwrap();
        assert_eq!(state.phase, TransferPhase::Requested);
        assert_eq!(state.progress, 0);
    }
}

#[test]
fn apply_peer_sync_result_updates_peer_and_transfer_state() {
    let mut router = lxmf::router::Router::default();
    let destination = [0xBB; 16];

    router.register_peer(destination);
    router.queue_peer_unhandled(destination, b"good-id");
    router.queue_peer_unhandled(destination, b"bad-id");
    let batch = router.build_peer_sync_batch(&destination, 8);
    assert_eq!(batch.len(), 2);

    let delivered = vec![b"good-id".to_vec()];
    let rejected = vec![b"bad-id".to_vec()];
    assert!(router.apply_peer_sync_result(&destination, &delivered, &rejected));

    let peer = router.peer(&destination).unwrap();
    assert_eq!(peer.handled_message_count(), 1);
    assert_eq!(peer.unhandled_message_count(), 1);
    assert_eq!(peer.sync_backoff(), 5);
    assert_eq!(router.stats().peer_sync_rejected_total, 1);

    assert_eq!(
        router.propagation_transfer_state(b"good-id").unwrap().phase,
        TransferPhase::Completed
    );
    assert_eq!(
        router.propagation_transfer_state(b"bad-id").unwrap().phase,
        TransferPhase::Cancelled
    );
}
