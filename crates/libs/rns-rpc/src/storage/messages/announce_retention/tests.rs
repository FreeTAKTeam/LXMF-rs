use super::*;

fn record(id: usize, peer: &str, timestamp: i64, stamp_cost: Option<u32>) -> AnnounceRecord {
    AnnounceRecord {
        id: format!("{id:08}"),
        peer: peer.into(),
        timestamp,
        name: Some(format!("name-{id}")),
        name_source: None,
        first_seen: 0,
        seen_count: 1,
        app_data_hex: None,
        capabilities: vec![],
        rssi: None,
        snr: None,
        q: None,
        stamp_cost,
        stamp_cost_flexibility: None,
        peering_cost: None,
    }
}

#[test]
fn retains_recent_history_latest_peers_and_latest_non_null_stamp() {
    let store = MessagesStore::in_memory().unwrap();
    for r in [
        record(1, "p", 1, Some(3)),
        record(2, "p", 1, Some(7)),
        record(3, "p", 2, None),
        record(4, "quiet", 0, Some(4)),
        record(5, "busy", 3, None),
        record(6, "busy", 4, None),
        record(7, "busy", 5, None),
    ] {
        store.insert_announce(&r).unwrap();
    }
    store.upsert_announce_identity("quiet", "ab", "cd", 0).unwrap();
    assert_eq!(store.prune_announce_history_batch(2).unwrap(), 2);
    let rows = store.list_announces(100, None, None).unwrap();
    assert_eq!(
        rows.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(),
        ["00000007", "00000006", "00000003", "00000002", "00000004"]
    );
    assert_eq!(store.latest_announce_stamp_cost_for("p").unwrap(), Some(7));
    assert_eq!(store.latest_announce_stamp_cost_for("P").unwrap(), None);
    assert_eq!(store.latest_announce_stamp_cost_for("busy").unwrap(), None);
    assert!(store.announce_identity_keys("quiet").unwrap().is_some());
    assert_eq!(store.prune_announce_history_batch(0).unwrap(), 1);
    assert_eq!(store.prune_announce_history_batch(0).unwrap(), 0);
    assert_eq!(store.operation_metrics_snapshot()["announces"]["retained_at_last_sweep"], 4);
}

#[test]
fn bounded_batches_resume_after_restart_and_converge_during_ingest() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("history.db");
    {
        let store = MessagesStore::open(&path).unwrap();
        for id in 0..1600 {
            store.insert_announce(&record(id, "p", id as i64, Some(5))).unwrap();
        }
        assert_eq!(store.prune_announce_history_batch(20).unwrap(), 512);
        assert_eq!(store.operation_metrics_snapshot()["announces"]["examined_total"], 512);
        assert_eq!(store.prune_announce_history_batch(20).unwrap(), 512);
        assert_eq!(
            store.operation_metrics_snapshot()["announces"]["retention_window"]["calls_total"],
            1
        );
    }
    let store = MessagesStore::open(&path).unwrap();
    for id in 1600..1605 {
        store.insert_announce(&record(id, "p", id as i64, None)).unwrap();
        assert!(store.prune_announce_history_batch(20).unwrap() <= 512);
    }
    for _ in 0..4 {
        store.prune_announce_history_batch(20).unwrap();
    }
    assert_eq!(store.list_announces(10000, None, None).unwrap().len(), 20);
    assert_eq!(store.latest_announce_stamp_cost_for("p").unwrap(), Some(5));
    // An out-of-order event behind the old cursor is examined in a later sweep.
    store.insert_announce(&record(2000, "p", -1, None)).unwrap();
    store.prune_announce_history_batch(20).unwrap();
    assert_eq!(store.list_announces(10000, None, None).unwrap().len(), 20);
}

#[test]
fn protected_rows_do_not_starve_old_history_and_clear_resets_cursor() {
    let store = MessagesStore::in_memory().unwrap();
    for id in 0..1100 {
        let peer = if id >= 500 { format!("p-{id}") } else { "old".into() };
        store.insert_announce(&record(id, &peer, id as i64, None)).unwrap();
    }
    assert_eq!(store.prune_announce_history_batch(0).unwrap(), 0);
    assert!(store.prune_announce_history_batch(0).unwrap() > 0);
    store.prune_announce_history_batch(0).unwrap();
    assert_eq!(store.list_announces(2000, None, None).unwrap().len(), 601);
    store.clear_announces().unwrap();
    assert!(store.list_announces(1, None, None).unwrap().is_empty());
    store.insert_announce(&record(9999, "new", 9999, None)).unwrap();
    assert_eq!(store.prune_announce_history_batch(0).unwrap(), 0);
}

#[test]
fn increasing_limit_mid_sweep_protects_new_boundary_and_pagination_keeps_ties() {
    let store = MessagesStore::in_memory().unwrap();
    for id in 0..1200 {
        store.insert_announce(&record(id, "p", 1, Some(6))).unwrap();
    }
    store.prune_announce_history_batch(1).unwrap();
    assert_eq!(store.prune_announce_history_batch(1200).unwrap(), 0);
    let page = store.list_announces(2, None, None).unwrap();
    assert_eq!(page[0].id, "00001199");
    let next = store.list_announces(2, Some(1), Some(&page[1].id)).unwrap();
    assert!(next[0].id < page[1].id);
}

#[test]
fn migration_builds_indexes_and_queries_seek_without_sorts() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("legacy.db");
    {
        let store = MessagesStore::open(&path).unwrap();
        for id in 0..1000 {
            store.insert_announce(&record(id, "p", id as i64, Some(6))).unwrap();
        }
        store.with_write_conn(|conn| conn.execute_batch("DROP INDEX idx_announces_peer_latest; DROP INDEX idx_announces_peer_stamp; DROP TABLE announce_retention_cursor;")).unwrap();
    }
    let store = MessagesStore::open(&path).unwrap();
    assert_eq!(store.latest_announce_stamp_cost_for("absent").unwrap(), None);
    assert_eq!(store.latest_announce_stamp_cost_for("p").unwrap(), Some(6));
    store.list_announces(2, Some(2), Some("00000002")).unwrap();
    let metrics = store.operation_metrics_snapshot();
    for family in ["stamp_cost", "history"] {
        assert_eq!(metrics["announces"][family]["sorts_total"], 0);
        assert_eq!(metrics["announces"][family]["fullscan_steps_total"], 0);
        assert!(metrics["announces"][family]["vm_steps_total"].as_u64().unwrap() < 150);
    }
    assert_eq!(store.prune_announce_history_batch(20).unwrap(), 512);
    let metrics = store.operation_metrics_snapshot();
    assert_eq!(metrics["announces"]["retention_candidates"]["sorts_total"], 0);
    assert_eq!(metrics["announces"]["retention_delete"]["sorts_total"], 0);
}

#[test]
fn failed_batch_rolls_back_deletes_and_cursor_and_can_retry() {
    let store = MessagesStore::in_memory().unwrap();
    for id in 0..600 {
        store.insert_announce(&record(id, "p", id as i64, Some(1))).unwrap();
    }
    store
        .with_write_conn(|conn| {
            conn.execute_batch(
                "CREATE TRIGGER reject_prune BEFORE DELETE ON announces WHEN OLD.id = '00000300'
         BEGIN SELECT RAISE(ABORT, 'injected failure'); END;",
            )
        })
        .unwrap();
    assert!(store.prune_announce_history_batch(10).is_err());
    assert_eq!(store.list_announces(1000, None, None).unwrap().len(), 600);
    let metrics = store.operation_metrics_snapshot();
    assert_eq!(metrics["announces"]["maintenance"]["errors_total"], 1);
    assert!(metrics["announces"]["maintenance"]["last_rows_deleted"].is_null());
    store.with_write_conn(|conn| conn.execute_batch("DROP TRIGGER reject_prune")).unwrap();
    assert_eq!(store.prune_announce_history_batch(10).unwrap(), 512);
    assert_eq!(store.prune_announce_history_batch(10).unwrap(), 78);
}
