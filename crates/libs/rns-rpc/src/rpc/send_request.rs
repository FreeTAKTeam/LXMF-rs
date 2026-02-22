use super::OutboundDeliveryOptions;
use serde::Deserialize;
use serde_json::Value as JsonValue;
use std::io::{Error, ErrorKind};

#[derive(Debug, Deserialize)]
struct SendMessageParams {
    id: String,
    source: String,
    destination: String,
    #[serde(default)]
    title: String,
    content: String,
    fields: Option<JsonValue>,
    #[serde(default)]
    source_private_key: Option<String>,
}

#[derive(Debug, Deserialize)]
struct SendMessageV2Params {
    id: String,
    source: String,
    destination: String,
    #[serde(default)]
    title: String,
    content: String,
    fields: Option<JsonValue>,
    #[serde(default)]
    method: Option<String>,
    #[serde(default)]
    stamp_cost: Option<u32>,
    #[serde(default)]
    include_ticket: Option<bool>,
    #[serde(default)]
    try_propagation_on_fail: Option<bool>,
    #[serde(default)]
    source_private_key: Option<String>,
}

#[derive(Debug)]
pub(super) struct NormalizedSendRequest {
    pub(super) id: String,
    pub(super) source: String,
    pub(super) destination: String,
    pub(super) title: String,
    pub(super) content: String,
    pub(super) fields: Option<JsonValue>,
    pub(super) method: Option<String>,
    pub(super) stamp_cost: Option<u32>,
    pub(super) options: OutboundDeliveryOptions,
    pub(super) include_ticket: Option<bool>,
}

pub(super) fn parse_outbound_send_request(
    method: &str,
    params: JsonValue,
) -> Result<NormalizedSendRequest, Error> {
    match method {
        "send_message" => {
            let parsed: SendMessageParams = serde_json::from_value(params)
                .map_err(|err| Error::new(ErrorKind::InvalidInput, err))?;
            validate_outbound_fields_strict(parsed.fields.as_ref())?;
            let options = OutboundDeliveryOptions {
                source_private_key: parsed.source_private_key,
                ..Default::default()
            };
            Ok(NormalizedSendRequest {
                id: parsed.id,
                source: parsed.source,
                destination: parsed.destination,
                title: parsed.title,
                content: parsed.content,
                fields: parsed.fields,
                method: None,
                stamp_cost: None,
                options,
                include_ticket: None,
            })
        }
        "send_message_v2" | "sdk_send_v2" => {
            let parsed: SendMessageV2Params = serde_json::from_value(params)
                .map_err(|err| Error::new(ErrorKind::InvalidInput, err))?;
            validate_outbound_fields_strict(parsed.fields.as_ref())?;
            let outbound_method = parsed.method.clone();
            let include_ticket = parsed.include_ticket;
            Ok(NormalizedSendRequest {
                id: parsed.id,
                source: parsed.source,
                destination: parsed.destination,
                title: parsed.title,
                content: parsed.content,
                fields: parsed.fields,
                method: outbound_method.clone(),
                stamp_cost: parsed.stamp_cost,
                options: OutboundDeliveryOptions {
                    method: outbound_method,
                    stamp_cost: parsed.stamp_cost,
                    include_ticket: include_ticket.unwrap_or_default(),
                    try_propagation_on_fail: parsed.try_propagation_on_fail.unwrap_or_default(),
                    ticket: None,
                    source_private_key: parsed.source_private_key,
                },
                include_ticket,
            })
        }
        _ => {
            Err(Error::new(ErrorKind::InvalidInput, format!("unsupported send method '{method}'")))
        }
    }
}

fn validate_outbound_fields_strict(fields: Option<&JsonValue>) -> Result<(), Error> {
    let Some(JsonValue::Object(map)) = fields else {
        return Ok(());
    };

    if map.contains_key("files") {
        return Err(Error::new(
            ErrorKind::InvalidInput,
            "legacy field 'files' is not allowed; use 'attachments'",
        ));
    }

    if map.contains_key("5") {
        return Err(Error::new(
            ErrorKind::InvalidInput,
            "public field '5' is not allowed; use 'attachments'",
        ));
    }

    let Some(attachments) = map.get("attachments") else {
        return Ok(());
    };

    let attachments = attachments.as_array().ok_or_else(|| {
        Error::new(
            ErrorKind::InvalidInput,
            "field 'attachments' must be an array of attachment objects",
        )
    })?;

    for entry in attachments {
        validate_attachment_entry(entry)?;
    }

    Ok(())
}

fn validate_attachment_entry(entry: &JsonValue) -> Result<(), Error> {
    let Some(map) = entry.as_object() else {
        return Err(Error::new(
            ErrorKind::InvalidInput,
            "attachments must be objects with canonical shape",
        ));
    };

    let name = map.get("name").and_then(JsonValue::as_str).map(str::trim).unwrap_or_default();
    if name.is_empty() {
        return Err(Error::new(
            ErrorKind::InvalidInput,
            "attachment entry must include string field 'name'",
        ));
    }

    let Some(data) = map.get("data") else {
        return Err(Error::new(
            ErrorKind::InvalidInput,
            "attachment entry must include field 'data'",
        ));
    };
    validate_attachment_data(data)
}

fn validate_attachment_data(value: &JsonValue) -> Result<(), Error> {
    match value {
        JsonValue::Array(items) => {
            for item in items {
                let valid = item
                    .as_u64()
                    .map(|value| value <= u8::MAX as u64)
                    .or_else(|| item.as_i64().map(|value| u8::try_from(value).is_ok()))
                    .unwrap_or(false);
                if !valid {
                    return Err(Error::new(
                        ErrorKind::InvalidInput,
                        "attachment data array must contain bytes between 0 and 255",
                    ));
                }
            }
            Ok(())
        }
        JsonValue::String(text) => validate_attachment_text_data(text),
        _ => Err(Error::new(
            ErrorKind::InvalidInput,
            "attachment data must be an array of bytes or prefixed text data",
        )),
    }
}

fn validate_attachment_text_data(text: &str) -> Result<(), Error> {
    let text = text.trim();
    if text.is_empty() {
        return Err(Error::new(ErrorKind::InvalidInput, "attachment data string cannot be empty"));
    }

    if let Some(payload) = text.strip_prefix("hex:").or_else(|| text.strip_prefix("HEX:")) {
        let payload = payload.trim();
        if payload.is_empty()
            || payload.len() % 2 != 0
            || !payload.chars().all(|ch| ch.is_ascii_hexdigit())
        {
            return Err(Error::new(
                ErrorKind::InvalidInput,
                "invalid hex attachment data after hex: prefix",
            ));
        }
        return Ok(());
    }

    if text.strip_prefix("base64:").or_else(|| text.strip_prefix("BASE64:")).is_some() {
        return Ok(());
    }

    Err(Error::new(
        ErrorKind::InvalidInput,
        "attachment text data must use explicit 'hex:' or 'base64:' prefix",
    ))
}
