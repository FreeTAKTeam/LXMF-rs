use lxmf::error::LxmfError;
use lxmf::message::{Payload, WireMessage};
use lxmf::propagation::{PropagationNode, VerificationMode, Verifier};
use lxmf::storage::FileStore;
use reticulum::identity::PrivateIdentity;

struct AlwaysInvalid;

impl Verifier for AlwaysInvalid {
    fn verify(&self, _message: &WireMessage) -> Result<bool, LxmfError> {
        Ok(false)
    }
}

#[test]
fn strict_rejects_unsigned_messages() {
    let dir = tempfile::tempdir().unwrap();
    let store = FileStore::new(dir.path());
    let mut node = PropagationNode::new(Box::new(store));

    let msg = WireMessage::new(
        [10u8; 16],
        [11u8; 16],
        Payload::new(1.0, Some("hi".into()), None, None),
    );

    let err = node.store(msg).unwrap_err();
    assert!(matches!(err, LxmfError::Verify(_)));
}

#[test]
fn strict_rejects_invalid_signature_when_verifier_present() {
    let dir = tempfile::tempdir().unwrap();
    let store = FileStore::new(dir.path());
    let verifier = Box::new(AlwaysInvalid);
    let mut node = PropagationNode::with_verifier(Box::new(store), VerificationMode::Strict, verifier);

    let mut msg = WireMessage::new(
        [12u8; 16],
        [13u8; 16],
        Payload::new(1.0, Some("hi".into()), None, None),
    );
    let signer = PrivateIdentity::new_from_name("lxmf-verify");
    msg.sign(&signer).unwrap();

    let err = node.store(msg).unwrap_err();
    assert!(matches!(err, LxmfError::Verify(_)));
}
