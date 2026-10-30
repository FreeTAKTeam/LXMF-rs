// Refill respects the global peer-entry limit enforced by the quota pass.
// Without it, quota deleted one mark per pass and refill recreated the same
// (peer, transient) pair, so peer history never settled.

const BUDGET_TTL: u64 = 1_000_000;

fn budget_inventory(store: &MessagesStore, count: usize) {
    for index in 0..count {
        store.upsert_propagation_entry(&refill_entry(index, index as i64 + 1)).expect("inventory entry");
    }
}

fn insert_peer_mark(store: &MessagesStore, peer: &str, index: usize, state: &str, updated_at: i64) {
    store
        .with_write_conn(|conn| {
            conn.execute(
                "INSERT INTO propagation_peer_entries (peer, transient_id, state, updated_at)
                 VALUES (?1, ?2, ?3, ?4)",
                params![peer, format!("{index:064x}"), state, updated_at],
            )?;
            Ok(())
        })
        .expect("peer mark");
}

/// (exact COUNT(*), trigger-maintained budget counter)
fn peer_entry_counts(store: &MessagesStore) -> (i64, i64) {
    store
        .with_read_conn(|conn| {
            conn.query_row(
                "SELECT (SELECT COUNT(*) FROM propagation_peer_entries),
                        (SELECT entry_count FROM propagation_peer_entry_budget WHERE singleton = 1)",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
        })
        .expect("peer entry counts")
}

fn assert_budget_counter_exact(store: &MessagesStore) -> i64 {
    let (exact, counter) = peer_entry_counts(store);
    assert_eq!(counter, exact, "budget counter must mirror COUNT(*)");
    exact
}

/// One maintenance quota pass followed by one refill per peer, all with fixed
/// limits; returns (marks deleted, marks inserted).
fn budget_cycle(
    store: &MessagesStore,
    peers: &[&str],
    completed_ttl: u64,
    total_limit: u64,
    per_peer: u64,
) -> (usize, usize) {
    let deleted = store
        .prune_propagation_peer_entries_to_policy(now_unix_secs(), BUDGET_TTL, completed_ttl, total_limit, per_peer)
        .expect("quota pass");
    let inserted = peers
        .iter()
        .map(|peer| store.mark_recent_propagation_unhandled_for_peer(peer, per_peer, total_limit).expect("refill"))
        .sum();
    (deleted, inserted)
}

/// Two inventory items; peer-a holds both as unhandled, peer-b one completed mark.
fn churn_reproduction() -> (tempfile::TempDir, MessagesStore) {
    let (temp, store) = refill_store_on_disk();
    budget_inventory(&store, 2);
    let now = now_unix_secs();
    insert_peer_mark(&store, "peer-a", 0, "unhandled", now - 200);
    insert_peer_mark(&store, "peer-a", 1, "unhandled", now - 199);
    insert_peer_mark(&store, "peer-b", 0, "handled", now - 300);
    (temp, store)
}

#[test]
fn quota_refill_cycle_converges_when_the_global_limit_is_full() {
    let (_temp, store) = churn_reproduction();
    let cycles: Vec<_> = (0..6).map(|_| budget_cycle(&store, &["peer-a", "peer-b"], BUDGET_TTL, 2, 2)).collect();
    // Before the fix every cycle was (1, 1): quota removed peer-a's older mark and
    // refill recreated it.
    assert_eq!(cycles[0], (1, 0), "quota trims once, refill defers");
    assert!(cycles[1..].iter().all(|cycle| *cycle == (0, 0)), "{cycles:?}");
    assert_eq!(assert_budget_counter_exact(&store), 2);
    assert!(refill_counters(&store)["skipped_global_capacity_total"].as_u64().expect("counter") >= 5);
    let state: String = store
        .with_read_conn(|conn| {
            conn.query_row(
                "SELECT state FROM propagation_peer_entries WHERE peer = 'peer-b'",
                [],
                |row| row.get(0),
            )
        })
        .expect("completed mark");
    assert_eq!(state, "handled", "completed history is kept");

    // Control: a global limit of three never trims or refills.
    let (_control_temp, control) = churn_reproduction();
    for _ in 0..6 {
        assert_eq!(budget_cycle(&control, &["peer-a", "peer-b"], BUDGET_TTL, 3, 2), (0, 0));
    }
    assert_eq!(assert_budget_counter_exact(&control), 3);
}

#[test]
fn preview32_reproduction_refilling_peer_a_settles_at_two_entries() {
    // The reported preview32 sequence on 8a69bfea was (1, 1) on all six cycles
    // with three rows left, and (0, 0) for the global-limit-of-three control.
    for (total_limit, expected_first, rows) in [(2, (1, 0), 2), (3, (0, 0), 3)] {
        let (_temp, store) = churn_reproduction();
        let cycles: Vec<_> = (0..6).map(|_| budget_cycle(&store, &["peer-a"], BUDGET_TTL, total_limit, 2)).collect();
        assert_eq!(cycles[0], expected_first, "total_limit={total_limit}");
        assert!(cycles[1..].iter().all(|cycle| *cycle == (0, 0)), "{cycles:?}");
        assert_eq!(assert_budget_counter_exact(&store), rows);
    }
}

#[test]
fn per_peer_trim_stays_quiet_until_a_pending_mark_completes() {
    let (_temp, store) = refill_store_on_disk();
    budget_inventory(&store, 3);
    let now = now_unix_secs();
    for index in 0..3 {
        insert_peer_mark(&store, "peer", index, "unhandled", now - 100 + index as i64);
    }
    assert_eq!(budget_cycle(&store, &["peer"], BUDGET_TTL, 100, 2), (1, 0), "per-peer trim, queue full");
    for _ in 0..3 {
        assert_eq!(budget_cycle(&store, &["peer"], BUDGET_TTL, 100, 2), (0, 0));
    }
    // Completing a surviving mark frees one per-peer slot; the trimmed pair returns once.
    store.mark_peer_handled_propagation("peer", &format!("{:064x}", 2)).expect("handled");
    assert_eq!(budget_cycle(&store, &["peer"], BUDGET_TTL, 100, 2), (0, 1));
    for _ in 0..3 {
        assert_eq!(budget_cycle(&store, &["peer"], BUDGET_TTL, 100, 2), (0, 0));
    }
    assert_eq!(assert_budget_counter_exact(&store), 3);
}

#[test]
fn deferred_refill_resumes_when_completed_history_expires_then_settles() {
    let (_temp, store) = churn_reproduction();
    for _ in 0..3 {
        budget_cycle(&store, &["peer-a", "peer-b"], BUDGET_TTL, 2, 2);
    }
    // A 250 s completed TTL now expires peer-b's 300 s old mark; peer-a's
    // deferred pair is offered again and the next cycles are quiet.
    let completed_ttl = 250;
    assert_eq!(budget_cycle(&store, &["peer-a", "peer-b"], completed_ttl, 2, 2), (1, 1));
    for _ in 0..4 {
        assert_eq!(budget_cycle(&store, &["peer-a", "peer-b"], completed_ttl, 2, 2), (0, 0));
    }
    let pending: i64 = store
        .with_read_conn(|conn| {
            conn.query_row(
                "SELECT COUNT(*) FROM propagation_peer_entries WHERE peer = 'peer-a' AND state = 'unhandled'",
                [],
                |row| row.get(0),
            )
        })
        .expect("peer-a pending");
    assert_eq!(pending, 2);
    assert_eq!(assert_budget_counter_exact(&store), 2);
}

#[test]
fn refill_inserts_only_the_remaining_global_room() {
    for (total_limit, expected) in [(5, 0), (4, 0), (8, 3), (15, 10), (0, 0)] {
        let (_temp, store) = refill_store_on_disk();
        budget_inventory(&store, 10);
        for index in 0..5 {
            insert_peer_mark(&store, "history", 100 + index, "handled", 1);
        }
        // A 128-candidate page exceeds every remaining room here.
        let inserted = store.mark_recent_propagation_unhandled_for_peer("peer", 100, total_limit).expect("refill");
        assert_eq!(inserted, expected, "total_limit={total_limit}");
        assert_eq!(assert_budget_counter_exact(&store), 5 + expected as i64);
        // Repeating with unchanged state inserts nothing more.
        assert_eq!(store.mark_recent_propagation_unhandled_for_peer("peer", 100, total_limit).expect("repeat"), 0);
    }
}

#[test]
fn concurrent_refills_on_two_connections_cannot_exceed_the_global_limit() {
    const TOTAL: u64 = 37;
    let temp = tempfile::tempdir().expect("temp dir");
    let path = temp.path().join("budget.db");
    let first = std::sync::Arc::new(MessagesStore::open(&path).expect("first connection set"));
    let second = std::sync::Arc::new(MessagesStore::open(&path).expect("second connection set"));
    budget_inventory(&first, 30);
    let workers: Vec<_> = (0..8)
        .map(|worker| {
            let store = std::sync::Arc::clone(if worker % 2 == 0 { &first } else { &second });
            std::thread::spawn(move || {
                let peer = format!("peer-{}", worker % 6);
                (0..5)
                    .map(|_| store.mark_recent_propagation_unhandled_for_peer(&peer, 30, TOTAL).expect("refill"))
                    .sum::<usize>()
            })
        })
        .collect();
    let inserted: usize = workers.into_iter().map(|worker| worker.join().expect("worker")).sum();
    assert_eq!(inserted as u64, TOTAL, "every slot is used exactly once");
    assert_eq!(assert_budget_counter_exact(&first) as u64, TOTAL);
}

#[test]
fn deferred_candidates_survive_reopen_and_resume_when_the_limit_grows() {
    let temp = tempfile::tempdir().expect("temp dir");
    let path = temp.path().join("deferred.db");
    {
        let store = MessagesStore::open(&path).expect("open");
        budget_inventory(&store, 10);
        assert_eq!(store.mark_recent_propagation_unhandled_for_peer("peer", 100, 4).expect("partial"), 4);
        assert_eq!(refill_counters(&store)["deferred_global_capacity_total"], 1);
        let complete: bool = store
            .with_read_conn(|conn| {
                conn.query_row(
                    "SELECT scan_complete FROM propagation_peer_refill_state WHERE peer = 'peer'",
                    [],
                    |row| row.get(0),
                )
            })
            .expect("refill state");
        assert!(!complete, "a deferred pass must not be recorded as complete");
    }
    let store = MessagesStore::open(&path).expect("reopen");
    assert_eq!(store.mark_recent_propagation_unhandled_for_peer("peer", 100, 4).expect("still full"), 0);
    assert_eq!(store.mark_recent_propagation_unhandled_for_peer("peer", 100, 100).expect("resume"), 6);
    let distinct: i64 = store
        .with_read_conn(|conn| {
            conn.query_row(
                "SELECT COUNT(DISTINCT transient_id) FROM propagation_peer_entries WHERE peer = 'peer'",
                [],
                |row| row.get(0),
            )
        })
        .expect("distinct marks");
    assert_eq!(distinct, 10, "every deferred candidate offered once");
    assert_eq!(store.mark_recent_propagation_unhandled_for_peer("peer", 100, 100).expect("settled"), 0);
}

#[test]
fn per_peer_capacity_change_still_refills_under_ample_global_room() {
    let (_temp, store) = refill_store_on_disk();
    budget_inventory(&store, 6);
    assert_eq!(store.mark_recent_propagation_unhandled_for_peer("peer", 2, 1_000).expect("fill"), 2);
    assert_eq!(store.mark_recent_propagation_unhandled_for_peer("peer", 2, 1_000).expect("full"), 0);
    assert_eq!(store.mark_recent_propagation_unhandled_for_peer("peer", 4, 1_000).expect("raised"), 2);
    assert_eq!(assert_budget_counter_exact(&store), 4);
}

#[test]
fn freed_global_room_is_shared_between_waiting_peers() {
    let (_temp, store) = refill_store_on_disk();
    budget_inventory(&store, 10);
    for index in 0..4 {
        insert_peer_mark(&store, "history", 100 + index, "handled", 1);
    }
    // Both peers record an unfinished pass while the limit is full.
    for peer in ["peer-a", "peer-b"] {
        assert_eq!(store.mark_recent_propagation_unhandled_for_peer(peer, 10, 5).expect("first slot"), usize::from(peer == "peer-a"));
    }
    // Four slots free up. Each invocation takes ceil(room / (other waiting peers + 1)),
    // so the first caller cannot take all of them.
    store
        .with_write_conn(|conn| conn.execute("DELETE FROM propagation_peer_entries WHERE peer = 'history'", []))
        .expect("free room");
    let shares: Vec<usize> = ["peer-a", "peer-b", "peer-b", "peer-a"]
        .iter()
        .map(|peer| store.mark_recent_propagation_unhandled_for_peer(peer, 10, 5).expect("share"))
        .collect();
    assert_eq!(shares, [2, 1, 1, 0]);
    assert_eq!(assert_budget_counter_exact(&store), 5);
}

#[test]
fn ample_global_room_is_not_rationed_across_stale_peers() {
    let (_temp, store) = refill_store_on_disk();
    budget_inventory(&store, 300);
    for peer in 0..200 {
        store.mark_recent_propagation_unhandled_for_peer(&format!("old-{peer}"), 1, 10_000).expect("old peer");
    }
    // New inventory makes every saved pass stale; 9,800 free slots still cover the need.
    store.upsert_propagation_entry(&refill_entry(5_000, 9_999)).expect("new inventory");
    assert_eq!(store.mark_recent_propagation_unhandled_for_peer("newcomer", 100, 10_000).expect("newcomer"), 100);
}

#[test]
fn failed_refill_page_consumes_no_room_and_keeps_progress() {
    let (_temp, store) = refill_store_on_disk();
    budget_inventory(&store, 6);
    store
        .with_write_conn(|conn| {
            conn.execute_batch(&format!(
                "CREATE TRIGGER fail_refill_insert BEFORE INSERT ON propagation_peer_entries
                 WHEN NEW.transient_id = '{:064x}' BEGIN SELECT RAISE(ABORT, 'refill failure'); END;",
                2
            ))
        })
        .expect("fault injection");
    let error = store.mark_recent_propagation_unhandled_for_peer("peer", 10, 4).expect_err("page fails");
    assert!(error.to_string().contains("refill failure"));
    assert_eq!(assert_budget_counter_exact(&store), 0, "rolled-back inserts are not counted");
    let saved: Option<i64> = store
        .with_read_conn(|conn| {
            conn.query_row("SELECT COUNT(*) FROM propagation_peer_refill_state", [], |row| row.get(0))
        })
        .ok();
    assert_eq!(saved, Some(0), "no refill progress recorded for the failed page");
    store
        .with_write_conn(|conn| conn.execute_batch("DROP TRIGGER fail_refill_insert;"))
        .expect("remove fault");
    assert_eq!(store.mark_recent_propagation_unhandled_for_peer("peer", 10, 4).expect("retry"), 4);
    assert_eq!(assert_budget_counter_exact(&store), 4);
}

#[test]
fn budget_counter_tracks_every_peer_entry_write_path_and_quota_repairs_drift() {
    let (_temp, store) = refill_store_on_disk();
    budget_inventory(&store, 6);
    let id = |index: usize| format!("{index:064x}");
    store.mark_peer_unhandled_propagation("Peer-X", &id(0)).expect("fanout");
    store.mark_peer_unhandled_propagation("peer-x", &id(0)).expect("ignored duplicate");
    store.mark_peer_handled_propagation("peer-x", &id(0)).expect("upsert");
    store.mark_peer_transferred_propagation("peer-y", &id(1)).expect("insert");
    store.mark_peer_received_propagation("peer-y", &id(1)).expect("upsert");
    store.mark_peer_transfer_limited_propagation("peer-z", &id(2)).expect("insert");
    store.import_peer_queue_marks("peer-w", &[id(3)], &[id(4), id(5)]).expect("import");
    insert_peer_mark(&store, "PEER-W", 3, "unhandled", 1);
    store.merge_case_insensitive_peer_propagation_marks("peer-w").expect("merge");
    assert_budget_counter_exact(&store);
    store.remove_peer_unhandled_propagation("peer-w", &id(4)).expect("remove");
    store.mark_recent_propagation_unhandled_for_peer("peer-v", 3, 100).expect("refill");
    assert_budget_counter_exact(&store);
    store.prune_propagation_peer_entries_to_policy(now_unix_secs(), BUDGET_TTL, BUDGET_TTL, 3, 1).expect("quota");
    assert_eq!(assert_budget_counter_exact(&store), 3);

    store
        .with_write_conn(|conn| conn.execute("UPDATE propagation_peer_entry_budget SET entry_count = 999", []))
        .expect("simulate drift");
    assert_eq!(
        store.prune_propagation_peer_entries_to_policy(now_unix_secs(), BUDGET_TTL, BUDGET_TTL, 1_000, 1_000).expect("idle quota"),
        0
    );
    assert_eq!(assert_budget_counter_exact(&store), 3, "quota rewrites a drifted counter");
}

#[test]
fn budget_counter_is_backfilled_for_existing_databases() {
    let temp = tempfile::tempdir().expect("temp dir");
    let path = temp.path().join("pre-budget.db");
    {
        let store = MessagesStore::open(&path).expect("open");
        store
            .with_write_conn(|conn| {
                conn.execute_batch(
                    "DROP TRIGGER propagation_peer_entry_budget_insert;
                     DROP TRIGGER propagation_peer_entry_budget_delete;
                     DROP TABLE propagation_peer_entry_budget;",
                )
            })
            .expect("simulate pre-budget schema");
        for index in 0..7 {
            insert_peer_mark(&store, "peer", index, "handled", 1);
        }
    }
    let store = MessagesStore::open(&path).expect("reopen");
    assert_eq!(assert_budget_counter_exact(&store), 7);
    insert_peer_mark(&store, "peer", 7, "unhandled", 1);
    assert_eq!(assert_budget_counter_exact(&store), 8);
}

/// A full limit over a large nonexpired history: every refill (each announce
/// or sync) is answered from the counter without the writer, a pending count
/// or a scan.
fn assert_full_budget_refill_is_cheap(history: usize, attempts: usize) {
    let (_temp, store) = refill_store_on_disk();
    budget_inventory(&store, 64);
    store
        .with_write_conn(|conn| {
            let tx = conn.unchecked_transaction()?;
            {
                let mut insert = tx.prepare(
                    "INSERT INTO propagation_peer_entries (peer, transient_id, state, updated_at)
                     VALUES (?1, ?2, 'handled', 1)",
                )?;
                for index in 0..history {
                    insert.execute(params![format!("history-{}", index % 64), format!("{:064x}", 1_000_000 + index)])?;
                }
            }
            tx.commit()
        })
        .expect("nonexpired history");
    let total = history as u64;
    // A deletion of a still-present payload sets rescan_required, like a quota trim.
    insert_peer_mark(&store, "peer", 0, "unhandled", 1);
    assert_eq!(store.mark_recent_propagation_unhandled_for_peer("peer", 10, total).expect("full"), 0);
    store
        .with_write_conn(|conn| conn.execute("DELETE FROM propagation_peer_entries WHERE peer = 'peer'", []))
        .expect("trim");

    // The trim removed the peer's last mark and with it the saved pass; the
    // first deferred call records it again, once.
    assert_eq!(store.mark_recent_propagation_unhandled_for_peer("peer", 10, total).expect("record"), 0);
    let writes = store.contention_snapshot().write_ops_total;
    let started = std::time::Instant::now();
    for _ in 0..attempts {
        assert_eq!(store.mark_recent_propagation_unhandled_for_peer("peer", 10, total).expect("deferred"), 0);
    }
    let elapsed = started.elapsed();
    assert_eq!(store.contention_snapshot().write_ops_total, writes, "no writer while the limit is full");
    assert_eq!(refill_counters(&store)["pages_total"], 0, "no refill page ran");
    assert!(refill_counters(&store)["skipped_global_capacity_total"].as_u64().expect("counter") >= attempts as u64);
    println!(
        "full-budget refill: history={history} attempts={attempts} elapsed={elapsed:?} per_attempt={:?}",
        elapsed / attempts as u32
    );
}

#[test]
fn full_global_limit_answers_refill_without_writer_or_scans() {
    assert_full_budget_refill_is_cheap(20_000, 200);
}

#[test]
#[ignore = "large-history evidence: cargo test -p reticulum-rs-rpc --release -- --ignored --nocapture full_global_limit_large"]
fn full_global_limit_large_history_refill_cost() {
    assert_full_budget_refill_is_cheap(1_000_000, 1_000);
}
