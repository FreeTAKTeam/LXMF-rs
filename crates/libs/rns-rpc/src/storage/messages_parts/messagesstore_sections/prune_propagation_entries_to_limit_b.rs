impl MessagesStore {

    pub fn prune_propagation_entries_to_limit_bytes(
        &self,
        limit_bytes: u64,
    ) -> rusqlite::Result<Vec<String>> {
        self.prune_propagation_entries_to_limit_bytes_with_priorities(limit_bytes, &[])
    }

    pub fn prune_propagation_entries_to_limit_bytes_with_priorities(
        &self,
        limit_bytes: u64,
        prioritised_destinations: &[String],
    ) -> rusqlite::Result<Vec<String>> {
        self.with_write_conn(|conn| {
            let tx = conn.unchecked_transaction()?;
            let total: i64 = tx.query_row(
                "SELECT COALESCE(SUM(size_bytes), 0) FROM propagation_entries",
                [],
                |row| row.get(0),
            )?;
            let mut total = total.max(0) as u64;
            if total <= limit_bytes {
                tx.commit()?;
                return Ok(Vec::new());
            }

            let entries = {
                let mut stmt = tx.prepare(
                    "SELECT transient_id, destination, size_bytes, received_at
                     FROM propagation_entries
                     ORDER BY received_at ASC, transient_id ASC",
                )?;
                let rows = stmt.query_map([], |row| {
                    let transient_id: String = row.get(0)?;
                    let destination: String = row.get(1)?;
                    let size_bytes: i64 = row.get(2)?;
                    let received_at: i64 = row.get(3)?;
                    Ok((transient_id, destination, size_bytes.max(0) as u64, received_at))
                })?;
                rows.collect::<rusqlite::Result<Vec<_>>>()?
            };
            let newest_received_at =
                entries.iter().map(|(_id, _destination, _size, received_at)| *received_at).max();
            let mut entries = entries;
            entries.sort_by(|left, right| {
                let left_weight = propagation_prune_weight(
                    left.1.as_str(),
                    left.2,
                    left.3,
                    newest_received_at,
                    prioritised_destinations,
                );
                let right_weight = propagation_prune_weight(
                    right.1.as_str(),
                    right.2,
                    right.3,
                    newest_received_at,
                    prioritised_destinations,
                );
                right_weight
                    .partial_cmp(&left_weight)
                    .unwrap_or(std::cmp::Ordering::Equal)
                    .then_with(|| left.3.cmp(&right.3))
                    .then_with(|| left.0.cmp(&right.0))
            });

            let mut pruned = Vec::new();
            for (transient_id, _destination, size_bytes, _received_at) in entries {
                if total <= limit_bytes {
                    break;
                }
                let affected = tx.execute(
                    "DELETE FROM propagation_entries WHERE transient_id = ?1",
                    params![transient_id],
                )?;
                if affected > 0 {
                    tx.execute(
                        "DELETE FROM propagation_peer_entries
                         WHERE transient_id = ?1
                           AND state = 'unhandled'",
                        params![transient_id],
                    )?;
                    total = total.saturating_sub(size_bytes);
                    pruned.push(transient_id);
                }
            }
            tx.commit()?;
            Ok(pruned)
        })
    }

    pub fn count_message_buckets(&self) -> rusqlite::Result<(u64, u64)> {
        let (queued, in_flight): (i64, i64) = self.with_read_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT
                    COALESCE(SUM(CASE
                        WHEN receipt_status IS NULL OR TRIM(receipt_status) = '' THEN 1
                        ELSE 0
                    END), 0) AS queued_count,
                    COALESCE(SUM(CASE
                        WHEN receipt_status IS NOT NULL
                            AND TRIM(receipt_status) <> ''
                            AND LOWER(receipt_status) NOT LIKE 'sent%'
                            AND LOWER(receipt_status) NOT LIKE 'failed%'
                            AND LOWER(receipt_status) NOT IN ('cancelled', 'delivered', 'failed', 'expired', 'rejected')
                        THEN 1
                        ELSE 0
                    END), 0) AS in_flight_count
                 FROM messages",
            )?;
            stmt.query_row([], |row| Ok((row.get(0)?, row.get(1)?)))
        })?;
        Ok((queued.max(0) as u64, in_flight.max(0) as u64))
    }

    pub fn count_outbound_messages(&self) -> rusqlite::Result<u64> {
        let count: i64 = self.with_read_conn(|conn| {
            conn.query_row("SELECT COUNT(*) FROM messages WHERE direction = 'out'", [], |row| {
                row.get(0)
            })
        })?;
        Ok(count.max(0) as u64)
    }

    pub fn expire_outbound_messages_before(&self, cutoff_ts: i64) -> rusqlite::Result<Vec<String>> {
        self.with_write_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT id
                 FROM messages
                 WHERE direction = 'out'
                   AND timestamp < ?1
                   AND (
                        receipt_status IS NULL
                        OR TRIM(receipt_status) = ''
                        OR (
                            LOWER(receipt_status) NOT LIKE 'sent%'
                            AND LOWER(receipt_status) NOT LIKE 'failed%'
                            AND LOWER(receipt_status) NOT IN ('cancelled', 'delivered', 'failed', 'expired', 'rejected')
                        )
                   )
                 ORDER BY timestamp ASC, id ASC",
            )?;
            let mut rows = stmt.query(params![cutoff_ts])?;
            let mut ids = Vec::new();
            while let Some(row) = rows.next()? {
                ids.push(row.get::<_, String>(0)?);
            }
            drop(rows);
            drop(stmt);
            for message_id in ids.iter() {
                conn.execute(
                    "UPDATE messages SET receipt_status = 'expired' WHERE id = ?1",
                    params![message_id],
                )?;
            }
            Ok(ids)
        })
    }

    pub fn prune_outbound_messages(
        &self,
        count: usize,
        eviction_priority: &str,
    ) -> rusqlite::Result<Vec<String>> {
        if count == 0 {
            return Ok(Vec::new());
        }
        self.with_write_conn(|conn| {
            let collect_ids = |query: &str, remaining: usize| -> rusqlite::Result<Vec<String>> {
                if remaining == 0 {
                    return Ok(Vec::new());
                }
                let mut stmt = conn.prepare(query)?;
                let mut rows = stmt.query(params![remaining as i64])?;
                let mut ids = Vec::new();
                while let Some(row) = rows.next()? {
                    ids.push(row.get::<_, String>(0)?);
                }
                Ok(ids)
            };

            let normalized = eviction_priority.trim().to_ascii_lowercase();
            let mut ids = if normalized == "terminal_first" {
                let mut selected = collect_ids(
                    "SELECT id
                     FROM messages
                     WHERE direction = 'out'
                       AND receipt_status IS NOT NULL
                       AND TRIM(receipt_status) <> ''
                       AND (
                            LOWER(receipt_status) LIKE 'sent%'
                            OR LOWER(receipt_status) LIKE 'failed%'
                            OR LOWER(receipt_status) IN ('cancelled', 'delivered', 'failed', 'expired', 'rejected')
                       )
                     ORDER BY timestamp ASC, id ASC
                     LIMIT ?1",
                    count,
                )?;
                let remaining = count.saturating_sub(selected.len());
                if remaining > 0 {
                    let mut non_terminal = collect_ids(
                        "SELECT id
                         FROM messages
                         WHERE direction = 'out'
                           AND (
                                receipt_status IS NULL
                                OR TRIM(receipt_status) = ''
                                OR (
                                    LOWER(receipt_status) NOT LIKE 'sent%'
                                    AND LOWER(receipt_status) NOT LIKE 'failed%'
                                    AND LOWER(receipt_status) NOT IN ('cancelled', 'delivered', 'failed', 'expired', 'rejected')
                                )
                           )
                         ORDER BY timestamp ASC, id ASC
                         LIMIT ?1",
                        remaining,
                    )?;
                    selected.append(&mut non_terminal);
                }
                selected
            } else {
                collect_ids(
                    "SELECT id
                     FROM messages
                     WHERE direction = 'out'
                     ORDER BY timestamp ASC, id ASC
                     LIMIT ?1",
                    count,
                )?
            };

            ids.sort();
            ids.dedup();
            for message_id in ids.iter() {
                conn.execute("DELETE FROM messages WHERE id = ?1", params![message_id])?;
            }
            if !ids.is_empty() {
                self.write_state
                    .message_count_cache
                    .fetch_sub(ids.len().min(u64::MAX as usize) as u64, Ordering::Relaxed);
            }
            Ok(ids)
        })
    }

    fn prune_messages_to_limit_bytes_direct(
        write_state: &WriteState,
        limit_bytes: u64,
    ) -> rusqlite::Result<Vec<String>> {
        Self::write_lock_and_run(write_state, |conn| {
            let current_bytes: i64 = conn.query_row(
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
            if current_bytes.max(0) as u64 <= limit_bytes {
                return Ok(Vec::new());
            }

            let mut stmt = conn.prepare(
                "SELECT id,
                        LENGTH(id) +
                        LENGTH(source) +
                        LENGTH(destination) +
                        LENGTH(title) +
                        LENGTH(content) +
                        LENGTH(direction) +
                        COALESCE(LENGTH(fields), 0) +
                        COALESCE(LENGTH(receipt_status), 0) AS approx_bytes
                 FROM messages
                 ORDER BY timestamp ASC, id ASC",
            )?;
            let mut rows = stmt.query([])?;
            let mut bytes = current_bytes.max(0) as u64;
            let mut ids = Vec::new();
            while let Some(row) = rows.next()? {
                if bytes <= limit_bytes {
                    break;
                }
                let id: String = row.get(0)?;
                let approx_bytes: i64 = row.get(1)?;
                ids.push(id);
                bytes = bytes.saturating_sub(approx_bytes.max(0) as u64);
            }
            drop(rows);
            drop(stmt);

            for message_id in ids.iter() {
                conn.execute("DELETE FROM messages WHERE id = ?1", params![message_id])?;
            }
            if !ids.is_empty() {
                write_state
                    .message_count_cache
                    .fetch_sub(ids.len().min(u64::MAX as usize) as u64, Ordering::Relaxed);
            }
            Ok(ids)
        })
    }

    pub fn prune_messages_to_limit_bytes(&self, limit_bytes: u64) -> rusqlite::Result<Vec<String>> {
        let (reply_tx, reply_rx) = mpsc::channel();
        self.outbound_write_tx
            .send(OutboundWriteCommand::PruneMessagesToLimitBytes {
                limit_bytes,
                reply: Some(reply_tx),
            })
            .map_err(|err| rusqlite::Error::ToSqlConversionFailure(Box::new(err)))?;
        reply_rx.recv().map_err(|err| rusqlite::Error::ToSqlConversionFailure(Box::new(err)))?
    }

    pub fn schedule_prune_messages_to_limit_bytes(&self, limit_bytes: u64) -> rusqlite::Result<()> {
        self.outbound_write_tx
            .send(OutboundWriteCommand::PruneMessagesToLimitBytes { limit_bytes, reply: None })
            .map_err(|err| rusqlite::Error::ToSqlConversionFailure(Box::new(err)))
    }

    pub fn update_receipt_status(&self, message_id: &str, status: &str) -> rusqlite::Result<()> {
        let (reply_tx, reply_rx) = mpsc::channel();
        self.outbound_write_tx
            .send(OutboundWriteCommand::UpdateReceiptStatus {
                message_id: message_id.to_string(),
                status: status.to_string(),
                reply: reply_tx,
            })
            .map_err(|err| rusqlite::Error::ToSqlConversionFailure(Box::new(err)))?;
        reply_rx.recv().map_err(|err| rusqlite::Error::ToSqlConversionFailure(Box::new(err)))?
    }

    pub fn update_message_fields(
        &self,
        message_id: &str,
        fields: Option<&JsonValue>,
    ) -> rusqlite::Result<()> {
        let fields_json = fields
            .map(serde_json::to_string)
            .transpose()
            .map_err(|err| rusqlite::Error::ToSqlConversionFailure(Box::new(err)))?;
        let (reply_tx, reply_rx) = mpsc::channel();
        self.outbound_write_tx
            .send(OutboundWriteCommand::UpdateMessageFields {
                message_id: message_id.to_string(),
                fields_json,
                reply: reply_tx,
            })
            .map_err(|err| rusqlite::Error::ToSqlConversionFailure(Box::new(err)))?;
        reply_rx.recv().map_err(|err| rusqlite::Error::ToSqlConversionFailure(Box::new(err)))?
    }

    pub fn resolve_receipt_status(
        &self,
        message_id: &str,
        candidate_status: &str,
    ) -> rusqlite::Result<Option<String>> {
        let (reply_tx, reply_rx) = mpsc::channel();
        self.outbound_write_tx
            .send(OutboundWriteCommand::ResolveReceiptStatus {
                message_id: message_id.to_string(),
                candidate_status: candidate_status.to_string(),
                reply: reply_tx,
            })
            .map_err(|err| rusqlite::Error::ToSqlConversionFailure(Box::new(err)))?;
        reply_rx.recv().map_err(|err| rusqlite::Error::ToSqlConversionFailure(Box::new(err)))?
    }

    pub fn clear_messages(&self) -> rusqlite::Result<()> {
        self.with_write_conn(|conn| {
            conn.execute("DELETE FROM messages", [])?;
            self.write_state.message_count_cache.store(0, Ordering::Relaxed);
            Ok(())
        })
    }

    pub fn insert_announce(&self, record: &AnnounceRecord) -> rusqlite::Result<()> {
        let (reply_tx, reply_rx) = mpsc::channel();
        self.outbound_write_tx
            .send(OutboundWriteCommand::InsertAnnounce { record: record.clone(), reply: reply_tx })
            .map_err(|err| rusqlite::Error::ToSqlConversionFailure(Box::new(err)))?;
        reply_rx.recv().map_err(|err| rusqlite::Error::ToSqlConversionFailure(Box::new(err)))?
    }
}
