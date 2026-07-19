use rusqlite::{params, Connection};
use serde_json::Value as JsonValue;

fn serialize_json_for_sql<T: serde::Serialize>(value: &T) -> rusqlite::Result<String> {
    serde_json::to_string(value)
        .map_err(|err| rusqlite::Error::ToSqlConversionFailure(Box::new(err)))
}

fn deserialize_json_column<T: serde::de::DeserializeOwned>(
    value: &str,
    column: usize,
) -> rusqlite::Result<T> {
    serde_json::from_str(value).map_err(|err| {
        rusqlite::Error::FromSqlConversionFailure(
            column,
            rusqlite::types::Type::Text,
            Box::new(err),
        )
    })
}

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
        let fields_json = record.fields.as_ref().map(serialize_json_for_sql).transpose()?;
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
                let fields = fields_json
                    .as_deref()
                    .map(|value| deserialize_json_column(value, 7))
                    .transpose()?;
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
                let fields = fields_json
                    .as_deref()
                    .map(|value| deserialize_json_column(value, 7))
                    .transpose()?;
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
        let capabilities_json = serialize_json_for_sql(&record.capabilities)?;
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

    fn init_schema(&self) -> rusqlite::Result<()> {
        let tx = self.conn.unchecked_transaction()?;
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
                stamp_cost_flexibility INTEGER,
                peering_cost INTEGER
            );",
        )?;
        Self::ensure_schema_column(&tx, "messages", "title", "title TEXT")?;
        tx.execute("UPDATE messages SET title = '' WHERE title IS NULL", [])?;
        Self::ensure_schema_column(&tx, "messages", "fields", "fields TEXT")?;
        Self::ensure_schema_column(&tx, "messages", "receipt_status", "receipt_status TEXT")?;
        Self::ensure_schema_column(&tx, "announces", "name", "name TEXT")?;
        Self::ensure_schema_column(&tx, "announces", "name_source", "name_source TEXT")?;
        Self::ensure_schema_column(&tx, "announces", "first_seen", "first_seen INTEGER")?;
        Self::ensure_schema_column(&tx, "announces", "seen_count", "seen_count INTEGER")?;
        tx.execute("UPDATE announces SET first_seen = timestamp WHERE first_seen IS NULL", [])?;
        tx.execute("UPDATE announces SET seen_count = 1 WHERE seen_count IS NULL", [])?;
        Self::ensure_schema_column(&tx, "announces", "app_data_hex", "app_data_hex TEXT")?;
        Self::ensure_schema_column(&tx, "announces", "capabilities", "capabilities TEXT")?;
        Self::ensure_schema_column(&tx, "announces", "rssi", "rssi REAL")?;
        Self::ensure_schema_column(&tx, "announces", "snr", "snr REAL")?;
        Self::ensure_schema_column(&tx, "announces", "q", "q REAL")?;
        Self::ensure_schema_column(
            &tx,
            "announces",
            "stamp_cost_flexibility",
            "stamp_cost_flexibility INTEGER",
        )?;
        Self::ensure_schema_column(&tx, "announces", "peering_cost", "peering_cost INTEGER")?;
        tx.commit()
    }

    fn schema_has_column(
        conn: &Connection,
        table: &'static str,
        column: &'static str,
    ) -> rusqlite::Result<bool> {
        let mut statement = conn.prepare(&format!("PRAGMA table_info({table})"))?;
        let mut rows = statement.query([])?;
        while let Some(row) = rows.next()? {
            let existing: String = row.get(1)?;
            if existing == column {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn ensure_schema_column(
        conn: &Connection,
        table: &'static str,
        column: &'static str,
        declaration: &'static str,
    ) -> rusqlite::Result<()> {
        if !Self::schema_has_column(conn, table, column)? {
            conn.execute(format!("ALTER TABLE {table} ADD COLUMN {declaration}").as_str(), [])?;
        }
        Ok(())
    }
}
