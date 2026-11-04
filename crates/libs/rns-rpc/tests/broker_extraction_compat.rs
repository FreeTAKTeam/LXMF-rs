//! Frozen pre-extraction storage/wire fixtures: do not generate these with the new core.
use rns_rpc::broker::*;
use rns_rpc::{BrokerCommand, MessageRecord, MessagesStore};
use rusqlite::{params, Connection};
use serde_json::{json, Value};

const JOURNAL: &str = "legacy-journal-014";
// HMAC-SHA256 with secret bytes 0..31 over the literal UTF-8 JSON array
// ["legacy-journal-014","rch","alice","rch",0,1].
const RECEIPT: &str = "0:1:06354f22c33cd5997c90d2f38a0d03b2b2b9c96dee5af6def30d57eb33609ae4";
const ISSUED: &str = "{\"start\":0,\"end\":1,\"receipt\":\"0:1:06354f22c33cd5997c90d2f38a0d03b2b2b9c96dee5af6def30d57eb33609ae4\"}";
const BUDGET: i64 = 32 * 1024 * 1024;

fn legacy_database(path: &std::path::Path, issued: bool) {
    let conn = Connection::open(path).expect("open legacy fixture database");
    // These table/column names and positional layouts predate the extraction.
    conn.execute_batch(
        "CREATE TABLE messages (
         id TEXT PRIMARY KEY, source TEXT NOT NULL, destination TEXT NOT NULL,
         title TEXT NOT NULL, content TEXT NOT NULL, timestamp INTEGER NOT NULL,
         direction TEXT NOT NULL, fields TEXT, receipt_status TEXT);
         CREATE TABLE broker_meta (
         singleton INTEGER PRIMARY KEY CHECK(singleton=1), journal_id TEXT NOT NULL,
         secret BLOB NOT NULL, next_position INTEGER NOT NULL, floor INTEGER NOT NULL,
         budget INTEGER NOT NULL, used_bytes INTEGER NOT NULL,
         min_writer INTEGER NOT NULL CHECK(min_writer=1));
         CREATE TABLE broker_events (
         position INTEGER PRIMARY KEY, local_destination TEXT NOT NULL, event_type TEXT NOT NULL,
         created_at INTEGER NOT NULL, payload TEXT NOT NULL, bytes INTEGER NOT NULL);
         CREATE TABLE broker_consumers (
         consumer_id TEXT PRIMARY KEY, owner TEXT NOT NULL, destination TEXT NOT NULL,
         stored INTEGER NOT NULL, issued TEXT);
         INSERT INTO messages VALUES('historical','remote','rch','','history',17,'in',NULL,NULL);
         INSERT INTO broker_events VALUES(1,'rch','inbound',17,'{\"legacy\":true}',256);",
    )
    .expect("create frozen legacy tables and historical rows");
    conn.execute(
        "INSERT INTO broker_meta VALUES(1,?1,?2,2,0,?3,4096,1)",
        params![JOURNAL, (0u8..32).collect::<Vec<_>>(), BUDGET],
    )
    .expect("seed legacy journal incarnation, secret and accounting");
    conn.execute(
        "INSERT INTO broker_consumers VALUES('rch','alice','rch',0,?1)",
        [issued.then_some(ISSUED)],
    )
    .expect("seed legacy consumer and outstanding issued range");
}

fn fetch(store: &MessagesStore, owner: &str, scope: &str) -> Result<Value, rusqlite::Error> {
    store.broker_command(BrokerCommand::Fetch {
        owner: owner.into(),
        destination: scope.into(),
        request: FetchRequest {
            consumer_id: ConsumerId("rch".into()),
            identity: "untrusted-request-identity".into(),
            max_events: MAX_BATCH_EVENTS,
            max_bytes: MAX_BATCH_BYTES,
        },
    })
}

fn ack(
    store: &MessagesStore,
    owner: &str,
    scope: &str,
    journal: &str,
    receipt: &str,
) -> Result<Value, rusqlite::Error> {
    store.broker_command(BrokerCommand::Ack {
        owner: owner.into(),
        destination: scope.into(),
        request: AckStoredRequest {
            consumer_id: ConsumerId("rch".into()),
            identity: "untrusted-request-identity".into(),
            journal_id: JournalId(journal.into()),
            end: EventPosition(1),
            receipt: StoredReceipt(receipt.into()),
        },
    })
}

fn metadata(conn: &Connection) -> (String, Vec<u8>, i64, i64, i64, i64) {
    conn.query_row(
        "SELECT journal_id,secret,next_position,floor,budget,used_bytes FROM broker_meta",
        [],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?, r.get(5)?)),
    )
    .expect("read journal incarnation, secret and accounting snapshot")
}

#[test]
fn pre_extraction_tables_incarnation_and_outstanding_receipt_survive_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("legacy.db");
    legacy_database(&path, true);
    let conn = Connection::open(&path).unwrap();
    let before = metadata(&conn);
    let store = MessagesStore::open(&path).unwrap();
    // A new configured budget must not replace an already admitted profile.
    store.enable_durable_broker(BUDGET * 2).unwrap();
    assert_eq!(metadata(&conn), before);
    assert!(store.get_message("historical").unwrap().is_some());
    assert_eq!(store.message_count().unwrap(), 1);
    let expected = json!({
        "journal_id":JOURNAL,"consumer_id":"rch","start":0,"end":1,
        "events":[{"version":1,"position":1,"created_at":17,
                   "event_type":"inbound","payload":{"legacy":true}}],
        "receipt":RECEIPT
    });
    assert_eq!(fetch(&store, "alice", "rch").unwrap(), expected);
    drop(store);
    let store = MessagesStore::open(&path).unwrap();
    assert_eq!(fetch(&store, "alice", "rch").unwrap(), expected);
    assert_eq!(ack(&store, "alice", "rch", JOURNAL, RECEIPT).unwrap(), json!({"stored":1}));
    assert_eq!(metadata(&conn).3, 1);
    assert_eq!(metadata(&conn).5, 4096 - 256 - ISSUED.len() as i64);
    // Pruning does not invalidate a previously issued authenticated ACK retry.
    drop(store);
    let store = MessagesStore::open(&path).unwrap();
    assert_eq!(ack(&store, "alice", "rch", JOURNAL, RECEIPT).unwrap(), json!({"stored":1}));
    assert_eq!(metadata(&conn).0, JOURNAL);
    assert_eq!(metadata(&conn).1, (0u8..32).collect::<Vec<_>>());
}

#[test]
fn legacy_mac_encoding_and_authority_binding_are_preserved() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("legacy.db");
    legacy_database(&path, false);
    let store = MessagesStore::open(&path).unwrap();
    // No issued descriptor was seeded: the current implementation must produce the old MAC.
    assert_eq!(fetch(&store, "alice", "rch").unwrap()["receipt"], RECEIPT);
    let conn = Connection::open(&path).unwrap();
    let before = metadata(&conn);
    for (owner, scope, journal, receipt, code) in [
        ("bob", "rch", JOURNAL, RECEIPT, "SDK_SECURITY_CONSUMER_FORBIDDEN"),
        ("alice", "other", JOURNAL, RECEIPT, "SDK_SECURITY_CONSUMER_FORBIDDEN"),
        ("alice", "rch", "stale-incarnation", RECEIPT, "SDK_BROKER_RECOVERY_REQUIRED"),
        ("alice", "rch", JOURNAL, "0:1:00", "SDK_BROKER_INVALID_RECEIPT"),
        ("alice", "rch", JOURNAL, "0:2:00", "SDK_BROKER_INVALID_RECEIPT"),
    ] {
        assert!(ack(&store, owner, scope, journal, receipt)
            .unwrap_err()
            .to_string()
            .contains(code));
        assert_eq!(metadata(&conn), before, "denied ACK cannot mutate custody/accounting");
    }
    // LXMF retains its unique owner/scope rule even though generic consumers need not.
    assert!(store
        .broker_command(BrokerCommand::Resume {
            owner: "alice".into(),
            destination: "rch".into(),
            request: ResumeRequest {
                consumer_id: ConsumerId("second-consumer".into()),
                identity: "untrusted-request-identity".into(),
                journal_id: None,
                stored: EventPosition(0),
            },
        })
        .is_err());
    assert_eq!(metadata(&conn), before);
    ack(&store, "alice", "rch", JOURNAL, RECEIPT).unwrap();
}

fn message(id: &str, direction: &str) -> MessageRecord {
    MessageRecord {
        id: id.into(),
        source: "rch".into(),
        destination: "remote".into(),
        title: "".into(),
        content: "body".into(),
        timestamp: 17,
        direction: direction.into(),
        fields: None,
        receipt_status: None,
    }
}

fn row_counts(conn: &Connection) -> (i64, i64, i64, i64) {
    conn.query_row(
        "SELECT (SELECT COUNT(*) FROM messages),(SELECT COUNT(*) FROM broker_events),
         (SELECT COUNT(*) FROM broker_message_keys),(SELECT COUNT(*) FROM broker_completions)",
        [],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
    )
    .expect("read message, journal, idempotency and completion row counts")
}

#[test]
fn shared_journal_failure_rolls_back_inbound_and_terminal_message_writes() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("atomic.db");
    let store = MessagesStore::open(&path).unwrap();
    store.enable_durable_broker(BUDGET).unwrap();
    let conn = Connection::open(&path).unwrap();
    conn.execute_batch(
        "CREATE TRIGGER reject_event BEFORE INSERT ON broker_events
         BEGIN SELECT RAISE(ABORT,'injected shared journal failure'); END;",
    )
    .unwrap();
    let before = (metadata(&conn), row_counts(&conn));
    let inbound = message("inbound", "in");
    assert!(store
        .broker_inbound(&inbound, Some(&[0, 1]))
        .unwrap_err()
        .to_string()
        .contains("injected shared journal failure"));
    assert_eq!((metadata(&conn), row_counts(&conn)), before);
    assert!(store.get_message("inbound").unwrap().is_none());
    conn.execute_batch("DROP TRIGGER reject_event;").unwrap();
    store.broker_inbound(&inbound, Some(&[0, 1])).unwrap();
    store.insert_message(&message("outbound", "out")).unwrap();
    let before = (metadata(&conn), row_counts(&conn));
    conn.execute_batch(
        "CREATE TRIGGER reject_receipt BEFORE INSERT ON broker_events
         WHEN NEW.event_type='receipt'
         BEGIN SELECT RAISE(ABORT,'injected terminal journal failure'); END;",
    )
    .unwrap();
    assert!(store
        .update_receipt_status("outbound", "delivered")
        .unwrap_err()
        .to_string()
        .contains("injected terminal journal failure"));
    assert_eq!((metadata(&conn), row_counts(&conn)), before);
    assert_eq!(store.get_message("outbound").unwrap().unwrap().receipt_status, None);
    conn.execute_batch("DROP TRIGGER reject_receipt;").unwrap();
    store.update_receipt_status("outbound", "delivered").unwrap();
    assert_eq!(
        store.get_message("outbound").unwrap().unwrap().receipt_status.as_deref(),
        Some("delivered")
    );
}

#[test]
fn legacy_wire_event_version_default_remains_one() {
    let old = json!({"position":9,"created_at":17,"event_type":"inbound","payload":{}});
    let event: BrokerEvent = serde_json::from_value(old.clone()).unwrap();
    assert_eq!(event.version, 1);
    let mut expected = old;
    expected["version"] = json!(1);
    assert_eq!(serde_json::to_value(event).unwrap(), expected);
    assert_eq!((MAX_BATCH_EVENTS, MAX_BATCH_BYTES), (128, 12 * 1024 * 1024));
}

#[test]
fn failed_legacy_message_snapshot_rolls_back_generic_schema_initialization() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("migration.db");
    let store = MessagesStore::open(&path).unwrap();
    store.insert_message(&message("historical", "in")).unwrap();
    let conn = Connection::open(&path).unwrap();
    // A malformed historical row fails after shared schema initialization begins.
    conn.execute("UPDATE messages SET fields='malformed-json'", []).unwrap();
    assert!(store.enable_durable_broker(BUDGET).is_err());
    let shared_tables: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM sqlite_master WHERE type='table'
             AND name IN ('broker_meta','broker_events','broker_consumers')",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(shared_tables, 0, "generic DDL and application snapshot share one transaction");
    assert!(!store.durable_broker_enabled().unwrap());
    conn.execute("UPDATE messages SET fields=NULL", []).unwrap();
    store.enable_durable_broker(BUDGET).unwrap();
    assert_eq!(row_counts(&conn), (1, 1, 1, 0));
    let kind: String =
        conn.query_row("SELECT event_type FROM broker_events", [], |r| r.get(0)).unwrap();
    assert_eq!(kind, "bootstrap_message");
}
