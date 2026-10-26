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
fn peer_policy_prune_keyset_page_respects_captured_upper_rowid() {
    let (_temp, store) = refill_store_on_disk();
    store
        .with_write_conn(|conn| {
            conn.execute(
                "INSERT INTO propagation_peer_entries
                    (rowid, peer, transient_id, state, updated_at)
                 VALUES (100, 'page-peer', 'before-boundary', 'unhandled', 1)",
                [],
            )?;
            Ok(())
        })
        .expect("seed initial row");
    let upper_rowid = store
        .with_read_conn(|conn| {
            conn.query_row("SELECT MAX(rowid) FROM propagation_peer_entries", [], |row| {
                row.get::<_, Option<i64>>(0)
            })
        })
        .expect("capture rowid boundary")
        .expect("row exists");
    store
        .with_write_conn(|conn| {
            conn.execute(
                "INSERT INTO propagation_peer_entries
                    (rowid, peer, transient_id, state, updated_at)
                 VALUES (101, 'page-peer', 'after-boundary', 'unhandled', 1)",
                [],
            )?;
            Ok(())
        })
        .expect("insert row after captured boundary");

    let page = store
        .select_expired_peer_entry_page(i64::MIN, upper_rowid, 5, 5)
        .expect("select bounded keyset page");

    assert_eq!(page.len(), 1);
    assert_eq!(page[0].3, "before-boundary");
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
