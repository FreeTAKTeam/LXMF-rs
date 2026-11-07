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
    w.sign(owner).expect("valid test fixture or operation");
    PreparedMessage::new(&w, owner, Some(recipient.as_identity()))
        .expect("valid test fixture or operation")
}

#[test]
fn saturated_status_metadata_preserves_work_and_notices_until_projected_retention_ends() {
    let dir = tempfile::tempdir().expect("valid test fixture or operation");
    let owner = PrivateIdentity::new_from_name("owner");
    let recipient = PrivateIdentity::new_from_name("recipient");
    let mut store = OwnedStore::open(
        dir.path().join("outbox.db"),
        owner.clone(),
        StoreLimits { capacity_bytes: 10000, retention_secs: 100 },
    )
    .expect("valid test fixture or operation");
    let first = store
        .deposit_own(prepared(&owner, &recipient, 1, 32), MessageKind::Chat, 10, None)
        .expect("valid test fixture or operation");
    let tx = store.conn.unchecked_transaction().expect("valid test fixture or operation");
    for i in 0..MAX_STATUS - 1 {
        let id = format!("{i:064x}");
        tx.execute("INSERT INTO propagation_owned(transient_id,message_id,destination,admitted_at,expires_at,encrypted_bytes,kind,state,changed_at)
            VALUES(?1,?1,?2,10,20,0,0,6,20)",params![id,first.destination]).expect("valid test fixture or operation");
    }
    tx.commit().expect("valid test fixture or operation");
    assert!(matches!(
        store.deposit_own(prepared(&owner, &recipient, 2, 32), MessageKind::Chat, 30, None),
        Err(Error::StoreFull)
    ));
    assert_eq!(store.status(30).expect("valid test fixture or operation").len(), MAX_STATUS);
    assert!(store.status(30).expect("valid test fixture or operation").contains(&first));
    store
        .conn
        .execute("UPDATE propagation_owned SET projected=1 WHERE state=6", [])
        .expect("valid test fixture or operation");
    assert!(matches!(
        store.deposit_own(prepared(&owner, &recipient, 2, 32), MessageKind::Chat, 40, None),
        Err(Error::StoreFull)
    ));
    let next = store
        .deposit_own(prepared(&owner, &recipient, 2, 32), MessageKind::Chat, 111, None)
        .expect("valid test fixture or operation");
    let statuses = store.status(111).expect("valid test fixture or operation");
    assert_eq!(statuses.len(), 2);
    assert!(statuses.contains(&next));
    assert!(statuses.iter().any(|s| s.message_id == first.message_id && s.state == State::Expired));
    let pages: i64 = store
        .conn
        .pragma_query_value(None, "max_page_count", |r| r.get(0))
        .expect("valid test fixture or operation");
    let journal: i64 = store
        .conn
        .pragma_query_value(None, "journal_size_limit", |r| r.get(0))
        .expect("valid test fixture or operation");
    assert_eq!(pages, (10000_i64 * 3 + 8 * 1024 * 1024 + 4095) / 4096);
    assert_eq!(journal, pages * (4096 + 8) + 512);
    assert!(
        std::fs::metadata(dir.path().join("outbox.db"))
            .expect("valid test fixture or operation")
            .len()
            <= pages as u64 * 4096
    );
}

#[test]
fn database_full_rolls_back_eviction_payload_and_status_together() {
    let dir = tempfile::tempdir().expect("valid test fixture or operation");
    let owner = PrivateIdentity::new_from_name("owner");
    let recipient = PrivateIdentity::new_from_name("recipient");
    let incoming = prepared(&owner, &recipient, 2, 8000);
    let capacity = incoming.encrypted_bytes() as u64;
    let mut store = OwnedStore::open(
        dir.path().join("outbox.db"),
        owner.clone(),
        StoreLimits { capacity_bytes: capacity, retention_secs: 100 },
    )
    .expect("valid test fixture or operation");
    let first = store
        .deposit_own(prepared(&owner, &recipient, 1, 32), MessageKind::Chat, 10, None)
        .expect("valid test fixture or operation");
    let pages: i64 = store
        .conn
        .pragma_query_value(None, "page_count", |r| r.get(0))
        .expect("valid test fixture or operation");
    store
        .conn
        .pragma_update(None, "max_page_count", pages)
        .expect("valid test fixture or operation");
    let error = store.deposit_own(incoming, MessageKind::Chat, 11, None).unwrap_err();
    assert!(
        matches!(error,Error::Storage(rusqlite::Error::SqliteFailure(code,_)) if code.code==rusqlite::ErrorCode::DiskFull)
    );
    assert_eq!(store.status(11).expect("valid test fixture or operation"), vec![first.clone()]);
    assert!(storage::get_propagation_entry(&store.conn, &first.transient_id)
        .expect("valid test fixture or operation")
        .is_some());
    assert_eq!(
        storage::propagation_entry_stats(&store.conn)
            .expect("valid test fixture or operation")
            .entries,
        1
    );
}
