fn refill_entry(index: usize, received_at: i64) -> PropagationEntryRecord {
    PropagationEntryRecord {
        transient_id: format!("{index:064x}"),
        destination: "11".repeat(16),
        payload_hex: "22".repeat(8),
        received_at,
        size_bytes: 8,
        stamp_value: None,
    }
}

#[derive(Debug, PartialEq, Eq)]
struct RefillState {
    head_received_at: i64,
    head_transient_id: String,
    cursor_received_at: Option<i64>,
    cursor_transient_id: Option<String>,
    observed_generation: i64,
    scan_complete: bool,
    rescan_required: bool,
}

fn refill_state(store: &MessagesStore, peer: &str) -> Option<RefillState> {
    store
        .with_read_conn(|conn| {
            conn.query_row(
                "SELECT head_received_at, head_transient_id, cursor_received_at,
                        cursor_transient_id, observed_generation, scan_complete, rescan_required
                 FROM propagation_peer_refill_state WHERE peer = ?1",
                params![peer],
                |row| {
                    Ok(RefillState {
                        head_received_at: row.get(0)?,
                        head_transient_id: row.get(1)?,
                        cursor_received_at: row.get(2)?,
                        cursor_transient_id: row.get(3)?,
                        observed_generation: row.get(4)?,
                        scan_complete: row.get(5)?,
                        rescan_required: row.get(6)?,
                    })
                },
            )
            .optional()
        })
        .expect("read refill scan state")
}

#[test]
fn refill_scans_known_inventory_in_bounded_pages_and_coalesces_completed_announces() {
    let store = MessagesStore::in_memory().expect("in-memory store");
    let peer = "peer-bounded-refill";
    for index in 0..260 {
        let entry = refill_entry(index, index as i64);
        store.upsert_propagation_entry(&entry).expect("insert inventory entry");
        if index >= 10 {
            store
                .mark_peer_handled_propagation(peer, &entry.transient_id)
                .expect("mark inventory known");
        }
    }

    assert_eq!(store.mark_recent_propagation_unhandled_for_peer(peer, 1_000).unwrap(), 10);
    assert_eq!(store.list_peer_unhandled_propagation_ids(peer).unwrap().len(), 10);

    let completed_state = refill_state(&store, peer).expect("completed scan state");
    assert!(completed_state.scan_complete);
    assert_eq!(
        store.mark_recent_propagation_unhandled_for_peer(peer, 1_000).unwrap(),
        0,
        "an unchanged completed inventory should not be rescanned on every announce"
    );
    assert_eq!(refill_state(&store, peer), Some(completed_state));

    let plans = store
        .with_read_conn(|conn| {
            let mut candidate_statement = conn.prepare(
                "EXPLAIN QUERY PLAN SELECT received_at, transient_id FROM propagation_entries
                 WHERE (received_at, transient_id) <= (?1, ?2)
                 ORDER BY received_at DESC, transient_id DESC LIMIT 128",
            )?;
            let candidate_rows = candidate_statement
                .query_map(params![259_i64, format!("{:064x}", 259)], |row| {
                    row.get::<_, String>(3)
                })?;
            let candidate_plan = candidate_rows.collect::<rusqlite::Result<Vec<_>>>()?.join(" ");
            let mut count_statement = conn.prepare(
                "EXPLAIN QUERY PLAN SELECT COUNT(*) FROM (
                    SELECT transient_id FROM propagation_peer_entries AS pending
                        INDEXED BY idx_propagation_peer_entries_unhandled_folded
                    WHERE LOWER(peer) = LOWER('peer-bounded-refill') AND state = 'unhandled'
                      AND NOT EXISTS (
                        SELECT 1 FROM propagation_peer_entries AS completed
                        WHERE LOWER(completed.peer) = LOWER('peer-bounded-refill')
                          AND completed.transient_id = pending.transient_id
                          AND completed.state <> 'unhandled'
                      )
                    GROUP BY transient_id LIMIT 1000
                )",
            )?;
            let count_rows = count_statement.query_map([], |row| row.get::<_, String>(3))?;
            let count_plan = count_rows.collect::<rusqlite::Result<Vec<_>>>()?.join(" ");
            Ok((candidate_plan, count_plan))
        })
        .expect("explain refill query plans");
    assert!(plans.0.contains("idx_propagation_entries_received_transient"), "{}", plans.0);
    assert!(!plans.0.contains("TEMP B-TREE"), "{}", plans.0);
    assert!(plans.1.contains("idx_propagation_peer_entries_unhandled_folded"), "{}", plans.1);
    assert!(!plans.1.contains("TEMP B-TREE"), "{}", plans.1);
}

#[test]
fn refill_scan_survives_reopen_and_observes_inventory_arriving_mid_scan() {
    let directory = tempfile::tempdir().expect("temp directory");
    let path = directory.path().join("propagation.sqlite");
    let peer = "peer-refill-reopen";
    let store = MessagesStore::open(&path).expect("open store");
    for index in 0..260 {
        let entry = refill_entry(index, index as i64);
        store.upsert_propagation_entry(&entry).expect("insert inventory entry");
        if index < 250 {
            store
                .mark_peer_handled_propagation(peer, &entry.transient_id)
                .expect("mark entry known");
        }
    }

    assert_eq!(
        store.mark_recent_propagation_unhandled_for_peer(peer, 10).expect("fill first page"),
        10
    );
    let prior = refill_state(&store, peer).expect("persisted cursor");
    assert_eq!(prior.cursor_received_at, Some(250));
    drop(store);

    let store = MessagesStore::open(&path).expect("reopen store");
    let new_entry = refill_entry(300, 300);
    store.upsert_propagation_entry(&new_entry).expect("new head after reopen");
    let older_unseen_id = refill_entry(1, 1).transient_id;
    store
        .with_write_conn(|conn| {
            conn.execute(
                "DELETE FROM propagation_peer_entries WHERE peer = ?1 AND transient_id = ?2",
                params![peer, older_unseen_id],
            )
        })
        .expect("make older candidate unseen");
    let freed_id = refill_entry(259, 259).transient_id;
    store.mark_peer_handled_propagation(peer, &freed_id).expect("free pending capacity");
    assert_eq!(
        store.mark_recent_propagation_unhandled_for_peer(peer, 10).unwrap(),
        1,
        "the newest candidate visible at call entry wins over the older saved cursor"
    );
    let next_generation = refill_state(&store, peer).expect("new generation scan state");
    assert_eq!(next_generation.head_received_at, 300);
    assert_eq!(next_generation.head_transient_id, new_entry.transient_id);
    assert_eq!(next_generation.cursor_received_at, Some(300));
    assert!(
        !next_generation.scan_complete,
        "the full pending queue stops the new pass at its head"
    );
    let unhandled = store.list_peer_unhandled_propagation_ids(peer).unwrap();
    assert_eq!(unhandled.len(), 10);
    assert!(unhandled.contains(&new_entry.transient_id));
    assert!(!unhandled.contains(&older_unseen_id));
}

#[test]
fn refill_reaches_older_unseen_entry_after_more_than_8192_known_candidates() {
    let store = MessagesStore::in_memory().expect("in-memory store");
    let peer = "peer-refill-long-pass";
    let tx = store.with_write_conn(|conn| {
        let tx = conn.unchecked_transaction()?;
        {
            let mut insert_inventory = tx.prepare(
                "INSERT INTO propagation_entries
                    (transient_id, destination, payload_hex, received_at, size_bytes, stamp_value)
                 VALUES (?1, ?2, '', ?3, 0, NULL)",
            )?;
            for index in 0..8_300 {
                insert_inventory.execute(params![
                    format!("{index:064x}"),
                    "33".repeat(16),
                    index as i64
                ])?;
            }
        }
        {
            let mut mark_known = tx.prepare(
                "INSERT INTO propagation_peer_entries (peer, transient_id, state, updated_at)
                 VALUES (?1, ?2, 'handled', 0)",
            )?;
            for index in 1..8_300 {
                mark_known.execute(params![peer, format!("{index:064x}")])?;
            }
        }
        tx.commit()
    });
    tx.expect("seed large known inventory");

    assert_eq!(store.mark_recent_propagation_unhandled_for_peer(peer, 1).unwrap(), 1);
    assert_eq!(
        store.list_peer_unhandled_propagation_ids(peer).unwrap(),
        vec![format!("{:064x}", 0)]
    );
    let state = refill_state(&store, peer).expect("continued refill cursor");
    assert_eq!(state.cursor_received_at, Some(0));
    assert!(state.scan_complete, "the final bounded page completes the captured pass");
}
