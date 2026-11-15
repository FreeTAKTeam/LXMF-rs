use serde_json::{json, Value};

pub(super) fn apply(message: &mut Value, fields: Option<&Value>) {
    let Some(fields) = fields.and_then(Value::as_object) else { return };
    let Some(history) = fields.get("_meshchat_import").and_then(Value::as_object) else { return };
    for key in [
        "state",
        "progress",
        "method",
        "timestamp",
        "delivery_attempts",
        "rssi",
        "snr",
        "quality",
        "created_at",
        "updated_at",
    ] {
        if let Some(value) = history.get(key) {
            message[key] = value.clone();
        }
    }
    let mut fields = fields.clone();
    fields.remove("_meshchat_import");
    message["fields"] = Value::Object(fields);
    message["imported"] = json!(true);
}
