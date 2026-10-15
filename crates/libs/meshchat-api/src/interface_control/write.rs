use super::store;
use crate::ApiError;
use axum::http::StatusCode;
use serde_json::Value;
use toml_edit::{DocumentMut, Item, Table};

pub(super) fn save_interface(
    doc: &mut DocumentMut,
    name: &str,
    body: &Value,
    overwrite: bool,
) -> Result<(), ApiError> {
    let kind = body
        .get("type")
        .and_then(Value::as_str)
        .filter(|kind| !kind.is_empty())
        .ok_or_else(|| ApiError(StatusCode::UNPROCESSABLE_ENTITY, "Type is required".into()))?;
    const SUPPORTED: &[&str] = &[
        "AutoInterface",
        "TCPClientInterface",
        "TCPServerInterface",
        "UDPInterface",
        "RNodeInterface",
        "RNodeMultiInterface",
        "SerialInterface",
        "KISSInterface",
        "AX25KISSInterface",
        "PipeInterface",
        "I2PInterface",
    ];
    if !SUPPORTED.contains(&kind) {
        return Err(ApiError(
            StatusCode::UNPROCESSABLE_ENTITY,
            format!("Unsupported interface type {kind}"),
        ));
    }
    let existing = store::interface_names(doc)?.iter().any(|item| item == name);
    if existing && !overwrite {
        return Err(ApiError(
            StatusCode::UNPROCESSABLE_ENTITY,
            "Name is already in use by another interface".into(),
        ));
    }
    validate_required(kind, body)?;
    let mut table = if existing {
        store::interface_mut(doc, name)
            .ok_or_else(|| ApiError::unavailable("Interface lookup failed"))?
            .clone()
    } else {
        Table::new()
    };
    table["type"] = toml_edit::value(kind);
    if kind == "I2PInterface" {
        table["connectable"] = toml_edit::value(true);
    }
    if !table.contains_key("enabled") && !table.contains_key("interface_enabled") {
        table["interface_enabled"] = toml_edit::value(true);
    }
    for key in editable_fields(kind) {
        match body.get(*key) {
            Some(value) if !value.is_null() && value != "" => table[*key] = json_item(key, value)?,
            _ => {
                table.remove(key);
            }
        }
    }
    if kind == "RNodeMultiInterface" {
        save_subinterfaces(&mut table, body)?;
    }
    if existing {
        store::remove_interface(doc, name)?;
    }
    store::insert_interface(doc, name, table)
}

fn validate_required(kind: &str, body: &Value) -> Result<(), ApiError> {
    let keys: &[(&str, &str)] = match kind {
        "TCPClientInterface" => &[("target_host", "Target Host"), ("target_port", "Target Port")],
        "TCPServerInterface" => &[("listen_ip", "Listen IP"), ("listen_port", "Listen Port")],
        "UDPInterface" => &[
            ("listen_ip", "Listen IP"),
            ("listen_port", "Listen Port"),
            ("forward_ip", "Forward IP"),
            ("forward_port", "Forward Port"),
        ],
        "RNodeInterface" => &[
            ("port", "Port"),
            ("frequency", "Frequency"),
            ("bandwidth", "Bandwidth"),
            ("txpower", "TX power"),
            ("spreadingfactor", "Spreading Factor"),
            ("codingrate", "Coding Rate"),
        ],
        "RNodeMultiInterface" | "SerialInterface" | "KISSInterface" | "AX25KISSInterface" => {
            &[("port", "Port")]
        }
        "PipeInterface" => &[("command", "Command"), ("respawn_delay", "Respawn delay")],
        _ => &[],
    };
    for (key, label) in keys {
        if body.get(*key).is_none_or(|value| value.is_null() || value == "") {
            return Err(ApiError(StatusCode::UNPROCESSABLE_ENTITY, format!("{label} is required")));
        }
    }
    Ok(())
}

fn editable_fields(kind: &str) -> &'static [&'static str] {
    match kind {
        "AutoInterface" => &[
            "group_id",
            "multicast_address_type",
            "devices",
            "ignored_devices",
            "discovery_scope",
            "discovery_port",
            "data_port",
            "bitrate",
            "mode",
            "network_name",
            "passphrase",
            "ifac_size",
        ],
        "TCPClientInterface" => &[
            "target_host",
            "target_port",
            "kiss_framing",
            "i2p_tunneled",
            "bitrate",
            "mode",
            "network_name",
            "passphrase",
            "ifac_size",
        ],
        "TCPServerInterface" => &[
            "listen_ip",
            "listen_port",
            "device",
            "prefer_ipv6",
            "bitrate",
            "mode",
            "network_name",
            "passphrase",
            "ifac_size",
        ],
        "UDPInterface" => &[
            "listen_ip",
            "listen_port",
            "forward_ip",
            "forward_port",
            "device",
            "bitrate",
            "mode",
            "network_name",
            "passphrase",
            "ifac_size",
        ],
        "RNodeInterface" => &[
            "port",
            "frequency",
            "bandwidth",
            "txpower",
            "spreadingfactor",
            "codingrate",
            "callsign",
            "id_interval",
            "airtime_limit_long",
            "airtime_limit_short",
            "bitrate",
            "mode",
            "network_name",
            "passphrase",
            "ifac_size",
        ],
        "RNodeMultiInterface" => {
            &["port", "bitrate", "mode", "network_name", "passphrase", "ifac_size"]
        }
        "SerialInterface" | "KISSInterface" | "AX25KISSInterface" => &[
            "port",
            "speed",
            "databits",
            "parity",
            "stopbits",
            "preamble",
            "txtail",
            "persistence",
            "slottime",
            "callsign",
            "ssid",
            "bitrate",
            "mode",
            "network_name",
            "passphrase",
            "ifac_size",
        ],
        "PipeInterface" => &[
            "command",
            "respawn_delay",
            "bitrate",
            "mode",
            "network_name",
            "passphrase",
            "ifac_size",
        ],
        "I2PInterface" => &["peers", "bitrate", "mode", "network_name", "passphrase", "ifac_size"],
        _ => &[],
    }
}

fn json_item(key: &str, value: &Value) -> Result<Item, ApiError> {
    const NUMBERS: &[&str] = &[
        "target_port",
        "listen_port",
        "forward_port",
        "discovery_port",
        "data_port",
        "frequency",
        "bandwidth",
        "txpower",
        "spreadingfactor",
        "codingrate",
        "vport",
        "speed",
        "databits",
        "stopbits",
        "preamble",
        "txtail",
        "persistence",
        "slottime",
        "ssid",
        "id_interval",
        "bitrate",
        "ifac_size",
    ];
    const FLOATS: &[&str] = &["respawn_delay", "airtime_limit_long", "airtime_limit_short"];
    const BOOLEANS: &[&str] =
        &["kiss_framing", "i2p_tunneled", "prefer_ipv6", "interface_enabled", "enabled"];
    if NUMBERS.contains(&key) {
        let number = value
            .as_i64()
            .or_else(|| value.as_str().and_then(|text| text.parse().ok()))
            .ok_or_else(|| {
            ApiError(StatusCode::UNPROCESSABLE_ENTITY, format!("{key} must be an integer"))
        })?;
        return Ok(toml_edit::value(number));
    }
    if FLOATS.contains(&key) {
        let number = value
            .as_f64()
            .or_else(|| value.as_str().and_then(|text| text.parse().ok()))
            .filter(|number| number.is_finite())
            .ok_or_else(|| {
                ApiError(StatusCode::UNPROCESSABLE_ENTITY, format!("{key} must be a finite number"))
            })?;
        return Ok(toml_edit::value(number));
    }
    if BOOLEANS.contains(&key) {
        let boolean = value
            .as_bool()
            .or_else(|| value.as_str().and_then(|text| text.parse().ok()))
            .ok_or_else(|| {
                ApiError(StatusCode::UNPROCESSABLE_ENTITY, format!("{key} must be a boolean"))
            })?;
        return Ok(toml_edit::value(boolean));
    }
    if ["devices", "ignored_devices", "peers"].contains(&key) {
        let strings = match value {
            Value::String(text) => text
                .split(',')
                .map(str::trim)
                .filter(|item| !item.is_empty())
                .map(str::to_string)
                .collect::<Vec<_>>(),
            Value::Array(items) => items
                .iter()
                .map(|item| {
                    item.as_str()
                        .map(str::to_string)
                        .ok_or_else(|| ApiError::bad(format!("{key} must be a list of strings")))
                })
                .collect::<Result<Vec<_>, _>>()?,
            _ => return Err(ApiError::bad(format!("{key} must be a list of strings"))),
        };
        let mut array = toml_edit::Array::new();
        for item in strings {
            array.push(item);
        }
        return Ok(Item::Value(toml_edit::Value::Array(array)));
    }
    if let Some(text) = value.as_str() {
        return Ok(toml_edit::value(text));
    }
    if let Some(boolean) = value.as_bool() {
        return Ok(toml_edit::value(boolean.to_string()));
    }
    if let Some(number) = value.as_i64() {
        return Ok(toml_edit::value(number.to_string()));
    }
    if let Some(number) = value.as_f64() {
        return Ok(toml_edit::value(number.to_string()));
    }
    if let Some(items) = value.as_array() {
        let mut array = toml_edit::Array::new();
        for item in items {
            let text = item
                .as_str()
                .ok_or_else(|| ApiError::bad(format!("{key} must be a list of strings")))?;
            array.push(text);
        }
        return Ok(Item::Value(toml_edit::Value::Array(array)));
    }
    Err(ApiError::bad(format!("Unsupported {key} value")))
}

fn save_subinterfaces(table: &mut Table, body: &Value) -> Result<(), ApiError> {
    let children = body
        .get("sub_interfaces")
        .and_then(Value::as_array)
        .filter(|items| !items.is_empty())
        .ok_or_else(|| {
            ApiError(
                StatusCode::UNPROCESSABLE_ENTITY,
                "At least one sub-interface is required".into(),
            )
        })?;
    let old = table
        .iter()
        .filter(|(_, value)| value.is_table())
        .map(|(name, _)| name.to_string())
        .collect::<Vec<_>>();
    for name in old {
        table.remove(&name);
    }
    for (index, child) in children.iter().enumerate() {
        let name =
            child.get("name").and_then(Value::as_str).filter(|name| !name.is_empty()).ok_or_else(
                || {
                    ApiError(
                        StatusCode::UNPROCESSABLE_ENTITY,
                        format!("Sub-interface {} is missing name", index + 1),
                    )
                },
            )?;
        let mut record = Table::new();
        record["interface_enabled"] = toml_edit::value(true);
        for key in ["frequency", "bandwidth", "txpower", "spreadingfactor", "codingrate", "vport"] {
            let value = child
                .get(key)
                .filter(|value| !value.is_null() && *value != "")
                .ok_or_else(|| {
                    ApiError(
                        StatusCode::UNPROCESSABLE_ENTITY,
                        format!("Sub-interface {} is missing {key}", index + 1),
                    )
                })?;
            record[key] = json_item(key, value)?;
        }
        table.insert(name, Item::Table(record));
    }
    Ok(())
}
