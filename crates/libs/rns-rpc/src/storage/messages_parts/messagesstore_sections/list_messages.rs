impl MessagesStore {

    pub fn list_messages(
        &self,
        limit: usize,
        before_ts: Option<i64>,
    ) -> rusqlite::Result<Vec<MessageRecord>> {
        self.list_messages_page(limit, before_ts, None)
    }

    pub fn list_messages_page(
        &self,
        limit: usize,
        before_ts: Option<i64>,
        before_id: Option<&str>,
    ) -> rusqlite::Result<Vec<MessageRecord>> {
        self.with_read_conn(|conn| {
            let mut records = Vec::new();
            if let Some(ts) = before_ts {
                let mut stmt = if before_id.is_some() {
                    conn.prepare(
                        "SELECT id, source, destination, title, content, timestamp, direction, fields, receipt_status FROM messages WHERE (timestamp < ?1 OR (timestamp = ?1 AND id < ?2)) ORDER BY timestamp DESC, id DESC LIMIT ?3",
                    )?
                } else {
                    conn.prepare(
                        "SELECT id, source, destination, title, content, timestamp, direction, fields, receipt_status FROM messages WHERE timestamp < ?1 ORDER BY timestamp DESC, id DESC LIMIT ?2",
                    )?
                };
                let mut rows = if let Some(before_id) = before_id {
                    stmt.query(params![ts, before_id, limit as i64])?
                } else {
                    stmt.query(params![ts, limit as i64])?
                };
                while let Some(row) = rows.next()? {
                    records.push(message_record_from_row(row)?);
                }
            } else {
                let mut stmt = conn.prepare(
                    "SELECT id, source, destination, title, content, timestamp, direction, fields, receipt_status FROM messages ORDER BY timestamp DESC, id DESC LIMIT ?1",
                )?;
                let mut rows = stmt.query(params![limit as i64])?;
                while let Some(row) = rows.next()? {
                    records.push(message_record_from_row(row)?);
                }
            }
            Ok(records)
        })
    }

    pub fn list_messages_page_for_peer(
        &self,
        limit: usize,
        before_ts: Option<i64>,
        before_id: Option<&str>,
        peer: &str,
    ) -> rusqlite::Result<Vec<MessageRecord>> {
        self.with_read_conn(|conn| {
            let mut records = Vec::new();
            if let Some(ts) = before_ts {
                let mut stmt = if before_id.is_some() {
                    conn.prepare(
                        "SELECT id, source, destination, title, content, timestamp, direction, fields, receipt_status FROM messages WHERE (LOWER(source) = LOWER(?1) OR LOWER(destination) = LOWER(?1)) AND (timestamp < ?2 OR (timestamp = ?2 AND id < ?3)) ORDER BY timestamp DESC, id DESC LIMIT ?4",
                    )?
                } else {
                    conn.prepare(
                        "SELECT id, source, destination, title, content, timestamp, direction, fields, receipt_status FROM messages WHERE (LOWER(source) = LOWER(?1) OR LOWER(destination) = LOWER(?1)) AND timestamp < ?2 ORDER BY timestamp DESC, id DESC LIMIT ?3",
                    )?
                };
                let mut rows = if let Some(before_id) = before_id {
                    stmt.query(params![peer, ts, before_id, limit as i64])?
                } else {
                    stmt.query(params![peer, ts, limit as i64])?
                };
                while let Some(row) = rows.next()? {
                    records.push(message_record_from_row(row)?);
                }
            } else {
                let mut stmt = conn.prepare(
                    "SELECT id, source, destination, title, content, timestamp, direction, fields, receipt_status FROM messages WHERE LOWER(source) = LOWER(?1) OR LOWER(destination) = LOWER(?1) ORDER BY timestamp DESC, id DESC LIMIT ?2",
                )?;
                let mut rows = stmt.query(params![peer, limit as i64])?;
                while let Some(row) = rows.next()? {
                    records.push(message_record_from_row(row)?);
                }
            }
            Ok(records)
        })
    }

    pub fn get_message(&self, message_id: &str) -> rusqlite::Result<Option<MessageRecord>> {
        self.with_read_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT id, source, destination, title, content, timestamp, direction, fields, receipt_status FROM messages WHERE id = ?1 LIMIT 1",
            )?;
            stmt.query_row(params![message_id], message_record_from_row).optional()
        })
    }

    pub fn message_count(&self) -> rusqlite::Result<u64> {
        Ok(self.write_state.message_count_cache.load(Ordering::Relaxed))
    }

    pub fn message_storage_stats(&self) -> rusqlite::Result<MessageStorageStats> {
        self.with_read_conn(|conn| {
            let count = self.write_state.message_count_cache.load(Ordering::Relaxed);
            let bytes: Option<i64> = conn.query_row(
                "SELECT COALESCE(SUM(
                    LENGTH(id) +
                    LENGTH(source) +
                    LENGTH(destination) +
                    LENGTH(title) +
                    LENGTH(content) +
                    LENGTH(direction) +
                    COALESCE(LENGTH(fields), 0) +
                    COALESCE(LENGTH(receipt_status), 0)
                ), 0) FROM messages",
                [],
                |row| row.get(0),
            )?;
            Ok(MessageStorageStats { count, bytes: bytes.unwrap_or(0).max(0) as u64 })
        })
    }

    pub fn peer_message_stats(&self, peer: &str) -> rusqlite::Result<PeerMessageStats> {
        self.with_read_conn(|conn| {
            let (outgoing, incoming, offered, unhandled): (i64, i64, i64, i64) = conn.query_row(
                "SELECT
                    COALESCE(SUM(CASE WHEN destination = ?1 AND direction = 'out' THEN 1 ELSE 0 END), 0),
                    COALESCE(SUM(CASE WHEN source = ?1 AND direction = 'in' THEN 1 ELSE 0 END), 0),
                    COALESCE(SUM(CASE
                        WHEN destination = ?1
                         AND direction = 'out'
                         AND (
                            receipt_status IS NULL
                            OR TRIM(receipt_status) = ''
                            OR (
                                LOWER(receipt_status) NOT LIKE 'sent%'
                                AND LOWER(receipt_status) NOT LIKE 'failed%'
                                AND LOWER(receipt_status) NOT IN ('cancelled', 'delivered', 'failed', 'expired', 'rejected')
                            )
                         )
                        THEN 1
                        ELSE 0
                    END), 0),
                    COALESCE(SUM(CASE WHEN source = ?1 AND direction = 'in' AND receipt_status IS NULL THEN 1 ELSE 0 END), 0)
                 FROM messages",
                params![peer],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )?;
            Ok(PeerMessageStats {
                outgoing: outgoing.max(0) as u64,
                incoming: incoming.max(0) as u64,
                offered: offered.max(0) as u64,
                unhandled: unhandled.max(0) as u64,
            })
        })
    }

    pub fn upsert_propagation_entry(
        &self,
        record: &PropagationEntryRecord,
    ) -> rusqlite::Result<()> {
        self.with_write_conn(|conn| {
            conn.execute(
                "INSERT INTO propagation_entries (
                    transient_id,
                    destination,
                    payload_hex,
                    received_at,
                    size_bytes,
                    stamp_value
                )
                VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                ON CONFLICT(transient_id) DO UPDATE SET
                    destination = excluded.destination,
                    payload_hex = excluded.payload_hex,
                    received_at = excluded.received_at,
                    size_bytes = excluded.size_bytes,
                    stamp_value = excluded.stamp_value",
                params![
                    record.transient_id,
                    record.destination,
                    record.payload_hex,
                    record.received_at,
                    record.size_bytes,
                    record.stamp_value,
                ],
            )?;
            Ok(())
        })
    }

    pub fn get_propagation_entry(
        &self,
        transient_id: &str,
    ) -> rusqlite::Result<Option<PropagationEntryRecord>> {
        self.with_read_conn(|conn| {
            conn.query_row(
                "SELECT transient_id, destination, payload_hex, received_at, size_bytes, stamp_value
                 FROM propagation_entries
                 WHERE transient_id = ?1
                 LIMIT 1",
                params![normalize_hex_key(transient_id)],
                propagation_entry_from_row,
            )
            .optional()
        })
    }

    pub fn mark_local_propagation_processed(&self, transient_id: &str) -> rusqlite::Result<bool> {
        self.with_write_conn(|conn| {
            let affected = conn.execute(
                "INSERT OR IGNORE INTO propagation_local_entries
                    (transient_id, processed_at)
                 VALUES (?1, ?2)",
                params![normalize_hex_key(transient_id), now_unix_secs()],
            )?;
            Ok(affected > 0)
        })
    }

    pub fn local_propagation_processed_mark_exists(
        &self,
        transient_id: &str,
    ) -> rusqlite::Result<bool> {
        self.with_read_conn(|conn| {
            let exists: Option<i64> = conn
                .query_row(
                    "SELECT 1 FROM propagation_local_entries
                     WHERE transient_id = ?1
                     LIMIT 1",
                    params![normalize_hex_key(transient_id)],
                    |row| row.get(0),
                )
                .optional()?;
            Ok(exists.is_some())
        })
    }

    pub fn propagation_entry_stats(&self) -> rusqlite::Result<PropagationEntryStats> {
        self.with_read_conn(|conn| {
            let (entries, bytes): (i64, Option<i64>) = conn.query_row(
                "SELECT COUNT(*), COALESCE(SUM(size_bytes), 0) FROM propagation_entries",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )?;
            Ok(PropagationEntryStats {
                entries: entries.max(0) as u64,
                bytes: bytes.unwrap_or(0).max(0) as u64,
            })
        })
    }

    pub fn mark_peer_unhandled_propagation(
        &self,
        peer: &str,
        transient_id: &str,
    ) -> rusqlite::Result<()> {
        self.with_write_conn(|conn| {
            let peer = normalize_peer_key(peer);
            conn.execute(
                "INSERT OR IGNORE INTO propagation_peer_entries
                    (peer, transient_id, state, updated_at)
                 VALUES (?1, ?2, 'unhandled', ?3)",
                params![peer, normalize_hex_key(transient_id), now_unix_secs()],
            )?;
            Ok(())
        })
    }

    pub fn mark_all_propagation_unhandled_for_peer(&self, peer: &str) -> rusqlite::Result<usize> {
        self.with_write_conn(|conn| {
            let peer = normalize_peer_key(peer);
            conn.execute(
                "INSERT OR IGNORE INTO propagation_peer_entries
                    (peer, transient_id, state, updated_at)
                 SELECT ?1, transient_id, 'unhandled', ?2
                 FROM propagation_entries",
                params![peer, now_unix_secs()],
            )
        })
    }

    pub fn merge_case_insensitive_peer_propagation_marks(
        &self,
        peer: &str,
    ) -> rusqlite::Result<()> {
        self.with_write_conn(|conn| {
            let peer = normalize_peer_key(peer);
            conn.execute(
                "INSERT INTO propagation_peer_entries (peer, transient_id, state, updated_at)
                 SELECT ?1,
                        transient_id,
                        CASE
                            WHEN SUM(CASE WHEN state = 'transfer_limited' THEN 1 ELSE 0 END) > 0 THEN 'transfer_limited'
                            WHEN SUM(CASE WHEN state = 'received' THEN 1 ELSE 0 END) > 0 THEN 'received'
                            WHEN SUM(CASE WHEN state = 'transferred' THEN 1 ELSE 0 END) > 0 THEN 'transferred'
                            WHEN SUM(CASE WHEN state = 'handled' THEN 1 ELSE 0 END) > 0 THEN 'handled'
                            ELSE 'unhandled'
                        END,
                        MAX(updated_at)
                 FROM propagation_peer_entries
                 WHERE LOWER(peer) = LOWER(?1)
                 GROUP BY transient_id
                 ON CONFLICT(peer, transient_id) DO UPDATE SET
                    state = CASE
                        WHEN propagation_peer_entries.state IN ('transfer_limited', 'received', 'transferred', 'handled') THEN propagation_peer_entries.state
                        WHEN excluded.state IN ('transfer_limited', 'received', 'transferred', 'handled') THEN excluded.state
                        ELSE excluded.state
                    END,
                    updated_at = MAX(propagation_peer_entries.updated_at, excluded.updated_at)",
                params![peer],
            )?;
            conn.execute(
                "DELETE FROM propagation_peer_entries
                 WHERE LOWER(peer) = LOWER(?1)
                   AND peer <> ?1",
                params![peer],
            )?;
            Ok(())
        })
    }

    pub fn mark_peer_handled_propagation(
        &self,
        peer: &str,
        transient_id: &str,
    ) -> rusqlite::Result<()> {
        self.with_write_conn(|conn| {
            let peer = normalize_peer_key(peer);
            let transient_id = normalize_hex_key(transient_id);
            conn.execute(
                "INSERT INTO propagation_peer_entries (peer, transient_id, state, updated_at)
                 VALUES (?1, ?2, 'handled', ?3)
                 ON CONFLICT(peer, transient_id) DO UPDATE SET
                    state = 'handled',
                    updated_at = excluded.updated_at
                 WHERE propagation_peer_entries.state NOT IN ('transferred', 'received', 'transfer_limited')",
                params![peer, transient_id, now_unix_secs()],
            )?;
            remove_case_variant_unhandled_peer_mark(conn, peer.as_str(), transient_id.as_str())?;
            Ok(())
        })
    }

    pub fn mark_peer_transferred_propagation(
        &self,
        peer: &str,
        transient_id: &str,
    ) -> rusqlite::Result<()> {
        self.with_write_conn(|conn| {
            let peer = normalize_peer_key(peer);
            let transient_id = normalize_hex_key(transient_id);
            conn.execute(
                "INSERT INTO propagation_peer_entries (peer, transient_id, state, updated_at)
             VALUES (?1, ?2, 'transferred', ?3)
                 ON CONFLICT(peer, transient_id) DO UPDATE SET
                    state = 'transferred',
                    updated_at = excluded.updated_at
                 WHERE propagation_peer_entries.state NOT IN ('received', 'transfer_limited')",
                params![peer, transient_id, now_unix_secs()],
            )?;
            remove_case_variant_unhandled_peer_mark(conn, peer.as_str(), transient_id.as_str())?;
            Ok(())
        })
    }

    pub fn mark_peer_received_propagation(
        &self,
        peer: &str,
        transient_id: &str,
    ) -> rusqlite::Result<()> {
        self.with_write_conn(|conn| {
            let peer = normalize_peer_key(peer);
            let transient_id = normalize_hex_key(transient_id);
            conn.execute(
                "INSERT INTO propagation_peer_entries (peer, transient_id, state, updated_at)
                 VALUES (?1, ?2, 'received', ?3)
                 ON CONFLICT(peer, transient_id) DO UPDATE SET
                    state = 'received',
                    updated_at = excluded.updated_at
                 WHERE propagation_peer_entries.state NOT IN ('transferred', 'transfer_limited')",
                params![peer, transient_id, now_unix_secs()],
            )?;
            remove_case_variant_unhandled_peer_mark(conn, peer.as_str(), transient_id.as_str())?;
            Ok(())
        })
    }

    pub fn mark_peer_transfer_limited_propagation(
        &self,
        peer: &str,
        transient_id: &str,
    ) -> rusqlite::Result<()> {
        self.with_write_conn(|conn| {
            let peer = normalize_peer_key(peer);
            let transient_id = normalize_hex_key(transient_id);
            conn.execute(
                "INSERT INTO propagation_peer_entries (peer, transient_id, state, updated_at)
                 VALUES (?1, ?2, 'transfer_limited', ?3)
                 ON CONFLICT(peer, transient_id) DO UPDATE SET
                    state = 'transfer_limited',
                    updated_at = excluded.updated_at
                 WHERE propagation_peer_entries.state IN ('unhandled', 'transfer_limited')",
                params![peer, transient_id, now_unix_secs()],
            )?;
            remove_case_variant_unhandled_peer_mark(conn, peer.as_str(), transient_id.as_str())?;
            Ok(())
        })
    }

    pub fn remove_peer_unhandled_propagation(
        &self,
        peer: &str,
        transient_id: &str,
    ) -> rusqlite::Result<bool> {
        self.with_write_conn(|conn| {
            let affected = conn.execute(
                "DELETE FROM propagation_peer_entries
                 WHERE LOWER(peer) = LOWER(?1) AND transient_id = ?2 AND state = 'unhandled'",
                params![peer, normalize_hex_key(transient_id)],
            )?;
            Ok(affected > 0)
        })
    }

    pub fn remove_stale_peer_unhandled_propagation(&self, peer: &str) -> rusqlite::Result<usize> {
        self.remove_stale_peer_unhandled_propagation_ids(peer).map(|ids| ids.len())
    }
}

fn message_record_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<MessageRecord> {
    let fields_json: Option<String> = row.get(7)?;
    let fields = fields_json.as_ref().and_then(|value| serde_json::from_str(value).ok());
    let receipt_status: Option<String> = row.get(8)?;
    Ok(MessageRecord {
        id: row.get(0)?,
        source: row.get(1)?,
        destination: row.get(2)?,
        title: row.get(3)?,
        content: row.get(4)?,
        timestamp: row.get(5)?,
        direction: row.get(6)?,
        fields,
        receipt_status,
    })
}
