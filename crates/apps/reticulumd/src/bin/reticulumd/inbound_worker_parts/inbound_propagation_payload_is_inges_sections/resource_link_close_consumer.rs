#[tokio::test]
async fn production_daemon_consumer_reports_link_close_as_failure_not_timeout() {
    let message_id = "daemon-resource-link-close-consumer";
    let mut peer = start_daemon_resource_peer(message_id, ResourcePeerMode::HoldResourceRequests)
        .await;
    let resource_hash = peer
        .transport
        .send_resource(&peer.link_id, vec![0x91; 128], None)
        .await
        .expect("send Resource over active daemon Link");
    let resource_hash_hex = hex::encode(resource_hash.as_slice());
    track_message_resource(&peer, message_id, resource_hash);

    tokio::time::timeout(std::time::Duration::from_secs(5), peer.resource_request_rx.recv())
        .await
        .expect("peer receives the Resource request")
        .expect("held request notification");
    let peer_link = peer
        .peer_transport
        .find_in_link(&peer.link_id)
        .await
        .expect("peer retains the active Link");
    let teardown = peer_link.lock().await.teardown();
    peer.peer_transport
        .send_packet(teardown.expect("active Link produces teardown packet"))
        .await;

    let receipt = tokio::time::timeout(std::time::Duration::from_secs(5), peer.receipt_rx.recv())
        .await
        .expect("daemon observes terminal Link-close failure")
        .expect("receipt channel remains open");
    assert_eq!(receipt.message_id, message_id);
    assert_eq!(receipt.status, "failed: resource transfer failed");
    assert_eq!(receipt.delivery_kind.as_deref(), Some("resource-failed"));
    assert_eq!(receipt.resource_hash.as_deref(), Some(resource_hash_hex.as_str()));
    assert!(peer.resource_map.lock().expect("resource map").is_empty());

    persist_receipt_update(
        peer.daemon.as_ref(),
        receipt,
        &std::sync::Arc::new(std::sync::Mutex::new(std::collections::HashMap::new())),
        &peer.resource_map,
    )
    .expect("persist generic Resource failure status");
    assert_eq!(
        peer.daemon.message_receipt_status(message_id).expect("receipt status"),
        Some("failed: resource transfer failed".to_string())
    );
    assert!(matches!(
        peer.receipt_rx.try_recv(),
        Err(tokio::sync::mpsc::error::TryRecvError::Empty)
    ));
}
