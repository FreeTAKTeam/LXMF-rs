fn cache_release_counters(store: &MessagesStore) -> serde_json::Value {
    store.operation_metrics_snapshot()["maintenance"]["sqlite_cache_release"].clone()
}

#[test]
fn sqlite_cache_release_preserves_file_records_and_journal_without_checkpointing() {
    let (_temp, path, store) = wal_store_on_disk();
    store.enable_durable_broker(32 * 1024 * 1024).expect("broker");
    let mut record = outbound_message(&"ab".repeat(32), 123, None);
    record.direction = "in".into();
    store.broker_inbound(&record, None).expect("durable inbound");
    let read_journal = |store: &MessagesStore| {
        store
            .with_read_conn(|conn| {
                let mut statement =
                    conn.prepare("SELECT position,payload FROM broker_events ORDER BY position")?;
                let rows = statement
                    .query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)))?;
                rows.collect::<rusqlite::Result<Vec<_>>>()
            })
            .expect("read journal")
    };
    let before_record = store.get_message(&record.id).expect("record");
    let before_journal = read_journal(&store);
    assert!(!before_journal.is_empty());
    let before_wal = wal_counters(&store);
    store.release_unused_sqlite_memory().expect("release pager caches");
    assert_eq!(wal_counters(&store), before_wal, "cache release must not reclaim WAL");
    let counters = cache_release_counters(&store);
    assert_eq!(counters["attempts_total"], 1);
    assert_eq!(counters["runs_total"], 1);
    assert_eq!(counters["errors_total"], 0);
    assert!(counters["duration_ns_total"].as_u64().expect("duration") > 0);
    assert_eq!(store.get_message(&record.id).expect("record after release"), before_record);
    assert_eq!(read_journal(&store), before_journal);
    drop(store);
    let reopened = MessagesStore::open(&path).expect("reopen");
    assert_eq!(reopened.get_message(&record.id).expect("record after reopen"), before_record);
    assert_eq!(read_journal(&reopened), before_journal);
}

#[test]
fn sqlite_cache_release_rejects_active_transaction_and_counts_failure() {
    let store = MessagesStore::in_memory().expect("store");
    store.write_state.conn.lock().expect("writer").execute_batch("BEGIN").expect("begin");
    let error = store.release_unused_sqlite_memory().expect_err("active transaction");
    assert!(error.to_string().contains("autocommit"));
    assert_eq!(cache_release_counters(&store)["attempts_total"], 1);
    assert_eq!(cache_release_counters(&store)["runs_total"], 1);
    assert_eq!(cache_release_counters(&store)["errors_total"], 1);
    store.write_state.conn.lock().expect("writer").execute_batch("ROLLBACK").expect("rollback");
    store.release_unused_sqlite_memory().expect("release after transaction");
    assert_eq!(cache_release_counters(&store)["runs_total"], 2);
    assert_eq!(cache_release_counters(&store)["errors_total"], 1);
}

#[test]
fn sqlite_cache_release_reports_poisoned_writer_instead_of_panicking() {
    let store = std::sync::Arc::new(MessagesStore::in_memory().expect("store"));
    let poison = std::sync::Arc::clone(&store);
    assert!(std::thread::spawn(move || {
        let _writer = poison.write_state.conn.lock().expect("writer");
        panic!("poison the test writer");
    })
    .join()
    .is_err());
    let error = store.release_unused_sqlite_memory().expect_err("poisoned writer");
    assert!(error.to_string().contains("poisoned"));
    assert_eq!(cache_release_counters(&store)["attempts_total"], 1);
    assert_eq!(cache_release_counters(&store)["runs_total"], 1);
    assert_eq!(cache_release_counters(&store)["errors_total"], 1);
}

#[test]
fn sqlite_cache_release_does_not_wait_for_reader_or_maintenance_connections() {
    let (_temp, _path, store) = wal_store_on_disk();
    let store = std::sync::Arc::new(store);
    let reader = store.read_conn.as_ref().expect("reader").lock().expect("reader lock");
    let maintenance =
        store.maintenance_conn.as_ref().expect("maintenance").lock().expect("maintenance lock");
    let release_store = std::sync::Arc::clone(&store);
    let (tx, rx) = std::sync::mpsc::channel();
    let worker = std::thread::spawn(move || tx.send(release_store.release_unused_sqlite_memory()));
    let result = rx.recv_timeout(std::time::Duration::from_secs(1));
    drop((reader, maintenance));
    worker.join().expect("release worker").expect("send result");
    result.expect("release must use only the writer connection").expect("release");
}
