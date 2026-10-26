const PEER_ENTRY_PRUNE_BATCH: usize = 512;
type PeerEntryCandidate = (i64, i64, String, String);
const PER_PEER_EXCESS_RECHECK: &str = "state = 'unhandled' AND (
    SELECT COUNT(*) FROM (
        SELECT 1 FROM propagation_peer_entries AS newer
        WHERE newer.peer = propagation_peer_entries.peer
          AND newer.state = 'unhandled'
          AND (newer.updated_at > propagation_peer_entries.updated_at
               OR (newer.updated_at = propagation_peer_entries.updated_at
                   AND newer.transient_id > propagation_peer_entries.transient_id))
        LIMIT ?5
    )
 ) >= ?5";

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
        let total_limit = total_limit.max(1);
        let per_peer_limit = per_peer_limit.max(1);

        let expired = self.with_read_conn(|conn| {
            let mut statement = conn.prepare(
                "SELECT rowid, updated_at, peer, transient_id
                 FROM propagation_peer_entries
                 WHERE NOT EXISTS (
                     SELECT 1 FROM propagation_entries e
                     WHERE e.transient_id = propagation_peer_entries.transient_id
                 )
                    OR (state = 'unhandled' AND updated_at < ?1)
                    OR (state IN ('handled', 'transferred', 'received', 'transfer_limited')
                        AND updated_at < ?2)",
            )?;
            let rows = statement.query_map(
                rusqlite::params![unhandled_cutoff, completed_cutoff],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                    ))
                },
            )?;
            rows.collect()
        })?;
        let mut deleted = self.delete_peer_entry_candidates(
            expired,
            "(NOT EXISTS (
                SELECT 1 FROM propagation_entries e
                WHERE e.transient_id = propagation_peer_entries.transient_id
             ))
             OR (state = 'unhandled' AND updated_at < ?5)
             OR (state IN ('handled', 'transferred', 'received', 'transfer_limited')
                 AND updated_at < ?6)",
            unhandled_cutoff,
            completed_cutoff,
            None,
        )?;

        let per_peer_excess = self.with_read_conn(|conn| {
            let mut statement = conn.prepare(
                "SELECT rowid, updated_at, peer, transient_id
                 FROM (
                     SELECT rowid, updated_at, peer, transient_id,
                            ROW_NUMBER() OVER (
                                PARTITION BY peer
                                ORDER BY updated_at DESC, transient_id DESC
                            ) AS peer_rank
                     FROM propagation_peer_entries
                     WHERE state = 'unhandled'
                 )
                 WHERE peer_rank > ?1",
            )?;
            let rows = statement.query_map(rusqlite::params![sql_u64(per_peer_limit)?], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                ))
            })?;
            rows.collect()
        })?;
        deleted = deleted.saturating_add(self.delete_peer_entry_candidates(
            per_peer_excess,
            PER_PEER_EXCESS_RECHECK,
            sql_u64(per_peer_limit)?,
            0,
            None,
        )?);

        let total = self.peer_entry_count()?;
        if total > total_limit {
            let excess = total.saturating_sub(total_limit);
            let unhandled = self.oldest_peer_entry_candidates("state = 'unhandled'", excess)?;
            deleted = deleted.saturating_add(self.delete_peer_entry_candidates(
                unhandled,
                "state = 'unhandled'",
                0,
                0,
                Some(total_limit),
            )?);

            let remaining = self.peer_entry_count()?;
            if remaining > total_limit {
                let completed_excess = remaining.saturating_sub(total_limit);
                let completed = self.oldest_peer_entry_candidates(
                    "state IN ('handled', 'transferred', 'received', 'transfer_limited')",
                    completed_excess,
                )?;
                deleted = deleted.saturating_add(self.delete_peer_entry_candidates(
                    completed,
                    "state IN ('handled', 'transferred', 'received', 'transfer_limited')",
                    0,
                    0,
                    Some(total_limit),
                )?);
            }
        }
        Ok(deleted)
    }

    fn peer_entry_count(&self) -> rusqlite::Result<u64> {
        self.with_read_conn(|conn| {
            let count: i64 =
                conn.query_row("SELECT COUNT(*) FROM propagation_peer_entries", [], |row| {
                    row.get(0)
                })?;
            Ok(count.max(0) as u64)
        })
    }

    fn oldest_peer_entry_candidates(
        &self,
        state_filter: &'static str,
        limit: u64,
    ) -> rusqlite::Result<Vec<PeerEntryCandidate>> {
        self.with_read_conn(|conn| {
            let query = format!(
                "SELECT rowid, updated_at, peer, transient_id FROM propagation_peer_entries
                 WHERE {state_filter}
                 ORDER BY updated_at ASC, transient_id ASC, peer ASC LIMIT ?1"
            );
            let mut statement = conn.prepare(&query)?;
            let rows = statement.query_map(
                rusqlite::params![limit.min(i64::MAX as u64) as i64],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, i64>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, String>(3)?,
                    ))
                },
            )?;
            rows.collect()
        })
    }

    /// Delete one read-selected candidate set in bounded writer transactions.
    /// `recheck` is a static policy predicate; updated rows and state transitions
    /// between selection and deletion therefore survive this maintenance pass.
    fn delete_peer_entry_candidates(
        &self,
        candidates: Vec<PeerEntryCandidate>,
        recheck: &'static str,
        parameter_1: i64,
        parameter_2: i64,
        total_limit_guard: Option<u64>,
    ) -> rusqlite::Result<usize> {
        let mut deleted = 0usize;
        let page_count = candidates.len().div_ceil(PEER_ENTRY_PRUNE_BATCH);
        for (page_index, page) in candidates.chunks(PEER_ENTRY_PRUNE_BATCH).enumerate() {
            let page_deleted = self.with_write_conn(|conn| {
                let tx = conn.unchecked_transaction()?;
                let mut remaining_budget = if let Some(limit) = total_limit_guard {
                    let count: i64 =
                        tx.query_row("SELECT COUNT(*) FROM propagation_peer_entries", [], |row| {
                            row.get(0)
                        })?;
                    count
                        .max(0)
                        .saturating_sub(limit.min(i64::MAX as u64) as i64)
                        .min(page.len() as i64) as usize
                } else {
                    page.len()
                };
                let sql = format!(
                    "DELETE FROM propagation_peer_entries
                     WHERE rowid = ?1 AND updated_at = ?2
                       AND peer = ?3 AND transient_id = ?4 AND ({recheck})"
                );
                let mut statement = tx.prepare_cached(&sql)?;
                let parameter_count = statement.parameter_count();
                let mut page_deleted = 0usize;
                for (rowid, updated_at, peer, transient_id) in page {
                    if remaining_budget == 0 {
                        break;
                    }
                    let parameters = rusqlite::params![
                        rowid, updated_at, peer, transient_id, parameter_1, parameter_2
                    ];
                    let changed = statement.execute(&parameters[..parameter_count])?;
                    page_deleted = page_deleted.saturating_add(changed);
                    remaining_budget = remaining_budget.saturating_sub(changed);
                }
                drop(statement);
                tx.commit()?;
                Ok(page_deleted)
            })?;
            deleted = deleted.saturating_add(page_deleted);
            if page_index + 1 < page_count {
                self.wait_for_priority_writers();
            }
        }
        Ok(deleted)
    }
}
