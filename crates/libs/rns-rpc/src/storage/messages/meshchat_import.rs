//! Read-only legacy history import; imported outbound rows never enter delivery queues.
use super::{params, Connection, MessagesStore, OpenFlags, Ordering};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use rusqlite::{types::ValueRef, Row};
use serde_json::{json, Map, Value};
use std::{io, path::Path};

fn invalid(message: impl Into<String>) -> rusqlite::Error {
    rusqlite::Error::ToSqlConversionFailure(Box::new(io::Error::new(
        io::ErrorKind::InvalidData,
        message.into(),
    )))
}

fn text(row: &Row<'_>, name: &str) -> rusqlite::Result<String> {
    match row.get_ref(name)? {
        ValueRef::Text(bytes) if bytes.len() <= 52_428_800 => std::str::from_utf8(bytes)
            .map(str::to_owned)
            .map_err(|_| invalid(format!("MeshChat {name} is not UTF-8"))),
        _ => Err(invalid(format!("Invalid or oversized MeshChat {name}"))),
    }
}

fn hex_text(row: &Row<'_>, name: &str, length: usize) -> rusqlite::Result<String> {
    let value = text(row, name)?;
    if value.len() != length || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(invalid(format!("Invalid MeshChat {name}")));
    }
    Ok(value.to_ascii_lowercase())
}

fn metadata(row: &Row<'_>) -> rusqlite::Result<Map<String, Value>> {
    let mut result = Map::new();
    for name in [
        "state",
        "progress",
        "method",
        "delivery_attempts",
        "rssi",
        "snr",
        "quality",
        "created_at",
        "updated_at",
    ] {
        let index = match row.as_ref().column_index(name) {
            Ok(index) => index,
            Err(_) if matches!(name, "state" | "progress") => {
                return Err(invalid(format!("Missing MeshChat {name}")))
            }
            Err(_) => continue,
        };
        let value = match row.get_ref(index)? {
            ValueRef::Null => Value::Null,
            ValueRef::Integer(value) => json!(value),
            ValueRef::Real(value) if value.is_finite() => json!(value),
            ValueRef::Text(_) => json!(text(row, name)?),
            _ => return Err(invalid(format!("Invalid MeshChat {name}"))),
        };
        let valid = match name {
            "state" => value.as_str().is_some_and(|state| {
                matches!(
                    state,
                    "unknown"
                        | "generating"
                        | "draft"
                        | "outbound"
                        | "sending"
                        | "sent"
                        | "delivered"
                        | "failed"
                        | "cancelled"
                        | "rejected"
                )
            }),
            "progress" => value.as_f64().is_some_and(|number| (0.0..=100.0).contains(&number)),
            "delivery_attempts" => value.as_i64().is_some_and(|number| number >= 0),
            "rssi" | "snr" | "quality" => value.is_null() || value.as_f64().is_some(),
            _ => value.is_null() || value.as_str().is_some(),
        };
        if !valid {
            return Err(invalid(format!("Invalid MeshChat {name}")));
        }
        result.insert(name.into(), value);
    }
    Ok(result)
}

fn validate_fields(fields: &Map<String, Value>) -> rusqlite::Result<()> {
    let valid_bytes = |value: Option<&Value>| {
        value.and_then(Value::as_str).is_some_and(|encoded| BASE64.decode(encoded).is_ok())
    };
    if let Some(files) = fields.get("file_attachments") {
        let valid = files.as_array().is_some_and(|files| {
            files.iter().all(|file| {
                file.get("file_name")
                    .and_then(Value::as_str)
                    .is_some_and(|name| !name.is_empty() && !name.contains(['/', '\\', '\0']))
                    && valid_bytes(file.get("file_bytes"))
            })
        });
        if !valid {
            return Err(invalid("Invalid MeshChat file attachments"));
        }
    }
    for (key, kind, bytes) in
        [("image", "image_type", "image_bytes"), ("audio", "audio_mode", "audio_bytes")]
    {
        if let Some(value) = fields.get(key) {
            let valid_kind = if key == "audio" {
                value.get(kind).and_then(Value::as_u64).is_some()
            } else {
                value.get(kind).and_then(Value::as_str).is_some_and(|kind| {
                    !kind.is_empty()
                        && kind.len() <= 16
                        && kind.chars().all(|ch| ch.is_ascii_alphanumeric())
                })
            };
            if !valid_kind || !valid_bytes(value.get(bytes)) {
                return Err(invalid(format!("Invalid MeshChat {key}")));
            }
        }
    }
    Ok(())
}

impl MessagesStore {
    /// Merge a legacy database while offline. Existing hashes are never overwritten.
    pub fn import_meshchat_history(&self, source: &Path, local: &str) -> rusqlite::Result<Value> {
        if self.durable_broker_enabled()? {
            return Err(invalid(
                "MeshChat history cannot be imported into a durable broker profile",
            ));
        }
        let source = Connection::open_with_flags(source, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
        let snapshot = source.unchecked_transaction()?;
        let mut select = snapshot.prepare("SELECT * FROM lxmf_messages ORDER BY timestamp, id")?;
        let mut rows = select.query([])?;
        self.with_write_conn(|conn| {
            let tx = conn.unchecked_transaction()?;
            let mut inserted = 0u64;
            let mut total = 0u64;
            while let Some(row) = rows.next()? {
                let id = hex_text(row, "hash", 64)?;
                let source = hex_text(row, "source_hash", 32)?;
                let destination = hex_text(row, "destination_hash", 32)?;
                let incoming: i64 = row.get("is_incoming")?;
                if !matches!(incoming, 0 | 1)
                    || (incoming == 1 && destination != local)
                    || (incoming == 0 && source != local)
                {
                    return Err(invalid(format!("MeshChat message {id} does not belong to this identity")));
                }
                let timestamp: f64 = row.get("timestamp")?;
                if !timestamp.is_finite() || timestamp < 0.0 || timestamp >= i64::MAX as f64 {
                    return Err(invalid(format!("Invalid timestamp for MeshChat message {id}")));
                }
                let mut fields: Map<String, Value> = serde_json::from_str(&text(row, "fields")?)
                    .map_err(|_| invalid(format!("Invalid fields for MeshChat message {id}")))?;
                if fields.contains_key("_meshchat_import") {
                    return Err(invalid("MeshChat fields contain reserved import metadata"));
                }
                validate_fields(&fields)?;
                let mut historical = metadata(row)?;
                historical.insert("timestamp".into(), json!(timestamp));
                fields.insert("_meshchat_import".into(), Value::Object(historical));
                inserted += tx.execute(
                    "INSERT INTO messages (id, source, destination, title, content, timestamp, direction, fields, receipt_status)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 'cancelled') ON CONFLICT(id) DO NOTHING",
                    params![id, source, destination, text(row, "title")?, text(row, "content")?, timestamp.floor() as i64,
                        if incoming == 1 { "in" } else { "out" }, Value::Object(fields).to_string()],
                )? as u64;
                total += 1;
            }
            tx.commit()?;
            self.write_state.message_count_cache.fetch_add(inserted, Ordering::Relaxed);
            Ok(json!({"imported":inserted,"skipped":total-inserted,"total":total}))
        })
    }
}

#[cfg(test)]
mod tests;
