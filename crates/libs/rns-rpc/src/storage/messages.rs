use rusqlite::{params, Connection, OptionalExtension};
use serde_json::Value as JsonValue;

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct MessageRecord {
    pub id: String,
    pub source: String,
    pub destination: String,
    pub title: String,
    pub content: String,
    pub timestamp: i64,
    pub direction: String,
    pub fields: Option<JsonValue>,
    pub receipt_status: Option<String>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct AnnounceRecord {
    pub id: String,
    pub peer: String,
    pub timestamp: i64,
    pub name: Option<String>,
    pub name_source: Option<String>,
    pub first_seen: i64,
    pub seen_count: u64,
    pub app_data_hex: Option<String>,
    #[serde(default)]
    pub capabilities: Vec<String>,
    pub rssi: Option<f64>,
    pub snr: Option<f64>,
    pub q: Option<f64>,
    pub stamp_cost_flexibility: Option<u32>,
    pub peering_cost: Option<u32>,
}

pub struct MessagesStore {
    conn: Connection,
}

impl MessagesStore {
    const SDK_DOMAIN_SNAPSHOT_KEY: &'static str = "sdk_domains.v1";

    pub fn in_memory() -> rusqlite::Result<Self> {
        let conn = Connection::open_in_memory()?;
        let store = Self { conn };
        store.init_schema()?;
        Ok(store)
    }

    pub fn open(path: &std::path::Path) -> rusqlite::Result<Self> {
        let conn = Connection::open(path)?;
        let store = Self { conn };
        store.init_schema()?;
        Ok(store)
    }

    pub fn insert_message(&self, record: &MessageRecord) -> rusqlite::Result<()> {
        let fields_json =
            record.fields.as_ref().map(|value| serde_json::to_string(value).unwrap_or_default());
        self.conn.execute(
            "INSERT OR REPLACE INTO messages (id, source, destination, title, content, timestamp, direction, fields, receipt_status) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                &record.id,
                &record.source,
                &record.destination,
                &record.title,
                &record.content,
                record.timestamp,
                &record.direction,
                fields_json,
                &record.receipt_status,
            ],
        )?;
        Ok(())
    }

    pub fn list_messages(
        &self,
        limit: usize,
        before_ts: Option<i64>,
    ) -> rusqlite::Result<Vec<MessageRecord>> {
        let mut records = Vec::new();
        if let Some(ts) = before_ts {
            let mut stmt = self.conn.prepare(
                "SELECT id, source, destination, title, content, timestamp, direction, fields, receipt_status FROM messages WHERE timestamp < ?1 ORDER BY timestamp DESC LIMIT ?2",
            )?;
            let mut rows = stmt.query(params![ts, limit as i64])?;
            while let Some(row) = rows.next()? {
                let fields_json: Option<String> = row.get(7)?;
                let fields =
                    fields_json.as_ref().and_then(|value| serde_json::from_str(value).ok());
                let receipt_status: Option<String> = row.get(8)?;
                records.push(MessageRecord {
                    id: row.get(0)?,
                    source: row.get(1)?,
                    destination: row.get(2)?,
                    title: row.get(3)?,
                    content: row.get(4)?,
                    timestamp: row.get(5)?,
                    direction: row.get(6)?,
                    fields,
                    receipt_status,
                });
            }
        } else {
            let mut stmt = self.conn.prepare(
                "SELECT id, source, destination, title, content, timestamp, direction, fields, receipt_status FROM messages ORDER BY timestamp DESC LIMIT ?1",
            )?;
            let mut rows = stmt.query(params![limit as i64])?;
            while let Some(row) = rows.next()? {
                let fields_json: Option<String> = row.get(7)?;
                let fields =
                    fields_json.as_ref().and_then(|value| serde_json::from_str(value).ok());
                let receipt_status: Option<String> = row.get(8)?;
                records.push(MessageRecord {
                    id: row.get(0)?,
                    source: row.get(1)?,
                    destination: row.get(2)?,
                    title: row.get(3)?,
                    content: row.get(4)?,
                    timestamp: row.get(5)?,
                    direction: row.get(6)?,
                    fields,
                    receipt_status,
                });
            }
        }
        Ok(records)
    }

    pub fn get_message(&self, message_id: &str) -> rusqlite::Result<Option<MessageRecord>> {
        let mut stmt = self.conn.prepare(
            "SELECT id, source, destination, title, content, timestamp, direction, fields, receipt_status FROM messages WHERE id = ?1 LIMIT 1",
        )?;
        stmt.query_row(params![message_id], |row| {
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
        })
        .optional()
    }

    pub fn count_message_buckets(&self) -> rusqlite::Result<(u64, u64)> {
        let mut stmt = self.conn.prepare(
            "SELECT
                COALESCE(SUM(CASE
                    WHEN receipt_status IS NULL OR TRIM(receipt_status) = '' THEN 1
                    ELSE 0
                END), 0) AS queued_count,
                COALESCE(SUM(CASE
                    WHEN receipt_status IS NOT NULL
                        AND TRIM(receipt_status) <> ''
                        AND LOWER(receipt_status) NOT LIKE 'sent%'
                        AND LOWER(receipt_status) NOT IN ('cancelled', 'delivered', 'failed', 'expired', 'rejected')
                    THEN 1
                    ELSE 0
                END), 0) AS in_flight_count
             FROM messages",
        )?;
        let (queued, in_flight): (i64, i64) =
            stmt.query_row([], |row| Ok((row.get(0)?, row.get(1)?)))?;
        Ok((queued.max(0) as u64, in_flight.max(0) as u64))
    }

    pub fn count_outbound_messages(&self) -> rusqlite::Result<u64> {
        let count: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM messages WHERE direction = 'out'",
            [],
            |row| row.get(0),
        )?;
        Ok(count.max(0) as u64)
    }

    pub fn expire_outbound_messages_before(&self, cutoff_ts: i64) -> rusqlite::Result<Vec<String>> {
        let mut stmt = self.conn.prepare(
            "SELECT id
             FROM messages
             WHERE direction = 'out'
               AND timestamp < ?1
               AND (
                    receipt_status IS NULL
                    OR TRIM(receipt_status) = ''
                    OR (
                        LOWER(receipt_status) NOT LIKE 'sent%'
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
        for message_id in ids.iter() {
            self.conn.execute(
                "UPDATE messages SET receipt_status = 'expired' WHERE id = ?1",
                params![message_id],
            )?;
        }
        Ok(ids)
    }

    pub fn prune_outbound_messages(
        &self,
        count: usize,
        eviction_priority: &str,
    ) -> rusqlite::Result<Vec<String>> {
        if count == 0 {
            return Ok(Vec::new());
        }
        let collect_ids = |query: &str, remaining: usize| -> rusqlite::Result<Vec<String>> {
            if remaining == 0 {
                return Ok(Vec::new());
            }
            let mut stmt = self.conn.prepare(query)?;
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
            self.conn.execute("DELETE FROM messages WHERE id = ?1", params![message_id])?;
        }
        Ok(ids)
    }

    pub fn update_receipt_status(&self, message_id: &str, status: &str) -> rusqlite::Result<()> {
        self.conn.execute(
            "UPDATE messages SET receipt_status = ?1 WHERE id = ?2",
            params![status, message_id],
        )?;
        Ok(())
    }

    pub fn clear_messages(&self) -> rusqlite::Result<()> {
        self.conn.execute("DELETE FROM messages", [])?;
        Ok(())
    }

    pub fn insert_announce(&self, record: &AnnounceRecord) -> rusqlite::Result<()> {
        let capabilities_json = serde_json::to_string(&record.capabilities).unwrap_or_default();
        self.conn.execute(
            "INSERT OR REPLACE INTO announces (id, peer, timestamp, name, name_source, first_seen, seen_count, app_data_hex, capabilities, rssi, snr, q, stamp_cost_flexibility, peering_cost) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
            params![
                &record.id,
                &record.peer,
                record.timestamp,
                &record.name,
                &record.name_source,
                record.first_seen,
                record.seen_count as i64,
                &record.app_data_hex,
                capabilities_json,
                record.rssi,
                record.snr,
                record.q,
                record.stamp_cost_flexibility,
                record.peering_cost,
            ],
        )?;
        Ok(())
    }

    pub fn list_announces(
        &self,
        limit: usize,
        before_ts: Option<i64>,
        before_id: Option<&str>,
    ) -> rusqlite::Result<Vec<AnnounceRecord>> {
        let mut records = Vec::new();
        let parse_row = |row: &rusqlite::Row| -> rusqlite::Result<AnnounceRecord> {
            let capabilities_json: Option<String> = row.get(8)?;
            let capabilities = capabilities_json
                .as_deref()
                .and_then(|value| serde_json::from_str::<Vec<String>>(value).ok())
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
                stamp_cost_flexibility: row.get(12)?,
                peering_cost: row.get(13)?,
            })
        };
        if let Some(ts) = before_ts {
            let query_with_id = "SELECT id, peer, timestamp, name, name_source, first_seen, seen_count, app_data_hex, capabilities, rssi, snr, q, stamp_cost_flexibility, peering_cost FROM announces WHERE (timestamp < ?1 OR (timestamp = ?1 AND id < ?2)) ORDER BY timestamp DESC, id DESC LIMIT ?3";
            let query_without_id = "SELECT id, peer, timestamp, name, name_source, first_seen, seen_count, app_data_hex, capabilities, rssi, snr, q, stamp_cost_flexibility, peering_cost FROM announces WHERE timestamp < ?1 ORDER BY timestamp DESC, id DESC LIMIT ?2";
            if let Some(ann_id) = before_id {
                let mut stmt = self.conn.prepare(query_with_id)?;
                let mut rows = stmt.query(params![ts, ann_id, limit as i64])?;
                while let Some(row) = rows.next()? {
                    records.push(parse_row(row)?);
                }
            } else {
                let mut stmt = self.conn.prepare(query_without_id)?;
                let mut rows = stmt.query(params![ts, limit as i64])?;
                while let Some(row) = rows.next()? {
                    records.push(parse_row(row)?);
                }
            }
        } else {
            let mut stmt = self.conn.prepare(
                "SELECT id, peer, timestamp, name, name_source, first_seen, seen_count, app_data_hex, capabilities, rssi, snr, q, stamp_cost_flexibility, peering_cost FROM announces ORDER BY timestamp DESC LIMIT ?1",
            )?;
            let mut rows = stmt.query(params![limit as i64])?;
            while let Some(row) = rows.next()? {
                records.push(parse_row(row)?);
            }
        }
        Ok(records)
    }

    pub fn clear_announces(&self) -> rusqlite::Result<()> {
        self.conn.execute("DELETE FROM announces", [])?;
        Ok(())
    }

    pub fn put_sdk_domain_snapshot(&self, snapshot: &JsonValue) -> rusqlite::Result<()> {
        let snapshot_json = serde_json::to_string(snapshot)
            .map_err(|err| rusqlite::Error::ToSqlConversionFailure(Box::new(err)))?;
        self.conn.execute(
            "INSERT INTO sdk_domain_state (domain, state_json) VALUES (?1, ?2)
             ON CONFLICT(domain) DO UPDATE SET state_json = excluded.state_json",
            params![Self::SDK_DOMAIN_SNAPSHOT_KEY, snapshot_json],
        )?;
        Ok(())
    }

    pub fn get_sdk_domain_snapshot(&self) -> rusqlite::Result<Option<JsonValue>> {
        let snapshot_json: Option<String> = self
            .conn
            .query_row(
                "SELECT state_json FROM sdk_domain_state WHERE domain = ?1 LIMIT 1",
                params![Self::SDK_DOMAIN_SNAPSHOT_KEY],
                |row| row.get(0),
            )
            .optional()?;
        let Some(snapshot_json) = snapshot_json else {
            return Ok(None);
        };
        let parsed = serde_json::from_str(snapshot_json.as_str()).map_err(|err| {
            rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(err))
        })?;
        Ok(Some(parsed))
    }

    pub fn clear_sdk_domain_snapshot(&self) -> rusqlite::Result<()> {
        self.conn.execute(
            "DELETE FROM sdk_domain_state WHERE domain = ?1",
            params![Self::SDK_DOMAIN_SNAPSHOT_KEY],
        )?;
        Ok(())
    }

    fn init_schema(&self) -> rusqlite::Result<()> {
        self.conn.execute_batch(
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
                stamp_cost_flexibility INTEGER,
                peering_cost INTEGER
            );
            CREATE TABLE IF NOT EXISTS sdk_domain_state (
                domain TEXT PRIMARY KEY,
                state_json TEXT NOT NULL
            );",
        )?;
        let _ = self.conn.execute("ALTER TABLE messages ADD COLUMN title TEXT", []);
        let _ = self.conn.execute("UPDATE messages SET title = '' WHERE title IS NULL", []);
        let _ = self.conn.execute("ALTER TABLE messages ADD COLUMN fields TEXT", []);
        let _ = self.conn.execute("ALTER TABLE messages ADD COLUMN receipt_status TEXT", []);
        let _ = self.conn.execute("ALTER TABLE announces ADD COLUMN name TEXT", []);
        let _ = self.conn.execute("ALTER TABLE announces ADD COLUMN name_source TEXT", []);
        let _ = self.conn.execute("ALTER TABLE announces ADD COLUMN first_seen INTEGER", []);
        let _ = self.conn.execute("ALTER TABLE announces ADD COLUMN seen_count INTEGER", []);
        let _ = self.conn.execute("ALTER TABLE announces ADD COLUMN app_data_hex TEXT", []);
        let _ = self.conn.execute("ALTER TABLE announces ADD COLUMN capabilities TEXT", []);
        let _ = self.conn.execute("ALTER TABLE announces ADD COLUMN rssi REAL", []);
        let _ = self.conn.execute("ALTER TABLE announces ADD COLUMN snr REAL", []);
        let _ = self.conn.execute("ALTER TABLE announces ADD COLUMN q REAL", []);
        let _ = self
            .conn
            .execute("ALTER TABLE announces ADD COLUMN stamp_cost_flexibility INTEGER", []);
        let _ = self.conn.execute("ALTER TABLE announces ADD COLUMN peering_cost INTEGER", []);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn outbound_message(id: &str, timestamp: i64, receipt_status: Option<&str>) -> MessageRecord {
        MessageRecord {
            id: id.to_string(),
            source: "src".to_string(),
            destination: "dst".to_string(),
            title: "title".to_string(),
            content: "body".to_string(),
            timestamp,
            direction: "out".to_string(),
            fields: None,
            receipt_status: receipt_status.map(ToString::to_string),
        }
    }

    #[test]
    fn sdk_domain_snapshot_roundtrip() {
        let store = MessagesStore::in_memory().expect("in-memory store");
        let initial = store.get_sdk_domain_snapshot().expect("query snapshot");
        assert!(initial.is_none(), "snapshot should be absent before first write");

        let snapshot = json!({
            "topics": [{ "topic_id": "topic-1" }],
            "attachments": [],
            "markers": [],
        });
        store.put_sdk_domain_snapshot(&snapshot).expect("persist snapshot");

        let loaded = store.get_sdk_domain_snapshot().expect("load snapshot");
        assert_eq!(loaded, Some(snapshot));
    }

    #[test]
    fn sdk_domain_snapshot_clear_removes_record() {
        let store = MessagesStore::in_memory().expect("in-memory store");
        store
            .put_sdk_domain_snapshot(&json!({ "voice_sessions": [{ "session_id": "voice-1" }] }))
            .expect("persist snapshot");
        store.clear_sdk_domain_snapshot().expect("clear snapshot");
        let loaded = store.get_sdk_domain_snapshot().expect("load snapshot");
        assert!(loaded.is_none(), "snapshot should be removed after clear");
    }

    #[test]
    fn expire_outbound_messages_marks_non_terminal_records() {
        let store = MessagesStore::in_memory().expect("in-memory store");
        store
            .insert_message(&outbound_message("out-non-terminal", 10, None))
            .expect("insert non-terminal");
        store
            .insert_message(&outbound_message("out-terminal", 10, Some("delivered")))
            .expect("insert terminal");
        let expired = store.expire_outbound_messages_before(11).expect("expire outbound");
        assert_eq!(expired, vec!["out-non-terminal".to_string()]);
        let non_terminal = store
            .get_message("out-non-terminal")
            .expect("load non-terminal")
            .expect("non-terminal exists");
        assert_eq!(non_terminal.receipt_status.as_deref(), Some("expired"));
        let terminal =
            store.get_message("out-terminal").expect("load terminal").expect("terminal exists");
        assert_eq!(terminal.receipt_status.as_deref(), Some("delivered"));
    }

    #[test]
    fn prune_outbound_messages_terminal_first_prefers_terminal_records() {
        let store = MessagesStore::in_memory().expect("in-memory store");
        store
            .insert_message(&outbound_message("msg-terminal-old", 1, Some("sent: direct")))
            .expect("insert terminal old");
        store
            .insert_message(&outbound_message("msg-non-terminal", 2, None))
            .expect("insert non-terminal");
        store
            .insert_message(&outbound_message("msg-terminal-new", 3, Some("delivered")))
            .expect("insert terminal new");

        let pruned = store.prune_outbound_messages(2, "terminal_first").expect("prune outbound");
        assert_eq!(pruned.len(), 2);
        assert!(pruned.iter().any(|id| id == "msg-terminal-old"));
        assert!(pruned.iter().any(|id| id == "msg-terminal-new"));
        assert!(
            store.get_message("msg-non-terminal").expect("load non-terminal").is_some(),
            "non-terminal record should remain when terminal records satisfy prune count"
        );
    }
}
