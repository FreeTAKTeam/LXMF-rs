use crate::capability::EffectiveLimits;
use crate::types::Profile;

const CAP_CURSOR_REPLAY: &str = "sdk.capability.cursor_replay";
const CAP_ASYNC_EVENTS: &str = "sdk.capability.async_events";
const CAP_MANUAL_TICK: &str = "sdk.capability.manual_tick";
const CAP_TOKEN_AUTH: &str = "sdk.capability.token_auth";
const CAP_MTLS_AUTH: &str = "sdk.capability.mtls_auth";
const CAP_RECEIPT_TERMINALITY: &str = "sdk.capability.receipt_terminality";
const CAP_CONFIG_REVISION_CAS: &str = "sdk.capability.config_revision_cas";
const CAP_IDEMPOTENCY_TTL: &str = "sdk.capability.idempotency_ttl";
const CAP_TOPICS: &str = "sdk.capability.topics";
const CAP_TOPIC_SUBSCRIPTIONS: &str = "sdk.capability.topic_subscriptions";
const CAP_TOPIC_FANOUT: &str = "sdk.capability.topic_fanout";
const CAP_TELEMETRY_QUERY: &str = "sdk.capability.telemetry_query";
const CAP_TELEMETRY_STREAM: &str = "sdk.capability.telemetry_stream";
const CAP_ATTACHMENTS: &str = "sdk.capability.attachments";
const CAP_ATTACHMENT_DELETE: &str = "sdk.capability.attachment_delete";
const CAP_MARKERS: &str = "sdk.capability.markers";
const CAP_IDENTITY_MULTI: &str = "sdk.capability.identity_multi";
const CAP_IDENTITY_IMPORT_EXPORT: &str = "sdk.capability.identity_import_export";
const CAP_IDENTITY_HASH_RESOLUTION: &str = "sdk.capability.identity_hash_resolution";
const CAP_PAPER_MESSAGES: &str = "sdk.capability.paper_messages";
const CAP_REMOTE_COMMANDS: &str = "sdk.capability.remote_commands";
const CAP_VOICE_SIGNALING: &str = "sdk.capability.voice_signaling";
const CAP_SHARED_INSTANCE_RPC_AUTH: &str = "sdk.capability.shared_instance_rpc_auth";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemoryBudget {
    pub max_heap_bytes: usize,
    pub max_event_queue_bytes: usize,
    pub max_attachment_spool_bytes: usize,
}

const DESKTOP_FULL_REQUIRED: &[&str] = &[
    CAP_CURSOR_REPLAY,
    CAP_ASYNC_EVENTS,
    CAP_RECEIPT_TERMINALITY,
    CAP_CONFIG_REVISION_CAS,
    CAP_IDEMPOTENCY_TTL,
];

const DESKTOP_LOCAL_RUNTIME_REQUIRED: &[&str] =
    &[CAP_CURSOR_REPLAY, CAP_RECEIPT_TERMINALITY, CAP_CONFIG_REVISION_CAS, CAP_IDEMPOTENCY_TTL];

const EMBEDDED_ALLOC_REQUIRED: &[&str] =
    &[CAP_MANUAL_TICK, CAP_CONFIG_REVISION_CAS, CAP_IDEMPOTENCY_TTL];

const DESKTOP_FULL_SUPPORTED: &[&str] = &[
    CAP_CURSOR_REPLAY,
    CAP_ASYNC_EVENTS,
    CAP_MANUAL_TICK,
    CAP_TOKEN_AUTH,
    CAP_MTLS_AUTH,
    CAP_RECEIPT_TERMINALITY,
    CAP_CONFIG_REVISION_CAS,
    CAP_IDEMPOTENCY_TTL,
    CAP_TOPICS,
    CAP_TOPIC_SUBSCRIPTIONS,
    CAP_TOPIC_FANOUT,
    CAP_TELEMETRY_QUERY,
    CAP_TELEMETRY_STREAM,
    CAP_ATTACHMENTS,
    CAP_ATTACHMENT_DELETE,
    CAP_MARKERS,
    CAP_IDENTITY_MULTI,
    CAP_IDENTITY_IMPORT_EXPORT,
    CAP_IDENTITY_HASH_RESOLUTION,
    CAP_PAPER_MESSAGES,
    CAP_REMOTE_COMMANDS,
    CAP_VOICE_SIGNALING,
    CAP_SHARED_INSTANCE_RPC_AUTH,
];

const DESKTOP_LOCAL_RUNTIME_SUPPORTED: &[&str] = &[
    CAP_CURSOR_REPLAY,
    CAP_ASYNC_EVENTS,
    CAP_MANUAL_TICK,
    CAP_TOKEN_AUTH,
    CAP_MTLS_AUTH,
    CAP_RECEIPT_TERMINALITY,
    CAP_CONFIG_REVISION_CAS,
    CAP_IDEMPOTENCY_TTL,
    CAP_TOPICS,
    CAP_TOPIC_SUBSCRIPTIONS,
    CAP_TOPIC_FANOUT,
    CAP_TELEMETRY_QUERY,
    CAP_TELEMETRY_STREAM,
    CAP_ATTACHMENTS,
    CAP_ATTACHMENT_DELETE,
    CAP_MARKERS,
    CAP_IDENTITY_MULTI,
    CAP_IDENTITY_IMPORT_EXPORT,
    CAP_IDENTITY_HASH_RESOLUTION,
    CAP_PAPER_MESSAGES,
    CAP_REMOTE_COMMANDS,
    CAP_VOICE_SIGNALING,
    CAP_SHARED_INSTANCE_RPC_AUTH,
];

const EMBEDDED_ALLOC_SUPPORTED: &[&str] = &[
    CAP_CURSOR_REPLAY,
    CAP_MANUAL_TICK,
    CAP_TOKEN_AUTH,
    CAP_RECEIPT_TERMINALITY,
    CAP_CONFIG_REVISION_CAS,
    CAP_IDEMPOTENCY_TTL,
    CAP_TOPICS,
    CAP_TOPIC_SUBSCRIPTIONS,
    CAP_TOPIC_FANOUT,
    CAP_TELEMETRY_QUERY,
    CAP_TELEMETRY_STREAM,
    CAP_ATTACHMENTS,
    CAP_ATTACHMENT_DELETE,
    CAP_MARKERS,
    CAP_IDENTITY_MULTI,
    CAP_IDENTITY_IMPORT_EXPORT,
    CAP_IDENTITY_HASH_RESOLUTION,
    CAP_PAPER_MESSAGES,
    CAP_REMOTE_COMMANDS,
    CAP_VOICE_SIGNALING,
    CAP_SHARED_INSTANCE_RPC_AUTH,
];

pub fn default_effective_limits(profile: Profile) -> EffectiveLimits {
    match profile {
        Profile::DesktopFull => EffectiveLimits {
            max_poll_events: 256,
            max_event_bytes: 65_536,
            max_batch_bytes: 1_048_576,
            max_extension_keys: 32,
            idempotency_ttl_ms: 86_400_000,
        },
        Profile::DesktopLocalRuntime => EffectiveLimits {
            max_poll_events: 64,
            max_event_bytes: 32_768,
            max_batch_bytes: 1_048_576,
            max_extension_keys: 32,
            idempotency_ttl_ms: 43_200_000,
        },
        Profile::EmbeddedAlloc => EffectiveLimits {
            max_poll_events: 32,
            max_event_bytes: 8_192,
            max_batch_bytes: 262_144,
            max_extension_keys: 32,
            idempotency_ttl_ms: 7_200_000,
        },
    }
}

pub fn default_memory_budget(profile: Profile) -> MemoryBudget {
    match profile {
        Profile::DesktopFull => MemoryBudget {
            max_heap_bytes: 268_435_456,
            max_event_queue_bytes: 67_108_864,
            max_attachment_spool_bytes: 536_870_912,
        },
        Profile::DesktopLocalRuntime => MemoryBudget {
            max_heap_bytes: 134_217_728,
            max_event_queue_bytes: 33_554_432,
            max_attachment_spool_bytes: 268_435_456,
        },
        Profile::EmbeddedAlloc => MemoryBudget {
            max_heap_bytes: 8_388_608,
            max_event_queue_bytes: 2_097_152,
            max_attachment_spool_bytes: 16_777_216,
        },
    }
}

pub fn required_capabilities(profile: Profile) -> &'static [&'static str] {
    match profile {
        Profile::DesktopFull => DESKTOP_FULL_REQUIRED,
        Profile::DesktopLocalRuntime => DESKTOP_LOCAL_RUNTIME_REQUIRED,
        Profile::EmbeddedAlloc => EMBEDDED_ALLOC_REQUIRED,
    }
}

pub fn supports_capability(profile: Profile, capability_id: &str) -> bool {
    let supported = match profile {
        Profile::DesktopFull => DESKTOP_FULL_SUPPORTED,
        Profile::DesktopLocalRuntime => DESKTOP_LOCAL_RUNTIME_SUPPORTED,
        Profile::EmbeddedAlloc => EMBEDDED_ALLOC_SUPPORTED,
    };
    supported.contains(&capability_id)
}

pub fn is_profile_method_required(profile: Profile, method: &str) -> bool {
    match profile {
        Profile::DesktopFull => !matches!(method, "tick"),
        Profile::DesktopLocalRuntime => !matches!(method, "tick" | "subscribe_events"),
        Profile::EmbeddedAlloc => !matches!(method, "subscribe_events"),
    }
}

pub fn is_profile_method_supported(profile: Profile, method: &str) -> bool {
    match profile {
        Profile::DesktopFull => true,
        Profile::DesktopLocalRuntime => true,
        Profile::EmbeddedAlloc => method != "subscribe_events",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn embedded_alloc_limits_are_constrained() {
        let limits = default_effective_limits(Profile::EmbeddedAlloc);
        assert_eq!(limits.max_poll_events, 32);
        assert_eq!(limits.max_event_bytes, 8_192);
        let memory = default_memory_budget(Profile::EmbeddedAlloc);
        assert_eq!(memory.max_heap_bytes, 8_388_608);
        assert_eq!(memory.max_event_queue_bytes, 2_097_152);
    }

    #[test]
    fn embedded_alloc_requires_manual_tick() {
        assert!(required_capabilities(Profile::EmbeddedAlloc).contains(&CAP_MANUAL_TICK));
    }

    #[test]
    fn unknown_capability_is_not_supported() {
        assert!(!supports_capability(Profile::DesktopFull, "sdk.capability.unknown"));
        assert!(!supports_capability(Profile::DesktopLocalRuntime, "sdk.capability.unknown"));
        assert!(!supports_capability(Profile::EmbeddedAlloc, "sdk.capability.unknown"));
    }

    #[test]
    fn method_support_matrix_matches_contract() {
        assert!(is_profile_method_supported(Profile::DesktopFull, "subscribe_events"));
        assert!(is_profile_method_supported(Profile::DesktopLocalRuntime, "subscribe_events"));
        assert!(!is_profile_method_supported(Profile::EmbeddedAlloc, "subscribe_events"));
        assert!(is_profile_method_supported(Profile::DesktopFull, "tick"));
        assert!(is_profile_method_supported(Profile::DesktopLocalRuntime, "tick"));
        assert!(is_profile_method_supported(Profile::EmbeddedAlloc, "tick"));
    }

    #[test]
    fn memory_budgets_reduce_for_more_constrained_profiles() {
        let full = default_memory_budget(Profile::DesktopFull);
        let local = default_memory_budget(Profile::DesktopLocalRuntime);
        let embedded = default_memory_budget(Profile::EmbeddedAlloc);
        assert!(local.max_heap_bytes <= full.max_heap_bytes);
        assert!(embedded.max_heap_bytes <= local.max_heap_bytes);
        assert!(local.max_event_queue_bytes <= full.max_event_queue_bytes);
        assert!(embedded.max_event_queue_bytes <= local.max_event_queue_bytes);
        assert!(local.max_attachment_spool_bytes <= full.max_attachment_spool_bytes);
        assert!(embedded.max_attachment_spool_bytes <= local.max_attachment_spool_bytes);
    }
}
