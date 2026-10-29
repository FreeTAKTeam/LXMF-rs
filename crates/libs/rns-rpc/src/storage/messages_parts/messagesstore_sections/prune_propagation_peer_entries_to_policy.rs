const PEER_ENTRY_PRUNE_BATCH: usize = 512;
const COMPLETED_PEER_ENTRY_STATES: [&str; 4] =
    ["handled", "transferred", "received", "transfer_limited"];
type PeerEntryCandidate = (i64, i64, String, String);
const EXPIRED_PEER_ENTRY_PAGE_SQL: &str = "SELECT rowid, updated_at, peer, transient_id
     FROM propagation_peer_entries
     WHERE state = ?1 AND updated_at < ?2 AND (updated_at, rowid) > (?3, ?4)
     ORDER BY updated_at, rowid LIMIT ?5";

impl MessagesStore {
    pub fn prune_propagation_peer_entries_to_policy(
        &self,
        now: i64,
        unhandled_ttl_secs: u64,
        completed_ttl_secs: u64,
        total_limit: u64,
        per_peer_limit: u64,
    ) -> rusqlite::Result<usize> {
        let unhandled_cutoff =
            now.saturating_sub(i64::try_from(unhandled_ttl_secs).unwrap_or(i64::MAX));
        let completed_cutoff =
            now.saturating_sub(i64::try_from(completed_ttl_secs).unwrap_or(i64::MAX));

        let mut deleted = self.write_state.maintenance_metrics.expiry.measure(
            || self.prune_expired_peer_entry_pages(unhandled_cutoff, completed_cutoff),
            |count| *count,
        )?;

        // Keep quota ordering and concurrent-write semantics in the original
        // atomic transaction; only the measured expiry/orphan deletion is paged.
        deleted = deleted.saturating_add(self.write_state.maintenance_metrics.quota.measure(|| self.with_write_conn(|conn| {
            let tx = conn.unchecked_transaction()?;
            let mut quota_deleted = tx.execute(
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
                rusqlite::params![sql_u64(per_peer_limit.max(1))?],
            )?;

            let total: i64 = tx.query_row(
                "SELECT COUNT(*) FROM propagation_peer_entries",
                [],
                |row| row.get(0),
            )?;
            let total = total.max(0) as u64;
            let total_limit = total_limit.max(1);
            if total > total_limit {
                let excess = total.saturating_sub(total_limit);
                let unhandled_deleted = tx.execute(
                    "DELETE FROM propagation_peer_entries
                     WHERE rowid IN (
                         SELECT rowid
                         FROM propagation_peer_entries
                         WHERE state = 'unhandled'
                         ORDER BY updated_at ASC, transient_id ASC, peer ASC
                         LIMIT ?1
                     )",
                    rusqlite::params![excess.min(i64::MAX as u64) as i64],
                )?;
                quota_deleted = quota_deleted.saturating_add(unhandled_deleted);

                // Same transaction, so the count minus the deletions is exact.
                let remaining = total.saturating_sub(unhandled_deleted as u64);
                if remaining > total_limit {
                    let completed_excess = remaining.saturating_sub(total_limit);
                    quota_deleted = quota_deleted.saturating_add(tx.execute(
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
            Ok(quota_deleted)
        }), |count| *count)?);
        Ok(deleted)
    }

    /// Expired marks come from the `(state, updated_at)` index in bounded pages,
    /// so an idle pass reads a few index pages instead of the whole table (#663).
    /// Marks whose entry is gone keep their TTL/quota lifetime: the per-row
    /// orphan join scanned every page, and completed marks must survive a purge.
    fn prune_expired_peer_entry_pages(
        &self,
        unhandled_cutoff: i64,
        completed_cutoff: i64,
    ) -> rusqlite::Result<usize> {
        let mut deleted = 0usize;
        let states = std::iter::once(("unhandled", unhandled_cutoff))
            .chain(COMPLETED_PEER_ENTRY_STATES.iter().map(|state| (*state, completed_cutoff)));
        for (state, cutoff) in states {
            let mut cursor = (i64::MIN, i64::MIN);
            loop {
                let candidates = self.select_expired_peer_entry_page(state, cutoff, cursor)?;
                let Some(&(last_rowid, last_updated_at, _, _)) = candidates.last() else {
                    break;
                };
                let has_more = candidates.len() == PEER_ENTRY_PRUNE_BATCH;
                deleted = deleted.saturating_add(self.delete_expired_peer_entry_candidates(
                    &candidates,
                    unhandled_cutoff,
                    completed_cutoff,
                )?);
                drop(candidates);
                if !has_more {
                    break;
                }
                self.wait_for_priority_writers();
                cursor = (last_updated_at, last_rowid);
            }
        }
        Ok(deleted)
    }

    /// Keyset page of one state's expired marks in index order.
    fn select_expired_peer_entry_page(
        &self,
        state: &str,
        cutoff: i64,
        (cursor_updated_at, cursor_rowid): (i64, i64),
    ) -> rusqlite::Result<Vec<PeerEntryCandidate>> {
        self.with_read_conn(|conn| {
            let mut statement = conn.prepare_cached(EXPIRED_PEER_ENTRY_PAGE_SQL)?;
            let rows = statement.query_map(
                rusqlite::params![
                    state,
                    cutoff,
                    cursor_updated_at,
                    cursor_rowid,
                    PEER_ENTRY_PRUNE_BATCH as i64
                ],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )?;
            rows.collect()
        })
    }

    /// Apply the current orphan/TTL predicates to each selected identity.
    fn delete_expired_peer_entry_candidates(
        &self,
        candidates: &[PeerEntryCandidate],
        unhandled_cutoff: i64,
        completed_cutoff: i64,
    ) -> rusqlite::Result<usize> {
        self.with_write_conn(|conn| {
            let tx = conn.unchecked_transaction()?;
            let mut statement = tx.prepare_cached(
                "DELETE FROM propagation_peer_entries
                 WHERE rowid = ?1 AND updated_at = ?2 AND peer = ?3 AND transient_id = ?4
                   AND (
                       NOT EXISTS (
                           SELECT 1 FROM propagation_entries e
                           WHERE e.transient_id = propagation_peer_entries.transient_id
                       )
                       OR (state = 'unhandled' AND updated_at < ?5)
                       OR (state IN ('handled', 'transferred', 'received', 'transfer_limited')
                           AND updated_at < ?6)
                   )",
            )?;
            let mut deleted = 0usize;
            for (rowid, updated_at, peer, transient_id) in candidates {
                deleted = deleted.saturating_add(statement.execute(rusqlite::params![
                    rowid,
                    updated_at,
                    peer,
                    transient_id,
                    unhandled_cutoff,
                    completed_cutoff
                ])?);
            }
            drop(statement);
            tx.commit()?;
            Ok(deleted)
        })
    }
}
