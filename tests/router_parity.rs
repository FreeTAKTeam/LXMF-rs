#[test]
fn outbound_queue_dequeues() {
    let mut router = lxmf::router::Router::default();
    let msg = lxmf::message::WireMessage::new(
        [0u8; 16],
        [1u8; 16],
        lxmf::message::Payload::new(0.0, None, None, None, None),
    );
    router.enqueue_outbound(msg);
    let dequeued = router.dequeue_outbound().expect("expected message");
    assert_eq!(dequeued.destination, [0u8; 16]);
    assert_eq!(router.outbound_len(), 0);
}
