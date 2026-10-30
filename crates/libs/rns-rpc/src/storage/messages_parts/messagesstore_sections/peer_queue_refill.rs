// Propagation refill: offer unseen inventory to a peer in bounded pages.
//
// Two scan modes share one persisted state row:
// - a full pass walks the newest-first `(received_at, transient_id)` index from
//   the boundary head down, 128 candidates per writer transaction;
// - an incremental pass, taken when a completed peer sees new inventory, walks
//   only rows whose rowid lies above the last completed `floor_rowid`.
// `rescan_required` (freed capacity or a reordered row) forces the full pass.
//
// Inserts also respect the global peer-entry limit that the quota pass
// enforces. The room is read from `propagation_peer_entry_budget` inside the
// same IMMEDIATE transaction as the inserts, and each insert in the page counts
// against it. At the limit a page stops before the next unoffered candidate and
// leaves the saved pass unfinished (or `rescan_required` set), so the work is
// deferred rather than skipped. The next refill after quota, expiry or an entry
// deletion frees room resumes it. Without this, quota would delete a mark and
// refill would recreate it on every pass. Scarce room is shared: when the room
// is smaller than this peer's need, one invocation takes at most
// `ceil(room / (peers awaiting refill + 1))`.

impl MessagesStore {
    fn refill_recent_peer_propagation_on(
        &self,
        conn: &Connection,
        peer: &str,
        limit: u64,
        total_limit: u64,
        boundary: &PeerQueueRefillBoundary,
        share: &mut Option<u64>,
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
                deferred: false,
            });
        };
        let pending_count = Self::count_peer_queue_pending(&tx, &peer, limit)?;
        let remaining = limit.max(1).saturating_sub(pending_count);
        if remaining == 0 {
            tx.commit()?;
            return Ok(PeerQueueRefillPage {
                inserted: 0,
                capacity_reached: true,
                pass_finished: false,
                deferred: false,
            });
        }
        let peer_entries: i64 = tx.query_row(
            "SELECT entry_count FROM propagation_peer_entry_budget WHERE singleton = 1",
            [],
            |row| row.get(0),
        )?;
        let room = Self::global_peer_entry_room(peer_entries, total_limit);
        let share_left = match *share {
            Some(left) => left,
            None => {
                // Room for this peer's whole need leaves nothing to ration; every
                // new inventory item marks other passes stale, so counting them
                // here would throttle refill even with ample room.
                let waiting = if room > 0 && room < remaining {
                    Self::count_peers_awaiting_refill(&tx, &peer, boundary.generation, room)?
                } else {
                    0
                };
                *share.insert(room.div_ceil(waiting.saturating_add(1)))
            }
        };
        let fresh = || PeerQueueRefillSavedState {
            head_received_at: *boundary_head_received_at,
            head_transient_id: boundary_head_id.clone(),
            cursor: None,
            observed_generation: boundary.generation,
            scan_complete: false,
            rescan_required: false,
            floor_rowid: None,
            cursor_rowid: None,
        };
        let budget = room.min(share_left);
        if budget == 0 {
            // Saved pass state is untouched, so the candidates stay eligible; a
            // peer seen for the first time records an unfinished pass instead.
            if Self::load_peer_queue_refill_state(&tx, &peer)?.is_none() {
                Self::persist_peer_queue_refill_state(&tx, &peer, &fresh())?;
            }
            tx.commit()?;
            return Ok(PeerQueueRefillPage {
                inserted: 0,
                capacity_reached: false,
                pass_finished: false,
                deferred: true,
            });
        }

        let saved = Self::load_peer_queue_refill_state(&tx, &peer)?;
        let mut state = match saved {
            None => fresh(),
            Some(saved) => {
                let inventory_changed = saved.observed_generation != boundary.generation;
                if saved.scan_complete && !inventory_changed && !saved.rescan_required {
                    tx.commit()?;
                    return Ok(PeerQueueRefillPage {
                        inserted: 0,
                        capacity_reached: false,
                        pass_finished: true,
                        deferred: false,
                    });
                }
                if saved.rescan_required {
                    // Freed capacity or reordered rows: only a full pass is sound.
                    fresh()
                } else if saved.scan_complete {
                    // New inventory above a completed pass: incremental from the newest rowid.
                    PeerQueueRefillSavedState {
                        cursor_rowid: saved
                            .floor_rowid
                            .map(|_| boundary.max_rowid.unwrap_or(0).saturating_add(1)),
                        floor_rowid: saved.floor_rowid,
                        ..fresh()
                    }
                } else if saved.cursor_rowid.is_some() {
                    // Incremental pass in progress: restart from the new newest rowid when the
                    // inventory moved, otherwise continue where the last page stopped.
                    let cursor_rowid = if inventory_changed {
                        boundary.max_rowid.unwrap_or(0).saturating_add(1)
                    } else {
                        saved.cursor_rowid.unwrap_or(0)
                    };
                    PeerQueueRefillSavedState {
                        cursor_rowid: Some(cursor_rowid),
                        floor_rowid: saved.floor_rowid,
                        ..fresh()
                    }
                } else if inventory_changed {
                    // A pre-existing cursor must not let older candidates outrank inventory
                    // already visible when this refill invocation began.
                    fresh()
                } else {
                    PeerQueueRefillSavedState { rescan_required: false, ..saved }
                }
            }
        };

        let page_limit = remaining.min(budget);
        let mut page = if state.cursor_rowid.is_some() {
            self.refill_incremental_page(&tx, &peer, page_limit, boundary, &mut state)?
        } else {
            self.refill_full_page(&tx, &peer, page_limit, boundary, &mut state)?
        };
        Self::persist_peer_queue_refill_state(&tx, &peer, &state)?;
        tx.commit()?;
        // Only after commit: a rolled-back page consumes no share.
        let inserted = page.inserted as u64;
        *share = Some(share_left.saturating_sub(inserted));
        page.deferred = !page.pass_finished && budget < remaining && inserted >= budget;
        Ok(page)
    }

    /// One 128-candidate page of the newest-first full pass.
    fn refill_full_page(
        &self,
        tx: &rusqlite::Transaction<'_>,
        peer: &str,
        remaining: u64,
        boundary: &PeerQueueRefillBoundary,
        state: &mut PeerQueueRefillSavedState,
    ) -> rusqlite::Result<PeerQueueRefillPage> {
        let page_started = std::time::Instant::now();
        let candidate_ids: Vec<(i64, String)> = if let Some((cursor_received_at, cursor_id)) =
            &state.cursor
        {
            let mut statement = tx.prepare(
                "SELECT received_at, transient_id FROM propagation_entries
                 WHERE (received_at, transient_id) <= (?1, ?2)
                   AND (received_at, transient_id) < (?3, ?4)
                 ORDER BY received_at DESC, transient_id DESC LIMIT ?5",
            )?;
            let rows = statement.query_map(
                params![
                    state.head_received_at,
                    state.head_transient_id,
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
                params![
                    state.head_received_at,
                    state.head_transient_id,
                    PEER_QUEUE_REFILL_SCAN_BATCH as i64
                ],
                |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)),
            )?;
            rows.collect::<rusqlite::Result<Vec<_>>>()?
        };

        let mut inserted = 0u64;
        let mut scanned = 0usize;
        for (received_at, transient_id) in &candidate_ids {
            if inserted >= remaining {
                break;
            }
            if Self::offer_peer_queue_candidate(tx, peer, transient_id)? {
                inserted += 1;
            }
            state.cursor = Some((*received_at, transient_id.clone()));
            scanned += 1;
        }

        let exhausted_page =
            scanned == candidate_ids.len() && candidate_ids.len() < PEER_QUEUE_REFILL_SCAN_BATCH;
        if exhausted_page {
            Self::complete_peer_queue_refill(tx, boundary, state)?;
        }
        self.write_state.refill_metrics.record_page(
            candidate_ids.len(),
            inserted as usize,
            page_started.elapsed().as_nanos().min(u128::from(u64::MAX)) as u64,
        );
        Ok(PeerQueueRefillPage {
            inserted: inserted as usize,
            capacity_reached: false,
            pass_finished: exhausted_page,
            deferred: false,
        })
    }

    fn refill_recent_peer_propagation(
        &self,
        peer: &str,
        limit: u64,
        total_limit: u64,
    ) -> rusqlite::Result<usize> {
        let metrics = &self.write_state.refill_metrics;
        let _in_flight = metrics.track_in_flight();
        let started = std::time::Instant::now();
        metrics.invocations_total.fetch_add(1, Ordering::Relaxed);
        let normalized = normalize_peer_key(peer);
        let (boundary, skip) = self.with_read_conn(|conn| {
            let boundary = Self::capture_peer_queue_refill_boundary(conn)?;
            let skip = Self::peer_queue_refill_gate(conn, &normalized, limit, total_limit, &boundary)?;
            Ok((boundary, skip))
        })?;
        if let Some(skip) = skip {
            match skip {
                PeerQueueRefillSkip::Complete | PeerQueueRefillSkip::EmptyInventory => {
                    metrics.skipped_complete_total.fetch_add(1, Ordering::Relaxed);
                }
                PeerQueueRefillSkip::Capacity => {
                    metrics.skipped_capacity_total.fetch_add(1, Ordering::Relaxed);
                }
                PeerQueueRefillSkip::GlobalCapacity => {
                    metrics.skipped_global_capacity_total.fetch_add(1, Ordering::Relaxed);
                }
            }
            return Ok(0);
        }
        let mut inserted = 0usize;
        let mut share = None;
        loop {
            // Each page owns and commits its own transaction so a large mostly-known
            // inventory cannot hold SQLite's shared writer lock across the whole scan.
            let page = self.with_write_conn(|conn| {
                self.refill_recent_peer_propagation_on(
                    conn,
                    peer,
                    limit,
                    total_limit,
                    &boundary,
                    &mut share,
                )
            })?;
            inserted = inserted.saturating_add(page.inserted);
            if page.capacity_reached {
                metrics.capacity_reached_total.fetch_add(1, Ordering::Relaxed);
            }
            if page.deferred {
                metrics.deferred_global_capacity_total.fetch_add(1, Ordering::Relaxed);
            }
            if page.capacity_reached || page.pass_finished || page.deferred {
                break;
            }
            // Broker/control commands share this mutex; let a waiting one run first.
            self.yield_to_priority_writers();
        }
        metrics.wall_ns_total.fetch_add(
            started.elapsed().as_nanos().min(u128::from(u64::MAX)) as u64,
            Ordering::Relaxed,
        );
        Ok(inserted)
    }
}
