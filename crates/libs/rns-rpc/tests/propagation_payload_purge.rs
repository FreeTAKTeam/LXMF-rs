use rns_rpc::MessagesStore;
use rusqlite::Connection;

#[test]
fn reopening_installs_payload_purge_index_and_retains_daemon_accounting() {
    let dir = tempfile::tempdir().expect("directory");
    let path = dir.path().join("messages.db");
    let store = MessagesStore::open(&path).expect("initialize daemon store");
    let conn = Connection::open(&path).expect("inspection connection");
    conn.execute_batch(
        "DROP INDEX idx_propagation_peer_entries_unhandled_payload;
         INSERT INTO propagation_entries VALUES
             ('aa', 'ff', '00', 1, 1, NULL), ('bb', 'ff', '00', 2, 1, NULL);
         INSERT INTO propagation_peer_entries VALUES
             ('peer', 'aa', 'unhandled', 1), ('PEER', 'aa', 'unhandled', 1),
             ('completed', 'aa', 'received', 1), ('peer', 'bb', 'unhandled', 1);",
    )
    .expect("legacy schema and live trigger-backed marks");
    drop(store);
    let store = MessagesStore::open(&path).expect("migrate daemon database on reopen");
    let installed: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='index' AND name='idx_propagation_peer_entries_unhandled_payload')",
        [], |row| row.get(0),
    ).expect("migration index");
    assert!(installed);
    assert_eq!(budget(&conn), (4, 4));

    assert_eq!(
        store
            .purge_propagation_entries_for_destination("ff", &["aa".into()])
            .expect("recipient haves"),
        1
    );
    assert_eq!(budget(&conn), (2, 2));
    assert_eq!(store.prune_propagation_entries_to_limit_bytes(0).expect("retention"), ["bb"]);
    assert_eq!(budget(&conn), (1, 1));
    let remaining: (String, String, String) = conn
        .query_row("SELECT peer, transient_id, state FROM propagation_peer_entries", [], |row| {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?))
        })
        .expect("completed deduplication mark remains");
    assert_eq!(remaining, ("completed".into(), "aa".into(), "received".into()));
    drop(store);
    let store = MessagesStore::open(&path).expect("reopen purged store");
    assert_eq!(budget(&conn), (1, 1));
    assert_eq!(store.propagation_entry_stats().expect("payload stats").entries, 0);
}

fn budget(conn: &Connection) -> (i64, i64) {
    conn.query_row(
        "SELECT entry_count, (SELECT COUNT(*) FROM propagation_peer_entries)
         FROM propagation_peer_entry_budget WHERE singleton=1",
        [],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )
    .expect("trigger-maintained budget and actual marks")
}
