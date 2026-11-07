#[test]
fn broker_enabled_cache_never_takes_the_read_connection() {
    let (_temp, path, store) = wal_store_on_disk();
    assert!(!store.durable_broker_enabled().expect("disabled"));
    store.enable_durable_broker(32 * 1024 * 1024).expect("enable");
    let before = store.contention_snapshot();
    let _read = store.read_conn.as_ref().expect("disk reader").lock().expect("reader lock");
    assert!(store.durable_broker_enabled().expect("cached"));
    assert_eq!(store.contention_snapshot().read_ops_total, before.read_ops_total);
    drop(_read);
    drop(store);
    let reopened = MessagesStore::open(&path).expect("reopen");
    assert!(reopened.durable_broker_enabled().expect("restored cached capability"));
}

#[test]
fn broker_control_deadline_does_not_cancel_queued_resume() {
    use crate::broker::{ConsumerId, EventPosition, ResumeRequest};
    let (_temp, _path, store) = wal_store_on_disk();
    store.enable_durable_broker(32 * 1024 * 1024).expect("enable");
    let store = std::sync::Arc::new(store);
    let writer = store.write_state.conn.lock().expect("park writer");
    let command_store = std::sync::Arc::clone(&store);
    let started = std::time::Instant::now();
    let thread = std::thread::spawn(move || command_store.broker_command(
        crate::storage::broker::BrokerCommand::Resume {
            owner: "owner".into(), destination: "aa".repeat(16),
            request: ResumeRequest { consumer_id: ConsumerId("deadline".into()),
                identity: "identity".into(), journal_id: None, stored: EventPosition(0) },
        },
    ));
    let error = thread.join().expect("caller").expect_err("deadline");
    assert!(started.elapsed() < std::time::Duration::from_secs(3));
    match error {
        rusqlite::Error::ToSqlConversionFailure(source) => {
            assert_eq!(source.downcast_ref::<crate::storage::broker::BrokerError>()
                .expect("typed broker error").code, "SDK_STORAGE_BROKER_TIMEOUT");
        }
        other => panic!("unexpected error: {other}"),
    }
    drop(writer);
    // FIFO barrier observes the timed-out command's commit before inspecting it.
    store.update_receipt_status("unused-barrier", "delivered").expect("writer barrier");
    let count: i64 = store.with_read_conn(|conn| conn.query_row(
        "SELECT COUNT(*) FROM broker_consumers WHERE consumer_id='deadline'", [], |row| row.get(0),
    )).expect("consumer");
    assert_eq!(count, 1, "timed-out resume still executed");
}
