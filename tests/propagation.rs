use lxmf::message::{Payload, WireMessage};
use lxmf::propagation::PropagationNode;
use lxmf::storage::FileStore;
use reticulum::identity::PrivateIdentity;

#[test]
fn propagation_store_and_fetch() {
    let dir = tempfile::tempdir().unwrap();
    let store = FileStore::new(dir.path());
    let mut node = PropagationNode::new(Box::new(store));

    let mut msg = WireMessage::new(
        [7u8; 16],
        [8u8; 16],
        Payload::new(1.0, Some("hi".into()), None, None),
    );
    let signer = PrivateIdentity::new_from_name("lxmf-prop");
    msg.sign(&signer).unwrap();

    node.store(msg.clone()).unwrap();
    let fetched = node.fetch(&msg.message_id()).unwrap();
    assert_eq!(fetched.source, msg.source);
    assert!(fetched.signature.is_some());
}
