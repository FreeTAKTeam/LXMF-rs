use super::config::{
    EventSinkKind, OverflowPolicy, RedactionTransform, StoreForwardCapacityPolicy,
    StoreForwardEvictionPriority,
};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use std::collections::BTreeMap;

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
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

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
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

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shared_secret: Option<Option<String>>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[non_exhaustive]
pub struct MtlsAuthPatch {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ca_bundle_path: Option<Option<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub require_client_cert: Option<Option<bool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allowed_san: Option<Option<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_cert_path: Option<Option<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub client_key_path: Option<Option<String>>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
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

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[non_exhaustive]
pub struct StoreForwardPatch {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_messages: Option<Option<usize>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_message_age_ms: Option<Option<u64>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub capacity_policy: Option<Option<StoreForwardCapacityPolicy>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub eviction_priority: Option<Option<StoreForwardEvictionPriority>>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[non_exhaustive]
pub struct EventSinkPatch {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enabled: Option<Option<bool>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_event_bytes: Option<Option<usize>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub allow_kinds: Option<Option<Vec<EventSinkKind>>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extensions: Option<Option<BTreeMap<String, JsonValue>>>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
#[non_exhaustive]
pub struct ConfigPatch {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub overflow_policy: Option<Option<OverflowPolicy>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub block_timeout_ms: Option<Option<u64>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub store_forward: Option<Option<StoreForwardPatch>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event_sink: Option<Option<EventSinkPatch>>,
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
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_overflow_policy(mut self, policy: OverflowPolicy) -> Self {
        self.overflow_policy = Some(Some(policy));
        self
    }

    pub fn with_block_timeout_ms(mut self, timeout_ms: u64) -> Self {
        self.block_timeout_ms = Some(Some(timeout_ms));
        self
    }

    pub fn with_store_forward_patch(mut self, patch: StoreForwardPatch) -> Self {
        self.store_forward = Some(Some(patch));
        self
    }

    pub fn with_event_sink_patch(mut self, patch: EventSinkPatch) -> Self {
        self.event_sink = Some(Some(patch));
        self
    }

    pub fn with_idempotency_ttl_ms(mut self, ttl_ms: u64) -> Self {
        self.idempotency_ttl_ms = Some(Some(ttl_ms));
        self
    }

    pub fn with_extension(mut self, key: impl Into<String>, value: JsonValue) -> Self {
        let mut extensions = self.extensions.unwrap_or(Some(BTreeMap::new())).unwrap_or_default();
        extensions.insert(key.into(), value);
        self.extensions = Some(Some(extensions));
        self
    }

    pub fn is_empty(&self) -> bool {
        self.overflow_policy.is_none()
            && self.block_timeout_ms.is_none()
            && self.store_forward.is_none()
            && self.event_sink.is_none()
            && self.event_stream.is_none()
            && self.idempotency_ttl_ms.is_none()
            && self.redaction.is_none()
            && self.rpc_backend.is_none()
            && self.extensions.is_none()
    }
}
