impl MessagesStore {
    /// Read the payload-backed queue classification in one SQLite snapshot.
    /// Only identifiers are selected; completed marks win across peer casing.
    pub(crate) fn peer_queue_snapshot_ids(
        &self,
        peer: &str,
    ) -> rusqlite::Result<(Vec<String>, Vec<String>)> {
        self.with_read_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT marks.transient_id,
                        MAX(marks.state IN ('handled', 'transferred', 'received', 'transfer_limited'))
                 FROM propagation_peer_entries marks
                 INNER JOIN propagation_entries e ON e.transient_id = marks.transient_id
                 WHERE LOWER(marks.peer) = LOWER(?1)
                 GROUP BY marks.transient_id
                 ORDER BY marks.transient_id ASC",
            )?;
            let mut rows = stmt.query(params![peer])?;
            let mut handled = Vec::new();
            let mut unhandled = Vec::new();
            while let Some(row) = rows.next()? {
                let id = row.get(0)?;
                if row.get::<_, bool>(1)? { handled.push(id); } else { unhandled.push(id); }
            }
            Ok((handled, unhandled))
        })
    }

    /// Fill only the remaining pending capacity. The count and insertion are a
    /// single SQLite statement, so concurrent refills cannot exceed the limit.
    pub fn mark_recent_propagation_unhandled_for_peer(
        &self,
        peer: &str,
        limit: u64,
    ) -> rusqlite::Result<usize> {
        self.with_write_conn(|conn| {
            let peer = normalize_peer_key(peer);
            conn.execute(
                "INSERT OR IGNORE INTO propagation_peer_entries
                    (peer, transient_id, state, updated_at)
                 SELECT ?1, transient_id, 'unhandled', ?2
                 FROM propagation_entries
                 WHERE NOT EXISTS (
                     SELECT 1
                     FROM propagation_peer_entries existing
                     WHERE LOWER(existing.peer) = LOWER(?1)
                       AND existing.transient_id = propagation_entries.transient_id
                 )
                 ORDER BY received_at DESC, transient_id DESC
                 LIMIT (
                     SELECT MAX(0, ?3 - COUNT(*)) FROM (
                         SELECT transient_id
                         FROM propagation_peer_entries
                         WHERE LOWER(peer) = LOWER(?1)
                         GROUP BY transient_id
                         HAVING SUM(CASE WHEN state <> 'unhandled' THEN 1 ELSE 0 END) = 0
                     )
                 )",
                params![peer, now_unix_secs(), limit.max(1)],
            )
        })
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
