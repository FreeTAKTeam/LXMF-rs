#[derive(Debug)]
struct SdkCursorError {
    code: String,
    message: String,
}

fn parse_announce_cursor(cursor: Option<&str>) -> Option<(Option<i64>, Option<String>)> {
    let raw = cursor?.trim();
    if raw.is_empty() {
        return None;
    }
    if let Some((timestamp_raw, id)) = raw.split_once(':') {
        let timestamp = timestamp_raw.parse::<i64>().ok()?;
        let before_id = if id.is_empty() { None } else { Some(id.to_string()) };
        return Some((Some(timestamp), before_id));
    }
    raw.parse::<i64>().ok().map(|timestamp| (Some(timestamp), None))
}

fn delivery_reason_code(status: &str) -> Option<&'static str> {
    let normalized = status.trim().to_ascii_lowercase();
    if normalized.is_empty() {
        return None;
    }
    if normalized.contains("receipt timeout") {
        return Some("receipt_timeout");
    }
    if normalized.contains("timeout") {
        return Some("timeout");
    }
    if normalized.contains("no route")
        || normalized.contains("no path")
        || normalized.contains("no known path")
    {
        return Some("no_path");
    }
    if normalized.contains("no propagation relay selected") {
        return Some("relay_unset");
    }
    if normalized.contains("retry budget exhausted") {
        return Some("retry_budget_exhausted");
    }
    None
}

fn merge_json_patch(target: &mut JsonValue, patch: &JsonValue) {
    let JsonValue::Object(patch_map) = patch else {
        *target = patch.clone();
        return;
    };

    if !target.is_object() {
        *target = JsonValue::Object(JsonMap::new());
    }
    let target_map = target.as_object_mut().expect("target must be object after initialization");
    for (key, value) in patch_map {
        if value.is_null() {
            target_map.remove(key);
            continue;
        }
        match target_map.get_mut(key) {
            Some(existing) if existing.is_object() && value.is_object() => {
                merge_json_patch(existing, value);
            }
            _ => {
                target_map.insert(key.clone(), value.clone());
            }
        }
    }
}

