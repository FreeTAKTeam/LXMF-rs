pub(super) const STORE_FORWARD_MAX_MESSAGES_LIMIT: usize = 1_000_000;

pub(super) const EVENT_SINK_MAX_EVENT_BYTES_LIMIT: u64 = 2_097_152;

#[derive(Clone, Debug)]
pub(super) struct SdkStoreForwardPolicy {
    pub(super) max_messages: usize,
    pub(super) max_message_age_ms: u64,
    pub(super) capacity_policy: String,
    pub(super) eviction_priority: String,
}

use super::*;

pub(super) const SDK_VERSION: &str = "0.2.1";

pub(super) fn python_reference_meta() -> JsonValue {
    json!({
        "reticulum_conformance_ref": crate::RETICULUM_CONFORMANCE_REFERENCE_REF,
        "python_reticulum_version": crate::PYTHON_RETICULUM_REFERENCE_VERSION,
        "python_reticulum_ref": crate::PYTHON_RETICULUM_REFERENCE_REF,
        "python_lxmf_version": crate::PYTHON_LXMF_REFERENCE_VERSION,
        "python_lxmf_ref": crate::PYTHON_LXMF_REFERENCE_REF,
    })
}
