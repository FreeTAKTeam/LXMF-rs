use crate::error::{ErrorCategory, SdkError};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExecutionCertainty {
    NotSubmitted,
    Rejected,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ZmqRecoveryDecision {
    RetryWithinBudget,
    ReconcileOperation,
    RestoreSession,
    CredentialsRequired,
    StorageBackpressure,
    Stop,
}

impl ZmqRecoveryDecision {
    /// Session restoration is deliberately narrower than socket recovery.
    /// A transient transport failure never authorizes importing an identity again.
    pub fn for_error(error: &SdkError, replay_safe: bool) -> Self {
        match error.machine_code.as_str() {
            "SDK_BROKER_SESSION_REQUIRED"
            | "SDK_BROKER_NEGOTIATION_REQUIRED"
            | "SDK_RUNTIME_IDENTITY_NOT_FOUND" => Self::RestoreSession,
            "SDK_SECURITY_TOKEN_INVALID"
            | "SDK_SECURITY_AUTH_REQUIRED"
            | "SDK_SECURITY_AUTHZ_DENIED"
            | "SDK_SECURITY_IDENTITY_FORBIDDEN" => Self::CredentialsRequired,
            "SDK_STORAGE_BUSY"
            | "SDK_STORAGE_FULL"
            | "SDK_STORAGE_BROKER_FULL"
            | "SDK_STORAGE_WRITE_BUSY"
            | "SDK_TRANSPORT_ZMQ_BUSY" => Self::StorageBackpressure,
            _ if matches!(error.category, ErrorCategory::Transport | ErrorCategory::Timeout) => {
                if replay_safe {
                    Self::RetryWithinBudget
                } else {
                    Self::ReconcileOperation
                }
            }
            _ => Self::Stop,
        }
    }
}

pub(super) fn replay_safe(method: &str) -> bool {
    // These methods have durable fixed-range/idempotent contracts. Ordinary
    // send/config/identity mutations are intentionally excluded.
    matches!(
        method,
        "sdk_broker_announces_v1"
            | "sdk_broker_resume_v1"
            | "sdk_broker_fetch_v1"
            | "sdk_broker_ack_stored_v1"
            | "sdk_broker_reconcile_v1"
    )
}

pub(super) fn explicitly_rejected(error: &SdkError) -> bool {
    matches!(
        error.machine_code.as_str(),
        "SDK_STORAGE_BUSY"
            | "SDK_STORAGE_FULL"
            | "SDK_STORAGE_WRITE_BUSY"
            | "SDK_STORAGE_BROKER_FULL"
            | "SDK_TRANSPORT_ZMQ_BUSY"
            | "SDK_SECURITY_AUTH_REQUIRED"
            | "SDK_SECURITY_TOKEN_INVALID"
            | "SDK_SECURITY_AUTHZ_DENIED"
            | "SDK_SECURITY_IDENTITY_FORBIDDEN"
            | "SDK_VALIDATION_INVALID_ARGUMENT"
            | "SDK_VALIDATION_IDEMPOTENCY_CONFLICT"
            | "SDK_CAPABILITY_DISABLED"
            | "SDK_BROKER_NEGOTIATION_REQUIRED"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transient_and_permission_errors_never_restore_identity() {
        let timeout = SdkError::new("SDK_TRANSPORT_ZMQ_TIMEOUT", ErrorCategory::Timeout, "timeout");
        assert_eq!(
            ZmqRecoveryDecision::for_error(&timeout, true),
            ZmqRecoveryDecision::RetryWithinBudget
        );
        assert_eq!(
            ZmqRecoveryDecision::for_error(&timeout, false),
            ZmqRecoveryDecision::ReconcileOperation
        );
        let denied =
            SdkError::new("SDK_SECURITY_IDENTITY_FORBIDDEN", ErrorCategory::Security, "denied");
        assert_eq!(
            ZmqRecoveryDecision::for_error(&denied, true),
            ZmqRecoveryDecision::CredentialsRequired
        );
        assert!(!replay_safe("sdk_send_v2"));
        assert!(!replay_safe("sdk_identity_import_v2"));
    }
}
