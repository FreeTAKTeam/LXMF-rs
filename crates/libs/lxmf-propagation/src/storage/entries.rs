use rusqlite::{params, Connection, OptionalExtension};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PropagationEntryRecord {
    pub transient_id: String,
    pub destination: String,
    pub payload_hex: String,
    pub received_at: i64,
    pub size_bytes: u64,
    pub stamp_value: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PropagationEntryStats {
    pub entries: u64,
    pub bytes: u64,
}

pub fn propagation_entry_from_row(
    row: &rusqlite::Row<'_>,
) -> rusqlite::Result<PropagationEntryRecord> {
    let size_bytes: i64 = row.get(4)?;
    let stamp_value: Option<u32> = row.get(5)?;
    Ok(PropagationEntryRecord {
        transient_id: row.get(0)?,
        destination: row.get(1)?,
        payload_hex: row.get(2)?,
        received_at: row.get(3)?,
        size_bytes: size_bytes.max(0) as u64,
        stamp_value,
    })
}

pub fn upsert_propagation_entry(
    conn: &Connection,
    record: &PropagationEntryRecord,
) -> rusqlite::Result<()> {
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
            sql_u64(record.size_bytes)?,
            record.stamp_value,
        ],
    )?;
    Ok(())
}

pub fn get_propagation_entry(
    conn: &Connection,
    transient_id: &str,
) -> rusqlite::Result<Option<PropagationEntryRecord>> {
    conn.query_row(
        "SELECT transient_id, destination, payload_hex, received_at, size_bytes, stamp_value
                 FROM propagation_entries
                 WHERE transient_id = ?1
                 LIMIT 1",
        params![normalize_hex_key(transient_id)],
        propagation_entry_from_row,
    )
    .optional()
}

pub fn propagation_entry_stats(conn: &Connection) -> rusqlite::Result<PropagationEntryStats> {
    let (entries, bytes): (i64, Option<i64>) = conn.query_row(
        "SELECT COUNT(*), COALESCE(SUM(size_bytes), 0) FROM propagation_entries",
        [],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    Ok(PropagationEntryStats {
        entries: entries.max(0) as u64,
        bytes: bytes.unwrap_or(0).max(0) as u64,
    })
}

pub fn list_propagation_entry_metadata_for_destination(
    conn: &Connection,
    destination: &str,
) -> rusqlite::Result<Vec<(String, u64)>> {
    let mut stmt = conn.prepare(
        "SELECT transient_id, size_bytes FROM propagation_entries
                 WHERE destination = ?1 ORDER BY size_bytes ASC, transient_id ASC",
    )?;
    let rows = stmt.query_map(params![normalize_hex_key(destination)], |row| {
        let size: i64 = row.get(1)?;
        Ok((row.get(0)?, size.max(0) as u64))
    })?;
    rows.collect()
}

pub fn list_propagation_entries_for_destination(
    conn: &Connection,
    destination: &str,
) -> rusqlite::Result<Vec<PropagationEntryRecord>> {
    let mut stmt = conn.prepare(
        "SELECT transient_id, destination, payload_hex, received_at, size_bytes, stamp_value
                 FROM propagation_entries
                 WHERE destination = ?1
                 ORDER BY size_bytes ASC, transient_id ASC",
    )?;
    let rows =
        stmt.query_map(params![normalize_hex_key(destination)], propagation_entry_from_row)?;
    rows.collect()
}

pub fn fetch_propagation_payloads_for_destination(
    conn: &Connection,
    destination: &str,
    wanted: &[String],
    transfer_limit_bytes: Option<usize>,
) -> rusqlite::Result<Vec<Vec<u8>>> {
    let destination = normalize_hex_key(destination);
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
        let next_size =
            cumulative_size.saturating_add(stored_size.saturating_add(32) + per_message_overhead);
        if transfer_limit_bytes.is_some_and(|limit| next_size > limit) {
            continue;
        }
        let payload = hex::decode(entry.payload_hex.as_str()).map_err(|err| {
            rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(err))
        })?;
        cumulative_size = next_size;
        messages.push(payload);
    }
    Ok(messages)
}

pub fn purge_propagation_entries_for_destination(
    conn: &Connection,
    destination: &str,
    haves: &[String],
) -> rusqlite::Result<usize> {
    let destination = normalize_hex_key(destination);
    let mut purged = 0usize;
    for transient_id in haves {
        let transient_id = normalize_hex_key(transient_id);
        let affected = conn.execute(
            "DELETE FROM propagation_entries
                     WHERE transient_id = ?1 AND destination = ?2",
            params![transient_id, destination],
        )?;
        if affected > 0 {
            conn.execute(
                "DELETE FROM propagation_peer_entries
                         WHERE transient_id = ?1
                           AND state = 'unhandled'",
                params![transient_id],
            )?;
            purged = purged.saturating_add(affected);
        }
    }
    Ok(purged)
}

fn normalize_hex_key(value: &str) -> String {
    value.trim().to_ascii_lowercase()
}
fn sql_u64(value: u64) -> rusqlite::Result<i64> {
    i64::try_from(value).map_err(|err| rusqlite::Error::ToSqlConversionFailure(Box::new(err)))
}
