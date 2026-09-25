use rns_transport::resource::ResourceEventKind;

#[tokio::test]
async fn daemon_observes_inbound_retry_timeout_without_completion_and_recovers_on_link() {
    let message_id = "daemon-inbound-resource-retry-timeout";
    let mut peer =
        start_daemon_resource_peer(message_id, ResourcePeerMode::DropInboundResourceParts).await;
    let mut resource_events = peer.transport.resource_events();
    let failed_payload = vec![0x6D; 4096];
    let failed_hash = peer
        .peer_transport
        .send_resource(&peer.link_id, failed_payload, None)
        .await
        .expect("peer starts inbound Resource on active Link");

    let failure = tokio::time::timeout(std::time::Duration::from_secs(8), async {
        loop {
            match resource_events.recv().await {
                Ok(event) if event.hash == failed_hash => match event.kind {
                    ResourceEventKind::InboundFailed(failure) => break failure,
                    ResourceEventKind::Complete(_) => {
                        panic!("missing inbound Resource fragments must not complete")
                    }
                    _ => {}
                },
                Ok(_) | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                    panic!("Resource event stream closed before retry exhaustion")
                }
            }
        }
    })
    .await
    .expect("daemon observes receiver retry exhaustion");
    assert_eq!(failure.reason, "retry_limit_exhausted");
    assert_eq!(failure.progress.received_parts, 0);
    assert!(matches!(
        resource_events.try_recv(),
        Err(tokio::sync::broadcast::error::TryRecvError::Empty)
    ));
    assert!(matches!(
        peer.receipt_rx.try_recv(),
        Err(tokio::sync::mpsc::error::TryRecvError::Empty)
    ));

    assert!(peer
        .peer_transport
        .cancel_resource(&peer.link_id, failed_hash)
        .await
        .expect("cleanup peer-side timed-out send"));
    peer.drop_inbound_resource_parts
        .store(false, std::sync::atomic::Ordering::SeqCst);
    let recovery_payload = b"complete retry after inbound timeout".to_vec();
    let recovery_hash = peer
        .peer_transport
        .send_resource(&peer.link_id, recovery_payload.clone(), None)
        .await
        .expect("same Link accepts a new Resource after cleanup");
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            match resource_events.recv().await {
                Ok(event) if event.hash == recovery_hash => match event.kind {
                    ResourceEventKind::Complete(complete) => {
                        assert_eq!(complete.data, recovery_payload);
                        break;
                    }
                    ResourceEventKind::InboundFailed(failure) => {
                        panic!("post-timeout Resource failed: {}", failure.reason)
                    }
                    _ => {}
                },
                Ok(_) | Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                    panic!("Resource event stream closed before recovery completion")
                }
            }
        }
    })
    .await
    .expect("new Resource completes on the same Link after failure cleanup");
}
