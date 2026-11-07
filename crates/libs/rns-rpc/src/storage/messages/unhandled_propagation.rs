use super::MessagesStore;

// Aggregate each ID directly in folded-index order, instead of wrapping the
// groups in a derived table that forces SQLite to sort the final result. A
// completed legacy case variant wins; unknown states retain the old fallback.
const UNHANDLED_IDS_SQL: &str = "SELECT p.transient_id
    FROM propagation_peer_entries p
    WHERE LOWER(p.peer) = LOWER(?1)
    GROUP BY p.transient_id
    HAVING MAX(p.state IN ('handled', 'transferred', 'received', 'transfer_limited')) = 0
      AND EXISTS (SELECT 1 FROM propagation_entries e WHERE e.transient_id = p.transient_id)
    ORDER BY p.transient_id ASC";

impl MessagesStore {
    pub fn list_peer_unhandled_propagation_ids(&self, peer: &str) -> rusqlite::Result<Vec<String>> {
        self.with_read_conn(|conn| {
            let mut statement = conn.prepare(UNHANDLED_IDS_SQL)?;
            self.write_state.unhandled_propagation_metrics.measure(&mut statement, |statement| {
                statement.query_map([peer], |row| row.get(0))?.collect()
            })
        })
    }
}

#[cfg(test)]
mod tests;

#[cfg(all(test, target_os = "linux"))]
mod soak;
