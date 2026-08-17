#![cfg(feature = "storage")]
use rns_transport::storage::messages::MessagesStore;
use rusqlite::Connection;

fn temporary_database(name: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "lxmf-transport-{name}-{}-{}.sqlite",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock after epoch")
            .as_nanos()
    ))
}

fn table_has_column(conn: &Connection, table: &str, column: &str) -> bool {
    let mut statement = conn.prepare(&format!("PRAGMA table_info({table})")).expect("table info");
    let mut rows = statement.query([]).expect("query table info");
    while let Some(row) = rows.next().expect("read table info") {
        if row.get::<_, String>(1).expect("column name") == column {
            return true;
        }
    }
    false
}

#[test]
fn schema_upgrade_adds_legacy_message_and_announce_columns() {
    let path = temporary_database("schema-upgrade");
    {
        let conn = Connection::open(&path).expect("open legacy database");
        conn.execute_batch(
            "CREATE TABLE messages (
                id TEXT PRIMARY KEY,
                source TEXT NOT NULL,
                destination TEXT NOT NULL,
                content TEXT NOT NULL,
                timestamp INTEGER NOT NULL,
                direction TEXT NOT NULL
            );
            CREATE TABLE announces (
                id TEXT PRIMARY KEY,
                peer TEXT NOT NULL,
                timestamp INTEGER NOT NULL
            );
            INSERT INTO messages
                (id, source, destination, content, timestamp, direction)
                VALUES ('legacy-message', 'source', 'destination', 'body', 5, 'in');
            INSERT INTO announces (id, peer, timestamp)
                VALUES ('legacy-announce', 'peer', 7);",
        )
        .expect("create legacy schema");
    }

    let store = MessagesStore::open(&path).expect("upgrade legacy schema");
    let messages = store.list_messages(10, None).expect("read upgraded message");
    assert_eq!(messages[0].title, "");
    let announces = store.list_announces(10, None, None).expect("read upgraded announce");
    assert_eq!(announces[0].first_seen, 7);
    assert_eq!(announces[0].seen_count, 1);
    drop(store);
    let conn = Connection::open(&path).expect("reopen upgraded database");
    for (table, column) in [
        ("messages", "title"),
        ("messages", "fields"),
        ("messages", "receipt_status"),
        ("announces", "capabilities"),
        ("announces", "stamp_cost_flexibility"),
        ("announces", "peering_cost"),
    ] {
        assert!(table_has_column(&conn, table, column), "missing {table}.{column}");
    }
    drop(conn);
    std::fs::remove_file(path).expect("remove schema-upgrade database");
}

#[test]
fn malformed_persisted_json_is_reported() {
    let path = temporary_database("invalid-json");
    let store = MessagesStore::open(&path).expect("initialize database");
    drop(store);
    {
        let conn = Connection::open(&path).expect("open initialized database");
        conn.execute(
            "INSERT INTO messages
             (id, source, destination, title, content, timestamp, direction, fields)
             VALUES ('invalid-json', 'source', 'destination', '', 'body', 1, 'in', '{not-json')",
            [],
        )
        .expect("insert malformed fields");
    }

    let store = MessagesStore::open(&path).expect("reopen database");
    let err = store.list_messages(10, None).expect_err("malformed JSON must surface");
    assert!(matches!(err, rusqlite::Error::FromSqlConversionFailure(7, _, _)));
    drop(store);
    std::fs::remove_file(path).expect("remove invalid-json database");
}

#[test]
fn failed_schema_upgrade_rolls_back_partial_columns() {
    let path = temporary_database("schema-rollback");
    {
        let conn = Connection::open(&path).expect("open legacy database");
        conn.execute_batch(
            "CREATE TABLE messages (
                id TEXT PRIMARY KEY,
                source TEXT NOT NULL,
                destination TEXT NOT NULL,
                content TEXT NOT NULL,
                timestamp INTEGER NOT NULL,
                direction TEXT NOT NULL
            );
            CREATE TABLE announces (
                id TEXT PRIMARY KEY,
                peer TEXT NOT NULL,
                timestamp INTEGER NOT NULL
            );
            INSERT INTO announces (id, peer, timestamp) VALUES ('legacy', 'peer', 7);
            CREATE TRIGGER reject_announce_backfill
                BEFORE UPDATE ON announces
                BEGIN SELECT RAISE(ABORT, 'backfill blocked'); END;",
        )
        .expect("create failing legacy schema");
    }

    assert!(MessagesStore::open(&path).is_err(), "migration failure must surface");
    let conn = Connection::open(&path).expect("reopen rolled-back database");
    assert!(!table_has_column(&conn, "messages", "title"));
    assert!(!table_has_column(&conn, "announces", "first_seen"));
    drop(conn);
    std::fs::remove_file(path).expect("remove schema-rollback database");
}
