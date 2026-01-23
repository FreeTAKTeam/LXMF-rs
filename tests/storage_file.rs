use lxmf::message::{Payload, WireMessage};
use lxmf::storage::{FileStore, Store};
use reticulum::identity::PrivateIdentity;

#[test]
fn file_store_can_save_and_load() {
    let dir = tempfile::tempdir().unwrap();
    let store = FileStore::new(dir.path());
    let mut msg = WireMessage::new(
        [1u8; 16],
        [2u8; 16],
        Payload::new(1.0, Some("hi".into()), None, None),
    );
    let signer = PrivateIdentity::new_from_name("lxmf-store");
    msg.sign(&signer).unwrap();

    store.save(&msg).unwrap();
    let loaded = store.get(&msg.message_id()).unwrap();
    assert_eq!(loaded.source, msg.source);
    assert!(loaded.signature.is_some());
}

#[test]
fn file_store_accepts_unsigned_messages() {
    let dir = tempfile::tempdir().unwrap();
    let store = FileStore::new(dir.path());
    let msg = WireMessage::new(
        [3u8; 16],
        [4u8; 16],
        Payload::new(2.0, Some("unsigned".into()), None, None),
    );

    store.save(&msg).unwrap();
    let loaded = store.get(&msg.message_id()).unwrap();
    assert_eq!(loaded.source, msg.source);
    assert!(loaded.signature.is_none());
}
