//! Direct storage contract tests exercise the same primitives as the LXMF adapter.
use r3akt_event_broker::{storage, *};
use rusqlite::Connection;
use serde_json::json;

fn resume(conn: &Connection, id: &str) -> BrokerCheckpoint {
    storage::resume(
        conn,
        "principal",
        "topic",
        ResumeRequest {
            consumer_id: ConsumerId(id.into()),
            identity: "adapter-owned".into(),
            journal_id: None,
            stored: EventPosition(0),
        },
    )
    .expect("new consumer resumes")
}

fn request(id: &str, count: usize, bytes: usize) -> FetchRequest {
    FetchRequest {
        consumer_id: ConsumerId(id.into()),
        identity: "adapter-owned".into(),
        max_events: count,
        max_bytes: bytes,
    }
}

fn ack(conn: &Connection, batch: &BrokerBatch) {
    storage::ack(
        conn,
        "principal",
        "topic",
        AckStoredRequest {
            consumer_id: batch.consumer_id.clone(),
            identity: "adapter-owned".into(),
            journal_id: batch.journal_id.clone(),
            end: batch.end,
            receipt: batch.receipt.clone(),
        },
    )
    .expect("issued receipt advances custody");
}

#[test]
fn independent_consumers_scan_invisible_positions_and_replay_across_limit_changes() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let mut conn = Connection::open(directory.path().join("journal.db")).expect("disk database");
    let budget = 32 * 1024 * 1024;
    let limits = Limits { max_batch_bytes: 8192, max_batch_events: 3 };
    storage::configure_profile_with_limits(&mut conn, budget, limits).expect("durable profile");
    let tx = conn.transaction().expect("one application transaction");
    assert!(storage::initialize_schema(&tx, budget).expect("generic schema"));
    resume(&tx, "first");
    resume(&tx, "second");
    storage::append_with_limits(&tx, "topic", "one", &json!({"value":1}), false, limits)
        .expect("visible event");
    storage::append_with_limits(&tx, "topic", "two", &json!({"value":2}), false, limits)
        .expect("second visible event");
    storage::append_with_limits(&tx, "other-topic", "three", &json!({"value":3}), false, limits)
        .expect("invisible event");
    storage::append_with_limits(&tx, "topic", "four", &json!({"value":4}), false, limits)
        .expect("event beyond scan window");
    let first =
        storage::fetch_with_limits(&tx, "principal", "topic", request("first", 3, 8192), limits)
            .expect("bounded scan");
    assert_eq!(first.end, EventPosition(3));
    assert_eq!(first.events.len(), 2);
    let smaller = Limits { max_batch_bytes: 4096, max_batch_events: 1 };
    let replay =
        storage::fetch_with_limits(&tx, "principal", "topic", request("first", 0, 0), smaller)
            .expect("outstanding issued batch precedes new request limits");
    assert_eq!(replay, first);
    let second =
        storage::fetch_with_limits(&tx, "principal", "topic", request("second", 1, 8192), limits)
            .expect("independent consumer");
    assert_eq!(second.end, EventPosition(1));
    ack(&tx, &first);
    let floor: i64 = tx
        .query_row("SELECT floor FROM broker_meta", [], |row| row.get(0))
        .expect("global retention floor");
    assert_eq!(floor, 0, "unacknowledged second consumer retains history");
    ack(&tx, &second);
    let floor: i64 = tx
        .query_row("SELECT floor FROM broker_meta", [], |row| row.get(0))
        .expect("minimum independent checkpoint");
    assert_eq!(floor, 1);
    ack(&tx, &second); // Authenticated retry remains valid after pruning.
    tx.commit().expect("atomic application commit");
}

#[test]
fn journal_mutations_rollback_with_application_state_and_bounds_preserve_positions() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let mut conn = Connection::open(directory.path().join("journal.db")).expect("disk database");
    let budget = 32 * 1024 * 1024;
    let limits = Limits { max_batch_bytes: 8192, max_batch_events: 2 };
    storage::configure_profile_with_limits(&mut conn, budget, limits).expect("durable profile");
    {
        let tx = conn.transaction().expect("schema transaction");
        storage::initialize_schema(&tx, budget).expect("generic schema");
        tx.execute_batch("CREATE TABLE application(value INTEGER CHECK(value>0))")
            .expect("application table");
        tx.commit().expect("schema commit");
    }
    {
        let tx = conn.transaction().expect("application transaction");
        assert!(storage::append_with_limits(
            &tx,
            "topic",
            "too-large",
            &json!("x".repeat(8192)),
            false,
            limits,
        )
        .expect_err("payload cannot fit")
        .to_string()
        .contains("EVENT_TOO_LARGE"));
        assert_eq!(
            storage::append_with_limits(
                &tx,
                "topic",
                "accepted",
                &json!({"data":true}),
                false,
                limits,
            )
            .expect("position preserved after rejected payload"),
            1
        );
        tx.execute("INSERT INTO application VALUES(1)", []).expect("application mutation");
        assert!(tx.execute("INSERT INTO application VALUES(-1)", []).is_err());
        // Dropping the caller-owned transaction rolls back journal and application rows.
    }
    let state: (i64, i64, i64) = conn.query_row(
        "SELECT next_position,(SELECT COUNT(*) FROM broker_events),(SELECT COUNT(*) FROM application) FROM broker_meta",
        [], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
    ).expect("state after rollback");
    assert_eq!(state, (1, 0, 0));
}
