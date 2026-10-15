use super::{config_task, save_interface, store};
use crate::{ApiError, ApiResult, AppState};
use axum::{
    body::Bytes,
    extract::State,
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde_json::{json, Map, Value};
use std::sync::Arc;

pub(crate) async fn export(
    State(state): State<Arc<AppState>>,
    body: Bytes,
) -> Result<Response, ApiError> {
    let selection = serde_json::from_slice::<Value>(&body)
        .ok()
        .and_then(|value| value.get("selected_interface_names").and_then(Value::as_array).cloned());
    let interfaces = config_task(&state, |store| store.read(store::saved_interfaces)).await?;
    let mut output = String::new();
    for (name, record) in interfaces {
        if selection
            .as_ref()
            .is_some_and(|names| !names.iter().any(|item| item.as_str() == Some(&name)))
        {
            continue;
        }
        output.push_str(&format!("[[{name}]]\n"));
        let Some(fields) = record.as_object() else {
            continue;
        };
        for (key, value) in fields {
            if key == "sub_interfaces" {
                continue;
            }
            if let Some(text) = export_value(value) {
                output.push_str(&format!("    {key} = {text}\n"));
            }
        }
        output.push('\n');
        if let Some(children) = fields.get("sub_interfaces").and_then(Value::as_array) {
            for child in children {
                let Some(name) = child.get("name").and_then(Value::as_str) else {
                    continue;
                };
                output.push_str(&format!("  [[[{name}]]]\n"));
                if let Some(fields) = child.as_object() {
                    for (key, value) in fields {
                        if key == "name" {
                            continue;
                        }
                        if let Some(text) = export_value(value) {
                            output.push_str(&format!("      {key} = {text}\n"));
                        }
                    }
                }
                output.push('\n');
            }
        }
    }
    Ok((
        [
            (header::CONTENT_TYPE, "text/plain; charset=utf-8"),
            (header::CONTENT_DISPOSITION, "attachment; filename=meshchat_interfaces"),
        ],
        output,
    )
        .into_response())
}

fn export_value(value: &Value) -> Option<String> {
    match value {
        Value::String(text) if text.contains(',') => {
            Some(format!("\"{}\"", text.replace('"', "\\\"")))
        }
        Value::String(text) => Some(text.clone()),
        Value::Bool(boolean) => Some(boolean.to_string()),
        Value::Number(number) => Some(number.to_string()),
        Value::Array(items) => {
            Some(items.iter().filter_map(export_value).collect::<Vec<_>>().join(", "))
        }
        _ => None,
    }
}

pub(crate) async fn import_preview(Json(body): Json<Value>) -> ApiResult {
    let input = body
        .get("config")
        .and_then(Value::as_str)
        .ok_or_else(|| ApiError::bad("config text is required"))?;
    let interfaces = parse_config(input)?;
    Ok(Json(json!({"interfaces":interfaces})))
}

pub(crate) async fn import(
    State(state): State<Arc<AppState>>,
    Json(body): Json<Value>,
) -> ApiResult {
    let input = body
        .get("config")
        .and_then(Value::as_str)
        .ok_or_else(|| ApiError::bad("config text is required"))?;
    let interfaces = parse_config(input)?;
    let selected = body
        .get("selected_interface_names")
        .and_then(Value::as_array)
        .ok_or_else(|| ApiError::bad("selected_interface_names is required"))?;
    let selected: Vec<String> =
        selected.iter().filter_map(Value::as_str).map(str::to_string).collect();
    config_task(&state, move |store| {
        store.edit(|doc| {
            for mut record in interfaces {
                let name = record
                    .get("name")
                    .and_then(Value::as_str)
                    .ok_or_else(|| ApiError::bad("Interface name is required"))?
                    .to_string();
                if !selected.contains(&name) {
                    continue;
                }
                let object = record
                    .as_object_mut()
                    .ok_or_else(|| ApiError::bad("Interface must be an object"))?;
                object.remove("name");
                object.remove("enabled");
                object.insert("interface_enabled".into(), Value::Bool(true));
                if object.get("type").and_then(Value::as_str) == Some("RNodeMultiInterface") {
                    let children = object
                        .iter()
                        .filter_map(|(key, value)| {
                            value.as_object().map(|fields| {
                                let mut fields = fields.clone();
                                fields.insert("name".into(), Value::String(key.clone()));
                                Value::Object(fields)
                            })
                        })
                        .collect::<Vec<_>>();
                    let names = children
                        .iter()
                        .filter_map(|child| child.get("name").and_then(Value::as_str))
                        .map(str::to_string)
                        .collect::<Vec<_>>();
                    for name in names {
                        object.remove(&name);
                    }
                    object.insert("sub_interfaces".into(), Value::Array(children));
                }
                store::remove_interface(doc, &name)?;
                save_interface(doc, &name, &record, false)?;
                let saved = store::interface_mut(doc, &name)
                    .ok_or_else(|| ApiError::unavailable("Imported interface lookup failed"))?;
                saved.remove("enabled");
                saved["interface_enabled"] = toml_edit::value(true);
            }
            Ok(())
        })
    })
    .await?;
    Ok(Json(json!({"message":"Interfaces imported successfully"})))
}

fn parse_config(input: &str) -> Result<Vec<Value>, ApiError> {
    let mut interfaces: Vec<Value> = Vec::new();
    let mut current: Option<usize> = None;
    let mut child: Option<String> = None;
    let mut in_interfaces = true;
    for (line_number, raw) in input.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }
        if line.starts_with("[[[") && line.ends_with("]]]") {
            let name = section_name(line, 3, line_number)?;
            if current.is_none() || !in_interfaces {
                return Err(parse_error(line_number, "Sub-interface has no parent"));
            }
            child = Some(name);
            continue;
        }
        if line.starts_with("[[") && line.ends_with("]]") {
            let name = section_name(line, 2, line_number)?;
            if !in_interfaces {
                continue;
            }
            interfaces.push(json!({"name":name}));
            current = Some(interfaces.len() - 1);
            child = None;
            continue;
        }
        if line.starts_with('[') && line.ends_with(']') {
            let name = section_name(line, 1, line_number)?;
            in_interfaces = name == "interfaces";
            current = None;
            child = None;
            continue;
        }
        if !in_interfaces {
            continue;
        }
        let (key, raw_value) =
            line.split_once('=').ok_or_else(|| parse_error(line_number, "Expected key = value"))?;
        let key = key.trim();
        if key.is_empty() {
            return Err(parse_error(line_number, "Empty key"));
        }
        let value = parse_value(raw_value.trim());
        let index =
            current.ok_or_else(|| parse_error(line_number, "Setting has no interface section"))?;
        let object = interfaces[index].as_object_mut().expect("parser creates objects");
        if let Some(child_name) = child.as_deref() {
            let children = object
                .entry("_sub_interfaces")
                .or_insert_with(|| Value::Array(Vec::new()))
                .as_array_mut()
                .expect("parser creates array");
            if children.last().and_then(|record| record.get("name")).and_then(Value::as_str)
                != Some(child_name)
            {
                children.push(json!({"name":child_name}));
            }
            children
                .last_mut()
                .and_then(Value::as_object_mut)
                .expect("parser creates child")
                .insert(key.into(), value);
        } else {
            object.insert(key.into(), value);
        }
    }
    for record in &mut interfaces {
        if let Some(object) = record.as_object_mut() {
            if let Some(children) = object.remove("_sub_interfaces") {
                if let Some(array) = children.as_array() {
                    for child in array {
                        let name =
                            child.get("name").and_then(Value::as_str).expect("parser child name");
                        let mut fields: Map<String, Value> =
                            child.as_object().expect("parser child object").clone();
                        fields.remove("name");
                        object.insert(name.into(), Value::Object(fields));
                    }
                }
            }
        }
    }
    Ok(interfaces)
}

fn parse_value(raw: &str) -> Value {
    if raw.len() >= 2
        && ((raw.starts_with('"') && raw.ends_with('"'))
            || (raw.starts_with('\'') && raw.ends_with('\'')))
    {
        return Value::String(raw[1..raw.len() - 1].replace("\\\"", "\""));
    }
    if raw.contains(',') {
        return Value::Array(
            raw.split(',').map(|item| Value::String(item.trim().to_string())).collect(),
        );
    }
    Value::String(raw.to_string())
}

fn section_name(line: &str, depth: usize, line_number: usize) -> Result<String, ApiError> {
    let name = line[depth..line.len() - depth].trim();
    if name.is_empty() || name.contains('[') || name.contains(']') {
        return Err(parse_error(line_number, "Invalid section name"));
    }
    Ok(name.to_string())
}

fn parse_error(line_number: usize, message: &str) -> ApiError {
    ApiError(
        StatusCode::UNPROCESSABLE_ENTITY,
        format!("Config line {}: {message}", line_number + 1),
    )
}
