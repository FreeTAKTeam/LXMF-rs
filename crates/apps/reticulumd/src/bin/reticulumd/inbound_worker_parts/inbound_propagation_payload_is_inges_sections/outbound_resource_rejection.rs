#[test]
fn outbound_resource_rejection_emits_terminal_receipt_and_clears_tracking_once() {
    let resource_hash = Hash::new_from_slice(&[0x53; 32]);
    let resource_hash_hex = hex::encode(resource_hash.as_slice());
    let map = Arc::new(Mutex::new(HashMap::new()));
    super::super::outbound_resources::track_outbound_resource(
        &map,
        resource_hash_hex.clone(),
        super::super::outbound_resources::OutboundResourceTracking {
            message_id: "resource-rejected-message".to_string(),
            peer: "peer-resource-rejected".to_string(),
            bytes: 512,
            sent_status: "sent: link resource".to_string(),
        },
    );
    let (tx, mut rx) = tokio::sync::mpsc::channel(2);

    super::handle_outbound_resource_rejection(&map, &tx, &resource_hash);
    super::handle_outbound_resource_rejection(&map, &tx, &resource_hash);

    assert!(super::super::outbound_resources::take_outbound_resource_tracking(
        &map,
        resource_hash_hex.as_str()
    )
    .is_err());
    let event = rx.try_recv().expect("rejected receipt event");
    assert_eq!(event.message_id, "resource-rejected-message");
    assert_eq!(event.status, "rejected");
    assert_eq!(event.resource_hash.as_deref(), Some(resource_hash_hex.as_str()));
    assert_eq!(event.peer.as_deref(), Some("peer-resource-rejected"));
    assert_eq!(event.delivery_kind.as_deref(), Some("resource-rejected"));
    assert_eq!(event.bytes, Some(512));
    assert!(matches!(
        rx.try_recv(),
        Err(tokio::sync::mpsc::error::TryRecvError::Empty)
    ));
}
