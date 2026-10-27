// Propagation refill scan state: schema, boundary capture, the saved per-peer
// row and the read-only gate that decides whether a refill needs the writer.
//
// The live database is never VACUUMed. Deleting the largest rowid can still
// allow rowid reuse, so it invalidates peer scans and advances `rescan_epoch`.
// A pass spanning that deletion cannot establish a safe incremental floor.
// Reordering `received_at` also advances the epoch and forces a full rescan.

const PEER_QUEUE_REFILL_SCAN_BATCH: usize = 128;

struct PeerQueueRefillPage {
    inserted: usize,
    capacity_reached: bool,
    pass_finished: bool,
}

#[derive(Clone)]
struct PeerQueueRefillBoundary {
    generation: i64,
    head: Option<(i64, String)>,
    max_rowid: Option<i64>,
    rescan_epoch: i64,
}

/// One `propagation_peer_refill_state` row.
#[derive(Clone, Debug)]
struct PeerQueueRefillSavedState {
    head_received_at: i64,
    head_transient_id: String,
    cursor: Option<(i64, String)>,
    observed_generation: i64,
    scan_complete: bool,
    rescan_required: bool,
    floor_rowid: Option<i64>,
    cursor_rowid: Option<i64>,
}

/// Why a refill returned without taking the write connection.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PeerQueueRefillSkip {
    /// Inventory empty and nothing recorded for the peer.
    EmptyInventory,
    /// The last pass completed and the inventory generation is unchanged.
    Complete,
    /// The peer's pending queue is already at its configured capacity.
    Capacity,
}

impl MessagesStore {
    fn capture_peer_queue_refill_boundary(
        conn: &Connection,
    ) -> rusqlite::Result<PeerQueueRefillBoundary> {
        conn.query_row(
            "SELECT inventory.generation, entry.received_at, entry.transient_id,
                    (SELECT MAX(rowid) FROM propagation_entries), inventory.rescan_epoch
             FROM propagation_inventory_generation AS inventory
             LEFT JOIN propagation_entries AS entry ON entry.transient_id = (
                 SELECT transient_id FROM propagation_entries
                 ORDER BY received_at DESC, transient_id DESC LIMIT 1
             )
             WHERE inventory.singleton = 1",
            [],
            |row| {
                let generation = row.get(0)?;
                let received_at = row.get::<_, Option<i64>>(1)?;
                let transient_id = row.get::<_, Option<String>>(2)?;
                let max_rowid = row.get::<_, Option<i64>>(3)?;
                Ok(PeerQueueRefillBoundary {
                    generation,
                    head: received_at.zip(transient_id),
                    max_rowid,
                    rescan_epoch: row.get(4)?,
                })
            },
        )
    }

    fn ensure_peer_queue_refill_schema(conn: &Connection) -> rusqlite::Result<()> {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS propagation_peer_refill_state (
                peer TEXT PRIMARY KEY,
                head_received_at INTEGER NOT NULL,
                head_transient_id TEXT NOT NULL,
                cursor_received_at INTEGER,
                cursor_transient_id TEXT,
                observed_generation INTEGER NOT NULL,
                scan_complete INTEGER NOT NULL DEFAULT 0,
                rescan_required INTEGER NOT NULL DEFAULT 0
            );
            CREATE TABLE IF NOT EXISTS propagation_inventory_generation (
                singleton INTEGER PRIMARY KEY CHECK(singleton = 1),
                generation INTEGER NOT NULL
            );
            INSERT OR IGNORE INTO propagation_inventory_generation(singleton, generation) VALUES (1, 0);
            CREATE INDEX IF NOT EXISTS idx_propagation_peer_entries_unhandled_folded
                ON propagation_peer_entries(LOWER(peer), transient_id) WHERE state = 'unhandled';
            CREATE TRIGGER IF NOT EXISTS propagation_refill_inventory_insert
                AFTER INSERT ON propagation_entries
                BEGIN UPDATE propagation_inventory_generation SET generation = generation + 1 WHERE singleton = 1; END;
            CREATE TRIGGER IF NOT EXISTS propagation_refill_inventory_update
                AFTER UPDATE OF received_at ON propagation_entries
                WHEN OLD.received_at <> NEW.received_at
                BEGIN UPDATE propagation_inventory_generation SET generation = generation + 1 WHERE singleton = 1; END;
            CREATE TRIGGER IF NOT EXISTS propagation_refill_inventory_delete
                AFTER DELETE ON propagation_entries
                BEGIN UPDATE propagation_inventory_generation SET generation = generation + 1 WHERE singleton = 1; END;
            -- A reordered row may sit below every peer's rowid floor; offer it through a full rescan.
            CREATE TRIGGER IF NOT EXISTS propagation_refill_inventory_reorder
                AFTER UPDATE OF received_at ON propagation_entries
                WHEN OLD.received_at <> NEW.received_at
                BEGIN UPDATE propagation_peer_refill_state SET rescan_required = 1; END;
            DROP TRIGGER IF EXISTS propagation_refill_peer_delete;
            CREATE TRIGGER propagation_refill_peer_delete
                AFTER DELETE ON propagation_peer_entries
                BEGIN
                    -- Only a still-present inventory row can be re-offered; pruned inventory needs no rescan.
                    UPDATE propagation_peer_refill_state SET rescan_required = 1
                    WHERE peer = LOWER(OLD.peer)
                      AND EXISTS (SELECT 1 FROM propagation_entries WHERE transient_id = OLD.transient_id);
                    DELETE FROM propagation_peer_refill_state
                    WHERE peer = LOWER(OLD.peer)
                      AND NOT EXISTS (SELECT 1 FROM propagation_peer_entries WHERE LOWER(peer) = LOWER(OLD.peer));
                END;",
        )?;
        Self::ensure_schema_column(
            conn,
            "propagation_peer_refill_state",
            "floor_rowid",
            "floor_rowid INTEGER",
        )?;
        Self::ensure_schema_column(
            conn,
            "propagation_peer_refill_state",
            "cursor_rowid",
            "cursor_rowid INTEGER",
        )?;
        Self::ensure_schema_column(
            conn,
            "propagation_inventory_generation",
            "rescan_epoch",
            "rescan_epoch INTEGER NOT NULL DEFAULT 0",
        )?;
        conn.execute_batch(
            "CREATE TRIGGER IF NOT EXISTS propagation_refill_inventory_reorder_epoch
                AFTER UPDATE OF received_at ON propagation_entries
                WHEN OLD.received_at <> NEW.received_at
                BEGIN
                    UPDATE propagation_inventory_generation SET rescan_epoch = rescan_epoch + 1
                    WHERE singleton = 1;
                END;
            CREATE TRIGGER IF NOT EXISTS propagation_refill_inventory_tail_delete
                AFTER DELETE ON propagation_entries
                WHEN NOT EXISTS (SELECT 1 FROM propagation_entries WHERE rowid > OLD.rowid)
                BEGIN
                    UPDATE propagation_inventory_generation SET rescan_epoch = rescan_epoch + 1
                    WHERE singleton = 1;
                    UPDATE propagation_peer_refill_state SET rescan_required = 1;
                END;",
        )
    }

    fn complete_peer_queue_refill(
        conn: &Connection,
        boundary: &PeerQueueRefillBoundary,
        state: &mut PeerQueueRefillSavedState,
    ) -> rusqlite::Result<()> {
        let rescan_epoch: i64 = conn.query_row(
            "SELECT rescan_epoch FROM propagation_inventory_generation WHERE singleton = 1",
            [],
            |row| row.get(0),
        )?;
        state.scan_complete = true;
        state.floor_rowid = if rescan_epoch == boundary.rescan_epoch { boundary.max_rowid } else { None };
        Ok(())
    }

    fn load_peer_queue_refill_state(
        conn: &Connection,
        peer: &str,
    ) -> rusqlite::Result<Option<PeerQueueRefillSavedState>> {
        conn.query_row(
            "SELECT head_received_at, head_transient_id, cursor_received_at,
                    cursor_transient_id, scan_complete, observed_generation, rescan_required,
                    floor_rowid, cursor_rowid
             FROM propagation_peer_refill_state WHERE peer = ?1",
            params![peer],
            |row| {
                let cursor_at = row.get::<_, Option<i64>>(2)?;
                let cursor_id = row.get::<_, Option<String>>(3)?;
                Ok(PeerQueueRefillSavedState {
                    head_received_at: row.get(0)?,
                    head_transient_id: row.get(1)?,
                    cursor: cursor_at.zip(cursor_id),
                    scan_complete: row.get(4)?,
                    observed_generation: row.get(5)?,
                    rescan_required: row.get(6)?,
                    floor_rowid: row.get(7)?,
                    cursor_rowid: row.get(8)?,
                })
            },
        )
        .optional()
    }

    /// Effective pending entries for `peer`, capped at `limit`: completed or
    /// received marks for the same transient id outrank a pending one.
    fn count_peer_queue_pending(conn: &Connection, peer: &str, limit: u64) -> rusqlite::Result<u64> {
        let pending: i64 = conn.query_row(
            "SELECT COUNT(*) FROM (
                SELECT transient_id
                FROM propagation_peer_entries AS pending
                    INDEXED BY idx_propagation_peer_entries_unhandled_folded
                WHERE LOWER(peer) = LOWER(?1)
                  AND state = 'unhandled'
                  AND NOT EXISTS (
                    SELECT 1 FROM propagation_peer_entries AS completed
                    WHERE LOWER(completed.peer) = LOWER(?1)
                      AND completed.transient_id = pending.transient_id
                      AND completed.state <> 'unhandled'
                  )
                GROUP BY transient_id
                LIMIT ?2
            )",
            params![peer, sql_u64(limit.max(1))?],
            |row| row.get(0),
        )?;
        Ok(pending.max(0) as u64)
    }

    /// Read-only decision made on the read connection before any writer work:
    /// a completed, unchanged pass or a full pending queue needs nothing from
    /// the writer. SQL triggers keep `generation` and `rescan_required`
    /// authoritative, so no in-memory mirror is needed.
    fn peer_queue_refill_gate(
        conn: &Connection,
        peer: &str,
        limit: u64,
        boundary: &PeerQueueRefillBoundary,
    ) -> rusqlite::Result<Option<PeerQueueRefillSkip>> {
        let saved = Self::load_peer_queue_refill_state(conn, peer)?;
        if boundary.head.is_none() {
            return Ok(saved.is_none().then_some(PeerQueueRefillSkip::EmptyInventory));
        }
        if let Some(saved) = &saved {
            if saved.scan_complete
                && saved.observed_generation == boundary.generation
                && !saved.rescan_required
            {
                return Ok(Some(PeerQueueRefillSkip::Complete));
            }
        }
        if Self::count_peer_queue_pending(conn, peer, limit)? >= limit.max(1) {
            return Ok(Some(PeerQueueRefillSkip::Capacity));
        }
        Ok(None)
    }

    fn persist_peer_queue_refill_state(
        tx: &rusqlite::Transaction<'_>,
        peer: &str,
        state: &PeerQueueRefillSavedState,
    ) -> rusqlite::Result<()> {
        tx.execute(
            "INSERT INTO propagation_peer_refill_state
                (peer, head_received_at, head_transient_id, cursor_received_at,
                 cursor_transient_id, observed_generation, scan_complete, rescan_required,
                 floor_rowid, cursor_rowid)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
             ON CONFLICT(peer) DO UPDATE SET
                head_received_at = excluded.head_received_at,
                head_transient_id = excluded.head_transient_id,
                cursor_received_at = excluded.cursor_received_at,
                cursor_transient_id = excluded.cursor_transient_id,
                observed_generation = excluded.observed_generation,
                scan_complete = excluded.scan_complete,
                rescan_required = excluded.rescan_required,
                floor_rowid = excluded.floor_rowid,
                cursor_rowid = excluded.cursor_rowid",
            params![
                peer,
                state.head_received_at,
                state.head_transient_id,
                state.cursor.as_ref().map(|(received_at, _)| *received_at),
                state.cursor.as_ref().map(|(_, id)| id.as_str()),
                state.observed_generation,
                state.scan_complete,
                state.rescan_required,
                state.floor_rowid,
                state.cursor_rowid,
            ],
        )?;
        Ok(())
    }

    /// Insert one candidate as pending for `peer` unless any mark already exists;
    /// returns whether a row was inserted.
    fn offer_peer_queue_candidate(
        tx: &rusqlite::Transaction<'_>,
        peer: &str,
        transient_id: &str,
    ) -> rusqlite::Result<bool> {
        tx.execute(
            "INSERT OR IGNORE INTO propagation_peer_entries
                (peer, transient_id, state, updated_at)
             SELECT ?1, ?2, 'unhandled', ?3
             WHERE NOT EXISTS (
                SELECT 1 FROM propagation_peer_entries
                WHERE LOWER(peer) = LOWER(?1) AND transient_id = ?2
             )",
            params![peer, transient_id, now_unix_secs()],
        )?;
        Ok(tx.changes() > 0)
    }
}
