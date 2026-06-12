use super::*;

use base64::engine::general_purpose::{URL_SAFE, URL_SAFE_NO_PAD};

type InboundSdkCommandUpdate = (
    String,
    &'static str,
    Option<bool>,
    Option<JsonValue>,
    JsonMap<String, JsonValue>,
    Option<String>,
    Option<&'static str>,
);
