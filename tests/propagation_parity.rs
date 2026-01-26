use std::path::Path;

use lxmf::message::{Payload, WireMessage};
use lxmf::propagation::{NoopVerifier, PropagationNode};
use lxmf::storage::FileStore;

#[test]
fn strict_mode_requires_signature() {
    let store = Box::new(FileStore::new(Path::new("/tmp")));
    let verifier = Box::new(NoopVerifier);
    let mut node = PropagationNode::new_strict(store, verifier);
    let msg = WireMessage::new([0u8; 16], [0u8; 16], Payload::new(0.0, None, None, None));
    assert!(node.store(msg).is_err());
}
