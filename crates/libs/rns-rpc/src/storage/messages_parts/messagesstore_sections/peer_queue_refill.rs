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
}

impl MessagesStore {
    fn capture_peer_queue_refill_boundary(
        conn: &Connection,
    ) -> rusqlite::Result<PeerQueueRefillBoundary> {
        conn.query_row(
            "SELECT inventory.generation, entry.received_at, entry.transient_id
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
                Ok(PeerQueueRefillBoundary { generation, head: received_at.zip(transient_id) })
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
            CREATE INDEX IF NOT EXISTS idx_propagation_entries_received_transient
                ON propagation_entries(received_at DESC, transient_id DESC);
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
            CREATE TRIGGER IF NOT EXISTS propagation_refill_peer_delete
                AFTER DELETE ON propagation_peer_entries
                BEGIN
                    UPDATE propagation_peer_refill_state SET rescan_required = 1 WHERE peer = LOWER(OLD.peer);
                    DELETE FROM propagation_peer_refill_state
                    WHERE peer = LOWER(OLD.peer)
                      AND NOT EXISTS (SELECT 1 FROM propagation_peer_entries WHERE LOWER(peer) = LOWER(OLD.peer));
                END;",
        )
    }

    fn refill_recent_peer_propagation_on(
        &self,
        conn: &Connection,
        peer: &str,
        limit: u64,
        boundary: &PeerQueueRefillBoundary,
    ) -> rusqlite::Result<PeerQueueRefillPage> {
        let tx =
            rusqlite::Transaction::new_unchecked(conn, rusqlite::TransactionBehavior::Immediate)?;
        let peer = normalize_peer_key(peer);
        let Some((boundary_head_received_at, boundary_head_id)) = &boundary.head else {
            tx.execute("DELETE FROM propagation_peer_refill_state WHERE peer = ?1", params![peer])?;
            tx.commit()?;
            return Ok(PeerQueueRefillPage {
                inserted: 0,
                capacity_reached: false,
                pass_finished: true,
            });
        };
        let pending_count: i64 = tx.query_row(
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
        let remaining =
            (sql_u64(limit.max(1))?.max(0) as u64).saturating_sub(pending_count.max(0) as u64);
        if remaining == 0 {
            tx.commit()?;
            return Ok(PeerQueueRefillPage {
                inserted: 0,
                capacity_reached: true,
                pass_finished: false,
            });
        }

        let saved_state = tx
            .query_row(
                "SELECT head_received_at, head_transient_id, cursor_received_at,
                        cursor_transient_id, scan_complete, observed_generation, rescan_required
                 FROM propagation_peer_refill_state WHERE peer = ?1",
                params![peer],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, Option<i64>>(2)?,
                        row.get::<_, Option<String>>(3)?,
                        row.get::<_, bool>(4)?,
                        row.get::<_, i64>(5)?,
                        row.get::<_, bool>(6)?,
                    ))
                },
            )
            .optional()?;

        let (
            mut scan_head_received_at,
            mut scan_head_id,
            mut cursor,
            mut observed_generation,
            mut rescan_required,
        ) = match saved_state {
            Some((
                saved_head_at,
                saved_head_id,
                cursor_at,
                cursor_id,
                complete,
                saved_generation,
                rescan,
            )) => {
                let mut cursor = cursor_at.zip(cursor_id);
                let mut observed_generation = saved_generation;
                let mut rescan_required = rescan;
                let mut head_at = saved_head_at;
                let mut head_id = saved_head_id;
                let inventory_changed = saved_generation != boundary.generation;
                if complete {
                    if inventory_changed || rescan_required {
                        head_at = *boundary_head_received_at;
                        head_id.clone_from(boundary_head_id);
                        cursor = None;
                        observed_generation = boundary.generation;
                        rescan_required = false;
                    } else {
                        tx.commit()?;
                        return Ok(PeerQueueRefillPage {
                            inserted: 0,
                            capacity_reached: false,
                            pass_finished: true,
                        });
                    }
                } else if inventory_changed || rescan_required {
                    // A pre-existing cursor must not let older candidates outrank
                    // inventory already visible when this refill invocation began.
                    head_at = *boundary_head_received_at;
                    head_id.clone_from(boundary_head_id);
                    cursor = None;
                    observed_generation = boundary.generation;
                    rescan_required = false;
                }
                (head_at, head_id, cursor, observed_generation, rescan_required)
            }
            None => (
                *boundary_head_received_at,
                boundary_head_id.clone(),
                None,
                boundary.generation,
                false,
            ),
        };

        let candidate_ids = if let Some((cursor_received_at, cursor_id)) = &cursor {
            let mut statement = tx.prepare(
                "SELECT received_at, transient_id FROM propagation_entries
                 WHERE (received_at, transient_id) <= (?1, ?2)
                   AND (received_at, transient_id) < (?3, ?4)
                 ORDER BY received_at DESC, transient_id DESC LIMIT ?5",
            )?;
            let rows = statement.query_map(
                params![
                    scan_head_received_at,
                    scan_head_id,
                    cursor_received_at,
                    cursor_id,
                    PEER_QUEUE_REFILL_SCAN_BATCH as i64
                ],
                |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)),
            )?;
            rows.collect::<rusqlite::Result<Vec<_>>>()?
        } else {
            let mut statement = tx.prepare(
                "SELECT received_at, transient_id FROM propagation_entries
                 WHERE (received_at, transient_id) <= (?1, ?2)
                 ORDER BY received_at DESC, transient_id DESC LIMIT ?3",
            )?;
            let rows = statement.query_map(
                params![scan_head_received_at, scan_head_id, PEER_QUEUE_REFILL_SCAN_BATCH as i64],
                |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)),
            )?;
            rows.collect::<rusqlite::Result<Vec<_>>>()?
        };

        let page_started = std::time::Instant::now();
        let mut inserted = 0u64;
        let mut scanned = 0usize;
        for (received_at, transient_id) in &candidate_ids {
            if inserted >= remaining {
                break;
            }
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
            inserted += tx.changes();
            cursor = Some((*received_at, transient_id.clone()));
            scanned += 1;
        }

        let exhausted_page =
            scanned == candidate_ids.len() && candidate_ids.len() < PEER_QUEUE_REFILL_SCAN_BATCH;
        let complete = if exhausted_page {
            if observed_generation != boundary.generation || rescan_required {
                scan_head_received_at = *boundary_head_received_at;
                scan_head_id.clone_from(boundary_head_id);
                observed_generation = boundary.generation;
                cursor = None;
                rescan_required = false;
                false
            } else {
                true
            }
        } else {
            false
        };

        tx.execute(
            "INSERT INTO propagation_peer_refill_state
                (peer, head_received_at, head_transient_id, cursor_received_at,
                 cursor_transient_id, observed_generation, scan_complete, rescan_required)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(peer) DO UPDATE SET
                head_received_at = excluded.head_received_at,
                head_transient_id = excluded.head_transient_id,
                cursor_received_at = excluded.cursor_received_at,
                cursor_transient_id = excluded.cursor_transient_id,
                observed_generation = excluded.observed_generation,
                scan_complete = excluded.scan_complete,
                rescan_required = excluded.rescan_required",
            params![
                peer,
                scan_head_received_at,
                scan_head_id,
                cursor.as_ref().map(|(received_at, _)| *received_at),
                cursor.as_ref().map(|(_, id)| id.as_str()),
                observed_generation,
                complete,
                rescan_required
            ],
        )?;
        tx.commit()?;
        self.write_state.refill_metrics.record_page(
            candidate_ids.len(),
            inserted as usize,
            page_started.elapsed().as_nanos().min(u128::from(u64::MAX)) as u64,
        );
        Ok(PeerQueueRefillPage {
            inserted: inserted as usize,
            capacity_reached: false,
            pass_finished: exhausted_page && complete,
        })
    }

    fn refill_recent_peer_propagation(&self, peer: &str, limit: u64) -> rusqlite::Result<usize> {
        let metrics = &self.write_state.refill_metrics;
        let started = std::time::Instant::now();
        metrics.invocations_total.fetch_add(1, Ordering::Relaxed);
        let boundary = self.with_read_conn(Self::capture_peer_queue_refill_boundary)?;
        let mut inserted = 0usize;
        loop {
            // Each page owns and commits its own transaction so a large mostly-known
            // inventory cannot hold SQLite's shared writer lock across the whole scan.
            let page = self.with_write_conn(|conn| {
                self.refill_recent_peer_propagation_on(conn, peer, limit, &boundary)
            })?;
            inserted = inserted.saturating_add(page.inserted);
            if page.capacity_reached {
                metrics.capacity_reached_total.fetch_add(1, Ordering::Relaxed);
            }
            if page.capacity_reached || page.pass_finished {
                break;
            }
        }
        metrics.wall_ns_total.fetch_add(
            started.elapsed().as_nanos().min(u128::from(u64::MAX)) as u64,
            Ordering::Relaxed,
        );
        Ok(inserted)
    }
}

#[cfg(test)]
mod invocation_boundary_tests {
    use super::*;

    #[test]
    fn refill_defers_inventory_inserted_between_pages_until_next_invocation() {
        let store = MessagesStore::in_memory().expect("in-memory store");
        let peer = "peer-refill-fixed-boundary";
        for index in 0..260 {
            let entry = PropagationEntryRecord {
                transient_id: format!("{index:064x}"),
                destination: "44".repeat(16),
                payload_hex: String::new(),
                received_at: index,
                size_bytes: 0,
                stamp_value: None,
            };
            store.upsert_propagation_entry(&entry).expect("insert inventory");
            store.mark_peer_handled_propagation(peer, &entry.transient_id).expect("mark known");
        }

        let boundary = store
            .with_read_conn(MessagesStore::capture_peer_queue_refill_boundary)
            .expect("capture boundary");
        let first_page = store
            .with_write_conn(|conn| {
                store.refill_recent_peer_propagation_on(conn, peer, 1_000, &boundary)
            })
            .expect("scan first page");
        assert!(!first_page.pass_finished);

        let new_entry = PropagationEntryRecord {
            transient_id: format!("{:064x}", 300),
            destination: "44".repeat(16),
            payload_hex: String::new(),
            received_at: 300,
            size_bytes: 0,
            stamp_value: None,
        };
        store.upsert_propagation_entry(&new_entry).expect("insert after boundary capture");

        loop {
            let page = store
                .with_write_conn(|conn| {
                    store.refill_recent_peer_propagation_on(conn, peer, 1_000, &boundary)
                })
                .expect("continue captured pass");
            assert_eq!(page.inserted, 0);
            if page.pass_finished {
                break;
            }
        }
        assert!(store.list_peer_unhandled_propagation_ids(peer).unwrap().is_empty());

        assert_eq!(store.mark_recent_propagation_unhandled_for_peer(peer, 1_000).unwrap(), 1);
        assert_eq!(
            store.list_peer_unhandled_propagation_ids(peer).unwrap(),
            vec![new_entry.transient_id]
        );
    }
}
