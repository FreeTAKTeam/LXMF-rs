impl MessagesStore {

    pub fn list_announces(
        &self,
        limit: usize,
        before_ts: Option<i64>,
        before_id: Option<&str>,
    ) -> rusqlite::Result<Vec<AnnounceRecord>> {
        self.with_read_conn(|conn| {
            let mut records = Vec::new();
            let parse_row = |row: &rusqlite::Row| -> rusqlite::Result<AnnounceRecord> {
                let capabilities_json: Option<String> = row.get(8)?;
                let capabilities = capabilities_json
                    .as_deref()
                    .map(|value| deserialize_json_column(value, 8))
                    .transpose()?
                    .unwrap_or_default();
                let seen_count: i64 = row.get(6)?;
                Ok(AnnounceRecord {
                    id: row.get(0)?,
                    peer: row.get(1)?,
                    timestamp: row.get(2)?,
                    name: row.get(3)?,
                    name_source: row.get(4)?,
                    first_seen: row.get(5)?,
                    seen_count: seen_count.max(0) as u64,
                    app_data_hex: row.get(7)?,
                    capabilities,
                    rssi: row.get(9)?,
                    snr: row.get(10)?,
                    q: row.get(11)?,
                    stamp_cost: row.get(12)?,
                    stamp_cost_flexibility: row.get(13)?,
                    peering_cost: row.get(14)?,
                })
            };
            if let Some(ts) = before_ts {
                let query_with_id = "SELECT id, peer, timestamp, name, name_source, first_seen, seen_count, app_data_hex, capabilities, rssi, snr, q, stamp_cost, stamp_cost_flexibility, peering_cost FROM announces WHERE (timestamp < ?1 OR (timestamp = ?1 AND id < ?2)) ORDER BY timestamp DESC, id DESC LIMIT ?3";
                let query_without_id = "SELECT id, peer, timestamp, name, name_source, first_seen, seen_count, app_data_hex, capabilities, rssi, snr, q, stamp_cost, stamp_cost_flexibility, peering_cost FROM announces WHERE timestamp < ?1 ORDER BY timestamp DESC, id DESC LIMIT ?2";
                if let Some(ann_id) = before_id {
                    let mut stmt = conn.prepare(query_with_id)?;
                    let mut rows = stmt.query(params![ts, ann_id, limit as i64])?;
                    while let Some(row) = rows.next()? {
                        records.push(parse_row(row)?);
                    }
                } else {
                    let mut stmt = conn.prepare(query_without_id)?;
                    let mut rows = stmt.query(params![ts, limit as i64])?;
                    while let Some(row) = rows.next()? {
                        records.push(parse_row(row)?);
                    }
                }
            } else {
                let mut stmt = conn.prepare(
                    "SELECT id, peer, timestamp, name, name_source, first_seen, seen_count, app_data_hex, capabilities, rssi, snr, q, stamp_cost, stamp_cost_flexibility, peering_cost FROM announces ORDER BY timestamp DESC LIMIT ?1",
                )?;
                let mut rows = stmt.query(params![limit as i64])?;
                while let Some(row) = rows.next()? {
                    records.push(parse_row(row)?);
                }
            }
            Ok(records)
        })
    }

    pub fn upsert_announce_identity(
        &self,
        peer: &str,
        public_key_hex: &str,
        verifying_key_hex: &str,
        updated_at: i64,
    ) -> rusqlite::Result<()> {
        let (reply_tx, reply_rx) = mpsc::channel();
        self.outbound_write_tx
            .send(OutboundWriteCommand::UpsertAnnounceIdentity {
                peer: normalize_peer_key(peer),
                public_key_hex: normalize_hex_key(public_key_hex),
                verifying_key_hex: normalize_hex_key(verifying_key_hex),
                updated_at,
                reply: reply_tx,
            })
            .map_err(|err| rusqlite::Error::ToSqlConversionFailure(Box::new(err)))?;
        reply_rx.recv().map_err(|err| rusqlite::Error::ToSqlConversionFailure(Box::new(err)))?
    }

    pub fn announce_identity_keys(
        &self,
        peer: &str,
    ) -> rusqlite::Result<Option<(String, String)>> {
        let peer = normalize_peer_key(peer);
        self.with_read_conn(|conn| {
            conn.query_row(
                "SELECT public_key_hex, verifying_key_hex
                 FROM announce_identities
                 WHERE peer = ?1
                 LIMIT 1",
                params![peer],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
        })
    }

    pub fn latest_announce_stamp_cost_for(&self, peer: &str) -> rusqlite::Result<Option<u32>> {
        self.with_read_conn(|conn| {
            conn.query_row(
                "SELECT stamp_cost FROM announces WHERE peer = ?1 AND stamp_cost IS NOT NULL ORDER BY timestamp DESC, id DESC LIMIT 1",
                params![peer],
                |row| row.get(0),
            )
            .optional()
        })
    }

    pub fn get_ticket(&self, destination: &str) -> rusqlite::Result<Option<(String, i64)>> {
        self.with_read_conn(|conn| {
            conn.query_row(
                "SELECT ticket, expires_at FROM tickets WHERE destination = ?1 ORDER BY expires_at DESC, ticket DESC LIMIT 1",
                params![destination],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
        })
    }

    pub fn get_tickets_for_destination(
        &self,
        destination: &str,
    ) -> rusqlite::Result<Vec<(String, i64)>> {
        self.with_read_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT ticket, expires_at FROM tickets WHERE destination = ?1 ORDER BY expires_at DESC, ticket DESC",
            )?;
            let rows = stmt.query_map(params![destination], |row| Ok((row.get(0)?, row.get(1)?)))?;
            rows.collect()
        })
    }

    pub fn upsert_ticket(
        &self,
        destination: &str,
        ticket: &str,
        expires_at: i64,
    ) -> rusqlite::Result<()> {
        let (reply_tx, reply_rx) = mpsc::channel();
        self.outbound_write_tx
            .send(OutboundWriteCommand::UpsertTicket {
                destination: destination.to_string(),
                ticket: ticket.to_string(),
                expires_at,
                reply: reply_tx,
            })
            .map_err(|err| rusqlite::Error::ToSqlConversionFailure(Box::new(err)))?;
        reply_rx.recv().map_err(|err| rusqlite::Error::ToSqlConversionFailure(Box::new(err)))?
    }

    pub fn prune_expired_tickets(&self, now: i64, inbound_grace_secs: i64) -> rusqlite::Result<()> {
        let (reply_tx, reply_rx) = mpsc::channel();
        self.outbound_write_tx
            .send(OutboundWriteCommand::PruneExpiredTickets {
                now,
                inbound_grace_secs,
                reply: reply_tx,
            })
            .map_err(|err| rusqlite::Error::ToSqlConversionFailure(Box::new(err)))?;
        reply_rx.recv().map_err(|err| rusqlite::Error::ToSqlConversionFailure(Box::new(err)))?
    }

    pub fn get_outbound_ticket(
        &self,
        destination: &str,
    ) -> rusqlite::Result<Option<(String, i64)>> {
        self.with_read_conn(|conn| {
            conn.query_row(
                "SELECT ticket, expires_at FROM outbound_tickets WHERE destination = ?1",
                params![destination],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
        })
    }

    pub fn upsert_outbound_ticket(
        &self,
        destination: &str,
        ticket: &str,
        expires_at: i64,
    ) -> rusqlite::Result<()> {
        let (reply_tx, reply_rx) = mpsc::channel();
        self.outbound_write_tx
            .send(OutboundWriteCommand::UpsertOutboundTicket {
                destination: destination.to_string(),
                ticket: ticket.to_string(),
                expires_at,
                reply: reply_tx,
            })
            .map_err(|err| rusqlite::Error::ToSqlConversionFailure(Box::new(err)))?;
        reply_rx.recv().map_err(|err| rusqlite::Error::ToSqlConversionFailure(Box::new(err)))?
    }

    pub fn get_ticket_last_delivery(&self, destination: &str) -> rusqlite::Result<Option<i64>> {
        self.with_read_conn(|conn| {
            conn.query_row(
                "SELECT delivered_at FROM ticket_deliveries WHERE destination = ?1",
                params![destination],
                |row| row.get(0),
            )
            .optional()
        })
    }

    pub fn upsert_ticket_last_delivery(
        &self,
        destination: &str,
        delivered_at: i64,
    ) -> rusqlite::Result<()> {
        let (reply_tx, reply_rx) = mpsc::channel();
        self.outbound_write_tx
            .send(OutboundWriteCommand::UpsertTicketLastDelivery {
                destination: destination.to_string(),
                delivered_at,
                reply: reply_tx,
            })
            .map_err(|err| rusqlite::Error::ToSqlConversionFailure(Box::new(err)))?;
        reply_rx.recv().map_err(|err| rusqlite::Error::ToSqlConversionFailure(Box::new(err)))?
    }

    pub fn clear_announces(&self) -> rusqlite::Result<()> {
        self.with_write_conn(|conn| {
            conn.execute("DELETE FROM announces", [])?;
            conn.execute("DELETE FROM announce_identities", [])?;
            Ok(())
        })
    }

    pub fn put_sdk_domain_snapshot(&self, snapshot: &JsonValue) -> rusqlite::Result<()> {
        let snapshot_json = serde_json::to_string(snapshot)
            .map_err(|err| rusqlite::Error::ToSqlConversionFailure(Box::new(err)))?;
        self.with_write_conn(|conn| {
            conn.execute(
                "INSERT INTO sdk_domain_state (domain, state_json) VALUES (?1, ?2)
                 ON CONFLICT(domain) DO UPDATE SET state_json = excluded.state_json",
                params![Self::SDK_DOMAIN_SNAPSHOT_KEY, snapshot_json],
            )?;
            Ok(())
        })
    }

    pub fn get_sdk_domain_snapshot(&self) -> rusqlite::Result<Option<JsonValue>> {
        let snapshot_json: Option<String> = self.with_read_conn(|conn| {
            conn.query_row(
                "SELECT state_json FROM sdk_domain_state WHERE domain = ?1 LIMIT 1",
                params![Self::SDK_DOMAIN_SNAPSHOT_KEY],
                |row| row.get(0),
            )
            .optional()
        })?;
        let Some(snapshot_json) = snapshot_json else {
            return Ok(None);
        };
        let parsed = serde_json::from_str(snapshot_json.as_str()).map_err(|err| {
            rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(err))
        })?;
        Ok(Some(parsed))
    }

    pub fn clear_sdk_domain_snapshot(&self) -> rusqlite::Result<()> {
        self.with_write_conn(|conn| {
            conn.execute(
                "DELETE FROM sdk_domain_state WHERE domain = ?1",
                params![Self::SDK_DOMAIN_SNAPSHOT_KEY],
            )?;
            Ok(())
        })
    }

    fn configure_connection(&self) -> rusqlite::Result<()> {
        self.with_write_conn(|conn| {
            conn.pragma_update(None, "journal_mode", "WAL")?;
            conn.pragma_update(None, "synchronous", "NORMAL")?;
            conn.pragma_update(None, "busy_timeout", 5_000i64)?;
            Ok(())
        })?;
        if self.read_conn.is_some() {
            self.with_read_conn(|conn| {
                conn.pragma_update(None, "busy_timeout", 5_000i64)?;
                Ok(())
            })?;
        }
        Ok(())
    }

    #[cfg(test)]
    fn busy_timeout_ms(&self) -> rusqlite::Result<i64> {
        self.with_write_conn(|conn| conn.query_row("PRAGMA busy_timeout", [], |row| row.get(0)))
    }

    fn init_schema(&self) -> rusqlite::Result<()> {
        self.with_write_conn(|conn| {
            let tx = conn.unchecked_transaction()?;
            tx.execute_batch(
                "CREATE TABLE IF NOT EXISTS messages (
                    id TEXT PRIMARY KEY,
                    source TEXT NOT NULL,
                    destination TEXT NOT NULL,
                    title TEXT NOT NULL,
                    content TEXT NOT NULL,
                    timestamp INTEGER NOT NULL,
                    direction TEXT NOT NULL,
                    fields TEXT,
                    receipt_status TEXT
                );
                CREATE TABLE IF NOT EXISTS announces (
                    id TEXT PRIMARY KEY,
                    peer TEXT NOT NULL,
                    timestamp INTEGER NOT NULL,
                    name TEXT,
                    name_source TEXT,
                    first_seen INTEGER NOT NULL,
                    seen_count INTEGER NOT NULL,
                    app_data_hex TEXT,
                    capabilities TEXT,
                    rssi REAL,
                    snr REAL,
                    q REAL,
                    stamp_cost INTEGER,
                    stamp_cost_flexibility INTEGER,
                    peering_cost INTEGER
                );
                CREATE TABLE IF NOT EXISTS announce_identities (
                    peer TEXT PRIMARY KEY,
                    public_key_hex TEXT NOT NULL,
                    verifying_key_hex TEXT NOT NULL,
                    updated_at INTEGER NOT NULL
                );
                CREATE TABLE IF NOT EXISTS sdk_domain_state (
                    domain TEXT PRIMARY KEY,
                    state_json TEXT NOT NULL
                );
                CREATE TABLE IF NOT EXISTS tickets (
                    destination TEXT NOT NULL,
                    ticket TEXT NOT NULL,
                    expires_at INTEGER NOT NULL,
                    PRIMARY KEY(destination, ticket)
                );
                CREATE TABLE IF NOT EXISTS outbound_tickets (
                    destination TEXT PRIMARY KEY,
                    ticket TEXT NOT NULL,
                    expires_at INTEGER NOT NULL
                );
                CREATE TABLE IF NOT EXISTS ticket_deliveries (
                    destination TEXT PRIMARY KEY,
                    delivered_at INTEGER NOT NULL
                );
                CREATE TABLE IF NOT EXISTS propagation_entries (
                    transient_id TEXT PRIMARY KEY,
                    destination TEXT NOT NULL,
                    payload_hex TEXT NOT NULL,
                    received_at INTEGER NOT NULL,
                    size_bytes INTEGER NOT NULL,
                    stamp_value INTEGER
                );
                CREATE TABLE IF NOT EXISTS propagation_peer_entries (
                    peer TEXT NOT NULL,
                    transient_id TEXT NOT NULL,
                    state TEXT NOT NULL,
                    updated_at INTEGER NOT NULL,
                    PRIMARY KEY(peer, transient_id)
                );
                CREATE TABLE IF NOT EXISTS propagation_local_entries (
                    transient_id TEXT PRIMARY KEY,
                    processed_at INTEGER NOT NULL
                );",
            )?;
            Self::ensure_multi_ticket_schema(&tx)?;
            tx.execute(
                "CREATE INDEX IF NOT EXISTS idx_tickets_destination_expires
                    ON tickets(destination, expires_at DESC)",
                [],
            )?;
            Self::ensure_schema_column(&tx, "messages", "title", "title TEXT")?;
            tx.execute("UPDATE messages SET title = '' WHERE title IS NULL", [])?;
            Self::ensure_schema_column(&tx, "messages", "fields", "fields TEXT")?;
            Self::ensure_schema_column(
                &tx,
                "messages",
                "receipt_status",
                "receipt_status TEXT",
            )?;
            Self::ensure_schema_column(&tx, "announces", "name", "name TEXT")?;
            Self::ensure_schema_column(&tx, "announces", "name_source", "name_source TEXT")?;
            Self::ensure_schema_column(&tx, "announces", "first_seen", "first_seen INTEGER")?;
            Self::ensure_schema_column(&tx, "announces", "seen_count", "seen_count INTEGER")?;
            tx.execute(
                "UPDATE announces SET first_seen = timestamp WHERE first_seen IS NULL",
                [],
            )?;
            tx.execute("UPDATE announces SET seen_count = 1 WHERE seen_count IS NULL", [])?;
            Self::ensure_schema_column(&tx, "announces", "app_data_hex", "app_data_hex TEXT")?;
            Self::ensure_schema_column(&tx, "announces", "capabilities", "capabilities TEXT")?;
            Self::ensure_schema_column(&tx, "announces", "rssi", "rssi REAL")?;
            Self::ensure_schema_column(&tx, "announces", "snr", "snr REAL")?;
            Self::ensure_schema_column(&tx, "announces", "q", "q REAL")?;
            Self::ensure_schema_column(&tx, "announces", "stamp_cost", "stamp_cost INTEGER")?;
            Self::ensure_schema_column(
                &tx,
                "announces",
                "stamp_cost_flexibility",
                "stamp_cost_flexibility INTEGER",
            )?;
            Self::ensure_schema_column(
                &tx,
                "announces",
                "peering_cost",
                "peering_cost INTEGER",
            )?;
            tx.execute_batch(
                "CREATE INDEX IF NOT EXISTS idx_messages_timestamp_desc
                    ON messages(timestamp DESC);
                CREATE INDEX IF NOT EXISTS idx_messages_direction_timestamp_desc
                    ON messages(direction, timestamp DESC);
                CREATE INDEX IF NOT EXISTS idx_messages_receipt_status
                    ON messages(receipt_status);
                CREATE INDEX IF NOT EXISTS idx_announces_timestamp_id_desc
                    ON announces(timestamp DESC, id DESC);
                CREATE INDEX IF NOT EXISTS idx_propagation_entries_destination_size
                    ON propagation_entries(destination, size_bytes, transient_id);
                CREATE INDEX IF NOT EXISTS idx_propagation_peer_entries_state
                    ON propagation_peer_entries(peer, state, transient_id);",
            )?;
            tx.commit()
        })
    }

    fn ensure_multi_ticket_schema(conn: &Connection) -> rusqlite::Result<()> {
        let mut stmt = conn.prepare("PRAGMA table_info(tickets)")?;
        let mut rows = stmt.query([])?;
        let mut primary_key_columns = Vec::new();
        while let Some(row) = rows.next()? {
            let name: String = row.get(1)?;
            let pk_order: i64 = row.get(5)?;
            if pk_order > 0 {
                primary_key_columns.push((pk_order, name));
            }
        }
        primary_key_columns.sort_by_key(|(pk_order, _)| *pk_order);
        let primary_key_columns: Vec<String> =
            primary_key_columns.into_iter().map(|(_, name)| name).collect();

        if primary_key_columns != ["destination"] {
            return Ok(());
        }

        conn.execute_batch(
            "ALTER TABLE tickets RENAME TO tickets_single_destination;
             CREATE TABLE tickets (
                destination TEXT NOT NULL,
                ticket TEXT NOT NULL,
                expires_at INTEGER NOT NULL,
                PRIMARY KEY(destination, ticket)
             );
             INSERT OR IGNORE INTO tickets (destination, ticket, expires_at)
                SELECT destination, ticket, expires_at FROM tickets_single_destination;
             DROP TABLE tickets_single_destination;",
        )
    }
}
