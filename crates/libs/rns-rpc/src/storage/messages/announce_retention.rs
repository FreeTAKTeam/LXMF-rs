use super::*;

const BATCH: usize = 512;

impl MessagesStore {
    pub(super) fn init_announce_retention(conn: &Connection) -> rusqlite::Result<()> {
        conn.execute_batch(
            "CREATE INDEX IF NOT EXISTS idx_announces_peer_latest
                ON announces(peer, timestamp DESC, id DESC);
             CREATE INDEX IF NOT EXISTS idx_announces_peer_stamp
                ON announces(peer, timestamp DESC, id DESC, stamp_cost)
                WHERE stamp_cost IS NOT NULL;
             CREATE TABLE IF NOT EXISTS announce_retention_cursor (
                singleton INTEGER PRIMARY KEY CHECK(singleton = 1),
                timestamp INTEGER NOT NULL, id TEXT NOT NULL, max_rows INTEGER NOT NULL
             );",
        )
    }

    /// Examines at most 512 historical rows in one transaction. The newest
    /// `max_rows` events and each peer's latest event/non-null stamp are retained.
    pub fn prune_announce_history_batch(&self, max_rows: u64) -> rusqlite::Result<usize> {
        let max_rows = sql_u64(max_rows)?;
        let metrics = &self.write_state.announce_metrics;
        metrics.maintenance.measure(|| self.with_write_conn(|conn| {
            let tx = conn.unchecked_transaction()?;
            let cursor: Option<(i64, String, i64)> = tx.query_row(
                "SELECT timestamp, id, max_rows FROM announce_retention_cursor WHERE singleton = 1",
                [], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            ).optional()?;
            // Capture the recent window once per sweep; repeated OFFSET walks
            // otherwise reread N entries for every 512-row batch.
            let (boundary, resume) = if let Some((ts, id, saved_limit)) = cursor.filter(|c| c.2 == max_rows) {
                debug_assert_eq!(saved_limit, max_rows);
                (Some((ts, id)), true)
            } else {
                let mut statement = tx.prepare(
                    "SELECT timestamp, id FROM announces ORDER BY timestamp DESC, id DESC LIMIT 1 OFFSET ?1",
                )?;
                let boundary = metrics.window.measure(&mut statement, |statement| {
                    statement.query_row([max_rows], |row| Ok((row.get(0)?, row.get(1)?))).optional()
                })?;
                (boundary, false)
            };
            let candidates = if let Some((ts, id)) = boundary {
                let sql = if resume {
                    "SELECT timestamp, id FROM announces WHERE (timestamp, id) < (?1, ?2)
                     ORDER BY timestamp DESC, id DESC LIMIT 512"
                } else {
                    "SELECT timestamp, id FROM announces WHERE (timestamp, id) <= (?1, ?2)
                     ORDER BY timestamp DESC, id DESC LIMIT 512"
                };
                let mut statement = tx.prepare(sql)?;
                metrics.candidates.measure(&mut statement, |statement| {
                    statement.query_map(params![ts, id], |row| {
                        Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
                    })?.collect::<rusqlite::Result<Vec<_>>>()
                })?
            } else { Vec::new() };
            let mut removed = 0;
            {
                let mut statement = tx.prepare(
                    "DELETE FROM announces WHERE id = ?1
                     AND id != (SELECT a.id FROM announces a WHERE a.peer = announces.peer
                                ORDER BY a.timestamp DESC, a.id DESC LIMIT 1)
                     AND id IS NOT (SELECT a.id FROM announces a
                                    WHERE a.peer = announces.peer AND a.stamp_cost IS NOT NULL
                                    ORDER BY a.timestamp DESC, a.id DESC LIMIT 1)",
                )?;
                for (_, id) in &candidates {
                    removed += metrics.prune.measure(&mut statement, |statement| statement.execute([id]))?;
                }
            }
            let retained = if candidates.len() < BATCH {
                tx.execute("DELETE FROM announce_retention_cursor", [])?;
                let mut statement = tx.prepare("SELECT COUNT(*) FROM announces")?;
                Some(metrics.count.measure(&mut statement, |statement| {
                    statement.query_row([], |row| row.get::<_, i64>(0))
                })?)
            } else {
                let (ts, id) = candidates.last().expect("full nonempty batch");
                tx.execute("INSERT OR REPLACE INTO announce_retention_cursor VALUES (1, ?1, ?2, ?3)", params![ts, id, max_rows])?;
                None
            };
            tx.commit()?;
            metrics.examined_total.fetch_add(candidates.len() as u64, Ordering::Relaxed);
            if let Some(retained) = retained {
                metrics.retained_at_last_sweep.store(retained as u64, Ordering::Relaxed);
            }
            Ok(removed)
        }), |removed| *removed)
    }
}

#[cfg(test)]
mod tests;
