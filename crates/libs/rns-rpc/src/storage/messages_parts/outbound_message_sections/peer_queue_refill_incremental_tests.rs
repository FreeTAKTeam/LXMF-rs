// Issue 663 item 3: refill coalescing (read-only gate) and rowid-floor
// incremental rescans. Stores are opened on disk so `with_read_conn` uses the
// separate read connection; `in_memory()` would route reads through the write
// mutex and hide the behavior under test.

fn refill_store_on_disk() -> (tempfile::TempDir, MessagesStore) {
    let temp = tempfile::tempdir().expect("temp dir");
    let store = MessagesStore::open(&temp.path().join("refill.db")).expect("open store");
    (temp, store)
}

fn refill_counters(store: &MessagesStore) -> serde_json::Value {
    store.operation_metrics_snapshot()["refill"].clone()
}

fn refill_floor(store: &MessagesStore, peer: &str) -> (Option<i64>, Option<i64>, bool, bool) {
    store
        .with_read_conn(|conn| {
            conn.query_row(
                "SELECT floor_rowid, cursor_rowid, scan_complete, rescan_required
                 FROM propagation_peer_refill_state WHERE peer = ?1",
                params![peer],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
        })
        .expect("refill state")
}

fn seed_known_inventory(store: &MessagesStore, peer: &str, count: usize) {
    store
        .with_write_conn(|conn| {
            let tx = conn.unchecked_transaction()?;
            for index in 0..count {
                let entry = refill_entry(index, index as i64);
                tx.execute(
                    "INSERT INTO propagation_entries
                        (transient_id, destination, payload_hex, received_at, size_bytes, stamp_value)
                     VALUES (?1, ?2, ?3, ?4, ?5, NULL)",
                    params![
                        entry.transient_id,
                        entry.destination,
                        entry.payload_hex,
                        entry.received_at,
                        entry.size_bytes as i64
                    ],
                )?;
                tx.execute(
                    "INSERT INTO propagation_peer_entries (peer, transient_id, state, updated_at)
                     VALUES (?1, ?2, 'handled', ?3)",
                    params![peer, entry.transient_id, entry.received_at],
                )?;
            }
            tx.commit()
        })
        .expect("seed inventory");
}

#[test]
fn refill_repeat_with_complete_unchanged_inventory_takes_no_write_lock() {
    let (_temp, store) = refill_store_on_disk();
    let peer = "peer-gate-complete";
    for index in 0..300 {
        store.upsert_propagation_entry(&refill_entry(index, index as i64)).expect("insert");
    }
    assert_eq!(store.mark_recent_propagation_unhandled_for_peer(peer, 1_000, u64::MAX).expect("first"), 300);
    let before_state = refill_state(&store, peer).expect("state after pass");
    assert!(before_state.scan_complete);
    let writes_before = store.contention_snapshot().write_ops_total;
    let skipped_before = refill_counters(&store)["skipped_complete_total"].as_u64().unwrap_or(0);

    assert_eq!(store.mark_recent_propagation_unhandled_for_peer(peer, 1_000, u64::MAX).expect("repeat"), 0);

    assert_eq!(store.contention_snapshot().write_ops_total, writes_before, "repeat refill took the writer");
    assert_eq!(refill_counters(&store)["skipped_complete_total"], skipped_before + 1);
    assert_eq!(refill_state(&store, peer).expect("state unchanged"), before_state);
}

#[test]
fn refill_with_full_pending_queue_takes_no_write_lock() {
    let (_temp, store) = refill_store_on_disk();
    let peer = "peer-gate-capacity";
    for index in 0..12 {
        store.upsert_propagation_entry(&refill_entry(index, index as i64)).expect("insert");
    }
    assert_eq!(store.mark_recent_propagation_unhandled_for_peer(peer, 4, u64::MAX).expect("fill"), 4);
    let writes_before = store.contention_snapshot().write_ops_total;
    assert_eq!(store.mark_recent_propagation_unhandled_for_peer(peer, 4, u64::MAX).expect("full"), 0);
    assert_eq!(store.contention_snapshot().write_ops_total, writes_before);
    assert_eq!(refill_counters(&store)["skipped_capacity_total"], 1);
}

#[test]
fn incremental_pass_examines_at_most_one_page_after_single_insert_into_known_inventory() {
    let (_temp, store) = refill_store_on_disk();
    let peer = "peer-incremental";
    seed_known_inventory(&store, peer, 20_000);
    assert_eq!(store.mark_recent_propagation_unhandled_for_peer(peer, 50_000, u64::MAX).expect("full pass"), 0);
    let (floor, cursor, complete, rescan) = refill_floor(&store, peer);
    assert_eq!(floor, Some(20_000), "full pass records the newest rowid as floor");
    assert_eq!(cursor, None);
    assert!(complete && !rescan);
    let examined_before = refill_counters(&store)["candidates_examined_total"].as_u64().unwrap_or(0);

    store.upsert_propagation_entry(&refill_entry(30_000, 30_000)).expect("newer entry");
    assert_eq!(store.mark_recent_propagation_unhandled_for_peer(peer, 50_000, u64::MAX).expect("incremental"), 1);

    let examined = refill_counters(&store)["candidates_examined_total"].as_u64().unwrap_or(0);
    assert!(examined - examined_before <= 129, "examined {} candidates", examined - examined_before);
    let (floor, cursor, complete, _) = refill_floor(&store, peer);
    assert_eq!(floor, Some(20_001));
    assert_eq!(cursor, None);
    assert!(complete);
    assert_eq!(
        store.list_peer_unhandled_propagation_ids(peer).expect("pending"),
        vec![format!("{:064x}", 30_000)]
    );
}

#[test]
fn incremental_pass_catches_out_of_order_received_at() {
    let (_temp, store) = refill_store_on_disk();
    let peer = "peer-out-of-order";
    seed_known_inventory(&store, peer, 2_000);
    assert_eq!(store.mark_recent_propagation_unhandled_for_peer(peer, 50_000, u64::MAX).expect("full pass"), 0);
    let examined_before = refill_counters(&store)["candidates_examined_total"].as_u64().unwrap_or(0);

    // Older timestamp than every known row (remote import keeps the remote clock),
    // and a same-second arrival whose id sorts below the head's id.
    store.upsert_propagation_entry(&refill_entry(5_000, 0)).expect("old timestamp");
    let head_time = 1_999;
    let mut same_second = refill_entry(0, head_time);
    // '/' sorts below every hex digit, so this id is new and sorts below the head's id.
    same_second.transient_id = "0".repeat(63) + "/";
    store.upsert_propagation_entry(&same_second).expect("same-second arrival");

    let offered = store.mark_recent_propagation_unhandled_for_peer(peer, 50_000, u64::MAX).expect("incremental");
    assert_eq!(offered, 2);
    let examined = refill_counters(&store)["candidates_examined_total"].as_u64().unwrap_or(0);
    assert!(examined - examined_before <= 130, "examined {}", examined - examined_before);
    let mut pending = store.list_peer_unhandled_propagation_ids(peer).expect("pending");
    pending.sort();
    let mut expected = vec![format!("{:064x}", 5_000), same_second.transient_id.clone()];
    expected.sort();
    assert_eq!(pending, expected);
}

#[test]
fn refill_offers_reused_rowid_after_pruning_the_inventory_tail() {
    let (_temp, store) = refill_store_on_disk();
    let peer = "peer-reused-rowid";
    seed_known_inventory(&store, peer, 3);
    store.upsert_propagation_entry(&refill_entry(2, -100)).expect("make tail oldest");
    assert_eq!(store.mark_recent_propagation_unhandled_for_peer(peer, 100, u64::MAX).unwrap(), 0);
    assert_eq!(refill_floor(&store, peer).0, Some(3));

    assert_eq!(store.prune_propagation_entries_to_limit_bytes(16).unwrap(), vec![format!("{:064x}", 2)]);
    store.upsert_propagation_entry(&refill_entry(900, 900)).expect("reuse pruned rowid");

    assert_eq!(store.mark_recent_propagation_unhandled_for_peer(peer, 100, u64::MAX).unwrap(), 1);
    assert_eq!(store.list_peer_unhandled_propagation_ids(peer).unwrap(), vec![format!("{:064x}", 900)]);
}

#[test]
fn refill_offers_rowid_reused_between_pages_on_the_next_invocation() {
    for incremental in [false, true] {
        let (_temp, store) = refill_store_on_disk();
        let peer = "peer-reused-between-pages";
        seed_known_inventory(&store, peer, 260);
        if incremental {
            assert_eq!(store.mark_recent_propagation_unhandled_for_peer(peer, 1_000, u64::MAX).unwrap(), 0);
            for index in 260..520 {
                let entry = refill_entry(index, index as i64);
                store.upsert_propagation_entry(&entry).unwrap();
                store.mark_peer_handled_propagation(peer, &entry.transient_id).unwrap();
            }
        }
        let boundary = store.with_read_conn(MessagesStore::capture_peer_queue_refill_boundary).unwrap();
        let page = store.with_write_conn(|conn| {
            store.refill_recent_peer_propagation_on(conn, peer, 1_000, u64::MAX, &boundary, &mut None)
        }).unwrap();
        assert!(!page.pass_finished);
        store.with_write_conn(|conn| {
            conn.execute("DELETE FROM propagation_entries WHERE rowid = ?1", params![boundary.max_rowid])
        }).unwrap();
        store.upsert_propagation_entry(&refill_entry(900, 900)).unwrap();
        loop {
            let page = store.with_write_conn(|conn| {
                store.refill_recent_peer_propagation_on(conn, peer, 1_000, u64::MAX, &boundary, &mut None)
            }).unwrap();
            assert_eq!(page.inserted, 0);
            if page.pass_finished {
                break;
            }
        }

        assert_eq!(store.mark_recent_propagation_unhandled_for_peer(peer, 1_000, u64::MAX).unwrap(), 1,
            "rowid reused during incremental={incremental} pass was skipped");
        assert_eq!(store.list_peer_unhandled_propagation_ids(peer).unwrap(), vec![format!("{:064x}", 900)]);
    }
}

#[test]
fn refill_offers_inventory_reordered_between_pages_on_the_next_invocation() {
    for incremental in [false, true] {
        let (_temp, store) = refill_store_on_disk();
        let peer = "peer-reordered-between-pages";
        seed_known_inventory(&store, peer, 260);
        if incremental {
            assert_eq!(store.mark_recent_propagation_unhandled_for_peer(peer, 1_000, u64::MAX).unwrap(), 0);
        }
        store.upsert_propagation_entry(&refill_entry(900, -100)).unwrap();
        if incremental {
            for index in 260..520 {
                let entry = refill_entry(index, index as i64);
                store.upsert_propagation_entry(&entry).unwrap();
                store.mark_peer_handled_propagation(peer, &entry.transient_id).unwrap();
            }
        }
        let boundary = store.with_read_conn(MessagesStore::capture_peer_queue_refill_boundary).unwrap();
        let page = store.with_write_conn(|conn| {
            store.refill_recent_peer_propagation_on(conn, peer, 1_000, u64::MAX, &boundary, &mut None)
        }).unwrap();
        assert!(!page.pass_finished);
        store.upsert_propagation_entry(&refill_entry(900, 900)).unwrap();
        loop {
            let page = store.with_write_conn(|conn| {
                store.refill_recent_peer_propagation_on(conn, peer, 1_000, u64::MAX, &boundary, &mut None)
            }).unwrap();
            assert_eq!(page.inserted, 0);
            if page.pass_finished {
                break;
            }
        }

        assert_eq!(store.mark_recent_propagation_unhandled_for_peer(peer, 1_000, u64::MAX).unwrap(), 1,
            "inventory reordered during incremental={incremental} pass was skipped");
    }
}

#[test]
fn received_at_update_forces_full_rescan() {
    let (_temp, store) = refill_store_on_disk();
    let peer = "peer-reorder";
    seed_known_inventory(&store, peer, 300);
    // One entry the peer was never offered, dated before the rest.
    store.upsert_propagation_entry(&refill_entry(900, 0)).expect("unoffered old entry");
    // Hide it from this pass by marking it handled, then complete the pass.
    store.mark_peer_handled_propagation(peer, &format!("{:064x}", 900)).expect("mark");
    assert_eq!(store.mark_recent_propagation_unhandled_for_peer(peer, 50_000, u64::MAX).expect("pass"), 0);
    // Reordering a row must flag every peer for a full rescan.
    store
        .with_write_conn(|conn| {
            conn.execute(
                "UPDATE propagation_entries SET received_at = 5000 WHERE transient_id = ?1",
                params![format!("{:064x}", 900)],
            )
            .map(|_| ())
        })
        .expect("reorder");
    let (_, _, _, rescan) = refill_floor(&store, peer);
    assert!(rescan, "received_at update sets rescan_required");
    assert_eq!(store.mark_recent_propagation_unhandled_for_peer(peer, 50_000, u64::MAX).expect("rescan"), 0);
    let (floor, _, complete, rescan) = refill_floor(&store, peer);
    assert!(complete && !rescan);
    assert_eq!(floor, Some(301));
}

#[test]
fn refill_resumes_after_capacity_frees_via_rescan_required() {
    let (_temp, store) = refill_store_on_disk();
    let peer = "peer-capacity-freed";
    for index in 0..12 {
        store.upsert_propagation_entry(&refill_entry(index, index as i64)).expect("insert");
    }
    assert_eq!(store.mark_recent_propagation_unhandled_for_peer(peer, 4, u64::MAX).expect("fill"), 4);
    // Deliver one: the handled mark outranks pending, freeing a slot, and the
    // pending row's removal flags a rescan.
    let delivered = store.list_peer_unhandled_propagation_ids(peer).expect("pending")[0].clone();
    store.mark_peer_handled_propagation(peer, &delivered).expect("handled");
    store
        .with_write_conn(|conn| {
            conn.execute(
                "DELETE FROM propagation_peer_entries WHERE LOWER(peer) = LOWER(?1) AND transient_id = ?2 AND state = 'unhandled'",
                params![peer, delivered],
            )
            .map(|_| ())
        })
        .expect("remove delivered pending row");
    let writes_before = store.contention_snapshot().write_ops_total;
    assert_eq!(store.mark_recent_propagation_unhandled_for_peer(peer, 4, u64::MAX).expect("refill"), 1);
    assert!(store.contention_snapshot().write_ops_total > writes_before, "refill took the write path");
    assert_eq!(store.list_peer_unhandled_propagation_ids(peer).expect("pending").len(), 4);
}

#[test]
fn legacy_refill_state_without_floor_does_full_rescan_once() {
    let (_temp, store) = refill_store_on_disk();
    let peer = "peer-legacy-floor";
    seed_known_inventory(&store, peer, 500);
    assert_eq!(store.mark_recent_propagation_unhandled_for_peer(peer, 50_000, u64::MAX).expect("pass"), 0);
    // Simulate a row written before floor_rowid existed.
    store
        .with_write_conn(|conn| {
            conn.execute(
                "UPDATE propagation_peer_refill_state SET floor_rowid = NULL WHERE peer = ?1",
                params![peer],
            )
            .map(|_| ())
        })
        .expect("drop floor");
    store.upsert_propagation_entry(&refill_entry(900, 900)).expect("new inventory");
    let examined_before = refill_counters(&store)["candidates_examined_total"].as_u64().unwrap_or(0);
    assert_eq!(store.mark_recent_propagation_unhandled_for_peer(peer, 50_000, u64::MAX).expect("legacy"), 1);
    let examined = refill_counters(&store)["candidates_examined_total"].as_u64().unwrap_or(0);
    assert!(examined - examined_before >= 500, "legacy state walks the inventory once");
    let (floor, _, complete, _) = refill_floor(&store, peer);
    assert_eq!(floor, Some(501));
    assert!(complete);

    store.upsert_propagation_entry(&refill_entry(901, 901)).expect("more inventory");
    let examined_before = examined;
    assert_eq!(store.mark_recent_propagation_unhandled_for_peer(peer, 50_000, u64::MAX).expect("incremental"), 1);
    let examined = refill_counters(&store)["candidates_examined_total"].as_u64().unwrap_or(0);
    assert!(examined - examined_before <= 129);
}

#[test]
fn priority_writer_lock_wait_stays_bounded_during_multi_page_refill() {
    let (_temp, store) = refill_store_on_disk();
    let store = std::sync::Arc::new(store);
    let peer = "peer-fairness";
    seed_known_inventory(&store, peer, 20_000);
    // Force a scan of every page: the handled marks are removed so each page inserts.
    store
        .with_write_conn(|conn| {
            conn.execute("DELETE FROM propagation_peer_entries", []).map(|_| ())
        })
        .expect("clear marks");
    let refill_store = std::sync::Arc::clone(&store);
    let refill = std::thread::spawn(move || {
        refill_store.mark_recent_propagation_unhandled_for_peer(peer, 50_000, u64::MAX).expect("refill")
    });
    let mut max_wait = std::time::Duration::ZERO;
    for index in 0..50 {
        let started = std::time::Instant::now();
        store
            .insert_message(&MessageRecord {
                id: format!("fairness-{index}"),
                source: "aa".repeat(16),
                destination: "bb".repeat(16),
                title: String::new(),
                content: String::new(),
                timestamp: index,
                direction: "in".to_string(),
                fields: None,
                receipt_status: None,
            })
            .expect("priority write");
        max_wait = max_wait.max(started.elapsed());
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    assert_eq!(refill.join().expect("refill thread"), 20_000);
    assert!(max_wait < std::time::Duration::from_millis(500), "priority write waited {max_wait:?}");
    assert!(refill_counters(&store)["priority_yields_total"].as_u64().unwrap_or(0) > 0);
}
