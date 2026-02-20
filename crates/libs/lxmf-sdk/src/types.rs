use crate::capability::EffectiveLimits;
use crate::error::{code, ErrorCategory, SdkError};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
#[non_exhaustive]
pub enum Profile {
    DesktopFull,
    DesktopLocalRuntime,
    EmbeddedAlloc,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum BindMode {
    LocalOnly,
    Remote,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum AuthMode {
    LocalTrusted,
    Token,
    Mtls,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum OverflowPolicy {
    Reject,
    DropOldest,
    Block,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[non_exhaustive]
pub struct EventStreamConfig {
    pub max_poll_events: usize,
    pub max_event_bytes: usize,
    pub max_batch_bytes: usize,
    pub max_extension_keys: usize,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum RedactionTransform {
    Hash,
    Truncate,
    Redact,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[non_exhaustive]
pub struct RedactionConfig {
    pub enabled: bool,
    pub sensitive_transform: RedactionTransform,
    pub break_glass_allowed: bool,
    pub break_glass_ttl_ms: Option<u64>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[non_exhaustive]
pub struct TokenAuthConfig {
    pub issuer: String,
    pub audience: String,
    pub jti_cache_ttl_ms: u64,
    pub clock_skew_ms: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[non_exhaustive]
pub struct MtlsAuthConfig {
    pub ca_bundle_path: String,
    pub require_client_cert: bool,
    pub allowed_san: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[non_exhaustive]
pub struct RpcBackendConfig {
    pub listen_addr: String,
    pub read_timeout_ms: u64,
    pub write_timeout_ms: u64,
    pub max_header_bytes: usize,
    pub max_body_bytes: usize,
    pub token_auth: Option<TokenAuthConfig>,
    pub mtls_auth: Option<MtlsAuthConfig>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[non_exhaustive]
pub struct SdkConfig {
    pub profile: Profile,
    pub bind_mode: BindMode,
    pub auth_mode: AuthMode,
    pub overflow_policy: OverflowPolicy,
    pub block_timeout_ms: Option<u64>,
    pub event_stream: EventStreamConfig,
    pub idempotency_ttl_ms: u64,
    pub redaction: RedactionConfig,
    pub rpc_backend: Option<RpcBackendConfig>,
    #[serde(default)]
    pub extensions: BTreeMap<String, JsonValue>,
}

impl SdkConfig {
    pub fn validate(&self) -> Result<(), SdkError> {
        if self.overflow_policy == OverflowPolicy::Block && self.block_timeout_ms.is_none() {
            return Err(SdkError::new(
                code::VALIDATION_INVALID_ARGUMENT,
                ErrorCategory::Validation,
                "overflow_policy=block requires block_timeout_ms",
            )
            .with_user_actionable(true)
            .with_detail("field", JsonValue::String("block_timeout_ms".to_owned())));
        }

        if self.event_stream.max_extension_keys > 32 {
            return Err(SdkError::new(
                code::VALIDATION_MAX_EXTENSION_KEYS_EXCEEDED,
                ErrorCategory::Validation,
                "event stream extension key limit exceeds contract maximum",
            )
            .with_user_actionable(true)
            .with_detail("limit_name", JsonValue::String("max_extension_keys".to_owned()))
            .with_detail("limit_value", JsonValue::from(self.event_stream.max_extension_keys)));
        }

        match self.bind_mode {
            BindMode::LocalOnly => {
                if self.auth_mode != AuthMode::LocalTrusted {
                    return Err(SdkError::new(
                        code::SECURITY_AUTH_REQUIRED,
                        ErrorCategory::Security,
                        "local_only bind mode requires local_trusted auth mode",
                    )
                    .with_user_actionable(true));
                }
            }
            BindMode::Remote => {
                if !matches!(self.auth_mode, AuthMode::Token | AuthMode::Mtls) {
                    return Err(SdkError::new(
                        code::SECURITY_REMOTE_BIND_DISALLOWED,
                        ErrorCategory::Security,
                        "remote bind mode requires token or mtls auth mode",
                    )
                    .with_user_actionable(true));
                }
            }
        }

        match self.auth_mode {
            AuthMode::LocalTrusted => {}
            AuthMode::Token => {
                let token_auth =
                    self.rpc_backend.as_ref().and_then(|backend| backend.token_auth.as_ref());
                if token_auth.is_none() {
                    return Err(SdkError::new(
                        code::SECURITY_AUTH_REQUIRED,
                        ErrorCategory::Security,
                        "token auth mode requires rpc_backend.token_auth configuration",
                    )
                    .with_user_actionable(true));
                }
            }
            AuthMode::Mtls => {
                let mtls_auth =
                    self.rpc_backend.as_ref().and_then(|backend| backend.mtls_auth.as_ref());
                if mtls_auth.is_none() {
                    return Err(SdkError::new(
                        code::SECURITY_AUTH_REQUIRED,
                        ErrorCategory::Security,
                        "mtls auth mode requires rpc_backend.mtls_auth configuration",
                    )
                    .with_user_actionable(true));
                }
            }
        }

        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[non_exhaustive]
pub struct EventStreamPatch {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_poll_events: Option<Option<usize>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_event_bytes: Option<Option<usize>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_batch_bytes: Option<Option<usize>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_extension_keys: Option<Option<usize>>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[non_exhaustive]
pub struct RedactionPatch {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<Option<bool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sensitive_transform: Option<Option<RedactionTransform>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub break_glass_allowed: Option<Option<bool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub break_glass_ttl_ms: Option<Option<u64>>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[non_exhaustive]
pub struct TokenAuthPatch {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub issuer: Option<Option<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audience: Option<Option<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub jti_cache_ttl_ms: Option<Option<u64>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub clock_skew_ms: Option<Option<u64>>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[non_exhaustive]
pub struct MtlsAuthPatch {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ca_bundle_path: Option<Option<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub require_client_cert: Option<Option<bool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allowed_san: Option<Option<String>>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[non_exhaustive]
pub struct RpcBackendPatch {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub listen_addr: Option<Option<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub read_timeout_ms: Option<Option<u64>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub write_timeout_ms: Option<Option<u64>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_header_bytes: Option<Option<usize>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_body_bytes: Option<Option<usize>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token_auth: Option<Option<TokenAuthPatch>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mtls_auth: Option<Option<MtlsAuthPatch>>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[non_exhaustive]
pub struct ConfigPatch {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub overflow_policy: Option<Option<OverflowPolicy>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub block_timeout_ms: Option<Option<u64>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event_stream: Option<Option<EventStreamPatch>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub idempotency_ttl_ms: Option<Option<u64>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub redaction: Option<Option<RedactionPatch>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rpc_backend: Option<Option<RpcBackendPatch>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extensions: Option<Option<BTreeMap<String, JsonValue>>>,
}

impl ConfigPatch {
    pub fn is_empty(&self) -> bool {
        self.overflow_policy.is_none()
            && self.block_timeout_ms.is_none()
            && self.event_stream.is_none()
            && self.idempotency_ttl_ms.is_none()
            && self.redaction.is_none()
            && self.rpc_backend.is_none()
            && self.extensions.is_none()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[non_exhaustive]
pub struct StartRequest {
    pub supported_contract_versions: Vec<u16>,
    pub requested_capabilities: Vec<String>,
    pub config: SdkConfig,
}

impl StartRequest {
    pub fn validate(&self) -> Result<(), SdkError> {
        if self.supported_contract_versions.is_empty() {
            return Err(SdkError::new(
                code::CAPABILITY_CONTRACT_INCOMPATIBLE,
                ErrorCategory::Capability,
                "supported_contract_versions must not be empty",
            )
            .with_user_actionable(true));
        }

        let mut seen_versions = BTreeSet::new();
        for version in &self.supported_contract_versions {
            if !seen_versions.insert(*version) {
                return Err(SdkError::new(
                    code::VALIDATION_INVALID_ARGUMENT,
                    ErrorCategory::Validation,
                    "supported_contract_versions must be unique",
                )
                .with_user_actionable(true));
            }
        }

        let mut seen_caps = BTreeSet::new();
        for capability in &self.requested_capabilities {
            let trimmed = capability.trim();
            if trimmed.is_empty() {
                return Err(SdkError::new(
                    code::VALIDATION_INVALID_ARGUMENT,
                    ErrorCategory::Validation,
                    "requested capability IDs must not be empty",
                )
                .with_user_actionable(true));
            }
            if !seen_caps.insert(trimmed.to_owned()) {
                return Err(SdkError::new(
                    code::VALIDATION_INVALID_ARGUMENT,
                    ErrorCategory::Validation,
                    "requested capability IDs must be unique",
                )
                .with_user_actionable(true));
            }
        }

        self.config.validate()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[non_exhaustive]
pub struct ClientHandle {
    pub runtime_id: String,
    pub active_contract_version: u16,
    pub effective_capabilities: Vec<String>,
    pub effective_limits: EffectiveLimits,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[non_exhaustive]
pub struct SendRequest {
    pub source: String,
    pub destination: String,
    pub payload: JsonValue,
    pub idempotency_key: Option<String>,
    pub ttl_ms: Option<u64>,
    pub correlation_id: Option<String>,
    #[serde(default)]
    pub extensions: BTreeMap<String, JsonValue>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct MessageId(pub String);

impl From<String> for MessageId {
    fn from(value: String) -> Self {
        Self(value)
    }
}

impl From<&str> for MessageId {
    fn from(value: &str) -> Self {
        Self(value.to_owned())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum DeliveryState {
    Queued,
    Dispatching,
    InFlight,
    Sent,
    Delivered,
    Failed,
    Cancelled,
    Expired,
    Rejected,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[non_exhaustive]
pub struct DeliverySnapshot {
    pub message_id: MessageId,
    pub state: DeliveryState,
    pub terminal: bool,
    pub last_updated_ms: u64,
    pub attempts: u32,
    pub reason_code: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[non_exhaustive]
pub struct Ack {
    pub accepted: bool,
    pub revision: Option<u64>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum RuntimeState {
    New,
    Starting,
    Running,
    Draining,
    Stopped,
    Failed,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[non_exhaustive]
pub struct RuntimeSnapshot {
    pub runtime_id: String,
    pub state: RuntimeState,
    pub active_contract_version: u16,
    pub event_stream_position: u64,
    pub config_revision: u64,
    pub queued_messages: u64,
    pub in_flight_messages: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ShutdownMode {
    Graceful,
    Immediate,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[non_exhaustive]
pub struct TickBudget {
    pub max_work_items: usize,
    pub max_duration_ms: Option<u64>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[non_exhaustive]
pub struct TickResult {
    pub processed_items: usize,
    pub yielded: bool,
    pub next_recommended_delay_ms: Option<u64>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum CancelResult {
    Accepted,
    AlreadyTerminal,
    NotFound,
    TooLateToCancel,
    Unsupported,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_config() -> SdkConfig {
        SdkConfig {
            profile: Profile::DesktopFull,
            bind_mode: BindMode::LocalOnly,
            auth_mode: AuthMode::LocalTrusted,
            overflow_policy: OverflowPolicy::Reject,
            block_timeout_ms: None,
            event_stream: EventStreamConfig {
                max_poll_events: 128,
                max_event_bytes: 32_768,
                max_batch_bytes: 1_048_576,
                max_extension_keys: 32,
            },
            idempotency_ttl_ms: 86_400_000,
            redaction: RedactionConfig {
                enabled: true,
                sensitive_transform: RedactionTransform::Hash,
                break_glass_allowed: false,
                break_glass_ttl_ms: None,
            },
            rpc_backend: None,
            extensions: BTreeMap::new(),
        }
    }

    #[test]
    fn start_request_rejects_duplicate_contract_versions() {
        let request = StartRequest {
            supported_contract_versions: vec![2, 2],
            requested_capabilities: vec!["sdk.capability.cursor_replay".to_owned()],
            config: base_config(),
        };
        let err = request.validate().expect_err("duplicate versions must fail");
        assert_eq!(err.machine_code, code::VALIDATION_INVALID_ARGUMENT);
    }

    #[test]
    fn config_rejects_remote_bind_without_token_or_mtls() {
        let mut config = base_config();
        config.bind_mode = BindMode::Remote;
        config.auth_mode = AuthMode::LocalTrusted;
        let err = config.validate().expect_err("remote bind requires explicit secure auth");
        assert_eq!(err.machine_code, code::SECURITY_REMOTE_BIND_DISALLOWED);
    }

    #[test]
    fn config_accepts_local_trusted_profile() {
        let config = base_config();
        assert!(config.validate().is_ok());
    }

    #[test]
    fn config_patch_serialization_preserves_absent_vs_null() {
        let absent_patch = ConfigPatch {
            overflow_policy: None,
            block_timeout_ms: None,
            event_stream: None,
            idempotency_ttl_ms: None,
            redaction: None,
            rpc_backend: None,
            extensions: None,
        };
        let absent_json = serde_json::to_value(&absent_patch).expect("serialize absent patch");
        assert!(!absent_json.as_object().expect("object").contains_key("overflow_policy"));

        let clear_patch = ConfigPatch {
            overflow_policy: Some(None),
            block_timeout_ms: None,
            event_stream: None,
            idempotency_ttl_ms: None,
            redaction: None,
            rpc_backend: None,
            extensions: None,
        };
        let clear_json = serde_json::to_value(&clear_patch).expect("serialize clear patch");
        assert!(clear_json["overflow_policy"].is_null());
    }
}
