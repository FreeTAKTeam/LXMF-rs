#[derive(Debug, Deserialize)]
struct PathLookupParams {
    #[serde(alias = "destination_hash", alias = "hash")]
    destination: String,
    #[serde(default)]
    timeout_secs: Option<u64>,
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
