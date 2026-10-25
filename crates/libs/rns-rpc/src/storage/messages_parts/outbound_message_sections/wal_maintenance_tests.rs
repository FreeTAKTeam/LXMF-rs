// Issue 663 item 3: WAL maintenance off the write path.

fn wal_store_on_disk() -> (tempfile::TempDir, std::path::PathBuf, MessagesStore) {
    let temp = tempfile::tempdir().expect("temp dir");
    let path = temp.path().join("wal.db");
    let store = MessagesStore::open(&path).expect("open store");
    (temp, path, store)
}

fn wal_bytes(path: &std::path::Path) -> u64 {
    std::fs::metadata(format!("{}-wal", path.display())).map(|meta| meta.len()).unwrap_or(0)
}

fn grow_wal(store: &MessagesStore, rows: usize, payload_bytes: usize) {
    for index in 0..rows {
        let mut entry = refill_entry(index, index as i64);
        entry.payload_hex = "ab".repeat(payload_bytes);
        entry.size_bytes = payload_bytes as u64;
        store.upsert_propagation_entry(&entry).expect("grow wal");
    }
}

fn wal_counters(store: &MessagesStore) -> serde_json::Value {
    store.operation_metrics_snapshot()["wal"].clone()
}

#[test]
fn wal_maintenance_truncates_without_reader() {
    let (_temp, path, store) = wal_store_on_disk();
    grow_wal(&store, 64, 32 * 1024);
    assert!(wal_bytes(&path) > 256 * 1024, "wal grew to {}", wal_bytes(&path));
    let outcome = store.maintain_wal_with_threshold(256 * 1024).expect("maintain");
    assert_eq!(outcome.outcome, crate::storage::operation_metrics::WalCheckpointOutcome::Truncated);
    assert!(outcome.size_after <= 256 * 1024, "wal after {}", outcome.size_after);
    assert_eq!(wal_bytes(&path), outcome.size_after);
    let counters = wal_counters(&store);
    assert_eq!(counters["truncated_total"], 1);
    assert_eq!(counters["checkpoint_attempts_total"], 1);
    assert_eq!(counters["emergency_reclaim_total"], 0);
}

#[test]
fn wal_maintenance_reports_busy_with_pinned_reader_and_recovers_after_release() {
    let (_temp, path, store) = wal_store_on_disk();
    grow_wal(&store, 8, 32 * 1024);
    let reader = Connection::open(&path).expect("external reader");
    reader.execute_batch("BEGIN; SELECT COUNT(*) FROM propagation_entries;").expect("pin reader");
    grow_wal(&store, 64, 32 * 1024);
    let before = wal_bytes(&path);
    let started = std::time::Instant::now();
    let outcome = store.maintain_wal_with_threshold(256 * 1024).expect("maintain with reader");
    assert!(started.elapsed() < std::time::Duration::from_millis(500), "busy must not wait");
    assert!(
        matches!(
            outcome.outcome,
            crate::storage::operation_metrics::WalCheckpointOutcome::Busy
                | crate::storage::operation_metrics::WalCheckpointOutcome::PassiveIncomplete
        ),
        "{outcome:?}"
    );
    assert_eq!(wal_bytes(&path), before, "pinned reader leaves the WAL in place");
    reader.execute_batch("ROLLBACK;").expect("release reader");
    let recovered = store.maintain_wal_with_threshold(256 * 1024).expect("maintain after release");
    assert_eq!(recovered.outcome, crate::storage::operation_metrics::WalCheckpointOutcome::Truncated);
    assert!(wal_bytes(&path) <= 256 * 1024);
    let counters = wal_counters(&store);
    assert_eq!(counters["checkpoint_attempts_total"], 2);
    assert_eq!(counters["truncated_total"], 1);
    assert_eq!(counters["errors_total"], 0);
}

#[test]
fn plain_writes_do_not_run_checkpoints_below_emergency_threshold() {
    let (_temp, _path, store) = wal_store_on_disk();
    store.enable_durable_broker(32 * 1024 * 1024).expect("broker");
    for index in 0..200 {
        store.upsert_propagation_entry(&refill_entry(index, index as i64)).expect("write");
        store
            .insert_message(&MessageRecord {
                id: format!("plain-{index}"),
                source: "aa".repeat(16),
                destination: "bb".repeat(16),
                title: String::new(),
                content: String::new(),
                timestamp: index as i64,
                direction: "in".to_string(),
                fields: None,
                receipt_status: None,
            })
            .expect("insert message");
    }
    let counters = wal_counters(&store);
    assert_eq!(counters["checkpoint_attempts_total"], 0);
    assert_eq!(counters["emergency_reclaim_total"], 0);
}

#[test]
fn journal_size_limit_follows_budget() {
    let (_temp, _path, store) = wal_store_on_disk();
    let limit: i64 = store
        .with_control_conn(|conn| conn.query_row("PRAGMA journal_size_limit", [], |row| row.get(0)))
        .expect("plain limit");
    assert_eq!(limit, 64 * 1024 * 1024);
    store.enable_durable_broker(8 * 1024 * 1024 * 1024).expect("broker");
    let limit: i64 = store
        .with_control_conn(|conn| conn.query_row("PRAGMA journal_size_limit", [], |row| row.get(0)))
        .expect("broker limit");
    assert_eq!(limit, 128 * 1024 * 1024);
    assert_eq!(crate::storage::broker::truncate_limit_bytes(Some(1024)), 64 * 1024 * 1024);
}
