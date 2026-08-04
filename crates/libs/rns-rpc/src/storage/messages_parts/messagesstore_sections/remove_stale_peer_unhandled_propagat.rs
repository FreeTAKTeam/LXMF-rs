impl MessagesStore {

    pub fn remove_stale_peer_unhandled_propagation_ids(
        &self,
        peer: &str,
    ) -> rusqlite::Result<Vec<String>> {
        self.with_write_conn(|conn| {
            let stale_ids = {
                let mut stmt = conn.prepare(
                    "SELECT transient_id
                     FROM propagation_peer_entries
                     WHERE LOWER(peer) = LOWER(?1)
                       AND state = 'unhandled'
                       AND NOT EXISTS (
                           SELECT 1
                           FROM propagation_entries e
                           WHERE e.transient_id = propagation_peer_entries.transient_id
                       )
                     ORDER BY transient_id ASC",
                )?;
                let rows = stmt.query_map(params![peer], |row| row.get(0))?;
                rows.collect::<rusqlite::Result<Vec<String>>>()?
            };
            conn.execute(
                "DELETE FROM propagation_peer_entries
                 WHERE LOWER(peer) = LOWER(?1)
                   AND state = 'unhandled'
                   AND NOT EXISTS (
                       SELECT 1
                       FROM propagation_entries e
                       WHERE e.transient_id = propagation_peer_entries.transient_id
                   )",
                params![peer],
            )?;
            Ok(stale_ids)
        })
    }

    pub fn remove_stale_peer_completed_propagation_ids(
        &self,
        peer: &str,
    ) -> rusqlite::Result<Vec<String>> {
        self.with_write_conn(|conn| {
            let stale_ids = {
                let mut stmt = conn.prepare(
                    "SELECT transient_id
                     FROM propagation_peer_entries
                     WHERE LOWER(peer) = LOWER(?1)
                       AND state IN ('handled', 'transferred', 'received', 'transfer_limited')
                       AND NOT EXISTS (
                           SELECT 1
                           FROM propagation_entries e
                           WHERE e.transient_id = propagation_peer_entries.transient_id
                       )
                     ORDER BY transient_id ASC",
                )?;
                let rows = stmt.query_map(params![peer], |row| row.get(0))?;
                rows.collect::<rusqlite::Result<Vec<String>>>()?
            };
            conn.execute(
                "DELETE FROM propagation_peer_entries
                 WHERE LOWER(peer) = LOWER(?1)
                   AND state IN ('handled', 'transferred', 'received', 'transfer_limited')
                   AND NOT EXISTS (
                       SELECT 1
                       FROM propagation_entries e
                       WHERE e.transient_id = propagation_peer_entries.transient_id
                   )",
                params![peer],
            )?;
            Ok(stale_ids)
        })
    }

    pub fn list_peer_unhandled_propagation(
        &self,
        peer: &str,
    ) -> rusqlite::Result<Vec<PropagationEntryRecord>> {
        self.with_read_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT e.transient_id, e.destination, e.payload_hex, e.received_at, e.size_bytes, e.stamp_value
                 FROM propagation_entries e
                 INNER JOIN (
                    SELECT transient_id,
                           CASE
                               WHEN SUM(CASE WHEN state = 'transfer_limited' THEN 1 ELSE 0 END) > 0 THEN 'transfer_limited'
                               WHEN SUM(CASE WHEN state = 'received' THEN 1 ELSE 0 END) > 0 THEN 'received'
                               WHEN SUM(CASE WHEN state = 'transferred' THEN 1 ELSE 0 END) > 0 THEN 'transferred'
                               WHEN SUM(CASE WHEN state = 'handled' THEN 1 ELSE 0 END) > 0 THEN 'handled'
                               ELSE 'unhandled'
                           END AS state
                    FROM propagation_peer_entries
                    WHERE LOWER(peer) = LOWER(?1)
                    GROUP BY transient_id
                 ) p
                    ON p.transient_id = e.transient_id
                 WHERE p.state = 'unhandled'
                 ORDER BY e.received_at ASC, e.transient_id ASC",
            )?;
            let rows = stmt.query_map(params![peer], propagation_entry_from_row)?;
            rows.collect()
        })
    }

    pub fn list_peer_prospective_unhandled_propagation(
        &self,
        peer: &str,
    ) -> rusqlite::Result<Vec<PropagationEntryRecord>> {
        self.with_read_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT e.transient_id, e.destination, e.payload_hex, e.received_at, e.size_bytes, e.stamp_value
                 FROM propagation_entries e
                 LEFT JOIN (
                    SELECT transient_id,
                           CASE
                               WHEN SUM(CASE WHEN state = 'transfer_limited' THEN 1 ELSE 0 END) > 0 THEN 'transfer_limited'
                               WHEN SUM(CASE WHEN state = 'received' THEN 1 ELSE 0 END) > 0 THEN 'received'
                               WHEN SUM(CASE WHEN state = 'transferred' THEN 1 ELSE 0 END) > 0 THEN 'transferred'
                               WHEN SUM(CASE WHEN state = 'handled' THEN 1 ELSE 0 END) > 0 THEN 'handled'
                               ELSE 'unhandled'
                           END AS state
                    FROM propagation_peer_entries
                    WHERE LOWER(peer) = LOWER(?1)
                    GROUP BY transient_id
                 ) p
                    ON p.transient_id = e.transient_id
                 WHERE p.state IS NULL OR p.state = 'unhandled'
                 ORDER BY e.received_at ASC, e.transient_id ASC",
            )?;
            let rows = stmt.query_map(params![peer], propagation_entry_from_row)?;
            rows.collect()
        })
    }

    pub fn list_peer_handled_propagation_ids(&self, peer: &str) -> rusqlite::Result<Vec<String>> {
        self.with_read_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT marks.transient_id
                 FROM (
                    SELECT transient_id,
                           CASE
                               WHEN SUM(CASE WHEN state = 'transfer_limited' THEN 1 ELSE 0 END) > 0 THEN 'transfer_limited'
                               WHEN SUM(CASE WHEN state = 'received' THEN 1 ELSE 0 END) > 0 THEN 'received'
                               WHEN SUM(CASE WHEN state = 'transferred' THEN 1 ELSE 0 END) > 0 THEN 'transferred'
                               WHEN SUM(CASE WHEN state = 'handled' THEN 1 ELSE 0 END) > 0 THEN 'handled'
                               ELSE 'unhandled'
                           END AS state
                    FROM propagation_peer_entries
                    WHERE LOWER(peer) = LOWER(?1)
                    GROUP BY transient_id
                 ) marks
                 INNER JOIN propagation_entries e
                    ON e.transient_id = marks.transient_id
                 WHERE marks.state IN ('handled', 'transferred', 'received', 'transfer_limited')
                 ORDER BY marks.transient_id ASC",
            )?;
            let rows = stmt.query_map(params![peer], |row| row.get(0))?;
            rows.collect()
        })
    }

    pub fn peer_completed_propagation_mark_exists(
        &self,
        peer: &str,
        transient_id: &str,
    ) -> rusqlite::Result<bool> {
        self.with_read_conn(|conn| {
            conn.query_row(
                "SELECT EXISTS(
                    SELECT 1
                    FROM propagation_peer_entries
                    WHERE LOWER(peer) = LOWER(?1)
                      AND transient_id = ?2
                      AND state IN ('handled', 'transferred', 'received', 'transfer_limited')
                    LIMIT 1
                 )",
                params![peer, normalize_hex_key(transient_id)],
                |row| row.get(0),
            )
        })
    }

    pub fn peer_propagation_mark_exists(
        &self,
        peer: &str,
        transient_id: &str,
    ) -> rusqlite::Result<bool> {
        self.with_read_conn(|conn| {
            conn.query_row(
                "SELECT EXISTS(
                    SELECT 1
                    FROM propagation_peer_entries
                    WHERE LOWER(peer) = LOWER(?1)
                      AND transient_id = ?2
                    LIMIT 1
                 )",
                params![peer, normalize_hex_key(transient_id)],
                |row| row.get(0),
            )
        })
    }

    pub fn peer_received_propagation_mark_exists(
        &self,
        peer: &str,
        transient_id: &str,
    ) -> rusqlite::Result<bool> {
        self.with_read_conn(|conn| {
            conn.query_row(
                "SELECT EXISTS(
                    SELECT 1
                    FROM propagation_peer_entries
                    WHERE LOWER(peer) = LOWER(?1)
                      AND transient_id = ?2
                      AND state = 'received'
                    LIMIT 1
                 )",
                params![peer, normalize_hex_key(transient_id)],
                |row| row.get(0),
            )
        })
    }

    pub fn list_peer_unhandled_propagation_ids(&self, peer: &str) -> rusqlite::Result<Vec<String>> {
        self.with_read_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT marks.transient_id
                 FROM (
                    SELECT transient_id,
                           CASE
                               WHEN SUM(CASE WHEN state = 'transfer_limited' THEN 1 ELSE 0 END) > 0 THEN 'transfer_limited'
                               WHEN SUM(CASE WHEN state = 'received' THEN 1 ELSE 0 END) > 0 THEN 'received'
                               WHEN SUM(CASE WHEN state = 'transferred' THEN 1 ELSE 0 END) > 0 THEN 'transferred'
                               WHEN SUM(CASE WHEN state = 'handled' THEN 1 ELSE 0 END) > 0 THEN 'handled'
                               ELSE 'unhandled'
                           END AS state
                    FROM propagation_peer_entries
                    WHERE LOWER(peer) = LOWER(?1)
                    GROUP BY transient_id
                 ) marks
                 INNER JOIN propagation_entries e
                    ON e.transient_id = marks.transient_id
                 WHERE marks.state = 'unhandled'
                 ORDER BY marks.transient_id ASC",
            )?;
            let rows = stmt.query_map(params![peer], |row| row.get(0))?;
            rows.collect()
        })
    }

    pub fn list_peer_unhandled_propagation_ids_limited(
        &self,
        peer: &str,
        limit: u64,
    ) -> rusqlite::Result<Vec<String>> {
        self.with_read_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT marks.transient_id
                 FROM (
                    SELECT transient_id,
                           CASE
                               WHEN SUM(CASE WHEN state = 'transfer_limited' THEN 1 ELSE 0 END) > 0 THEN 'transfer_limited'
                               WHEN SUM(CASE WHEN state = 'received' THEN 1 ELSE 0 END) > 0 THEN 'received'
                               WHEN SUM(CASE WHEN state = 'transferred' THEN 1 ELSE 0 END) > 0 THEN 'transferred'
                               WHEN SUM(CASE WHEN state = 'handled' THEN 1 ELSE 0 END) > 0 THEN 'handled'
                               ELSE 'unhandled'
                           END AS state
                    FROM propagation_peer_entries
                    WHERE LOWER(peer) = LOWER(?1)
                    GROUP BY transient_id
                 ) marks
                 INNER JOIN propagation_entries e
                    ON e.transient_id = marks.transient_id
                 WHERE marks.state = 'unhandled'
                 ORDER BY e.received_at DESC, marks.transient_id DESC
                 LIMIT ?2",
            )?;
            let rows = stmt.query_map(params![peer, limit.max(1)], |row| row.get(0))?;
            rows.collect()
        })
    }

    pub fn clear_peer_propagation_marks(&self, peer: &str) -> rusqlite::Result<usize> {
        self.with_write_conn(|conn| {
            let affected = conn.execute(
                "DELETE FROM propagation_peer_entries WHERE LOWER(peer) = LOWER(?1)",
                params![peer],
            )?;
            Ok(affected)
        })
    }

    pub fn clear_all_peer_propagation_marks(&self) -> rusqlite::Result<usize> {
        self.with_write_conn(|conn| {
            let affected = conn.execute("DELETE FROM propagation_peer_entries", [])?;
            Ok(affected)
        })
    }

    pub fn peer_propagation_mark_stats(
        &self,
        peer: &str,
    ) -> rusqlite::Result<PropagationEntryStats> {
        self.with_read_conn(|conn| {
            let (entries, bytes): (i64, Option<i64>) = conn.query_row(
                "SELECT
                    COALESCE(SUM(CASE WHEN e.transient_id IS NOT NULL THEN 1 ELSE 0 END), 0),
                    COALESCE(SUM(CASE WHEN e.transient_id IS NOT NULL THEN e.size_bytes ELSE 0 END), 0)
                 FROM (
                    SELECT transient_id
                    FROM propagation_peer_entries
                    WHERE LOWER(peer) = LOWER(?1)
                    GROUP BY transient_id
                 ) marks
                 LEFT JOIN propagation_entries e
                    ON e.transient_id = marks.transient_id",
                params![peer],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )?;
            Ok(PropagationEntryStats {
                entries: entries.max(0) as u64,
                bytes: bytes.unwrap_or(0).max(0) as u64,
            })
        })
    }

    pub fn peer_propagation_message_stats(
        &self,
        peer: &str,
    ) -> rusqlite::Result<PeerPropagationMessageStats> {
        self.with_read_conn(|conn| {
            let (outgoing, incoming, offered, unhandled, offered_bytes, unhandled_bytes): (
                i64,
                i64,
                i64,
                i64,
                Option<i64>,
                Option<i64>,
            ) = conn.query_row(
                "SELECT
                    COALESCE(SUM(CASE WHEN e.transient_id IS NOT NULL AND state = 'transferred' THEN 1 ELSE 0 END), 0),
                    COALESCE(SUM(CASE WHEN e.transient_id IS NOT NULL AND state = 'received' THEN 1 ELSE 0 END), 0),
                    COALESCE(SUM(CASE WHEN e.transient_id IS NOT NULL AND state IN ('handled', 'transferred') THEN 1 ELSE 0 END), 0),
                    COALESCE(SUM(CASE WHEN e.transient_id IS NOT NULL AND state = 'unhandled' THEN 1 ELSE 0 END), 0),
                    COALESCE(SUM(CASE WHEN e.transient_id IS NOT NULL AND state IN ('handled', 'transferred') THEN e.size_bytes ELSE 0 END), 0),
                    COALESCE(SUM(CASE WHEN e.transient_id IS NOT NULL AND state = 'unhandled' THEN e.size_bytes ELSE 0 END), 0)
                 FROM (
                    SELECT transient_id,
                           CASE
                               WHEN SUM(CASE WHEN state = 'transfer_limited' THEN 1 ELSE 0 END) > 0 THEN 'transfer_limited'
                               WHEN SUM(CASE WHEN state = 'received' THEN 1 ELSE 0 END) > 0 THEN 'received'
                               WHEN SUM(CASE WHEN state = 'transferred' THEN 1 ELSE 0 END) > 0 THEN 'transferred'
                               WHEN SUM(CASE WHEN state = 'handled' THEN 1 ELSE 0 END) > 0 THEN 'handled'
                               ELSE 'unhandled'
                           END AS state
                    FROM propagation_peer_entries
                    WHERE LOWER(peer) = LOWER(?1)
                    GROUP BY transient_id
                 ) marks
                 LEFT JOIN propagation_entries e
                    ON e.transient_id = marks.transient_id",
                params![peer],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get(5)?,
                    ))
                },
            )?;
            Ok(PeerPropagationMessageStats {
                outgoing: outgoing.max(0) as u64,
                incoming: incoming.max(0) as u64,
                offered: offered.max(0) as u64,
                unhandled: unhandled.max(0) as u64,
                offered_bytes: offered_bytes.unwrap_or(0).max(0) as u64,
                unhandled_bytes: unhandled_bytes.unwrap_or(0).max(0) as u64,
            })
        })
    }

    pub fn list_propagation_entries_for_destination(
        &self,
        destination: &str,
    ) -> rusqlite::Result<Vec<PropagationEntryRecord>> {
        self.with_read_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT transient_id, destination, payload_hex, received_at, size_bytes, stamp_value
                 FROM propagation_entries
                 WHERE destination = ?1
                 ORDER BY size_bytes ASC, transient_id ASC",
            )?;
            let rows = stmt
                .query_map(params![normalize_hex_key(destination)], propagation_entry_from_row)?;
            rows.collect()
        })
    }

    pub fn fetch_propagation_payloads_for_destination(
        &self,
        destination: &str,
        wanted: &[String],
        transfer_limit_bytes: Option<usize>,
    ) -> rusqlite::Result<Vec<Vec<u8>>> {
        let destination = normalize_hex_key(destination);
        self.with_read_conn(|conn| {
            let mut messages = Vec::new();
            let per_message_overhead = 16usize;
            let mut cumulative_size = 24usize;
            let mut stmt = conn.prepare(
                "SELECT transient_id, destination, payload_hex, received_at, size_bytes, stamp_value
                 FROM propagation_entries
                 WHERE transient_id = ?1 AND destination = ?2
                 LIMIT 1",
            )?;
            for transient_id in wanted {
                let Some(entry) = stmt
                    .query_row(
                        params![normalize_hex_key(transient_id), destination],
                        propagation_entry_from_row,
                    )
                    .optional()?
                else {
                    continue;
                };
                let stored_size = usize::try_from(entry.size_bytes).unwrap_or(usize::MAX);
                let next_size = cumulative_size
                    .saturating_add(stored_size.saturating_add(32) + per_message_overhead);
                if transfer_limit_bytes.is_some_and(|limit| next_size > limit) {
                    continue;
                }
                let payload = hex::decode(entry.payload_hex.as_str()).map_err(|err| {
                    rusqlite::Error::FromSqlConversionFailure(
                        0,
                        rusqlite::types::Type::Text,
                        Box::new(err),
                    )
                })?;
                cumulative_size = next_size;
                messages.push(payload);
            }
            Ok(messages)
        })
    }

    pub fn purge_propagation_entries_for_destination(
        &self,
        destination: &str,
        haves: &[String],
    ) -> rusqlite::Result<usize> {
        let destination = normalize_hex_key(destination);
        self.with_write_conn(|conn| {
            let tx = conn.unchecked_transaction()?;
            let mut purged = 0usize;
            for transient_id in haves {
                let transient_id = normalize_hex_key(transient_id);
                let affected = tx.execute(
                    "DELETE FROM propagation_entries
                     WHERE transient_id = ?1 AND destination = ?2",
                    params![transient_id, destination],
                )?;
                if affected > 0 {
                    tx.execute(
                        "DELETE FROM propagation_peer_entries
                         WHERE transient_id = ?1
                           AND state = 'unhandled'",
                        params![transient_id],
                    )?;
                    purged = purged.saturating_add(affected);
                }
            }
            tx.commit()?;
            Ok(purged)
        })
    }
}
