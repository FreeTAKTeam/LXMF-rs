#[derive(Debug, Deserialize)]
struct PathLookupParams {
    #[serde(alias = "destination_hash", alias = "hash")]
    destination: String,
    #[serde(default)]
    timeout_secs: Option<u64>,
    #[serde(default, alias = "interface")]
    on_iface: Option<String>,
    #[serde(default, alias = "tag")]
    tag_hex: Option<String>,
}

fn normalize_destination_hash_param(destination: &str) -> Result<String, std::io::Error> {
    let destination = destination.trim();
    let decoded = hex::decode(destination).map_err(|err| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("destination must be hex-encoded: {err}"),
        )
    })?;
    if decoded.len() != 16 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "destination must decode to a 16-byte RNS destination hash",
        ));
    }
    Ok(destination.to_ascii_lowercase())
}

fn normalize_optional_iface_hash_param(
    on_iface: Option<&str>,
) -> Result<Option<String>, std::io::Error> {
    let Some(on_iface) = on_iface else {
        return Ok(None);
    };
    let on_iface = on_iface.trim();
    if on_iface.is_empty() {
        return Ok(None);
    }
    normalize_destination_hash_param(on_iface)
        .map(Some)
        .map_err(|err| std::io::Error::new(err.kind(), format!("on_iface {err}")))
}

fn normalize_optional_tag_hex_param(tag_hex: Option<&str>) -> Result<Option<Vec<u8>>, std::io::Error> {
    let Some(tag_hex) = tag_hex else {
        return Ok(None);
    };
    let tag_hex = tag_hex.trim();
    if tag_hex.is_empty() {
        return Ok(None);
    }
    let tag = hex::decode(tag_hex).map_err(|err| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("tag_hex must be hex-encoded: {err}"),
        )
    })?;
    if tag.is_empty() || tag.len() > 16 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "tag_hex must decode to 1..=16 bytes",
        ));
    }
    Ok(Some(tag))
}

fn path_found_from_status(status_fields: &JsonValue) -> bool {
    status_fields
        .get("path_found")
        .and_then(JsonValue::as_bool)
        .or_else(|| status_fields.get("known").and_then(JsonValue::as_bool))
        .unwrap_or(false)
}

fn path_lookup_result(
    destination: String,
    status_fields: JsonValue,
    requested: Option<bool>,
    missing_status: &str,
) -> JsonValue {
    let mut object = match status_fields {
        JsonValue::Object(object) => object,
        _ => JsonMap::new(),
    };
    let path_found = object
        .get("path_found")
        .and_then(JsonValue::as_bool)
        .unwrap_or_else(|| object.get("known").and_then(JsonValue::as_bool).unwrap_or(false));

    object.insert("destination".to_string(), json!(destination.clone()));
    object.insert("destination_hash".to_string(), json!(destination));
    object.entry("known".to_string()).or_insert_with(|| json!(path_found));
    object.entry("path_found".to_string()).or_insert_with(|| json!(path_found));
    if let Some(requested) = requested {
        object.insert("requested".to_string(), json!(requested));
    }
    object
        .entry("status".to_string())
        .or_insert_with(|| json!(if path_found { "found" } else { missing_status }));
    JsonValue::Object(object)
}

fn add_path_request_scope_fields(
    result: &mut JsonValue,
    on_iface: Option<&str>,
    tag: Option<&[u8]>,
) {
    let JsonValue::Object(object) = result else {
        return;
    };
    if let Some(on_iface) = on_iface {
        object.insert("on_iface".to_string(), json!(on_iface));
        object.insert("interface_scope".to_string(), json!(on_iface));
    }
    if let Some(tag) = tag {
        object.insert("tag_hex".to_string(), json!(hex::encode(tag)));
    }
}
