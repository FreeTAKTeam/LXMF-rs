use crate::error::{code, ErrorCategory, SdkError};

use serde::{Deserialize, Serialize};

use serde_json::Value as JsonValue;

use std::collections::BTreeMap;

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
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum StoreForwardCapacityPolicy {
    RejectNew,
    DropOldest,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum StoreForwardEvictionPriority {
    OldestFirst,
    TerminalFirst,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[non_exhaustive]
pub struct StoreForwardConfig {
    pub max_messages: usize,
    pub max_message_age_ms: u64,
    pub capacity_policy: StoreForwardCapacityPolicy,
    pub eviction_priority: StoreForwardEvictionPriority,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum EventSinkKind {
    Webhook,
    Mqtt,
    Custom,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[non_exhaustive]
pub struct EventSinkConfig {
    pub enabled: bool,
    pub max_event_bytes: usize,
    pub allow_kinds: Vec<EventSinkKind>,
    #[serde(default)]
    pub extensions: BTreeMap<String, JsonValue>,
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
    pub shared_secret: String,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[non_exhaustive]
pub struct MtlsAuthConfig {
    pub ca_bundle_path: String,
    pub require_client_cert: bool,
    pub allowed_san: Option<String>,
    pub client_cert_path: Option<String>,
    pub client_key_path: Option<String>,
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
    #[serde(default = "default_store_forward_for_deser")]
    pub store_forward: StoreForwardConfig,
    pub event_stream: EventStreamConfig,
    #[serde(default = "default_event_sink_for_deser")]
    pub event_sink: EventSinkConfig,
    pub idempotency_ttl_ms: u64,
    pub redaction: RedactionConfig,
    pub rpc_backend: Option<RpcBackendConfig>,
    #[serde(default)]
    pub extensions: BTreeMap<String, JsonValue>,
}

const DEFAULT_RPC_LISTEN_ADDR: &str = "unix:/tmp/lxmf-rpc.sock";

fn default_event_stream(profile: &Profile) -> EventStreamConfig {
    match profile {
        Profile::DesktopFull => EventStreamConfig {
            max_poll_events: 256,
            max_event_bytes: 65_536,
            max_batch_bytes: 1_048_576,
            max_extension_keys: 32,
        },
        Profile::DesktopLocalRuntime => EventStreamConfig {
            max_poll_events: 64,
            max_event_bytes: 32_768,
            max_batch_bytes: 1_048_576,
            max_extension_keys: 32,
        },
        Profile::EmbeddedAlloc => EventStreamConfig {
            max_poll_events: 32,
            max_event_bytes: 8_192,
            max_batch_bytes: 262_144,
            max_extension_keys: 32,
        },
    }
}

fn default_redaction() -> RedactionConfig {
    RedactionConfig {
        enabled: true,
        sensitive_transform: RedactionTransform::Hash,
        break_glass_allowed: false,
        break_glass_ttl_ms: None,
    }
}

fn default_rpc_backend(listen_addr: impl Into<String>) -> RpcBackendConfig {
    RpcBackendConfig {
        listen_addr: listen_addr.into(),
        read_timeout_ms: 5_000,
        write_timeout_ms: 5_000,
        max_header_bytes: 16_384,
        max_body_bytes: 1_048_576,
        token_auth: None,
        mtls_auth: None,
    }
}

fn default_store_forward(profile: &Profile) -> StoreForwardConfig {
    match profile {
        Profile::DesktopFull | Profile::DesktopLocalRuntime => StoreForwardConfig {
            max_messages: 50_000,
            max_message_age_ms: 604_800_000,
            capacity_policy: StoreForwardCapacityPolicy::DropOldest,
            eviction_priority: StoreForwardEvictionPriority::TerminalFirst,
        },
        Profile::EmbeddedAlloc => StoreForwardConfig {
            max_messages: 2_000,
            max_message_age_ms: 86_400_000,
            capacity_policy: StoreForwardCapacityPolicy::DropOldest,
            eviction_priority: StoreForwardEvictionPriority::TerminalFirst,
        },
    }
}

fn default_store_forward_for_deser() -> StoreForwardConfig {
    default_store_forward(&Profile::DesktopFull)
}

fn default_event_sink(profile: &Profile) -> EventSinkConfig {
    let max_event_bytes = match profile {
        Profile::DesktopFull => 65_536,
        Profile::DesktopLocalRuntime => 32_768,
        Profile::EmbeddedAlloc => 8_192,
    };
    EventSinkConfig {
        enabled: false,
        max_event_bytes,
        allow_kinds: vec![EventSinkKind::Webhook, EventSinkKind::Mqtt, EventSinkKind::Custom],
        extensions: BTreeMap::new(),
    }
}

fn default_event_sink_for_deser() -> EventSinkConfig {
    default_event_sink(&Profile::DesktopFull)
}
