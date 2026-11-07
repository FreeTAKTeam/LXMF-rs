use super::*;
use rusqlite::{Connection, StatementStatus};

// Exact pre-fix statement, retained as an independent behavior/cost oracle.
pub(super) const BASELINE_SQL: &str = r#"SELECT marks.transient_id
                 FROM (
                    SELECT transient_id,
                           CASE
                               WHEN SUM(CASE WHEN state = 'transfer_limited' THEN 1 ELSE 0 END) > 0 THEN 'transfer_limited'
                               WHEN SUM(CASE WHEN state = 'received' THEN 1 ELSE 0 END) > 0 THEN 'received'
                               WHEN SUM(CASE WHEN state = 'transferred' THEN 1 ELSE 0 END) > 0 THEN 'transferred'
                               WHEN SUM(CASE WHEN state = 'handled' THEN 1 ELSE 0 END) > 0 THEN 'handled'
                               ELSE 'unhandled'
                           END AS state
                    FROM propagation_peer_entries
                    WHERE LOWER(peer) = LOWER(?1)
                    GROUP BY transient_id
                 ) marks
                 INNER JOIN propagation_entries e
                    ON e.transient_id = marks.transient_id
                 WHERE marks.state = 'unhandled'
                 ORDER BY marks.transient_id ASC"#;

pub(super) fn ids_and_cost(conn: &Connection, sql: &str, peer: &str) -> (Vec<String>, i32, i32) {
    let mut statement = conn.prepare(sql).expect("query");
    let ids = statement
        .query_map([peer], |row| row.get(0))
        .expect("rows")
        .collect::<rusqlite::Result<Vec<_>>>()
        .expect("IDs");
    (
        ids,
        statement.get_status(StatementStatus::Sort),
        statement.get_status(StatementStatus::VmStep),
    )
}

#[test]
fn unhandled_ids_preserve_state_precedence_case_variants_orphans_and_order_after_reopen() {
    let dir = tempfile::tempdir().expect("directory");
    let path = dir.path().join("state.db");
    let states =
        ["unhandled", "handled", "transferred", "received", "transfer_limited", "future-state", ""];
    let mut expected = Vec::new();
    {
        let store = MessagesStore::open(&path).expect("store");
        store.with_write_conn(|conn| {
            let tx = conn.unchecked_transaction()?;
            for (left, first) in states.iter().enumerate() {
                for (right, second) in states.iter().enumerate() {
                    let id = format!("{:064x}", left * states.len() + right);
                    tx.execute("INSERT INTO propagation_entries VALUES(?1,'destination','00',0,1,NULL)", [&id])?;
                    for (peer, state) in [("Peer", first), ("pEer", second), ("PEER", &"future-state")] {
                        tx.execute("INSERT INTO propagation_peer_entries VALUES(?1,?2,?3,0)", rusqlite::params![peer, id, state])?;
                    }
                    if !states[1..5].contains(first) && !states[1..5].contains(second) {
                        expected.push(id);
                    }
                }
            }
            tx.execute("INSERT INTO propagation_peer_entries VALUES('Peer','orphan','unhandled',0)", [])?;
            tx.execute("INSERT INTO propagation_peer_entries VALUES('other','unrelated','unhandled',0)", [])?;
            tx.commit()
        }).expect("seed");
    }
    for _ in 0..2 {
        let store = MessagesStore::open(&path).expect("reopen");
        assert_eq!(
            store.list_peer_unhandled_propagation_ids("pEeR").expect("mixed peer"),
            expected
        );
        assert!(store
            .list_peer_unhandled_propagation_ids("missing")
            .expect("missing peer")
            .is_empty());
        store
            .with_read_conn(|conn| {
                assert_eq!(ids_and_cost(conn, BASELINE_SQL, "peer").0, expected);
                Ok(())
            })
            .expect("baseline agreement");
        let metrics = store.operation_metrics_snapshot();
        let query = &metrics["propagation_queries"]["unhandled_ids"];
        assert_eq!(query["calls_total"], 2);
        for key in ["errors_total", "sorts_total", "fullscan_steps_total", "autoindex_rows_total"] {
            assert_eq!(query[key], 0, "{key}");
        }
    }
}

#[test]
fn unhandled_ids_stream_without_the_previous_temporary_sort() {
    let store = MessagesStore::in_memory().expect("store");
    store.with_write_conn(|conn| {
        conn.execute_batch("WITH RECURSIVE n(x) AS (VALUES(1) UNION ALL SELECT x+1 FROM n WHERE x<12000)
            INSERT INTO propagation_entries(transient_id,destination,payload_hex,received_at,size_bytes)
            SELECT printf('%064x',12001-x),'destination','00',x,1 FROM n;
            INSERT INTO propagation_peer_entries SELECT 'Peer',transient_id,'unhandled',0 FROM propagation_entries;")?;
        Ok(())
    }).expect("seed");
    store
        .with_read_conn(|conn| {
            let baseline = ids_and_cost(conn, BASELINE_SQL, "peer");
            let candidate = ids_and_cost(conn, UNHANDLED_IDS_SQL, "peer");
            assert_eq!(baseline.0, candidate.0);
            assert_eq!(candidate.0.len(), 12000);
            assert_eq!(baseline.1, 1, "baseline must demonstrate the defect");
            assert_eq!(candidate.1, 0);
            assert!(
                candidate.2 < baseline.2,
                "VM steps candidate={} baseline={}",
                candidate.2,
                baseline.2
            );
            let plan = conn
                .prepare(&format!("EXPLAIN QUERY PLAN {UNHANDLED_IDS_SQL}"))?
                .query_map(["peer"], |row| row.get::<_, String>(3))?
                .collect::<rusqlite::Result<Vec<_>>>()?
                .join(" ");
            assert!(plan.contains("idx_propagation_peer_entries_folded_lookup"), "{plan}");
            assert!(!plan.contains("TEMP B-TREE"), "{plan}");
            assert!(!plan.contains("MATERIALIZE"), "{plan}");
            Ok(())
        })
        .expect("query cost");
}
