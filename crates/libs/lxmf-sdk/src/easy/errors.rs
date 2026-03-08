use crate::error::{code, ErrorCategory, SdkError};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use std::collections::BTreeMap;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "PascalCase")]
#[non_exhaustive]
pub enum EasyErrorCategory {
    Validation,
    Capability,
    Config,
    Policy,
    Delivery,
    Connectivity,
    Persistence,
    Security,
    Timeout,
    Runtime,
    Internal,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[non_exhaustive]
pub enum EasyErrorCode {
    ValidationInvalidArgument,
    ValidationUnknownField,
    CapabilityUnsupportedProfile,
    CapabilityRequiredFeatureMissing,
    ConfigInvalid,
    RuntimeInvalidState,
    RuntimeAlreadyRunningDifferentConfig,
    RuntimeStreamDegraded,
    RuntimeNotStarted,
    DeliveryQueuePressure,
    DeliveryPartialAcceptance,
    DeliveryRetryExhausted,
    DeliveryCancelled,
    ConnectivityDisconnected,
    ConnectivityReconnectFailed,
    PersistenceUnavailable,
    PersistenceRecoveryRequired,
    TimeoutOperationExpired,
    SecurityAuthRequired,
    SecurityAuthzDenied,
    SecurityRedactionRequired,
    InternalUnexpectedFailure,
    Unknown(String),
}

impl EasyErrorCode {
    pub fn as_str(&self) -> &str {
        match self {
            Self::ValidationInvalidArgument => "EASY_VALIDATION_INVALID_ARGUMENT",
            Self::ValidationUnknownField => "EASY_VALIDATION_UNKNOWN_FIELD",
            Self::CapabilityUnsupportedProfile => "EASY_CAPABILITY_UNSUPPORTED_PROFILE",
            Self::CapabilityRequiredFeatureMissing => "EASY_CAPABILITY_REQUIRED_FEATURE_MISSING",
            Self::ConfigInvalid => "EASY_CONFIG_INVALID",
            Self::RuntimeInvalidState => "EASY_RUNTIME_INVALID_STATE",
            Self::RuntimeAlreadyRunningDifferentConfig => {
                "EASY_RUNTIME_ALREADY_RUNNING_DIFFERENT_CONFIG"
            }
            Self::RuntimeStreamDegraded => "EASY_RUNTIME_STREAM_DEGRADED",
            Self::RuntimeNotStarted => "EASY_RUNTIME_NOT_STARTED",
            Self::DeliveryQueuePressure => "EASY_DELIVERY_QUEUE_PRESSURE",
            Self::DeliveryPartialAcceptance => "EASY_DELIVERY_PARTIAL_ACCEPTANCE",
            Self::DeliveryRetryExhausted => "EASY_DELIVERY_RETRY_EXHAUSTED",
            Self::DeliveryCancelled => "EASY_DELIVERY_CANCELLED",
            Self::ConnectivityDisconnected => "EASY_CONNECTIVITY_DISCONNECTED",
            Self::ConnectivityReconnectFailed => "EASY_CONNECTIVITY_RECONNECT_FAILED",
            Self::PersistenceUnavailable => "EASY_PERSISTENCE_UNAVAILABLE",
            Self::PersistenceRecoveryRequired => "EASY_PERSISTENCE_RECOVERY_REQUIRED",
            Self::TimeoutOperationExpired => "EASY_TIMEOUT_OPERATION_EXPIRED",
            Self::SecurityAuthRequired => "EASY_SECURITY_AUTH_REQUIRED",
            Self::SecurityAuthzDenied => "EASY_SECURITY_AUTHZ_DENIED",
            Self::SecurityRedactionRequired => "EASY_SECURITY_REDACTION_REQUIRED",
            Self::InternalUnexpectedFailure => "EASY_INTERNAL_UNEXPECTED_FAILURE",
            Self::Unknown(code) => code.as_str(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[non_exhaustive]
pub struct EasyError {
    pub code: EasyErrorCode,
    pub category: EasyErrorCategory,
    pub retryable: bool,
    pub terminal: bool,
    pub user_action_required: bool,
    pub message: String,
    #[serde(default)]
    pub details: BTreeMap<String, JsonValue>,
    pub cause_code: Option<String>,
}

impl EasyError {
    pub fn not_started() -> Self {
        Self {
            code: EasyErrorCode::RuntimeNotStarted,
            category: EasyErrorCategory::Runtime,
            retryable: false,
            terminal: true,
            user_action_required: false,
            message: "easy runtime has not been started".to_owned(),
            details: BTreeMap::new(),
            cause_code: None,
        }
    }

    pub fn already_running_different_config() -> Self {
        Self {
            code: EasyErrorCode::RuntimeAlreadyRunningDifferentConfig,
            category: EasyErrorCategory::Runtime,
            retryable: false,
            terminal: true,
            user_action_required: true,
            message: "easy runtime is already running with a different config".to_owned(),
            details: BTreeMap::new(),
            cause_code: None,
        }
    }

    pub fn unsupported_profile(profile_id: &str) -> Self {
        let mut details = BTreeMap::new();
        details.insert("profile_id".to_owned(), JsonValue::String(profile_id.to_owned()));
        Self {
            code: EasyErrorCode::CapabilityUnsupportedProfile,
            category: EasyErrorCategory::Capability,
            retryable: false,
            terminal: true,
            user_action_required: true,
            message: format!("easy profile '{profile_id}' is not supported by the current runtime"),
            details,
            cause_code: None,
        }
    }
}

impl From<SdkError> for EasyError {
    fn from(err: SdkError) -> Self {
        let SdkError {
            machine_code,
            category,
            retryable,
            is_user_actionable,
            message,
            details,
            cause_code,
            extensions: _,
        } = err;

        let code = match machine_code.as_str() {
            code::VALIDATION_INVALID_ARGUMENT | code::VALIDATION_MAX_POLL_EVENTS_EXCEEDED => {
                EasyErrorCode::ValidationInvalidArgument
            }
            code::VALIDATION_UNKNOWN_FIELD => EasyErrorCode::ValidationUnknownField,
            code::CAPABILITY_DISABLED | code::CAPABILITY_CONTRACT_INCOMPATIBLE => {
                EasyErrorCode::CapabilityRequiredFeatureMissing
            }
            code::CONFIG_CONFLICT | code::CONFIG_UNKNOWN_KEY => EasyErrorCode::ConfigInvalid,
            code::RUNTIME_ALREADY_RUNNING_WITH_DIFFERENT_CONFIG => {
                EasyErrorCode::RuntimeAlreadyRunningDifferentConfig
            }
            code::RUNTIME_STREAM_DEGRADED | code::RUNTIME_CURSOR_EXPIRED => {
                EasyErrorCode::RuntimeStreamDegraded
            }
            "SDK_RUNTIME_STORE_FORWARD_CAPACITY_REACHED" => EasyErrorCode::DeliveryQueuePressure,
            code::RUNTIME_INVALID_STATE => EasyErrorCode::RuntimeInvalidState,
            code::SECURITY_AUTH_REQUIRED
            | code::SECURITY_TOKEN_INVALID
            | code::SECURITY_TOKEN_REPLAYED
            | code::SECURITY_RATE_LIMITED
            | code::SECURITY_REMOTE_BIND_DISALLOWED => EasyErrorCode::SecurityAuthRequired,
            code::SECURITY_AUTHZ_DENIED => EasyErrorCode::SecurityAuthzDenied,
            code::SECURITY_REDACTION_REQUIRED => EasyErrorCode::SecurityRedactionRequired,
            code::INTERNAL => EasyErrorCode::InternalUnexpectedFailure,
            other => EasyErrorCode::Unknown(other.to_owned()),
        };

        let category = match category {
            ErrorCategory::Validation => EasyErrorCategory::Validation,
            ErrorCategory::Capability => EasyErrorCategory::Capability,
            ErrorCategory::Config => EasyErrorCategory::Config,
            ErrorCategory::Policy => EasyErrorCategory::Policy,
            ErrorCategory::Transport => EasyErrorCategory::Connectivity,
            ErrorCategory::Storage => EasyErrorCategory::Persistence,
            ErrorCategory::Crypto => EasyErrorCategory::Security,
            ErrorCategory::Timeout => EasyErrorCategory::Timeout,
            ErrorCategory::Runtime if matches!(code, EasyErrorCode::DeliveryQueuePressure) => {
                EasyErrorCategory::Delivery
            }
            ErrorCategory::Runtime => EasyErrorCategory::Runtime,
            ErrorCategory::Security => EasyErrorCategory::Security,
            ErrorCategory::Internal => EasyErrorCategory::Internal,
        };

        let terminal = matches!(
            code,
            EasyErrorCode::RuntimeInvalidState
                | EasyErrorCode::RuntimeAlreadyRunningDifferentConfig
                | EasyErrorCode::RuntimeNotStarted
                | EasyErrorCode::RuntimeStreamDegraded
                | EasyErrorCode::DeliveryRetryExhausted
                | EasyErrorCode::DeliveryCancelled
                | EasyErrorCode::TimeoutOperationExpired
                | EasyErrorCode::SecurityAuthRequired
                | EasyErrorCode::SecurityAuthzDenied
                | EasyErrorCode::SecurityRedactionRequired
                | EasyErrorCode::InternalUnexpectedFailure
        );

        let cause_code = if matches!(code, EasyErrorCode::Unknown(_)) {
            cause_code.or_else(|| Some(machine_code.clone()))
        } else if code.as_str() == machine_code {
            cause_code
        } else {
            Some(machine_code)
        };

        let retryable = retryable || matches!(code, EasyErrorCode::DeliveryQueuePressure);

        Self {
            code,
            category,
            retryable,
            terminal,
            user_action_required: is_user_actionable,
            message,
            details,
            cause_code,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{EasyError, EasyErrorCategory, EasyErrorCode};
    use crate::error::{code, ErrorCategory, SdkError};

    #[test]
    fn maps_runtime_invalid_state_into_easy_error() {
        let easy = EasyError::from(SdkError::new(
            code::RUNTIME_INVALID_STATE,
            ErrorCategory::Runtime,
            "bad state",
        ));
        assert_eq!(easy.code, EasyErrorCode::RuntimeInvalidState);
        assert_eq!(easy.category, EasyErrorCategory::Runtime);
        assert!(easy.terminal);
    }

    #[test]
    fn preserves_original_code_as_cause_when_remapped() {
        let easy = EasyError::from(SdkError::new(
            code::CAPABILITY_DISABLED,
            ErrorCategory::Capability,
            "missing capability",
        ));
        assert_eq!(easy.code, EasyErrorCode::CapabilityRequiredFeatureMissing);
        assert_eq!(easy.cause_code.as_deref(), Some(code::CAPABILITY_DISABLED));
    }

    #[test]
    fn maps_queue_pressure_into_typed_delivery_error() {
        let easy = EasyError::from(SdkError::new(
            "SDK_RUNTIME_STORE_FORWARD_CAPACITY_REACHED",
            ErrorCategory::Runtime,
            "queue pressure",
        ));
        assert_eq!(easy.code, EasyErrorCode::DeliveryQueuePressure);
        assert_eq!(easy.category, EasyErrorCategory::Delivery);
        assert!(easy.retryable);
        assert!(!easy.terminal);
    }

    #[test]
    fn not_started_is_not_user_action_required() {
        let easy = EasyError::not_started();
        assert!(!easy.user_action_required);
        assert!(easy.terminal);
    }

    #[test]
    fn degraded_stream_is_terminal_for_the_subscription() {
        let easy = EasyError::from(SdkError::new(
            code::RUNTIME_STREAM_DEGRADED,
            ErrorCategory::Runtime,
            "degraded",
        ));
        assert_eq!(easy.code, EasyErrorCode::RuntimeStreamDegraded);
        assert!(easy.terminal);
    }
}
