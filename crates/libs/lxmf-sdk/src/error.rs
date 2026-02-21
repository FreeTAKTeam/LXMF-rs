use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use std::collections::BTreeMap;
use thiserror::Error;

pub mod code {
    pub const CAPABILITY_CONTRACT_INCOMPATIBLE: &str = "SDK_CAPABILITY_CONTRACT_INCOMPATIBLE";
    pub const CAPABILITY_DISABLED: &str = "SDK_CAPABILITY_DISABLED";
    pub const RUNTIME_INVALID_STATE: &str = "SDK_RUNTIME_INVALID_STATE";
    pub const RUNTIME_ALREADY_RUNNING_WITH_DIFFERENT_CONFIG: &str =
        "SDK_RUNTIME_ALREADY_RUNNING_WITH_DIFFERENT_CONFIG";
    pub const RUNTIME_ALREADY_TERMINAL: &str = "SDK_RUNTIME_ALREADY_TERMINAL";
    pub const RUNTIME_INVALID_CURSOR: &str = "SDK_RUNTIME_INVALID_CURSOR";
    pub const RUNTIME_CURSOR_EXPIRED: &str = "SDK_RUNTIME_CURSOR_EXPIRED";
    pub const RUNTIME_STREAM_DEGRADED: &str = "SDK_RUNTIME_STREAM_DEGRADED";
    pub const VALIDATION_IDEMPOTENCY_CONFLICT: &str = "SDK_VALIDATION_IDEMPOTENCY_CONFLICT";
    pub const VALIDATION_INVALID_ARGUMENT: &str = "SDK_VALIDATION_INVALID_ARGUMENT";
    pub const VALIDATION_CHECKSUM_MISMATCH: &str = "SDK_VALIDATION_CHECKSUM_MISMATCH";
    pub const VALIDATION_UNKNOWN_FIELD: &str = "SDK_VALIDATION_UNKNOWN_FIELD";
    pub const VALIDATION_MAX_POLL_EVENTS_EXCEEDED: &str = "SDK_VALIDATION_MAX_POLL_EVENTS_EXCEEDED";
    pub const VALIDATION_EVENT_TOO_LARGE: &str = "SDK_VALIDATION_EVENT_TOO_LARGE";
    pub const VALIDATION_BATCH_TOO_LARGE: &str = "SDK_VALIDATION_BATCH_TOO_LARGE";
    pub const VALIDATION_MAX_EXTENSION_KEYS_EXCEEDED: &str =
        "SDK_VALIDATION_MAX_EXTENSION_KEYS_EXCEEDED";
    pub const CONFIG_CONFLICT: &str = "SDK_CONFIG_CONFLICT";
    pub const CONFIG_UNKNOWN_KEY: &str = "SDK_CONFIG_UNKNOWN_KEY";
    pub const SECURITY_AUTH_REQUIRED: &str = "SDK_SECURITY_AUTH_REQUIRED";
    pub const SECURITY_AUTHZ_DENIED: &str = "SDK_SECURITY_AUTHZ_DENIED";
    pub const SECURITY_TOKEN_INVALID: &str = "SDK_SECURITY_TOKEN_INVALID";
    pub const SECURITY_TOKEN_REPLAYED: &str = "SDK_SECURITY_TOKEN_REPLAYED";
    pub const SECURITY_RATE_LIMITED: &str = "SDK_SECURITY_RATE_LIMITED";
    pub const SECURITY_REMOTE_BIND_DISALLOWED: &str = "SDK_SECURITY_REMOTE_BIND_DISALLOWED";
    pub const INTERNAL: &str = "SDK_INTERNAL_ERROR";
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "PascalCase")]
#[non_exhaustive]
pub enum ErrorCategory {
    Validation,
    Capability,
    Config,
    Policy,
    Transport,
    Storage,
    Crypto,
    Timeout,
    Runtime,
    Security,
    Internal,
}

pub type ErrorDetails = BTreeMap<String, JsonValue>;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Error)]
#[error("{machine_code}: {message}")]
#[non_exhaustive]
pub struct SdkError {
    pub machine_code: String,
    pub category: ErrorCategory,
    pub retryable: bool,
    pub is_user_actionable: bool,
    pub message: String,
    #[serde(default)]
    pub details: ErrorDetails,
    pub cause_code: Option<String>,
    #[serde(default)]
    pub extensions: BTreeMap<String, JsonValue>,
}

impl SdkError {
    pub fn new(
        machine_code: impl Into<String>,
        category: ErrorCategory,
        message: impl Into<String>,
    ) -> Self {
        Self {
            machine_code: machine_code.into(),
            category,
            retryable: false,
            is_user_actionable: false,
            message: message.into(),
            details: ErrorDetails::new(),
            cause_code: None,
            extensions: BTreeMap::new(),
        }
    }

    pub fn with_retryable(mut self, retryable: bool) -> Self {
        self.retryable = retryable;
        self
    }

    pub fn with_user_actionable(mut self, is_user_actionable: bool) -> Self {
        self.is_user_actionable = is_user_actionable;
        self
    }

    pub fn with_cause_code(mut self, cause_code: impl Into<String>) -> Self {
        self.cause_code = Some(cause_code.into());
        self
    }

    pub fn with_detail(mut self, key: impl Into<String>, value: JsonValue) -> Self {
        self.details.insert(key.into(), value);
        self
    }

    pub fn code(&self) -> &str {
        self.machine_code.as_str()
    }

    pub fn is_retryable(&self) -> bool {
        self.retryable
    }

    pub fn is_user_actionable(&self) -> bool {
        self.is_user_actionable
    }

    pub fn invalid_state(method: &'static str, state: &'static str) -> Self {
        Self::new(
            code::RUNTIME_INVALID_STATE,
            ErrorCategory::Runtime,
            format!("method '{method}' is not legal in state '{state}'"),
        )
        .with_user_actionable(true)
        .with_detail("method", JsonValue::String(method.to_owned()))
        .with_detail("state", JsonValue::String(state.to_owned()))
    }

    pub fn capability_disabled(capability_id: &str) -> Self {
        Self::new(
            code::CAPABILITY_DISABLED,
            ErrorCategory::Capability,
            format!("capability '{capability_id}' is not enabled"),
        )
        .with_user_actionable(true)
        .with_detail("capability_id", JsonValue::String(capability_id.to_owned()))
    }

    pub fn config_conflict(expected_revision: u64, observed_revision: u64) -> Self {
        Self::new(code::CONFIG_CONFLICT, ErrorCategory::Config, "configuration revision mismatch")
            .with_user_actionable(true)
            .with_detail("expected_revision", JsonValue::from(expected_revision))
            .with_detail("observed_revision", JsonValue::from(observed_revision))
    }
}
