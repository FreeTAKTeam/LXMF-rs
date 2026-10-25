use lxmf_core::{message::Payload, WireMessage};
use lxmf_propagation::{
    prepared::delivery_address, MessageKind, OwnedStore, PreparedMessage, StoreLimits,
};
use rns_core::identity::PrivateIdentity;

#[test]
fn existing_application_record_keeps_its_kind_and_signed_wire_after_reopen() {
    let dir = tempfile::tempdir().expect("test directory");
    let path = dir.path().join("custody.db");
    let owner = PrivateIdentity::new_from_name("application-owner");
    let recipient = PrivateIdentity::new_from_name("application-recipient");
    let limits = StoreLimits { capacity_bytes: 10_000, retention_secs: 100 };
    let mut wire = WireMessage::new(
        delivery_address(recipient.as_identity()),
        delivery_address(owner.as_identity()),
        Payload::new(10.0, Some(b"application payload".to_vec()), None, None, None),
    );
    wire.sign(&owner).expect("signed fixture");
    let prepared = PreparedMessage::new(&wire, &owner, Some(recipient.as_identity()))
        .expect("prepared fixture");
    let mut store = OwnedStore::open(&path, owner.clone(), limits).expect("open store");
    let admitted = store.deposit_own(prepared, MessageKind::Chat, 10, None).expect("admit fixture");
    drop(store);

    // Seed the existing persisted discriminator independently of the new API.
    let conn = rusqlite::Connection::open(&path).expect("open fixture database");
    conn.execute("UPDATE propagation_owned SET kind=1", []).expect("seed application record");
    drop(conn);

    let mut store = OwnedStore::open(&path, owner, limits).expect("reopen existing store");
    let status = store.message(&admitted.message_id).expect("read status").expect("stored message");
    assert_eq!(status.kind, MessageKind::Application);
    assert_eq!(status.state, admitted.state);
    assert_eq!(status.expires_at, admitted.expires_at);
    let recovery = store
        .recover(&admitted.transient_id, 11, 20, |_| true)
        .expect("recover existing application record")
        .expect("eligible recovery");
    assert_eq!(recovery.direct_wire, wire.pack().expect("original wire"));
}
