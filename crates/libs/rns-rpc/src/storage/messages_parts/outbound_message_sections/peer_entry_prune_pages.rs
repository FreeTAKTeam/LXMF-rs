#[test]
fn peer_policy_prune_does_not_delete_a_reused_rowid() {
    let (_temp, store) = refill_store_on_disk();
    let candidate = store.with_write_conn(|conn| {
        conn.execute("INSERT INTO propagation_peer_entries VALUES ('old-peer', 'old-id', 'unhandled', 1)", [])?;
        let rowid = conn.last_insert_rowid();
        conn.execute("DELETE FROM propagation_peer_entries WHERE rowid = ?1", params![rowid])?;
        conn.execute("INSERT INTO propagation_peer_entries VALUES ('new-peer', 'new-id', 'unhandled', 1)", [])?;
        assert_eq!(conn.last_insert_rowid(), rowid, "fixture reuses the deleted tail rowid");
        Ok((rowid, 1, "old-peer".to_string(), "old-id".to_string()))
    }).expect("replace candidate row");
    let deleted = store
        .delete_expired_peer_entry_candidates(&[candidate], 0, 0)
        .expect("recheck reused rowid");
    assert_eq!(deleted, 0, "replacement rows must not match an earlier selection");
    let remaining: i64 = store
        .with_read_conn(|conn| {
            conn.query_row("SELECT COUNT(*) FROM propagation_peer_entries", [], |row| row.get(0))
        })
        .expect("remaining marks");
    assert_eq!(remaining, 1);
}

#[test]
fn peer_policy_expiry_page_uses_the_state_index_and_keyset_cursor() {
    let (_temp, store) = refill_store_on_disk();
    store
        .with_write_conn(|conn| {
            conn.execute_batch(
                "INSERT INTO propagation_peer_entries (rowid, peer, transient_id, state, updated_at)
                 VALUES (10, 'p', 'a', 'unhandled', 1),
                        (11, 'p', 'b', 'unhandled', 1),
                        (12, 'p', 'c', 'unhandled', 2),
                        (13, 'p', 'd', 'unhandled', 9),
                        (14, 'p', 'e', 'handled', 1);",
            )
        })
        .expect("seed marks");

    let plan = store
        .with_read_conn(|conn| {
            let mut statement = conn.prepare(&format!("EXPLAIN QUERY PLAN {EXPIRED_PEER_ENTRY_PAGE_SQL}"))?;
            let rows = statement.query_map(params!["unhandled", 5, i64::MIN, i64::MIN, 512], |row| {
                row.get::<_, String>(3)
            })?;
            rows.collect::<rusqlite::Result<Vec<_>>>()
        })
        .expect("explain expiry page")
        .join(" ");
    assert!(plan.contains("USING INDEX idx_propagation_peer_entries_expiry"), "{plan}");
    assert!(!plan.contains("SCAN propagation_peer_entries"), "{plan}");
    assert!(!plan.contains("TEMP B-TREE"), "{plan}");

    let page = store
        .select_expired_peer_entry_page("unhandled", 5, (1, 10))
        .expect("select keyset page after rowid 10");
    let ids: Vec<&str> = page.iter().map(|candidate| candidate.3.as_str()).collect();
    assert_eq!(ids, ["b", "c"], "cursor skips row 10, cutoff skips row 13, state skips row 14");
}

#[test]
fn peer_policy_prune_deletes_expired_marks_in_bounded_writer_pages() {
    const EXPIRED: usize = 2_048;
    let (_temp, store) = refill_store_on_disk();
    store
        .with_write_conn(|conn| {
            let tx = conn.unchecked_transaction()?;
            for index in 0..EXPIRED {
                let transient_id = format!("{index:064x}");
                tx.execute(
                    "INSERT INTO propagation_entries
                        (transient_id, destination, payload_hex, received_at, size_bytes, stamp_value)
                     VALUES (?1, ?2, '', 1, 0, NULL)",
                    params![transient_id, "11".repeat(16)],
                )?;
                tx.execute(
                    "INSERT INTO propagation_peer_entries (peer, transient_id, state, updated_at)
                     VALUES ('expired-peer', ?1, 'unhandled', 1)",
                    params![transient_id],
                )?;
            }
            tx.commit()
        })
        .expect("seed expired peer marks");

    let store = std::sync::Arc::new(store);
    let writes_before = store.contention_snapshot().write_ops_total;
    let prune_store = std::sync::Arc::clone(&store);
    let prune = std::thread::spawn(move || {
        prune_store
            .prune_propagation_peer_entries_to_policy(10_000, 100, 200, 10_000, 10_000)
            .expect("prune expired peer marks")
    });

    let started = std::time::Instant::now();
    store
        .insert_message(&MessageRecord {
            id: "priority-during-prune".to_string(),
            source: "aa".repeat(16),
            destination: "bb".repeat(16),
            title: String::new(),
            content: String::new(),
            timestamp: 1,
            direction: "in".to_string(),
            fields: None,
            receipt_status: None,
        })
        .expect("priority broker write while pruning");
    let broker_wait = started.elapsed();

    assert_eq!(prune.join().expect("prune thread"), EXPIRED);
    let writes_after = store.contention_snapshot().write_ops_total;
    let remaining: i64 = store
        .with_read_conn(|conn| {
            conn.query_row("SELECT COUNT(*) FROM propagation_peer_entries", [], |row| row.get(0))
        })
        .expect("count remaining marks");
    assert_eq!(remaining, 0);
    assert!(
        writes_after - writes_before >= (EXPIRED / PEER_ENTRY_PRUNE_BATCH) as u64,
        "expected one writer acquisition per bounded page, got {}",
        writes_after - writes_before
    );
    assert!(
        broker_wait < std::time::Duration::from_millis(500),
        "broker write waited {broker_wait:?}"
    );
}

#[test]
fn peer_policy_prune_rechecks_candidates_after_a_refresh() {
    let (_temp, store) = refill_store_on_disk();
    let entry = refill_entry(91, 1);
    store.upsert_propagation_entry(&entry).expect("seed inventory entry");
    store
        .with_write_conn(|conn| {
            conn.execute(
                "INSERT INTO propagation_peer_entries (peer, transient_id, state, updated_at)
                 VALUES ('refresh-peer', ?1, 'unhandled', 1)",
                params![entry.transient_id],
            )?;
            Ok(())
        })
        .expect("seed old peer mark");

    let candidate = store
        .with_read_conn(|conn| {
            conn.query_row(
                "SELECT rowid, updated_at FROM propagation_peer_entries WHERE peer = 'refresh-peer'",
                [],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, i64>(1)?,
                        "refresh-peer".to_string(),
                        entry.transient_id.clone(),
                    ))
                },
            )
        })
        .expect("select candidate");
    store
        .with_write_conn(|conn| {
            conn.execute(
                "UPDATE propagation_peer_entries SET state = 'handled', updated_at = 9 WHERE rowid = ?1",
                params![candidate.0],
            )?;
            Ok(())
        })
        .expect("refresh candidate before page delete");

    let deleted = store
        .delete_expired_peer_entry_candidates(&[candidate], 5, 5)
        .expect("delete selected candidates with recheck");
    let state: String = store
        .with_read_conn(|conn| {
            conn.query_row(
                "SELECT state FROM propagation_peer_entries WHERE peer = 'refresh-peer'",
                [],
                |row| row.get(0),
            )
        })
        .expect("read refreshed state");

    assert_eq!(deleted, 0);
    assert_eq!(state, "handled");
}

#[test]
fn propagation_maintenance_metrics_attribute_expiry_quota_and_payload() {
    let store = MessagesStore::in_memory().expect("store");
    store.with_write_conn(|conn| conn.execute_batch(
        "INSERT INTO propagation_entries VALUES
            ('one', 'destination', '00', 100, 1, NULL),
            ('two', 'destination', '00', 101, 1, NULL);
         INSERT INTO propagation_peer_entries VALUES
            ('peer', 'orphan', 'handled', 1),
            ('peer', 'one', 'unhandled', 100),
            ('peer', 'two', 'unhandled', 101);"
    )).expect("maintenance fixture");
    assert_eq!(store.prune_propagation_peer_entries_to_policy(105, 10, 10, 100, 1).expect("peer prune"), 2);
    let before_payload = store.operation_metrics_snapshot()["maintenance"].clone();
    for phase in ["expiry", "quota"] {
        assert_eq!(before_payload[phase]["runs_total"], 1);
        assert_eq!(before_payload[phase]["rows_deleted_total"], 1);
        assert!(before_payload[phase]["last_duration_ns"].as_u64().expect("duration") > 0);
        assert_eq!(before_payload[phase]["errors_total"], 0);
        if cfg!(target_os = "linux") {
            assert!(before_payload[phase]["last_logical_read_bytes"].is_u64(), "{phase}");
        }
    }
    assert_eq!(before_payload["payload"]["runs_total"], 0);
    assert_eq!(store.prune_propagation_entries_to_limit_bytes(0).expect("payload prune").len(), 2);
    let payload = store.operation_metrics_snapshot()["maintenance"]["payload"].clone();
    assert_eq!(payload["runs_total"], 1);
    assert_eq!(payload["rows_deleted_total"], 2);
    assert_eq!(payload["last_rows_deleted"], 2);
    assert!(payload["last_duration_ns"].as_u64().expect("duration") > 0);
}

#[test]
fn payload_prune_releases_writer_between_candidate_windows() {
    let store = MessagesStore::in_memory().expect("store");
    seed_payload_prune_windows(&store);
    let before = store.contention_snapshot().write_ops_total;
    let removed = store.prune_propagation_entries_to_limit_bytes(0).expect("paged payload prune");
    assert_eq!(removed.len(), 5000);
    assert!(store.contention_snapshot().write_ops_total - before >= 2, "each window has its own writer transaction");
    assert_eq!(store.operation_metrics_snapshot()["maintenance"]["payload"]["rows_deleted_total"], 5000);
}

#[test]
fn payload_prune_failure_preserves_prior_committed_pages_and_reports_unknown_count() {
    let store = MessagesStore::in_memory().expect("store");
    seed_payload_prune_windows(&store);
    store.with_write_conn(|conn| conn.execute_batch(
        "CREATE TRIGGER fail_second_window BEFORE DELETE ON propagation_entries
         WHEN OLD.transient_id='id-04097' BEGIN SELECT RAISE(ABORT, 'page failure'); END;"
    )).expect("fault injection");
    let error = store.prune_propagation_entries_to_limit_bytes(0).expect_err("second page fails");
    assert!(error.to_string().contains("page failure"));
    let remaining: i64 = store.with_read_conn(|conn| conn.query_row(
        "SELECT COUNT(*) FROM propagation_entries", [], |row| row.get(0)
    )).expect("remaining rows");
    assert_eq!(remaining, 904, "first page committed, failing page rolled back");
    let metrics = store.operation_metrics_snapshot()["maintenance"]["payload"].clone();
    assert_eq!(metrics["errors_total"], 1);
    assert!(metrics["last_rows_deleted"].is_null());
}

fn seed_payload_prune_windows(store: &MessagesStore) {
    store.with_write_conn(|conn| conn.execute_batch(
        "WITH RECURSIVE n(x) AS (VALUES(1) UNION ALL SELECT x+1 FROM n WHERE x<5000)
         INSERT INTO propagation_entries SELECT printf('id-%05d',x), 'destination', '00', x, 1, NULL FROM n;"
    )).expect("paged payload fixture");
}
