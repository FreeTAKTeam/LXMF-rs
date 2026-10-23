use super::*;
use lxmf_core::{message::Payload, WireMessage};
use rns_core::identity::PrivateIdentity;
use rusqlite::params;

fn prepared(
    owner: &PrivateIdentity,
    recipient: &PrivateIdentity,
    sequence: u8,
    content_len: usize,
) -> PreparedMessage {
    let mut w = WireMessage::new(
        prepared::delivery_address(recipient.as_identity()),
        prepared::delivery_address(owner.as_identity()),
        Payload::new(sequence as f64, Some(vec![sequence; content_len]), None, None, None),
    );
    w.sign(owner).unwrap();
    PreparedMessage::new(&w, owner, Some(recipient.as_identity())).unwrap()
}

#[test]
fn saturated_status_metadata_preserves_work_and_notices_until_projected_retention_ends() {
    let dir = tempfile::tempdir().unwrap();
    let owner = PrivateIdentity::new_from_name("owner");
    let recipient = PrivateIdentity::new_from_name("recipient");
    let mut store = OwnedStore::open(
        dir.path().join("outbox.db"),
        owner.clone(),
        StoreLimits { capacity_bytes: 10000, retention_secs: 100 },
    )
    .unwrap();
    let first = store
        .deposit_own(prepared(&owner, &recipient, 1, 32), MessageKind::Chat, 10, None)
        .unwrap();
    let tx = store.conn.unchecked_transaction().unwrap();
    for i in 0..MAX_STATUS - 1 {
        let id = format!("{i:064x}");
        tx.execute("INSERT INTO propagation_owned(transient_id,message_id,destination,admitted_at,expires_at,encrypted_bytes,kind,state,changed_at)
            VALUES(?1,?1,?2,10,20,0,0,6,20)",params![id,first.destination]).unwrap();
    }
    tx.commit().unwrap();
    assert!(matches!(
        store.deposit_own(prepared(&owner, &recipient, 2, 32), MessageKind::Chat, 30, None),
        Err(Error::StoreFull)
    ));
    assert_eq!(store.status(30).unwrap().len(), MAX_STATUS);
    assert!(store.status(30).unwrap().contains(&first));
    store.conn.execute("UPDATE propagation_owned SET projected=1 WHERE state=6", []).unwrap();
    assert!(matches!(
        store.deposit_own(prepared(&owner, &recipient, 2, 32), MessageKind::Chat, 40, None),
        Err(Error::StoreFull)
    ));
    let next = store
        .deposit_own(prepared(&owner, &recipient, 2, 32), MessageKind::Chat, 111, None)
        .unwrap();
    let statuses = store.status(111).unwrap();
    assert_eq!(statuses.len(), 2);
    assert!(statuses.contains(&next));
    assert!(statuses.iter().any(|s| s.message_id == first.message_id && s.state == State::Expired));
    let pages: i64 = store.conn.pragma_query_value(None, "max_page_count", |r| r.get(0)).unwrap();
    let journal: i64 =
        store.conn.pragma_query_value(None, "journal_size_limit", |r| r.get(0)).unwrap();
    assert_eq!(pages, (10000_i64 * 3 + 8 * 1024 * 1024 + 4095) / 4096);
    assert_eq!(journal, pages * (4096 + 8) + 512);
    assert!(std::fs::metadata(dir.path().join("outbox.db")).unwrap().len() <= pages as u64 * 4096);
}

#[test]
fn database_full_rolls_back_eviction_payload_and_status_together() {
    let dir = tempfile::tempdir().unwrap();
    let owner = PrivateIdentity::new_from_name("owner");
    let recipient = PrivateIdentity::new_from_name("recipient");
    let incoming = prepared(&owner, &recipient, 2, 8000);
    let capacity = incoming.encrypted_bytes() as u64;
    let mut store = OwnedStore::open(
        dir.path().join("outbox.db"),
        owner.clone(),
        StoreLimits { capacity_bytes: capacity, retention_secs: 100 },
    )
    .unwrap();
    let first = store
        .deposit_own(prepared(&owner, &recipient, 1, 32), MessageKind::Chat, 10, None)
        .unwrap();
    let pages: i64 = store.conn.pragma_query_value(None, "page_count", |r| r.get(0)).unwrap();
    store.conn.pragma_update(None, "max_page_count", pages).unwrap();
    let error = store.deposit_own(incoming, MessageKind::Chat, 11, None).unwrap_err();
    assert!(
        matches!(error,Error::Storage(rusqlite::Error::SqliteFailure(code,_)) if code.code==rusqlite::ErrorCode::DiskFull)
    );
    assert_eq!(store.status(11).unwrap(), vec![first.clone()]);
    assert!(storage::get_propagation_entry(&store.conn, &first.transient_id).unwrap().is_some());
    assert_eq!(storage::propagation_entry_stats(&store.conn).unwrap().entries, 1);
}
