impl MessagesStore {
    pub fn prune_propagation_peer_entries_to_policy(
        &self,
        now: i64,
        unhandled_ttl_secs: u64,
        completed_ttl_secs: u64,
        total_limit: u64,
        per_peer_limit: u64,
    ) -> rusqlite::Result<usize> {
        let unhandled_cutoff = now.saturating_sub(i64::try_from(unhandled_ttl_secs).unwrap_or(i64::MAX));
        let completed_cutoff =
            now.saturating_sub(i64::try_from(completed_ttl_secs).unwrap_or(i64::MAX));
        let total_limit = total_limit.max(1);
        let per_peer_limit = per_peer_limit.max(1);

        self.with_write_conn(|conn| {
            let tx = conn.unchecked_transaction()?;
            let mut deleted = 0usize;

            deleted = deleted.saturating_add(tx.execute(
                "DELETE FROM propagation_peer_entries
                 WHERE NOT EXISTS (
                     SELECT 1
                     FROM propagation_entries e
                     WHERE e.transient_id = propagation_peer_entries.transient_id
                 )",
                [],
            )?);
            deleted = deleted.saturating_add(tx.execute(
                "DELETE FROM propagation_peer_entries
                 WHERE state = 'unhandled' AND updated_at < ?1",
                rusqlite::params![unhandled_cutoff],
            )?);
            deleted = deleted.saturating_add(tx.execute(
                "DELETE FROM propagation_peer_entries
                 WHERE state IN ('handled', 'transferred', 'received', 'transfer_limited')
                   AND updated_at < ?1",
                rusqlite::params![completed_cutoff],
            )?);

            deleted = deleted.saturating_add(tx.execute(
                "DELETE FROM propagation_peer_entries
                 WHERE rowid IN (
                     SELECT rowid
                     FROM (
                         SELECT rowid,
                                ROW_NUMBER() OVER (
                                    PARTITION BY peer
                                    ORDER BY updated_at DESC, transient_id DESC
                                ) AS peer_rank
                         FROM propagation_peer_entries
                         WHERE state = 'unhandled'
                     )
                     WHERE peer_rank > ?1
                 )",
                rusqlite::params![per_peer_limit],
            )?);

            let total: i64 = tx.query_row(
                "SELECT COUNT(*) FROM propagation_peer_entries",
                [],
                |row| row.get(0),
            )?;
            let total = total.max(0) as u64;
            if total > total_limit {
                let excess = total.saturating_sub(total_limit);
                deleted = deleted.saturating_add(tx.execute(
                    "DELETE FROM propagation_peer_entries
                     WHERE rowid IN (
                         SELECT rowid
                         FROM propagation_peer_entries
                         WHERE state = 'unhandled'
                         ORDER BY updated_at ASC, transient_id ASC, peer ASC
                         LIMIT ?1
                     )",
                    rusqlite::params![excess.min(i64::MAX as u64) as i64],
                )?);

                let remaining: i64 = tx.query_row(
                    "SELECT COUNT(*) FROM propagation_peer_entries",
                    [],
                    |row| row.get(0),
                )?;
                let remaining = remaining.max(0) as u64;
                if remaining > total_limit {
                    let completed_excess = remaining.saturating_sub(total_limit);
                    deleted = deleted.saturating_add(tx.execute(
                        "DELETE FROM propagation_peer_entries
                         WHERE rowid IN (
                             SELECT rowid
                             FROM propagation_peer_entries
                             WHERE state IN ('handled', 'transferred', 'received', 'transfer_limited')
                             ORDER BY updated_at ASC, transient_id ASC, peer ASC
                             LIMIT ?1
                         )",
                        rusqlite::params![completed_excess.min(i64::MAX as u64) as i64],
                    )?);
                }
            }

            tx.commit()?;
            Ok(deleted)
        })
    }
}
