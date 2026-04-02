use serde::Serialize;
use serde_json::json;
use std::env;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

#[derive(Debug, Serialize)]
struct SavedMessageContainer<'a> {
    state: u8,
    #[serde(with = "serde_bytes")]
    lxmf_bytes: &'a [u8],
    transport_encrypted: bool,
    transport_encryption: Option<&'a str>,
    method: u8,
}

pub(crate) fn run_on_inbound_command(
    command: &str,
    event: &serde_json::Value,
    messages_dir: Option<&Path>,
) -> Result<(), String> {
    let event_type =
        event.get("event_type").and_then(|value| value.as_str()).unwrap_or("<unknown>");
    if event_type != "inbound" {
        return Ok(());
    }

    let payload = event.get("payload").cloned().unwrap_or_else(|| json!({}));
    let message = payload.get("message").cloned().unwrap_or_else(|| json!({}));
    let body = serde_json::to_vec(&payload).map_err(|err| err.to_string())?;
    let message_path = write_inbound_message_file(messages_dir, &payload, &message)?;

    let shell = env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());
    let shell_command = if let Some(path) = message_path.as_ref() {
        format!("{command} \"{}\"", shell_escape(path))
    } else {
        command.to_string()
    };
    let mut child = Command::new(shell)
        .arg("-c")
        .arg(shell_command)
        .env("LXMD_EVENT_TYPE", "inbound")
        .env("LXMD_EVENT_JSON", compact_json(&payload)?)
        .env("LXMD_MESSAGE_JSON", compact_json(&message)?)
        .env("LXMD_MESSAGE_ID", json_env(&message, "id"))
        .env("LXMD_MESSAGE_SOURCE", json_env(&message, "source"))
        .env("LXMD_MESSAGE_DESTINATION", json_env(&message, "destination"))
        .env("LXMD_MESSAGE_TITLE", json_env(&message, "title"))
        .env("LXMD_MESSAGE_CONTENT", json_env(&message, "content"))
        .env("LXMD_MESSAGE_TIMESTAMP", json_env(&message, "timestamp"))
        .env(
            "LXMD_MESSAGE_PATH",
            message_path.as_ref().map(|path| path.display().to_string()).unwrap_or_default(),
        )
        .stdin(Stdio::piped())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|err| err.to_string())?;

    if let Some(mut stdin) = child.stdin.take() {
        stdin.write_all(&body).map_err(|err| err.to_string())?;
    }

    let status = child.wait().map_err(|err| err.to_string())?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("command exited with status {status}"))
    }
}

pub(crate) fn json_env(value: &serde_json::Value, key: &str) -> String {
    match value.get(key) {
        Some(serde_json::Value::String(text)) => text.clone(),
        Some(other) if !other.is_null() => other.to_string(),
        _ => String::new(),
    }
}

pub(crate) fn compact_json(value: &serde_json::Value) -> Result<String, String> {
    serde_json::to_string(value).map_err(|err| err.to_string())
}

fn write_inbound_message_file(
    messages_dir: Option<&Path>,
    payload: &serde_json::Value,
    message: &serde_json::Value,
) -> Result<Option<PathBuf>, String> {
    let Some(messages_dir) = messages_dir else {
        return Ok(None);
    };
    fs::create_dir_all(messages_dir).map_err(|err| err.to_string())?;
    let message_id = json_env(message, "id");
    let file_name = if message_id.is_empty() {
        format!("{}.json", now_epoch_millis())
    } else {
        sanitize_file_name(&message_id)
    };
    let path = messages_dir.join(file_name);
    let packed = pack_saved_inbound_message(payload, message)?;
    fs::write(&path, packed).map_err(|err| err.to_string())?;
    Ok(Some(path))
}

pub(crate) fn sanitize_file_name(input: &str) -> String {
    input
        .chars()
        .map(
            |ch| {
                if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-') {
                    ch
                } else {
                    '_'
                }
            },
        )
        .collect()
}

fn shell_escape(path: &Path) -> String {
    path.display().to_string().replace('"', "\\\"")
}

fn now_epoch_millis() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|value| value.as_millis())
        .unwrap_or(0)
}

fn pack_saved_inbound_message(
    payload: &serde_json::Value,
    message: &serde_json::Value,
) -> Result<Vec<u8>, String> {
    let lxmf_bytes =
        if let Some(raw_hex) = payload.get("lxmf_bytes_hex").and_then(|value| value.as_str()) {
            hex::decode(raw_hex).map_err(|err| err.to_string())?
        } else {
            reconstruct_inbound_wire_bytes(message)?
        };
    let container = SavedMessageContainer {
        state: 0x00,
        lxmf_bytes: &lxmf_bytes,
        transport_encrypted: false,
        transport_encryption: None,
        method: 0x00,
    };
    let mut out = Vec::new();
    let mut serializer = rmp_serde::Serializer::new(&mut out).with_struct_map();
    container.serialize(&mut serializer).map_err(|err| err.to_string())?;
    Ok(out)
}

fn reconstruct_inbound_wire_bytes(message: &serde_json::Value) -> Result<Vec<u8>, String> {
    let destination = decode_hash_field(message, "destination")?;
    let source = decode_hash_field(message, "source")?;
    let timestamp = message.get("timestamp").and_then(|value| value.as_i64()).unwrap_or(0);
    let title = message.get("title").and_then(|value| value.as_str()).unwrap_or("");
    let content = message.get("content").and_then(|value| value.as_str()).unwrap_or("");
    let fields = message.get("fields").map(json_to_rmpv).transpose()?.unwrap_or(rmpv::Value::Nil);
    let payload = rmpv::Value::Array(vec![
        rmpv::Value::from(timestamp),
        rmpv::Value::from(title),
        rmpv::Value::from(content),
        fields,
    ]);
    let packed_payload = rmp_serde::to_vec(&payload).map_err(|err| err.to_string())?;
    let mut wire = Vec::with_capacity(16 + 16 + 64 + packed_payload.len());
    wire.extend_from_slice(&destination);
    wire.extend_from_slice(&source);
    wire.extend_from_slice(&[0u8; 64]);
    wire.extend_from_slice(&packed_payload);
    Ok(wire)
}

fn decode_hash_field(message: &serde_json::Value, key: &str) -> Result<[u8; 16], String> {
    let value = message
        .get(key)
        .and_then(|value| value.as_str())
        .ok_or_else(|| format!("inbound message missing {key}"))?;
    let decoded = hex::decode(value).map_err(|err| format!("invalid {key} hex: {err}"))?;
    let decoded_len = decoded.len();
    decoded
        .try_into()
        .map_err(|_| format!("invalid {key} length {}, expected 16 bytes", decoded_len))
}

fn json_to_rmpv(value: &serde_json::Value) -> Result<rmpv::Value, String> {
    Ok(match value {
        serde_json::Value::Null => rmpv::Value::Nil,
        serde_json::Value::Bool(value) => rmpv::Value::Boolean(*value),
        serde_json::Value::Number(value) => {
            if let Some(value) = value.as_i64() {
                rmpv::Value::from(value)
            } else if let Some(value) = value.as_u64() {
                rmpv::Value::from(value)
            } else if let Some(value) = value.as_f64() {
                rmpv::Value::F64(value)
            } else {
                return Err("unsupported JSON number".to_string());
            }
        }
        serde_json::Value::String(value) => rmpv::Value::from(value.as_str()),
        serde_json::Value::Array(values) => {
            rmpv::Value::Array(values.iter().map(json_to_rmpv).collect::<Result<Vec<_>, _>>()?)
        }
        serde_json::Value::Object(map) => rmpv::Value::Map(
            map.iter()
                .map(|(key, value)| Ok((rmpv::Value::from(key.as_str()), json_to_rmpv(value)?)))
                .collect::<Result<Vec<_>, String>>()?,
        ),
    })
}

#[cfg(test)]
mod tests {
    use super::{compact_json, json_env, sanitize_file_name};
    use serde_json::json;

    #[test]
    fn json_env_handles_strings_numbers_and_missing() {
        let value = json!({
            "id": "m1",
            "timestamp": 123,
        });
        assert_eq!(json_env(&value, "id"), "m1");
        assert_eq!(json_env(&value, "timestamp"), "123");
        assert_eq!(json_env(&value, "missing"), "");
    }

    #[test]
    fn compact_json_produces_single_line_json() {
        let value = json!({ "message": { "id": "m1" } });
        assert_eq!(compact_json(&value).expect("json"), "{\"message\":{\"id\":\"m1\"}}");
    }

    #[test]
    fn sanitize_file_name_replaces_unsafe_characters() {
        assert_eq!(sanitize_file_name("msg:/id?"), "msg__id_");
    }
}
