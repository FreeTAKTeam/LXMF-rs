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

fn seed_history(store: &MessagesStore, count: usize) {
    store.with_write_conn(|conn| conn.execute_batch(&format!(
        "WITH RECURSIVE n(x) AS (VALUES(1) UNION ALL SELECT x+1 FROM n WHERE x<{count})
         INSERT INTO announces(id,peer,timestamp,first_seen,seen_count) SELECT printf('history-%08d',x),'same-peer',x,0,1 FROM n;"
    ))).expect("seed history");
}

#[test]
fn under_window_checks_do_not_walk_retained_history() {
    let mut measured = Vec::new();
    for count in [128, 10_000] {
        let store = MessagesStore::in_memory().expect("store");
        seed_history(&store, count);
        let before = store.operation_metrics_snapshot();
        for _ in 0..8 {
            assert_eq!(store.prune_announce_history_batch(100_000).expect("maintenance"), 0);
        }
        let after = store.operation_metrics_snapshot();
        let scans =
            after["announces"]["retention_window"]["fullscan_steps_total"].as_u64().expect("scan")
                - before["announces"]["retention_window"]["fullscan_steps_total"]
                    .as_u64()
                    .expect("scan");
        println!("under_window_rows={count} calls=8 history_fullscan_steps={scans}");
        measured.push((count, scans));
    }
    assert!(
        measured.iter().all(|(_, scans)| *scans < 64),
        "under-window history scans: {measured:?}"
    );
}

#[test]
fn repeated_replacements_at_the_limit_do_not_repeat_history_or_count_scans() {
    for recursive in [false, true] {
        let store = MessagesStore::in_memory().expect("store");
        store
            .with_write_conn(|conn| conn.pragma_update(None, "recursive_triggers", recursive))
            .expect("trigger mode");
        seed_history(&store, 1000);
        // Same primary key, different timestamps: replacement must not drift an
        // upper bound and turn every near-limit tick into an exact recount.
        let before = store.operation_metrics_snapshot();
        for timestamp in 2000..2016 {
            store
                .insert_announce(&AnnounceRecord {
                    id: "history-00000001".into(),
                    ..record(1, "same-peer", timestamp, None)
                })
                .expect("replace");
            assert_eq!(store.prune_announce_history_batch(1000).expect("maintenance"), 0);
        }
        let after = store.operation_metrics_snapshot();
        for family in ["retention_window", "retention_count"] {
            let calls = after["announces"][family]["calls_total"].as_u64().expect("calls")
                - before["announces"][family]["calls_total"].as_u64().expect("calls");
            assert_eq!(calls, 0, "replacements must not trigger {family}, recursive={recursive}");
        }
        store.insert_announce(&record(999_999, "same-peer", 9000, None)).expect("cross limit");
        assert_eq!(store.prune_announce_history_batch(1000).expect("prune oldest"), 1);
        assert_eq!(store.list_announces(2000, None, None).expect("history").len(), 1000);
    }
}

#[test]
#[ignore = "explicit controlled large retention boundary cost baseline"]
fn profile_under_at_and_above_large_retention_limit() {
    let disk = std::env::var_os("LXMF_ANNOUNCE_PROBE_DISK").is_some();
    for count in [99_999, 100_000, 100_001] {
        let root = tempfile::tempdir().expect("probe root");
        let store = if disk {
            MessagesStore::open(&root.path().join("history.db")).expect("disk store")
        } else {
            MessagesStore::in_memory().expect("store")
        };
        if disk {
            store.with_write_conn(|conn| conn.execute_batch(&format!(
                "WITH RECURSIVE n(x) AS (VALUES(1) UNION ALL SELECT x+1 FROM n WHERE x<{count})
                 INSERT INTO announces(id,peer,timestamp,first_seen,seen_count)
                 SELECT printf('announce-%d-%032x-%d',1700000000+x,1,x),printf('%032x',1),1700000000+x,0,1 FROM n;"
            ))).expect("wide synthetic file-backed history");
        } else {
            seed_history(&store, count);
        }
        let before = store.operation_metrics_snapshot();
        let started = std::time::Instant::now();
        let peer = if disk { format!("{:032x}", 1) } else { "same-peer".into() };
        let mut removed = 0;
        for step in 0..8 {
            if step > 0 && count == 100_001 {
                let ts = 1_800_000_000 + step as i64;
                let mut next = record(1_000_000 + step, &peer, ts, None);
                if disk {
                    next.id = format!("announce-{ts}-{:032x}-{}", 1, 1_000_000 + step);
                }
                store.insert_announce(&next).expect("one-row overflow");
            } else if step > 0 && count == 100_000 {
                store
                    .insert_announce(&AnnounceRecord {
                        id: if disk {
                            format!("announce-1700000001-{:032x}-1", 1)
                        } else {
                            "history-00000001".into()
                        },
                        ..record(1, &peer, 1_800_000_000 + step as i64, None)
                    })
                    .expect("at-limit replacement");
            }
            removed += store.prune_announce_history_batch(100_000).expect("maintenance");
        }
        let wall_ns = started.elapsed().as_nanos();
        let after = store.operation_metrics_snapshot();
        let database = store.with_control_conn(|conn| {
            let page_size: i64 = conn.pragma_query_value(None, "page_size", |r|r.get(0))?;
            let cache_size: i64 = conn.pragma_query_value(None, "cache_size", |r|r.get(0))?;
            let page_count: i64 = conn.pragma_query_value(None, "page_count", |r|r.get(0))?;
            let (id_bytes,peer_bytes):(f64,f64)=conn.query_row("SELECT AVG(LENGTH(CAST(id AS BLOB))), AVG(LENGTH(CAST(peer AS BLOB))) FROM announces",[],|r|Ok((r.get(0)?,r.get(1)?)))?;
            let sizes = (|| -> rusqlite::Result<Vec<(String,i64,i64)>> {
                let mut stmt=conn.prepare("SELECT name,COUNT(*),SUM(pgsize) FROM dbstat WHERE name='announces' OR name LIKE 'idx_announces_%' OR name LIKE 'sqlite_autoindex_announces_%' GROUP BY name")?;
                let rows=stmt.query_map([],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?)))?;
                rows.collect()
            })();
            let (sizes, unavailable) = match sizes {
                Ok(sizes) => (Some(sizes), None),
                Err(error) => (None, Some(error.to_string())),
            };
            Ok(serde_json::json!({"page_size":page_size,"cache_size":cache_size,"page_count":page_count,"mean_id_bytes":id_bytes,"mean_peer_bytes":peer_bytes,
                "index_pages_and_bytes":sizes,"index_sizes_unavailable":unavailable}))
        }).expect("database metadata after measurement");
        println!(
            "{}",
            serde_json::json!({"rows":count,"ticks":8,"storage":if disk {"disk"} else {"memory"},"removed":removed,"wall_ns":wall_ns,"database":database,
            "before":before["announces"],"after":after["announces"]})
        );
    }
}

fn cardinality(store: &MessagesStore) -> (i64, i64) {
    store
        .with_read_conn(|conn| {
            conn.query_row(
                "SELECT upper_bound, (SELECT COUNT(*) FROM announces)
         FROM announce_retention_count WHERE singleton = 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
        })
        .expect("cardinality")
}

#[test]
fn sparse_ids_direct_deletions_clear_and_limits_preserve_a_safe_bound() {
    let store = MessagesStore::in_memory().expect("store");
    for id in [2, 90_000, 9_000_000] {
        store.insert_announce(&record(id, "same-peer", id as i64, None)).expect("sparse row");
    }
    assert_eq!(cardinality(&store), (3, 3));
    store
        .with_write_conn(|conn| {
            conn.execute("DELETE FROM announces WHERE id = '00090000'", []).map(|_| ())
        })
        .expect("direct delete");
    assert_eq!(cardinality(&store), (3, 2), "deletion leaves a conservative bound");
    assert_eq!(store.prune_announce_history_batch(2).expect("reconcile"), 0);
    assert_eq!(cardinality(&store), (2, 2));
    for _ in 0..4 {
        assert_eq!(store.prune_announce_history_batch(2).expect("skip"), 0);
    }
    assert_eq!(store.prune_announce_history_batch(1).expect("lower limit"), 1);
    assert_eq!(store.list_announces(10, None, None).expect("remaining")[0].id, "09000000");
    store.clear_announces().expect("clear");
    assert_eq!(cardinality(&store), (0, 0));
    store
        .insert_announce(&record(10_000_000, "same-peer", -1, None))
        .expect("old timestamp after clear");
    assert_eq!(store.prune_announce_history_batch(1).expect("skip"), 0);
    assert_eq!(cardinality(&store), (1, 1));
}

#[test]
fn upper_bound_rollback_and_ignored_invalid_insert_never_undercount() {
    let store = MessagesStore::in_memory().expect("store");
    seed_history(&store, 3);
    let failed: rusqlite::Result<()> = store.with_write_conn(|conn| {
        let tx = conn.unchecked_transaction()?;
        tx.execute(
            "INSERT INTO announces(id,peer,timestamp,first_seen,seen_count)
                    VALUES('rolled-back','peer',100,0,1)",
            [],
        )?;
        Err(rusqlite::Error::InvalidQuery)
    });
    assert!(failed.is_err());
    assert_eq!(cardinality(&store), (3, 3), "trigger update rolls back too");
    store
        .with_write_conn(|conn| {
            conn.execute(
                "INSERT OR IGNORE INTO announces(id,peer,timestamp,first_seen,seen_count)
                    VALUES('invalid-ignored',NULL,100,0,1)",
                [],
            )
            .map(|_| ())
        })
        .expect("ignored insert");
    assert_eq!(cardinality(&store), (4, 3), "a BEFORE trigger may overcount, never undercount");
    assert_eq!(store.prune_announce_history_batch(3).expect("reconcile safely"), 0);
    assert_eq!(cardinality(&store), (3, 3));
}

#[test]
fn legacy_migration_and_restart_resume_without_recounting_each_batch() {
    let root = tempfile::tempdir().expect("root");
    let path = root.path().join("history.db");
    {
        let store = MessagesStore::open(&path).expect("store");
        seed_history(&store, 1100);
        assert_eq!(store.prune_announce_history_batch(10).expect("first batch"), 512);
        assert_eq!(
            store.operation_metrics_snapshot()["announces"]["retention_count"]["calls_total"],
            1
        );
        store
            .with_write_conn(|conn| {
                conn.execute_batch(
            "DROP TRIGGER announce_retention_count_new_row; DROP TABLE announce_retention_count;"
        )
            })
            .expect("legacy schema without count hint");
    }
    let store = MessagesStore::open(&path).expect("migrate and reopen");
    assert_eq!(cardinality(&store), (588, 588));
    assert_eq!(store.prune_announce_history_batch(10).expect("resumed batch"), 512);
    assert_eq!(
        store.operation_metrics_snapshot()["announces"]["retention_count"]["calls_total"],
        1
    );
    assert_eq!(
        store.operation_metrics_snapshot()["announces"]["retention_window"]["calls_total"],
        1
    );
    assert_eq!(store.prune_announce_history_batch(10).expect("complete sweep"), 66);
    assert_eq!(cardinality(&store), (10, 10));
    store.prune_announce_history_batch(10).expect("under window");
    assert_eq!(
        store.operation_metrics_snapshot()["announces"]["retention_count"]["calls_total"],
        2
    );
}

#[test]
fn concurrent_store_handles_share_the_transactional_bound() {
    let root = tempfile::tempdir().expect("root");
    let path = root.path().join("history.db");
    let first = MessagesStore::open(&path).expect("first");
    let second = MessagesStore::open(&path).expect("second");
    std::thread::scope(|scope| {
        for (store, offset) in [(&first, 0), (&second, 100)] {
            scope.spawn(move || {
                for id in offset..offset + 100 {
                    store
                        .insert_announce(&record(id, "same-peer", id as i64, None))
                        .expect("concurrent insertion");
                }
            });
        }
    });
    assert_eq!(cardinality(&first), (200, 200));
    assert_eq!(cardinality(&second), (200, 200));
    assert_eq!(first.prune_announce_history_batch(100).expect("prune after threshold"), 100);
    assert_eq!(cardinality(&second), (100, 100));
    second.prune_announce_history_batch(100).expect("other handle skips");
    assert_eq!(
        second.operation_metrics_snapshot()["announces"]["retention_window"]["calls_total"],
        0
    );
}

#[test]
fn skipped_checks_do_not_publish_the_bound_as_an_exact_retained_count() {
    let store = MessagesStore::in_memory().expect("store");
    seed_history(&store, 100);
    store.prune_announce_history_batch(100).expect("skip");
    let metrics = store.operation_metrics_snapshot();
    assert_eq!(metrics["announces"]["row_count_upper_bound"], 100);
    assert_eq!(metrics["announces"]["under_window_skips_total"], 1);
    assert_eq!(metrics["announces"]["retention_count"]["calls_total"], 0);
    assert_eq!(metrics["announces"]["retained_at_last_sweep"], 0, "no completed exact sweep yet");
    store
        .with_write_conn(|conn| {
            conn.execute("DELETE FROM announces WHERE timestamp <= 50", []).map(|_| ())
        })
        .expect("delete");
    store.prune_announce_history_batch(100).expect("conservative skip");
    assert_eq!(cardinality(&store), (100, 50));
    assert_eq!(store.operation_metrics_snapshot()["announces"]["retained_at_last_sweep"], 0);
    assert_eq!(store.prune_announce_history_batch(40).expect("real sweep"), 10);
    assert_eq!(store.operation_metrics_snapshot()["announces"]["retained_at_last_sweep"], 40);
}

#[test]
fn active_cursor_keeps_recent_rows_after_direct_deletion_with_or_without_reopen() {
    for reopen in [false, true] {
        let root = tempfile::tempdir().expect("root");
        let path = root.path().join("history.db");
        let store = MessagesStore::open(&path).expect("store");
        seed_history(&store, 2000);
        assert_eq!(store.prune_announce_history_batch(1000).expect("first batch"), 512);
        store
            .with_write_conn(|conn| {
                conn.execute("DELETE FROM announces WHERE timestamp > 1000", []).map(|_| ())
            })
            .expect("delete recent window");
        assert_eq!(cardinality(&store).1, 488);
        let other = reopen.then(|| MessagesStore::open(&path).expect("concurrent handle reseed"));
        let active = other.as_ref().unwrap_or(&store);
        assert_eq!(
            active.prune_announce_history_batch(1000).expect("revalidate active cursor"),
            0,
            "every remaining row belongs to the current newest-1000 window, reopen={reopen}"
        );
        assert_eq!(cardinality(active).1, 488);
    }
}

#[test]
fn active_cursor_revalidates_timestamp_replacements_in_both_trigger_modes() {
    for recursive in [false, true] {
        let store = MessagesStore::in_memory().expect("store");
        store
            .with_write_conn(|conn| conn.pragma_update(None, "recursive_triggers", recursive))
            .expect("trigger mode");
        seed_history(&store, 2000);
        assert_eq!(store.prune_announce_history_batch(1000).expect("first batch"), 512);
        for id in 1001..=2000 {
            store
                .insert_announce(&AnnounceRecord {
                    id: format!("history-{id:08}"),
                    ..record(id, "same-peer", -(id as i64), None)
                })
                .expect("move recent row backward via REPLACE");
        }
        let expected = store
            .list_announces(1000, None, None)
            .expect("current newest window")
            .into_iter()
            .map(|r| r.id)
            .collect::<Vec<_>>();
        assert_eq!(store.prune_announce_history_batch(1000).expect("revalidate"), 488);
        let actual = store
            .list_announces(3000, None, None)
            .expect("remaining")
            .into_iter()
            .map(|r| r.id)
            .collect::<Vec<_>>();
        assert_eq!(actual, expected, "current window must survive, recursive={recursive}");
    }
}

#[test]
fn active_cursor_revalidates_direct_timestamp_updates() {
    let store = MessagesStore::in_memory().expect("store");
    seed_history(&store, 2000);
    assert_eq!(store.prune_announce_history_batch(1000).expect("first batch"), 512);
    store
        .with_write_conn(|conn| {
            conn.execute("UPDATE announces SET timestamp = -timestamp WHERE timestamp > 1000", [])
                .map(|_| ())
        })
        .expect("move recent window");
    let expected = store
        .list_announces(1000, None, None)
        .expect("newest")
        .into_iter()
        .map(|r| r.id)
        .collect::<Vec<_>>();
    assert_eq!(store.prune_announce_history_batch(1000).expect("revalidate"), 488);
    assert_eq!(
        store
            .list_announces(3000, None, None)
            .expect("remaining")
            .into_iter()
            .map(|r| r.id)
            .collect::<Vec<_>>(),
        expected
    );
}

#[test]
fn repeated_mutations_do_not_restart_protected_prefix_and_starve_old_rows() {
    let store = MessagesStore::in_memory().expect("store");
    for id in 0..1100 {
        let peer = if id >= 500 { format!("protected-{id}") } else { "old".into() };
        store.insert_announce(&record(id, &peer, id as i64, None)).expect("history");
    }
    for tick in 0..6 {
        store
            .insert_announce(&record(1099, "protected-1099", 2000 + tick, None))
            .expect("invalidate current window");
        store.prune_announce_history_batch(0).expect("bounded progress");
    }
    let rows = store.list_announces(2000, None, None).expect("retained");
    assert_eq!(rows.iter().filter(|r| r.peer == "old").count(), 1);
    assert_eq!(rows.len(), 601, "latest-per-peer guards remain intact");
}

#[test]
fn active_cursor_revalidates_id_updates_with_timestamp_ties() {
    let store = MessagesStore::in_memory().expect("store");
    seed_history(&store, 2000);
    store
        .with_write_conn(|conn| conn.execute("UPDATE announces SET timestamp=1", []).map(|_| ()))
        .expect("ties");
    assert_eq!(store.prune_announce_history_batch(500).expect("first batch"), 512);
    store
        .with_write_conn(|conn| {
            conn.execute("UPDATE announces SET id='a-'||id WHERE id>'history-00001500'", [])
                .map(|_| ())
        })
        .expect("move recent IDs backward");
    let expected = store
        .list_announces(500, None, None)
        .expect("current window")
        .into_iter()
        .map(|r| r.id)
        .collect::<Vec<_>>();
    for _ in 0..3 {
        store.prune_announce_history_batch(500).expect("revalidate and finish");
    }
    assert_eq!(
        store
            .list_announces(3000, None, None)
            .expect("remaining")
            .into_iter()
            .map(|r| r.id)
            .collect::<Vec<_>>(),
        expected
    );
}

#[test]
fn another_handle_preserves_revision_and_resumes_without_a_new_window_scan() {
    let root = tempfile::tempdir().expect("root");
    let path = root.path().join("history.db");
    let first = MessagesStore::open(&path).expect("first");
    seed_history(&first, 2000);
    assert_eq!(first.prune_announce_history_batch(1000).expect("first batch"), 512);
    let revision = first
        .with_read_conn(|conn| {
            conn.query_row("SELECT window_revision FROM announce_retention_count", [], |r| {
                r.get::<_, i64>(0)
            })
        })
        .expect("revision");
    let second = MessagesStore::open(&path).expect("second");
    assert_eq!(
        second
            .with_read_conn(|conn| conn.query_row(
                "SELECT window_revision FROM announce_retention_count",
                [],
                |r| r.get::<_, i64>(0)
            ))
            .expect("preserved revision"),
        revision
    );
    assert_eq!(second.prune_announce_history_batch(1000).expect("resumed batch"), 488);
    assert_eq!(
        second.operation_metrics_snapshot()["announces"]["retention_window"]["calls_total"],
        0
    );
    assert_eq!(
        second.operation_metrics_snapshot()["announces"]["retention_count"]["calls_total"],
        1
    );
}

#[test]
fn failed_batch_rolls_back_mutation_revision_bound_and_cursor() {
    let store = MessagesStore::in_memory().expect("store");
    seed_history(&store, 600);
    store.with_write_conn(|conn| conn.execute_batch("CREATE TRIGGER reject_delete BEFORE DELETE ON announces WHEN OLD.id='history-00000300' BEGIN SELECT RAISE(ABORT,'reject'); END;")).expect("failure trigger");
    assert!(store.prune_announce_history_batch(20).is_err());
    assert_eq!(cardinality(&store), (600, 600));
    assert_eq!(
        store
            .with_read_conn(|conn| conn.query_row(
                "SELECT window_revision FROM announce_retention_count",
                [],
                |r| r.get::<_, i64>(0)
            ))
            .expect("rolled back revision"),
        0
    );
    assert_eq!(
        store
            .with_read_conn(|conn| conn.query_row(
                "SELECT COUNT(*) FROM announce_retention_cursor",
                [],
                |r| r.get::<_, i64>(0)
            ))
            .expect("no committed cursor"),
        0
    );
    assert_eq!(store.operation_metrics_snapshot()["announces"]["examined_total"], 0);
    store.with_write_conn(|conn| conn.execute_batch("DROP TRIGGER reject_delete")).expect("repair");
    assert_eq!(store.prune_announce_history_batch(20).expect("retry"), 512);
    assert_eq!(store.prune_announce_history_batch(20).expect("finish"), 68);
    assert_eq!(cardinality(&store), (20, 20));
}

#[test]
fn legacy_cursor_without_revision_is_revalidated_before_resuming() {
    let root = tempfile::tempdir().expect("root");
    let path = root.path().join("history.db");
    {
        let store = MessagesStore::open(&path).expect("store");
        seed_history(&store, 2000);
        assert_eq!(store.prune_announce_history_batch(1000).expect("first batch"), 512);
        store.with_write_conn(|conn| conn.execute_batch(
            "ALTER TABLE announce_retention_cursor RENAME TO old_cursor;
             CREATE TABLE announce_retention_cursor(singleton INTEGER PRIMARY KEY,timestamp INTEGER NOT NULL,id TEXT NOT NULL,max_rows INTEGER NOT NULL);
             INSERT INTO announce_retention_cursor SELECT singleton,timestamp,id,max_rows FROM old_cursor;
             DROP TABLE old_cursor;
             DELETE FROM announces WHERE timestamp>1000;"
        )).expect("legacy cursor and deletion");
    }
    let store = MessagesStore::open(&path).expect("migrate legacy cursor");
    assert_eq!(store.prune_announce_history_batch(1000).expect("safe skip"), 0);
    assert_eq!(cardinality(&store), (488, 488));
}

#[test]
fn nested_revision_trigger_deletion_forces_exact_completion_count() {
    for recursive in [false, true] {
        let store = MessagesStore::in_memory().expect("store");
        store
            .with_write_conn(|conn| conn.pragma_update(None, "recursive_triggers", recursive))
            .expect("trigger mode");
        seed_history(&store, 50);
        // With recursion off, the nested DELETE cannot re-enter the same
        // announcement revision trigger already on the stack. U/revision alone
        // therefore miss this extra deletion; total_changes must reject it.
        store.with_write_conn(|conn| conn.execute_batch(
            "CREATE TRIGGER nested_delete AFTER UPDATE OF window_revision ON announce_retention_count
             WHEN OLD.window_revision=0 AND NEW.window_revision=1
             BEGIN DELETE FROM announces WHERE id='history-00000001'; END;"
        )).expect("nested deletion");
        assert_eq!(store.prune_announce_history_batch(20).expect("prune"), 29);
        assert_eq!(
            cardinality(&store),
            (20, 20),
            "nested deletion must not corrupt exact count, recursive={recursive}"
        );
        let metrics = store.operation_metrics_snapshot();
        assert_eq!(metrics["announces"]["retained_at_last_sweep"], 20);
        assert_eq!(
            metrics["announces"]["retention_count"]["calls_total"], 2,
            "unknown writes require fallback count"
        );
    }
}

#[test]
fn extra_trigger_writes_force_completion_count_in_both_trigger_modes() {
    for recursive in [false, true] {
        for trigger in [
            "CREATE TRIGGER audit_delete AFTER DELETE ON announces BEGIN INSERT INTO audit VALUES(1); END;",
            "CREATE TRIGGER audit_reset AFTER UPDATE OF upper_bound ON announce_retention_count WHEN OLD.upper_bound=NEW.upper_bound BEGIN INSERT INTO audit VALUES(1); END;",
        ] {
            let store = MessagesStore::in_memory().expect("store");
            store.with_write_conn(|conn| {
                conn.pragma_update(None, "recursive_triggers", recursive)?;
                conn.execute_batch("CREATE TABLE audit(value INTEGER)")?;
                Ok(())
            }).expect("audit setup");
            seed_history(&store, 50);
            store.with_write_conn(|conn| conn.execute_batch(trigger)).expect("trigger");
            assert_eq!(store.prune_announce_history_batch(20).expect("prune"), 30);
            assert_eq!(cardinality(&store), (20,20));
            assert_eq!(store.operation_metrics_snapshot()["announces"]["retention_count"]["calls_total"], 2,
                "extra writes must reject inference, recursive={recursive}, trigger={trigger}");
        }
    }
}

#[test]
fn full_batch_unknown_changes_refresh_cursor_and_publish_stored_upper_bound() {
    for recursive in [false, true] {
        for insert in [false, true] {
            let store = MessagesStore::in_memory().expect("store");
            store
                .with_write_conn(|conn| conn.pragma_update(None, "recursive_triggers", recursive))
                .expect("trigger mode");
            seed_history(&store, 1100);
            let mutation = if insert {
                "INSERT INTO announces(id,peer,timestamp,first_seen,seen_count) VALUES('extra','same-peer',-1,0,1);"
            } else {
                "DELETE FROM announces WHERE id='history-00000001';"
            };
            store.with_write_conn(|conn| conn.execute_batch(&format!(
                "CREATE TRIGGER mutate_revision AFTER UPDATE OF window_revision ON announce_retention_count
                 WHEN OLD.window_revision=0 AND NEW.window_revision=1 BEGIN {mutation} END;"
            ))).expect("named nested mutation");
            assert_eq!(store.prune_announce_history_batch(20).expect("first bounded batch"), 512);
            let (upper, actual) = cardinality(&store);
            assert_eq!(upper, if insert { 1101 } else { 1100 });
            assert_eq!(actual, if insert { 589 } else { 587 });
            assert_eq!(
                store.operation_metrics_snapshot()["announces"]["row_count_upper_bound"],
                upper
            );
            let revision = store
                .with_read_conn(|conn| {
                    conn.query_row(
                        "SELECT window_revision FROM announce_retention_cursor",
                        [],
                        |r| r.get::<_, Option<i64>>(0),
                    )
                })
                .expect("cursor");
            assert_eq!(revision, None, "unknown changes require next-call window validation");
            for _ in 0..3 {
                store.prune_announce_history_batch(20).expect("converge");
            }
            assert_eq!(cardinality(&store), (20, 20));
        }
    }
}
