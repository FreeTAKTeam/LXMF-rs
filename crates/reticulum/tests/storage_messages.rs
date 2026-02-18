use reticulum::storage::messages::{MessageRecord, MessagesStore};
use rusqlite::params;

#[test]
fn stores_and_reads_message() {
    let db = MessagesStore::in_memory().unwrap();
    db.insert_message(&MessageRecord {
        id: "m1".into(),
        source: "a".into(),
        destination: "b".into(),
        title: "t1".into(),
        content: "hi".into(),
        timestamp: 1,
        direction: "in".into(),
        fields: None,
        receipt_status: None,
    })
    .unwrap();
    let items = db.list_messages(10, None).unwrap();
    assert_eq!(items.len(), 1);
}

#[test]
fn opens_disk_store() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("messages.db");
    let db = MessagesStore::open(&path).unwrap();
    db.insert_message(&MessageRecord {
        id: "m2".into(),
        source: "a".into(),
        destination: "b".into(),
        title: "t2".into(),
        content: "hello".into(),
        timestamp: 2,
        direction: "in".into(),
        fields: None,
        receipt_status: None,
    })
    .unwrap();
    drop(db);

    let db2 = MessagesStore::open(&path).unwrap();
    let items = db2.list_messages(10, None).unwrap();
    assert_eq!(items.len(), 1);
}

#[test]
fn migrates_missing_title_to_empty_string() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("legacy.db");
    let conn = rusqlite::Connection::open(&path).unwrap();
    conn.execute_batch(
        "CREATE TABLE messages (
            id TEXT PRIMARY KEY,
            source TEXT NOT NULL,
            destination TEXT NOT NULL,
            content TEXT NOT NULL,
            timestamp INTEGER NOT NULL,
            direction TEXT NOT NULL,
            fields TEXT,
            receipt_status TEXT
        );",
    )
    .unwrap();
    conn.execute(
        "INSERT INTO messages (id, source, destination, content, timestamp, direction) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params!["legacy-1", "a", "b", "hello", 1i64, "in"],
    )
    .unwrap();
    drop(conn);

    let db = MessagesStore::open(&path).unwrap();
    let items = db.list_messages(10, None).unwrap();
    assert_eq!(items[0].title, "");
}
