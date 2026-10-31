use super::*;

// Quota-phase regressions. The pre-index statements (global ROW_NUMBER over
// every unhandled mark, then COUNT(*)) are the oracle: the indexed quota phase
// must delete exactly the same marks. TTLs are far beyond the fixture, so
// expiry deletes nothing and the whole history reaches the quota phase.

const QUOTA_NOW: i64 = 10_000;
const QUOTA_TTL: u64 = 1_000_000;
type QuotaMark = (String, String, String, i64);

fn reference_quota_prune(conn: &Connection, total_limit: u64, per_peer_limit: u64) -> usize {
    let tx = conn.unchecked_transaction().expect("reference tx");
    let mut deleted = tx
        .execute(
            "DELETE FROM propagation_peer_entries WHERE rowid IN (
                SELECT rowid FROM (
                    SELECT rowid, ROW_NUMBER() OVER (
                        PARTITION BY peer ORDER BY updated_at DESC, transient_id DESC
                    ) AS peer_rank
                    FROM propagation_peer_entries WHERE state = 'unhandled'
                ) WHERE peer_rank > ?1
            )",
            params![per_peer_limit.max(1) as i64],
        )
        .expect("reference per-peer");
    let total: i64 = tx
        .query_row("SELECT COUNT(*) FROM propagation_peer_entries", [], |row| row.get(0))
        .expect("reference count");
    let (total, limit) = (total as u64, total_limit.max(1));
    if total > limit {
        let unhandled = tx
            .execute(
                "DELETE FROM propagation_peer_entries WHERE rowid IN (
                    SELECT rowid FROM propagation_peer_entries WHERE state = 'unhandled'
                    ORDER BY updated_at ASC, transient_id ASC, peer ASC LIMIT ?1)",
                params![(total - limit) as i64],
            )
            .expect("reference global unhandled");
        deleted += unhandled;
        let remaining = total - unhandled as u64;
        if remaining > limit {
            deleted += tx
                .execute(
                    "DELETE FROM propagation_peer_entries WHERE rowid IN (
                        SELECT rowid FROM propagation_peer_entries
                        WHERE state IN ('handled', 'transferred', 'received', 'transfer_limited')
                        ORDER BY updated_at ASC, transient_id ASC, peer ASC LIMIT ?1)",
                    params![(remaining - limit) as i64],
                )
                .expect("reference global completed");
        }
    }
    tx.commit().expect("reference commit");
    deleted
}

/// `unhandled[i]` pending marks and `completed` finished marks per peer. Ids are
/// shared across peers and timestamps fall in `buckets` values, so per-peer and
/// global orderings both hit ties.
fn quota_marks(peers: &[&str], unhandled: &[usize], completed: usize, buckets: i64) -> Vec<QuotaMark> {
    const COMPLETED: [&str; 4] = ["handled", "transferred", "received", "transfer_limited"];
    let mut marks = Vec::new();
    for (p, peer) in peers.iter().enumerate() {
        for n in 0..unhandled[p % unhandled.len()] {
            let updated_at = 1 + ((n as i64 * 7 + p as i64) % buckets);
            marks.push((peer.to_string(), format!("{n:064x}"), "unhandled".into(), updated_at));
        }
        for n in 0..completed {
            let updated_at = 1 + ((n as i64 * 5 + p as i64) % buckets);
            let state = COMPLETED[(n + p) % COMPLETED.len()];
            marks.push((peer.to_string(), format!("{:064x}", 1_000_000 + n), state.into(), updated_at));
        }
    }
    marks
}

fn seed_quota_marks(store: &MessagesStore, marks: &[QuotaMark]) {
    store
        .with_write_conn(|conn| {
            let tx = conn.unchecked_transaction()?;
            {
                let mut insert = tx.prepare(
                    "INSERT INTO propagation_peer_entries (peer, transient_id, state, updated_at)
                     VALUES (?1, ?2, ?3, ?4)",
                )?;
                for (peer, transient_id, state, updated_at) in marks {
                    insert.execute(params![peer, transient_id, state, updated_at])?;
                }
            }
            tx.commit()
        })
        .expect("seed quota marks");
}

fn stored_quota_marks(store: &MessagesStore) -> Vec<QuotaMark> {
    store
        .with_read_conn(|conn| {
            let mut statement = conn.prepare(
                "SELECT peer, transient_id, state, updated_at FROM propagation_peer_entries
                 ORDER BY peer, transient_id",
            )?;
            let rows = statement.query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)))?;
            rows.collect()
        })
        .expect("stored marks")
}

/// Runs the full policy prune and the oracle on identical stores; returns the
/// deletion count after asserting both delete the same marks.
fn assert_quota_matches_reference(marks: &[QuotaMark], total_limit: u64, per_peer_limit: u64) -> usize {
    let (_temp, store) = refill_store_on_disk();
    let (_reference_temp, reference) = refill_store_on_disk();
    seed_quota_marks(&store, marks);
    seed_quota_marks(&reference, marks);
    let deleted = store
        .prune_propagation_peer_entries_to_policy(QUOTA_NOW, QUOTA_TTL, QUOTA_TTL, total_limit, per_peer_limit)
        .expect("indexed quota prune");
    let expected = reference
        .with_write_conn(|conn| Ok(reference_quota_prune(conn, total_limit, per_peer_limit)))
        .expect("reference prune");
    let maintenance = store.operation_metrics_snapshot()["maintenance"].clone();
    assert_eq!(maintenance["expiry"]["last_rows_deleted"], 0, "fixture must not expire");
    assert_eq!(maintenance["quota"]["last_rows_deleted"], deleted as u64);
    assert_eq!(deleted, expected);
    assert_eq!(stored_quota_marks(&store), stored_quota_marks(&reference));
    deleted
}

const MIXED_CASE_PEERS: [&str; 6] = ["aa11", "AA11", "Aa11", "bb22", "BB22", "cc33"];

#[test]
fn peer_quota_idle_large_history_skips_the_writer_and_never_sorts() {
    let peers: Vec<String> = (0..48).map(|p| format!("{p:032x}")).collect();
    let peers: Vec<&str> = peers.iter().map(String::as_str).collect();
    let marks = quota_marks(&peers, &[100, 250], 300, 50);
    let unhandled = marks.iter().filter(|mark| mark.2 == "unhandled").count() as i64;
    let (_temp, store) = refill_store_on_disk();
    seed_quota_marks(&store, &marks);

    let writes_before = store.contention_snapshot().write_ops_total;
    let deleted = store
        .prune_propagation_peer_entries_to_policy(QUOTA_NOW, QUOTA_TTL, QUOTA_TTL, 1_000_000, 1_024)
        .expect("idle prune");
    assert_eq!(deleted, 0);
    assert_eq!(stored_quota_marks(&store).len(), marks.len());
    // Expiry pages read through the writer only when candidates exist; quota takes none.
    assert_eq!(store.contention_snapshot().write_ops_total, writes_before);

    let (over, total, sorts, full_scan_steps) = store
        .with_read_conn(|conn| {
            let mut gate = conn.prepare(QUOTA_GATE_SQL)?;
            let (over, total) = gate.query_row([1_024], |row| Ok((row.get::<_, bool>(0)?, row.get::<_, i64>(1)?)))?;
            Ok((
                over,
                total,
                gate.get_status(rusqlite::StatementStatus::Sort),
                gate.get_status(rusqlite::StatementStatus::FullscanStep),
            ))
        })
        .expect("gate counters");
    assert!(!over);
    assert_eq!(total, marks.len() as i64);
    assert_eq!(sorts, 0, "gate must not sort");
    assert!(
        i64::from(full_scan_steps) <= unhandled,
        "gate walks the unhandled index only, not completed history: {full_scan_steps} steps"
    );
}

#[test]
fn peer_quota_trims_one_overfull_peer_like_the_reference() {
    let peers: Vec<String> = (0..20).map(|p| format!("{p:032x}")).collect();
    let peers: Vec<&str> = peers.iter().map(String::as_str).collect();
    let mut unhandled = vec![40; peers.len()];
    unhandled[7] = 900;
    assert_eq!(assert_quota_matches_reference(&quota_marks(&peers, &unhandled, 60, 13), 1_000_000, 64), 836);
}

#[test]
fn peer_quota_keeps_exact_peer_keys_and_tie_order_across_many_peers() {
    // Case variants are separate peers: none exceeds 30 alone, though folded groups do.
    let quiet = quota_marks(&MIXED_CASE_PEERS, &[30, 25, 20], 10, 3);
    assert_eq!(assert_quota_matches_reference(&quiet, 1_000_000, 30), 0);
    let deleted = assert_quota_matches_reference(&quota_marks(&MIXED_CASE_PEERS, &[95, 41, 70], 12, 4), 1_000_000, 17);
    assert_eq!(deleted, (95 - 17) + (41 - 17) + (70 - 17) + (95 - 17) + (41 - 17) + (70 - 17));
}

#[test]
fn peer_quota_applies_per_peer_then_global_unhandled_then_completed() {
    let marks = quota_marks(&MIXED_CASE_PEERS, &[120, 80, 9], 150, 6);
    let after_per_peer = marks.len() - 2 * ((120 - 50) + (80 - 50));
    assert_eq!(assert_quota_matches_reference(&marks, 700, 50), marks.len() - 700);
    let unhandled_left = 2 * (50 + 50 + 9);
    assert!(after_per_peer - unhandled_left > 700, "fixture reaches the completed stage");
    // Global excess satisfied by unhandled marks alone keeps every completed mark.
    assert_eq!(assert_quota_matches_reference(&marks, after_per_peer as u64 - 40, 50), marks.len() - after_per_peer + 40);
    // Global excess alone must pass the read gate.
    let within_peer_quota = quota_marks(&MIXED_CASE_PEERS, &[20], 30, 5);
    assert_eq!(assert_quota_matches_reference(&within_peer_quota, 100, 50), within_peer_quota.len() - 100);
}

#[test]
fn peer_quota_treats_zero_limits_as_one() {
    let marks = quota_marks(&MIXED_CASE_PEERS, &[5, 1, 3], 2, 2);
    assert_eq!(assert_quota_matches_reference(&marks, 0, 0), marks.len() - 1);
    assert_eq!(assert_quota_matches_reference(&marks, 1_000_000, 0), 2 * (4 + 2));
}

#[test]
fn peer_quota_statements_use_the_rank_index_without_temp_btrees() {
    let (_temp, store) = refill_store_on_disk();
    seed_quota_marks(&store, &quota_marks(&MIXED_CASE_PEERS, &[30, 5], 5, 4));
    let explain = |sql: &str| {
        store
            .with_read_conn(|conn| {
                let mut statement = conn.prepare(&format!("EXPLAIN QUERY PLAN {sql}"))?;
                let params = vec![1_i64; statement.parameter_count()];
                let rows = statement
                    .query_map(rusqlite::params_from_iter(params), |row| row.get::<_, String>(3))?;
                rows.collect::<rusqlite::Result<Vec<_>>>()
            })
            .expect("explain")
            .join(" | ")
    };
    for sql in [QUOTA_GATE_SQL, PER_PEER_QUOTA_DELETE_SQL] {
        let plan = explain(sql);
        assert!(plan.contains("idx_propagation_peer_entries_unhandled_rank"), "{plan}");
        assert!(!plan.contains("TEMP B-TREE"), "{plan}");
        for scan in plan.split(" | ").filter(|step| step.starts_with("SCAN propagation_peer_entries")) {
            assert!(scan.contains("USING COVERING INDEX"), "{plan}");
        }
    }
    let global = explain(GLOBAL_UNHANDLED_QUOTA_DELETE_SQL);
    assert!(global.contains("USING INDEX idx_propagation_peer_entries_expiry (state=?)"), "{global}");
    // The pre-index ranking sorted every unhandled mark on each pass.
    let reference = explain(
        "SELECT rowid FROM (SELECT rowid, ROW_NUMBER() OVER (PARTITION BY peer ORDER BY updated_at DESC, transient_id DESC) AS r
         FROM propagation_peer_entries WHERE state = 'unhandled') WHERE r > ?1",
    );
    assert!(reference.contains("USE TEMP B-TREE FOR ORDER BY"), "{reference}");
}

#[test]
fn peer_quota_stays_exact_under_concurrent_inserts_and_state_changes() {
    const PER_PEER: u64 = 25;
    let (_temp, store) = refill_store_on_disk();
    let store = std::sync::Arc::new(store);
    let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let pruner = {
        let (store, stop) = (std::sync::Arc::clone(&store), std::sync::Arc::clone(&stop));
        std::thread::spawn(move || {
            // The pass that observes `stop` starts after the last write.
            loop {
                let last = stop.load(std::sync::atomic::Ordering::Acquire);
                store
                    .prune_propagation_peer_entries_to_policy(QUOTA_NOW, QUOTA_TTL, QUOTA_TTL, 1_000_000, PER_PEER)
                    .expect("concurrent prune");
                if last {
                    break;
                }
            }
        })
    };
    let mut flipped = 0;
    for n in 0..1_200_u64 {
        let peer = MIXED_CASE_PEERS[(n % 6) as usize];
        store
            .with_write_conn(|conn| {
                conn.execute(
                    "INSERT INTO propagation_peer_entries (peer, transient_id, state, updated_at)
                     VALUES (?1, ?2, 'unhandled', ?3)",
                    params![peer, format!("{n:064x}"), (n % 9) as i64],
                )?;
                if n % 4 == 0 {
                    // Complete the newest pending mark: per-peer quota must never take it afterwards.
                    flipped += conn.execute(
                        "UPDATE propagation_peer_entries SET state = 'handled', updated_at = 1
                         WHERE rowid = (SELECT rowid FROM propagation_peer_entries
                                        WHERE peer = ?1 AND state = 'unhandled'
                                        ORDER BY updated_at DESC, transient_id DESC LIMIT 1)",
                        params![peer],
                    )?;
                }
                Ok(())
            })
            .expect("concurrent writer");
    }
    stop.store(true, std::sync::atomic::Ordering::Release);
    pruner.join().expect("pruner thread");

    let marks = stored_quota_marks(&store);
    assert_eq!(marks.iter().filter(|mark| mark.2 == "handled").count(), flipped);
    for peer in MIXED_CASE_PEERS {
        let pending = marks.iter().filter(|mark| mark.0 == peer && mark.2 == "unhandled").count();
        assert!(pending as u64 <= PER_PEER, "{peer} kept {pending} pending marks");
    }
    // The settled table is a fixed point of the reference policy.
    assert_eq!(assert_quota_matches_reference(&marks, 1_000_000, PER_PEER), 0);
}

#[test]
fn peer_quota_rank_index_is_built_for_existing_databases() {
    let temp = tempfile::tempdir().expect("temp dir");
    let path = temp.path().join("pre-rank-index.db");
    let marks = quota_marks(&MIXED_CASE_PEERS, &[40, 3], 4, 5);
    {
        let store = MessagesStore::open(&path).expect("open store");
        store
            .with_write_conn(|conn| conn.execute_batch("DROP INDEX idx_propagation_peer_entries_unhandled_rank;"))
            .expect("simulate pre-index schema");
        seed_quota_marks(&store, &marks);
    }
    let store = MessagesStore::open(&path).expect("reopen store");
    let (definition, indexed): (String, i64) = store
        .with_read_conn(|conn| {
            Ok((
                conn.query_row(
                    "SELECT sql FROM sqlite_master WHERE name = 'idx_propagation_peer_entries_unhandled_rank'",
                    [],
                    |row| row.get(0),
                )?,
                conn.query_row(
                    "SELECT COUNT(*) FROM propagation_peer_entries
                     INDEXED BY idx_propagation_peer_entries_unhandled_rank WHERE state = 'unhandled'",
                    [],
                    |row| row.get(0),
                )?,
            ))
        })
        .expect("rank index after reopen");
    assert!(definition.contains("WHERE state = 'unhandled'"), "{definition}");
    assert_eq!(indexed, marks.iter().filter(|mark| mark.2 == "unhandled").count() as i64);
    let deleted = store
        .prune_propagation_peer_entries_to_policy(QUOTA_NOW, QUOTA_TTL, QUOTA_TTL, 1_000_000, 10)
        .expect("prune migrated store");
    assert_eq!(deleted, 3 * (40 - 10));
}
