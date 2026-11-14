use super::*;

const LOCAL: &str = "11111111111111111111111111111111";
const PEER: &str = "22222222222222222222222222222222";

fn legacy() -> (tempfile::TempDir, std::path::PathBuf, Connection) {
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("legacy.db");
    let conn = Connection::open(&path).unwrap();
    conn.execute_batch(
        "PRAGMA journal_mode=WAL; CREATE TABLE lxmf_messages (
        id INTEGER PRIMARY KEY, hash TEXT, source_hash TEXT, destination_hash TEXT,
        is_incoming INTEGER, title TEXT, content TEXT, timestamp REAL, fields TEXT,
        state TEXT, progress REAL, method TEXT, created_at TEXT, updated_at TEXT);",
    )
    .unwrap();
    (root, path, conn)
}

fn insert(conn: &Connection, index: u8, incoming: bool, timestamp: f64) {
    conn.execute("INSERT INTO lxmf_messages VALUES (?1, ?2, ?3, ?4, ?5, 'title', 'old message', ?6, ?7, 'outbound', 12.5, 'direct', '2026-10-01 00:00:00', '2026-10-01 00:00:01')",
        params![index, format!("{index:064x}"), if incoming {PEER} else {LOCAL}, if incoming {LOCAL} else {PEER}, incoming as i64, timestamp,
            json!({"file_attachments":[{"file_name":"test.txt","file_bytes":"aGk="}]}).to_string()]).unwrap();
}

#[test]
fn meshchat_import_preserves_wal_history_and_never_retries_or_overwrites() {
    let (_root, path, conn) = legacy();
    insert(&conn, 1, false, 100.25);
    insert(&conn, 2, true, 101.5);
    let before = std::fs::read(&path).unwrap();
    let wal_before = std::fs::read(path.with_file_name("legacy.db-wal")).unwrap();
    let target = MessagesStore::in_memory().unwrap();
    assert_eq!(target.import_meshchat_history(&path, LOCAL).unwrap()["imported"], 2);
    let record = target.get_message(&format!("{:064x}", 1)).unwrap().unwrap();
    assert_eq!(record.receipt_status.as_deref(), Some("cancelled"));
    assert_eq!(record.timestamp, 100);
    assert_eq!(record.fields.as_ref().unwrap()["_meshchat_import"]["timestamp"], 100.25);
    assert_eq!(record.fields.as_ref().unwrap()["file_attachments"][0]["file_bytes"], "aGk=");
    assert!(target.list_retryable_outbound_for_destination(PEER).unwrap().is_empty());
    assert_eq!(std::fs::read(path.with_file_name("legacy.db-wal")).unwrap(), wal_before);
    conn.execute("UPDATE lxmf_messages SET content='changed'", []).unwrap();
    assert_eq!(target.import_meshchat_history(&path, LOCAL).unwrap()["skipped"], 2);
    assert_eq!(target.get_message(&record.id).unwrap().unwrap(), record);
    assert_eq!(target.message_count().unwrap(), 2);
    // Read-only import did not checkpoint or rewrite the source main database.
    assert_eq!(std::fs::read(path).unwrap(), before);
    assert!(!wal_before.is_empty());
}

#[test]
fn meshchat_import_rolls_back_corrupt_or_wrong_identity_rows() {
    let (_root, path, conn) = legacy();
    insert(&conn, 1, false, 100.0);
    insert(&conn, 2, true, 101.0);
    let target = MessagesStore::in_memory().unwrap();
    conn.execute("UPDATE lxmf_messages SET fields='invalid' WHERE id=2", []).unwrap();
    assert!(target.import_meshchat_history(&path, LOCAL).is_err());
    assert_eq!(target.message_count().unwrap(), 0);
    conn.execute("UPDATE lxmf_messages SET fields='{}', progress='bad' WHERE id=2", []).unwrap();
    assert!(target.import_meshchat_history(&path, LOCAL).is_err());
    assert_eq!(target.message_count().unwrap(), 0);
    conn.execute("UPDATE lxmf_messages SET progress=0 WHERE id=2", []).unwrap();
    conn.execute("UPDATE lxmf_messages SET fields='{}', destination_hash=?1 WHERE id=2", [PEER])
        .unwrap();
    assert!(target.import_meshchat_history(&path, LOCAL).is_err());
    assert_eq!(target.message_count().unwrap(), 0);
}

#[test]
fn meshchat_import_merges_chronological_pages_with_newer_native_messages() {
    let (_root, path, conn) = legacy();
    insert(&conn, 1, false, 100.25);
    insert(&conn, 2, true, 101.5);
    let target = MessagesStore::in_memory().unwrap();
    target
        .insert_message(&crate::MessageRecord {
            id: "native".into(),
            source: LOCAL.into(),
            destination: PEER.into(),
            title: String::new(),
            content: "newer".into(),
            timestamp: 200,
            direction: "out".into(),
            fields: None,
            receipt_status: Some("delivered".into()),
        })
        .unwrap();
    target.import_meshchat_history(&path, LOCAL).unwrap();
    let first = target.list_meshchat_conversation(PEER, None, false, 1).unwrap();
    assert_eq!(first[0].1.timestamp, 100);
    let next = target.list_meshchat_conversation(PEER, Some(first[0].0), false, 2).unwrap();
    assert_eq!(next.iter().map(|(_, msg)| msg.timestamp).collect::<Vec<_>>(), [101, 200]);
    let newest = target.list_meshchat_conversation(PEER, None, true, 1).unwrap();
    assert_eq!(newest[0].1.timestamp, 200);
    let older = target.list_meshchat_conversation(PEER, Some(newest[0].0), true, 2).unwrap();
    assert_eq!(older.iter().map(|(_, msg)| msg.timestamp).collect::<Vec<_>>(), [101, 100]);
    insert(&conn, 3, true, 100.125);
    target.import_meshchat_history(&path, LOCAL).unwrap();
    let oldest = target.list_meshchat_conversation(PEER, None, false, 1).unwrap();
    assert_eq!(oldest[0].1.id, format!("{:064x}", 3));
    let after = target.list_meshchat_conversation(PEER, Some(oldest[0].0), false, 1).unwrap();
    assert_eq!(after[0].1.id, format!("{:064x}", 1));
}
