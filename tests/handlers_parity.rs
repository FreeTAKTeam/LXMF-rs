use lxmf::router::Router;

#[test]
fn delivery_announce_handler_updates_router_state() {
    let mut router = Router::default();
    router.set_auth_required(true);

    let mut handler = lxmf::handlers::DeliveryAnnounceHandler::new();
    let destination = [0x10; 16];
    assert!(!router.is_destination_allowed(&destination));

    handler
        .handle_with_router(&mut router, &destination)
        .expect("delivery announce");

    assert!(router.is_identity_registered(&destination));
    assert!(router.is_destination_allowed(&destination));
    assert_eq!(router.peer_count(), 1);
    assert!(router.peer(&destination).unwrap().last_seen().is_some());
}

#[test]
fn propagation_announce_handler_parses_and_updates_router_state() {
    let fixture = std::fs::read("tests/fixtures/python/lxmf/propagation_node_app_data_custom.bin")
        .expect("custom propagation fixture");
    let mut router = Router::default();
    router.set_auth_required(true);

    let mut handler = lxmf::handlers::PropagationAnnounceHandler::new();
    let destination = [0x11; 16];
    let event = handler
        .handle_with_router(&mut router, &destination, &fixture)
        .expect("propagation announce");

    assert_eq!(event.destination, destination);
    assert_eq!(event.name.as_deref(), Some("TestNode"));
    assert_eq!(event.stamp_cost, Some(20));

    assert!(router.is_identity_registered(&destination));
    assert_eq!(router.identity_name(&destination), Some("TestNode"));
    assert!(router.is_destination_allowed(&destination));
    assert!(router.is_destination_prioritised(&destination));
    assert_eq!(router.peer_count(), 1);

    let peer = router.peer(&destination).expect("peer state");
    assert_eq!(peer.name(), Some("TestNode"));
    assert!(peer.last_seen().is_some());
}
