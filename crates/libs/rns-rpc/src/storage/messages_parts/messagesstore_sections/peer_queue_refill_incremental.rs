// Incremental refill page: rows above the completed pass's rowid floor, newest
// rowid first, so a peer that already knows the inventory only pays for what
// arrived since. See peer_queue_refill_schema.rs for the floor invariant.

impl MessagesStore {
    fn refill_incremental_page(
        &self,
        tx: &rusqlite::Transaction<'_>,
        peer: &str,
        remaining: u64,
        boundary: &PeerQueueRefillBoundary,
        state: &mut PeerQueueRefillSavedState,
    ) -> rusqlite::Result<PeerQueueRefillPage> {
        let page_started = std::time::Instant::now();
        let floor = state.floor_rowid.unwrap_or(0);
        let cursor = state.cursor_rowid.unwrap_or(0);
        let candidates: Vec<(i64, String)> = {
            let mut statement = tx.prepare(
                "SELECT rowid, transient_id FROM propagation_entries
                 WHERE rowid > ?1 AND rowid < ?2
                 ORDER BY rowid DESC LIMIT ?3",
            )?;
            let rows = statement.query_map(
                params![floor, cursor, PEER_QUEUE_REFILL_SCAN_BATCH as i64],
                |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)),
            )?;
            rows.collect::<rusqlite::Result<Vec<_>>>()?
        };

        let mut inserted = 0u64;
        let mut scanned = 0usize;
        for (rowid, transient_id) in &candidates {
            if inserted >= remaining {
                break;
            }
            if Self::offer_peer_queue_candidate(tx, peer, transient_id)? {
                inserted += 1;
            }
            state.cursor_rowid = Some(*rowid);
            scanned += 1;
        }

        let exhausted = scanned == candidates.len() && candidates.len() < PEER_QUEUE_REFILL_SCAN_BATCH;
        if exhausted {
            state.scan_complete = true;
            state.cursor_rowid = None;
            state.floor_rowid = boundary.max_rowid.or(state.floor_rowid);
        }
        self.write_state.refill_metrics.record_page(
            candidates.len(),
            inserted as usize,
            page_started.elapsed().as_nanos().min(u128::from(u64::MAX)) as u64,
        );
        Ok(PeerQueueRefillPage {
            inserted: inserted as usize,
            capacity_reached: false,
            pass_finished: exhausted,
        })
    }
}
