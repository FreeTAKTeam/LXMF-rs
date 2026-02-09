#[test]
fn top_level_api_reexports_are_usable() {
    let payload = lxmf::Payload::new(1_700_000_000.0, Some(b"hello".to_vec()), None, None, None);

    let destination = [0x11; 16];
    let source = [0x22; 16];
    let wire = lxmf::WireMessage::new(destination, source, payload);

    let mut router = lxmf::Router::default();
    router.enqueue_outbound(wire.clone());
    assert_eq!(router.outbound_len(), 1);

    let temp = tempfile::tempdir().unwrap();
    let store = lxmf::storage::FileStore::new(temp.path());
    let mut node = lxmf::PropagationNode::new(Box::new(store));
    node.store(wire.clone()).unwrap();

    let id = wire.message_id();
    let fetched = node.fetch(&id).unwrap();
    assert_eq!(fetched.destination, destination);
    assert_eq!(fetched.source, source);
}
