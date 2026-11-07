impl MessagesStore {
    /// Fill remaining pending capacity in bounded, independently committed pages,
    /// never past `total_limit` peer entries overall (the quota pass's global limit).
    /// The durable cursor avoids restarting completed scans on unchanged inventory.
    pub fn mark_recent_propagation_unhandled_for_peer(
        &self,
        peer: &str,
        limit: u64,
        total_limit: u64,
    ) -> rusqlite::Result<usize> {
        self.refill_recent_peer_propagation(peer, limit, total_limit)
    }

    pub(crate) fn import_peer_queue_marks(
        &self,
        peer: &str,
        handled_ids: &[String],
        unhandled_ids: &[String],
    ) -> rusqlite::Result<()> {
        self.with_write_conn(|conn| {
            let tx = conn.unchecked_transaction()?;
            let peer = normalize_peer_key(peer);
            // Completed state wins over pending state, without downgrading
            // received/transferred/transfer_limited marks or loading payloads.
            for (state, ids) in [("handled", handled_ids), ("unhandled", unhandled_ids)] {
                let mut stmt = tx.prepare(
                    "INSERT INTO propagation_peer_entries (peer, transient_id, state, updated_at)
                     SELECT ?1, ?2, ?3, ?4
                     WHERE EXISTS (SELECT 1 FROM propagation_entries WHERE transient_id = ?2)
                     ON CONFLICT(peer, transient_id) DO UPDATE SET state = excluded.state, updated_at = excluded.updated_at
                     WHERE propagation_peer_entries.state = 'unhandled' AND excluded.state = 'handled'",
                )?;
                for id in ids {
                    stmt.execute(params![peer, normalize_hex_key(id), state, now_unix_secs()])?;
                }
            }
            tx.commit()
        })
    }
}
