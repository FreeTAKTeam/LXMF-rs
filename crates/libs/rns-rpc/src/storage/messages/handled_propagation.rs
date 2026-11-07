use super::MessagesStore;

const HANDLED_IDS_SQL: &str = "SELECT DISTINCT p.transient_id
    FROM propagation_peer_entries p
    WHERE LOWER(p.peer) = LOWER(?1)
      AND p.state IN ('handled', 'transferred', 'received', 'transfer_limited')
      AND EXISTS (SELECT 1 FROM propagation_entries e WHERE e.transient_id = p.transient_id)
    ORDER BY p.transient_id ASC";

impl MessagesStore {
    pub fn list_peer_handled_propagation_ids(&self, peer: &str) -> rusqlite::Result<Vec<String>> {
        self.with_read_conn(|conn| {
            // Any completed case variant wins; the folded index already orders and groups IDs.
            let mut statement = conn.prepare(HANDLED_IDS_SQL)?;
            self.write_state.handled_propagation_metrics.measure(&mut statement, |statement| {
                statement.query_map([peer], |row| row.get(0))?.collect()
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn handled_ids_stream_in_index_order_preserving_completed_case_variants() {
        let store = MessagesStore::in_memory().unwrap();
        store.with_write_conn(|conn| {
            conn.execute_batch("WITH RECURSIVE n(x) AS (VALUES(1) UNION ALL SELECT x+1 FROM n WHERE x<2000)
                INSERT INTO propagation_entries(transient_id,destination,payload_hex,received_at,size_bytes)
                SELECT printf('%064x',x),'destination','00',x,1 FROM n;
                INSERT INTO propagation_peer_entries(peer,transient_id,state,updated_at)
                SELECT 'Peer',transient_id,CASE received_at%5 WHEN 0 THEN 'unhandled' WHEN 1 THEN 'handled'
                    WHEN 2 THEN 'transferred' WHEN 3 THEN 'received' ELSE 'transfer_limited' END,0 FROM propagation_entries;
                INSERT INTO propagation_peer_entries(peer,transient_id,state,updated_at)
                SELECT 'peer',transient_id,'unhandled',0 FROM propagation_entries;
                INSERT INTO propagation_peer_entries VALUES('PEER','orphan','handled',0);
                INSERT INTO propagation_peer_entries VALUES('other','unrelated','handled',0);
                INSERT INTO propagation_peer_entries(peer,transient_id,state,updated_at)
                SELECT 'pEer',transient_id,'future-state',0 FROM propagation_entries;
                INSERT INTO propagation_peer_entries(peer,transient_id,state,updated_at)
                SELECT 'peER',transient_id,'received',0 FROM propagation_entries WHERE received_at%5<>0;")?;
            Ok(())
        }).unwrap();
        let expected =
            (1..=2000).filter(|n| n % 5 != 0).map(|n| format!("{n:064x}")).collect::<Vec<_>>();
        assert_eq!(store.list_peer_handled_propagation_ids("pEeR").unwrap(), expected);
        assert!(store.list_peer_handled_propagation_ids("missing").unwrap().is_empty());
        let metrics = store.operation_metrics_snapshot();
        let handled = &metrics["propagation_queries"]["handled_ids"];
        assert_eq!(handled["calls_total"], 2);
        assert_eq!(handled["errors_total"], 0);
        assert_eq!(handled["sorts_total"], 0);
        assert_eq!(handled["autoindex_rows_total"], 0);
        assert_eq!(handled["fullscan_steps_total"], 0);
        store
            .with_read_conn(|conn| {
                let plan = conn
                    .prepare(&format!("EXPLAIN QUERY PLAN {HANDLED_IDS_SQL}"))?
                    .query_map(["peer"], |r| r.get::<_, String>(3))?
                    .collect::<rusqlite::Result<Vec<_>>>()?
                    .join(" ");
                assert!(plan.contains("idx_propagation_peer_entries_folded_lookup"), "{plan}");
                assert!(!plan.contains("TEMP B-TREE"), "{plan}");
                Ok(())
            })
            .unwrap();
    }
}
