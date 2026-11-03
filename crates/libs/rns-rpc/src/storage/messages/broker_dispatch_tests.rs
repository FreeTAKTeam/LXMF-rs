use super::*;
use rusqlite::StatementStatus;

fn seed_operation(conn: &Connection, id: &str, timestamp: i64, due: i64, state: &str) {
    conn.execute(
        "INSERT INTO messages VALUES (?1,'source','destination','title','content',?2,'out',NULL,NULL)",
        params![id, timestamp],
    )
    .expect("message");
    conn.execute(
        "INSERT INTO broker_operations (owner,source,operation_id,fingerprint,message_id,state,options,prepared_reserve,next_attempt_at)
         VALUES ('owner','source',?1,'fingerprint',?1,?2,'{}',0,?3)",
        params![id, state, due],
    )
    .expect("operation");
}

fn empty_poll_work(store: &MessagesStore) -> i32 {
    store
        .with_read_conn(|conn| {
            let mut stmt = conn.prepare(BROKER_PENDING_DISPATCH_SQL)?;
            assert!(!stmt.exists([])?);
            Ok(stmt.get_status(StatementStatus::VmStep))
        })
        .expect("empty poll")
}

#[test]
fn deferred_dispatch_idle_poll_work_is_independent_of_queue_length() {
    let dir = tempfile::tempdir().expect("temp dir");
    let store = MessagesStore::open(&dir.path().join("broker.db")).expect("store");
    store.enable_durable_broker(32 * 1024 * 1024).expect("enable");
    let empty_steps = empty_poll_work(&store);
    store
        .with_write_conn(|conn| {
            let tx = conn.unchecked_transaction()?;
            for n in 0..4096 {
                seed_operation(&tx, &format!("deferred-{n:04}"), n, i64::MAX, "queued");
            }
            tx.commit()
        })
        .expect("deferred queue");
    let deferred_steps = empty_poll_work(&store);
    println!("dispatch idle VM steps: empty={empty_steps}, deferred={deferred_steps}");
    assert!(
        deferred_steps <= empty_steps + 20,
        "empty poll walked deferred queue: empty={empty_steps}, deferred={deferred_steps}"
    );
}

#[test]
fn dispatch_keeps_due_message_timestamp_and_id_order() {
    let dir = tempfile::tempdir().expect("temp dir");
    let path = dir.path().join("broker.db");
    let store = MessagesStore::open(&path).expect("store");
    store.enable_durable_broker(32 * 1024 * 1024).expect("enable");
    store
        .with_write_conn(|conn| {
            seed_operation(conn, "deferred", 0, i64::MAX, "queued");
            seed_operation(conn, "scheduled", 1, 0, "scheduled");
            seed_operation(conn, "terminal", 2, 0, "terminal");
            seed_operation(conn, "later", 20, 0, "queued");
            seed_operation(conn, "earlier-b", 10, 0, "queued");
            seed_operation(conn, "earlier-a", 10, 1, "queued");
            Ok(())
        })
        .expect("queue");
    let pending = store.broker_pending_dispatch().expect("dispatch");
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].0.id, "earlier-a");
    assert_eq!(pending[0].1, crate::rpc::OutboundDeliveryOptions::default());
    drop(store);
    let reopened = MessagesStore::open(&path).expect("reopen");
    assert_eq!(reopened.broker_pending_dispatch().expect("recovered")[0].0.id, "earlier-a");
}

#[test]
fn legacy_dispatch_index_is_upgraded_on_reopen_without_changing_work() {
    let dir = tempfile::tempdir().expect("temp dir");
    let path = dir.path().join("broker.db");
    let store = MessagesStore::open(&path).expect("store");
    store.enable_durable_broker(32 * 1024 * 1024).expect("enable");
    store
        .with_write_conn(|conn| {
            seed_operation(conn, "due", 10, 0, "queued");
            seed_operation(conn, "deferred", 0, i64::MAX, "queued");
            conn.execute_batch(
                "DROP INDEX broker_dispatch_due;
            CREATE INDEX broker_dispatch_pending ON broker_operations(state,message_id)",
            )
        })
        .expect("legacy profile");
    drop(store);
    let reopened = MessagesStore::open(&path).expect("upgrade");
    reopened
        .with_read_conn(|conn| {
            let columns = conn
                .prepare("PRAGMA index_info(broker_dispatch_due)")?
                .query_map([], |row| row.get::<_, String>(2))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            assert_eq!(columns, ["state", "next_attempt_at", "message_id"]);
            let legacy: bool = conn.query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name='broker_dispatch_pending')",
                [],
                |row| row.get(0),
            )?;
            assert!(!legacy, "obsolete index must not add write amplification");
            Ok(())
        })
        .expect("indexes");
    assert_eq!(reopened.broker_pending_dispatch().expect("dispatch")[0].0.id, "due");
}
