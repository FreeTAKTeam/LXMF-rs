use super::*;

const BATCH: usize = 512;

impl MessagesStore {
    pub(super) fn init_announce_retention(conn: &Connection) -> rusqlite::Result<()> {
        let count_missing: bool = conn.query_row(
            "SELECT NOT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='announce_retention_count')",
            [], |row| row.get(0),
        )?;
        conn.execute_batch(
            "CREATE INDEX IF NOT EXISTS idx_announces_peer_latest
                ON announces(peer, timestamp DESC, id DESC);
             CREATE INDEX IF NOT EXISTS idx_announces_peer_stamp
                ON announces(peer, timestamp DESC, id DESC, stamp_cost)
                WHERE stamp_cost IS NOT NULL;
             CREATE TABLE IF NOT EXISTS announce_retention_cursor (
                singleton INTEGER PRIMARY KEY CHECK(singleton = 1),
                timestamp INTEGER NOT NULL, id TEXT NOT NULL, max_rows INTEGER NOT NULL,
                window_revision INTEGER
             );
             CREATE TABLE IF NOT EXISTS announce_retention_count (
                singleton INTEGER PRIMARY KEY CHECK(singleton = 1),
                upper_bound INTEGER NOT NULL CHECK(upper_bound >= 0),
                window_revision INTEGER NOT NULL DEFAULT 0
             );",
        )?;
        Self::ensure_schema_column(
            conn,
            "announce_retention_cursor",
            "window_revision",
            "window_revision INTEGER",
        )?;
        Self::ensure_schema_column(
            conn,
            "announce_retention_count",
            "window_revision",
            "window_revision INTEGER NOT NULL DEFAULT 0",
        )?;
        // One startup count reconciles conservative overestimates. Preserve the
        // mutation revision: another handle opening must not create cursor ABA.
        conn.execute_batch(
            "INSERT INTO announce_retention_count(singleton,upper_bound)
                VALUES(1,(SELECT COUNT(*) FROM announces))
                ON CONFLICT(singleton) DO UPDATE SET upper_bound=excluded.upper_bound;
             CREATE TRIGGER IF NOT EXISTS announce_retention_count_new_row
             BEFORE INSERT ON announces
             WHEN NOT EXISTS (SELECT 1 FROM announces WHERE id = NEW.id)
             BEGIN UPDATE announce_retention_count SET upper_bound = upper_bound + 1; END;
             CREATE TRIGGER IF NOT EXISTS announce_retention_window_replace
             BEFORE INSERT ON announces
             WHEN EXISTS (SELECT 1 FROM announces WHERE id = NEW.id)
             BEGIN UPDATE announce_retention_count SET window_revision = window_revision + 1; END;
             CREATE TRIGGER IF NOT EXISTS announce_retention_window_delete
             AFTER DELETE ON announces
             BEGIN UPDATE announce_retention_count SET window_revision = window_revision + 1; END;
             CREATE TRIGGER IF NOT EXISTS announce_retention_window_update
             AFTER UPDATE OF timestamp,id ON announces
             WHEN OLD.timestamp IS NOT NEW.timestamp OR OLD.id IS NOT NEW.id
             BEGIN UPDATE announce_retention_count SET window_revision = window_revision + 1; END;",
        )?;
        if count_missing {
            conn.execute("UPDATE announce_retention_cursor SET window_revision=NULL", [])?;
        }
        Ok(())
    }

    /// Examines at most 512 historical rows in one transaction. The newest
    /// `max_rows` events and each peer's latest event/non-null stamp are retained.
    pub fn prune_announce_history_batch(&self, max_rows: u64) -> rusqlite::Result<usize> {
        let max_rows = sql_u64(max_rows)?;
        let metrics = &self.write_state.announce_metrics;
        metrics.maintenance.measure(|| self.with_write_conn(|conn| {
            let tx = conn.unchecked_transaction()?;
            let cursor: Option<RetentionCursor> = tx.query_row(
                "SELECT timestamp, id, max_rows, window_revision FROM announce_retention_cursor WHERE singleton = 1",
                [], |row| Ok(RetentionCursor { timestamp: row.get(0)?, id: row.get(1)?, limit: row.get(2)?, revision: row.get(3)? }),
            ).optional()?;
            let (upper_bound, revision): (i64, i64) = tx.query_row(
                "SELECT upper_bound, window_revision FROM announce_retention_count WHERE singleton = 1",
                [], |row| Ok((row.get(0)?, row.get(1)?)),
            )?;
            // An upper bound proves there is no out-of-window row, even when
            // a previously active cursor remains after deletion or limit changes.
            if upper_bound <= max_rows {
                tx.execute("DELETE FROM announce_retention_cursor", [])?;
                tx.commit()?;
                metrics.under_window_skips_total.fetch_add(1, Ordering::Relaxed);
                metrics.row_count_upper_bound.store(upper_bound as u64, Ordering::Relaxed);
                return Ok(0);
            }
            let cursor = cursor.filter(|cursor| cursor.limit == max_rows);
            let (boundary, resume, fresh_count, changes_before) = if let Some(cursor) = cursor.as_ref().filter(|c| c.revision == Some(revision)) {
                (Some((cursor.timestamp, cursor.id.clone())), true, None, tx.total_changes())
            } else {
                // Only a new/invalidated window counts and seeks. Active batches
                // retain bounded keyset progress without recounting every 512 rows.
                let (count, fresh) = window_boundary(&tx, max_rows, metrics)?;
                let changes_before = tx.total_changes();
                tx.execute("UPDATE announce_retention_count SET upper_bound=?1 WHERE singleton=1", [count])?;
                let (boundary, resume) = clip_progress(fresh, cursor.as_ref());
                (boundary, resume, Some(count), changes_before)
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
            let (upper_after, revision_after): (i64, i64) = tx.query_row(
                "SELECT upper_bound,window_revision FROM announce_retention_count WHERE singleton=1", [], |row| Ok((row.get(0)?,row.get(1)?)),
            )?;
            // With the supported built-in bookkeeping schema, a direct DELETE
            // changes one announce plus its revision row. Include the fresh U
            // reset in the change anchor; extra writes reject this proof. This
            // is not support for arbitrary user-installed metadata triggers.
            let removed_i64 = i64::try_from(removed).ok();
            let expected_changes = u64::try_from(removed).ok()
                .and_then(|n| n.checked_mul(2))
                .and_then(|n| n.checked_add(u64::from(fresh_count.is_some())));
            let known_changes = upper_after == fresh_count.unwrap_or(upper_bound)
                && removed_i64.and_then(|n| revision.checked_add(n)) == Some(revision_after)
                && tx.total_changes().checked_sub(changes_before) == expected_changes;
            let inferred = fresh_count.filter(|_| known_changes)
                .and_then(|count| removed_i64.and_then(|n| count.checked_sub(n)))
                .filter(|count| *count >= 0);
            let retained = if candidates.len() < BATCH {
                tx.execute("DELETE FROM announce_retention_cursor", [])?;
                if inferred.is_some() {
                    inferred
                } else {
                    let mut statement = tx.prepare("SELECT COUNT(*) FROM announces")?;
                    Some(metrics.count.measure(&mut statement, |statement| {
                        statement.query_row([], |row| row.get::<_, i64>(0))
                    })?)
                }
            } else {
                let (ts, id) = candidates.last().expect("full nonempty batch");
                // Own deletions changed the revision; record their committed
                // endpoint only when no unknown changes require revalidation.
                tx.execute("INSERT OR REPLACE INTO announce_retention_cursor(singleton,timestamp,id,max_rows,window_revision) VALUES (1,?1,?2,?3,?4)", params![ts,id,max_rows,known_changes.then_some(revision_after)])?;
                None
            };
            if let Some(retained) = retained {
                tx.execute("UPDATE announce_retention_count SET upper_bound = ?1 WHERE singleton = 1", [retained])?;
            }
            let committed_upper: i64 = tx.query_row("SELECT upper_bound FROM announce_retention_count WHERE singleton=1", [], |row|row.get(0))?;
            tx.commit()?;
            metrics.examined_total.fetch_add(candidates.len() as u64, Ordering::Relaxed);
            if let Some(retained) = retained {
                metrics.retained_at_last_sweep.store(retained as u64, Ordering::Relaxed);
            }
            metrics.row_count_upper_bound.store(committed_upper as u64, Ordering::Relaxed);
            Ok(removed)
        }), |removed| *removed)
    }
}

struct RetentionCursor {
    timestamp: i64,
    id: String,
    limit: i64,
    revision: Option<i64>,
}

type Boundary = (i64, String);

fn window_boundary(
    conn: &Connection,
    limit: i64,
    metrics: &crate::storage::announce_metrics::AnnounceMetrics,
) -> rusqlite::Result<(i64, Option<Boundary>)> {
    let mut count = conn.prepare("SELECT COUNT(*) FROM announces")?;
    let count: i64 =
        metrics.count.measure(&mut count, |statement| statement.query_row([], |row| row.get(0)))?;
    if count <= limit {
        return Ok((count, None));
    }
    // Never derive the ascending rank from a conservative upper bound: a stale
    // overestimate would select a boundary inside the current newest-N window.
    let ascending_offset = count - limit - 1;
    let (sql, offset) = if ascending_offset < limit {
        (
            "SELECT timestamp,id FROM announces ORDER BY timestamp ASC,id ASC LIMIT 1 OFFSET ?1",
            ascending_offset,
        )
    } else {
        (
            "SELECT timestamp,id FROM announces ORDER BY timestamp DESC,id DESC LIMIT 1 OFFSET ?1",
            limit,
        )
    };
    let mut statement = conn.prepare(sql)?;
    let boundary = metrics.window.measure(&mut statement, |statement| {
        statement.query_row([offset], |row| Ok((row.get(0)?, row.get(1)?))).optional()
    })?;
    Ok((count, boundary))
}

fn clip_progress(
    fresh: Option<Boundary>,
    cursor: Option<&RetentionCursor>,
) -> (Option<Boundary>, bool) {
    let Some(fresh) = fresh else {
        return (None, false);
    };
    if let Some(cursor) = cursor {
        if (fresh.0, fresh.1.as_str()) >= (cursor.timestamp, cursor.id.as_str()) {
            return (Some((cursor.timestamp, cursor.id.clone())), true);
        }
    }
    (Some(fresh), false)
}

#[cfg(test)]
mod tests;
