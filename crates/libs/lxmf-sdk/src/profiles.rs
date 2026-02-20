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
];

const EMBEDDED_ALLOC_SUPPORTED: &[&str] = &[
    CAP_CURSOR_REPLAY,
    CAP_MANUAL_TICK,
    CAP_TOKEN_AUTH,
    CAP_RECEIPT_TERMINALITY,
    CAP_CONFIG_REVISION_CAS,
    CAP_IDEMPOTENCY_TTL,
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
}
