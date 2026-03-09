use super::operations::RegistryError;
use crate::error::{code, ErrorCategory as SdkErrorCategory, SdkError};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use std::collections::BTreeMap;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "PascalCase")]
#[non_exhaustive]
pub enum ErrorCategory {
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
pub enum ErrorCode {
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

impl ErrorCode {
    pub fn as_str(&self) -> &str {
        match self {
            Self::ValidationInvalidArgument => "SDK_APP_VALIDATION_INVALID_ARGUMENT",
            Self::ValidationUnknownField => "SDK_APP_VALIDATION_UNKNOWN_FIELD",
            Self::CapabilityUnsupportedProfile => "SDK_APP_CAPABILITY_UNSUPPORTED_PROFILE",
            Self::CapabilityRequiredFeatureMissing => "SDK_APP_CAPABILITY_REQUIRED_FEATURE_MISSING",
            Self::ConfigInvalid => "SDK_APP_CONFIG_INVALID",
            Self::RuntimeInvalidState => "SDK_APP_RUNTIME_INVALID_STATE",
            Self::RuntimeAlreadyRunningDifferentConfig => {
                "SDK_APP_RUNTIME_ALREADY_RUNNING_DIFFERENT_CONFIG"
            }
            Self::RuntimeStreamDegraded => "SDK_APP_RUNTIME_STREAM_DEGRADED",
            Self::RuntimeNotStarted => "SDK_APP_RUNTIME_NOT_STARTED",
            Self::DeliveryQueuePressure => "SDK_APP_DELIVERY_QUEUE_PRESSURE",
            Self::DeliveryPartialAcceptance => "SDK_APP_DELIVERY_PARTIAL_ACCEPTANCE",
            Self::DeliveryRetryExhausted => "SDK_APP_DELIVERY_RETRY_EXHAUSTED",
            Self::DeliveryCancelled => "SDK_APP_DELIVERY_CANCELLED",
            Self::ConnectivityDisconnected => "SDK_APP_CONNECTIVITY_DISCONNECTED",
            Self::ConnectivityReconnectFailed => "SDK_APP_CONNECTIVITY_RECONNECT_FAILED",
            Self::PersistenceUnavailable => "SDK_APP_PERSISTENCE_UNAVAILABLE",
            Self::PersistenceRecoveryRequired => "SDK_APP_PERSISTENCE_RECOVERY_REQUIRED",
            Self::TimeoutOperationExpired => "SDK_APP_TIMEOUT_OPERATION_EXPIRED",
            Self::SecurityAuthRequired => "SDK_APP_SECURITY_AUTH_REQUIRED",
            Self::SecurityAuthzDenied => "SDK_APP_SECURITY_AUTHZ_DENIED",
            Self::SecurityRedactionRequired => "SDK_APP_SECURITY_REDACTION_REQUIRED",
            Self::InternalUnexpectedFailure => "SDK_APP_INTERNAL_UNEXPECTED_FAILURE",
            Self::Unknown(code) => code.as_str(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[non_exhaustive]
pub struct Error {
    pub code: ErrorCode,
    pub category: ErrorCategory,
    pub retryable: bool,
    pub terminal: bool,
    pub user_action_required: bool,
    pub message: String,
    #[serde(default)]
    pub details: BTreeMap<String, JsonValue>,
    pub cause_code: Option<String>,
}

impl Error {
    pub fn not_started() -> Self {
        Self {
            code: ErrorCode::RuntimeNotStarted,
            category: ErrorCategory::Runtime,
            retryable: false,
            terminal: true,
            user_action_required: false,
            message: "runtime has not been started".to_owned(),
            details: BTreeMap::new(),
            cause_code: None,
        }
    }

    pub fn already_running_different_config() -> Self {
        Self {
            code: ErrorCode::RuntimeAlreadyRunningDifferentConfig,
            category: ErrorCategory::Runtime,
            retryable: false,
            terminal: true,
            user_action_required: true,
            message: "runtime is already running with a different config".to_owned(),
            details: BTreeMap::new(),
            cause_code: None,
        }
    }

    pub fn unsupported_profile(profile_id: &str) -> Self {
        let mut details = BTreeMap::new();
        details.insert("profile_id".to_owned(), JsonValue::String(profile_id.to_owned()));
        Self {
            code: ErrorCode::CapabilityUnsupportedProfile,
            category: ErrorCategory::Capability,
            retryable: false,
            terminal: true,
            user_action_required: true,
            message: format!("profile '{profile_id}' is not supported by the current runtime"),
            details,
            cause_code: None,
        }
    }
}

impl From<RegistryError> for Error {
    fn from(err: RegistryError) -> Self {
        let (message, mut details) = match err {
            RegistryError::DuplicateOperationId { id } => (
                format!("duplicate operation id '{}'", id.as_str()),
                BTreeMap::from([(
                    "operation_id".to_owned(),
                    JsonValue::String(id.as_str().to_owned()),
                )]),
            ),
            RegistryError::DuplicateAlias { alias, existing_id, conflicting_id } => (
                format!("duplicate operation alias '{alias}'"),
                BTreeMap::from([
                    ("alias".to_owned(), JsonValue::String(alias)),
                    ("existing_id".to_owned(), JsonValue::String(existing_id.as_str().to_owned())),
                    (
                        "conflicting_id".to_owned(),
                        JsonValue::String(conflicting_id.as_str().to_owned()),
                    ),
                ]),
            ),
            RegistryError::AliasConflictsWithOperationId { alias, operation_id } => (
                format!("operation alias '{alias}' conflicts with canonical operation id"),
                BTreeMap::from([
                    ("alias".to_owned(), JsonValue::String(alias)),
                    (
                        "operation_id".to_owned(),
                        JsonValue::String(operation_id.as_str().to_owned()),
                    ),
                ]),
            ),
        };
        details.insert("registry_scope".to_owned(), JsonValue::String("app".to_owned()));
        Self {
            code: ErrorCode::ConfigInvalid,
            category: ErrorCategory::Config,
            retryable: false,
            terminal: true,
            user_action_required: true,
            message,
            details,
            cause_code: None,
        }
    }
}

impl From<SdkError> for Error {
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
                ErrorCode::ValidationInvalidArgument
            }
            code::VALIDATION_UNKNOWN_FIELD => ErrorCode::ValidationUnknownField,
            code::CAPABILITY_DISABLED | code::CAPABILITY_CONTRACT_INCOMPATIBLE => {
                ErrorCode::CapabilityRequiredFeatureMissing
            }
            code::CONFIG_CONFLICT | code::CONFIG_UNKNOWN_KEY => ErrorCode::ConfigInvalid,
            code::RUNTIME_ALREADY_RUNNING_WITH_DIFFERENT_CONFIG => {
                ErrorCode::RuntimeAlreadyRunningDifferentConfig
            }
            code::RUNTIME_STREAM_DEGRADED | code::RUNTIME_CURSOR_EXPIRED => {
                ErrorCode::RuntimeStreamDegraded
            }
            "SDK_RUNTIME_STORE_FORWARD_CAPACITY_REACHED" => ErrorCode::DeliveryQueuePressure,
            code::RUNTIME_INVALID_STATE => ErrorCode::RuntimeInvalidState,
            code::SECURITY_AUTH_REQUIRED
            | code::SECURITY_TOKEN_INVALID
            | code::SECURITY_TOKEN_REPLAYED
            | code::SECURITY_RATE_LIMITED
            | code::SECURITY_REMOTE_BIND_DISALLOWED => ErrorCode::SecurityAuthRequired,
            code::SECURITY_AUTHZ_DENIED => ErrorCode::SecurityAuthzDenied,
            code::SECURITY_REDACTION_REQUIRED => ErrorCode::SecurityRedactionRequired,
            code::INTERNAL => ErrorCode::InternalUnexpectedFailure,
            other => ErrorCode::Unknown(other.to_owned()),
        };

        let category = match category {
            SdkErrorCategory::Validation => ErrorCategory::Validation,
            SdkErrorCategory::Capability => ErrorCategory::Capability,
            SdkErrorCategory::Config => ErrorCategory::Config,
            SdkErrorCategory::Policy => ErrorCategory::Policy,
            SdkErrorCategory::Transport => ErrorCategory::Connectivity,
            SdkErrorCategory::Storage => ErrorCategory::Persistence,
            SdkErrorCategory::Crypto => ErrorCategory::Security,
            SdkErrorCategory::Timeout => ErrorCategory::Timeout,
            SdkErrorCategory::Runtime if matches!(code, ErrorCode::DeliveryQueuePressure) => {
                ErrorCategory::Delivery
            }
            SdkErrorCategory::Runtime => ErrorCategory::Runtime,
            SdkErrorCategory::Security => ErrorCategory::Security,
            SdkErrorCategory::Internal => ErrorCategory::Internal,
        };

        let terminal = matches!(
            code,
            ErrorCode::RuntimeInvalidState
                | ErrorCode::RuntimeAlreadyRunningDifferentConfig
                | ErrorCode::RuntimeNotStarted
                | ErrorCode::RuntimeStreamDegraded
                | ErrorCode::DeliveryRetryExhausted
                | ErrorCode::DeliveryCancelled
                | ErrorCode::TimeoutOperationExpired
                | ErrorCode::SecurityAuthRequired
                | ErrorCode::SecurityAuthzDenied
                | ErrorCode::SecurityRedactionRequired
                | ErrorCode::InternalUnexpectedFailure
        );

        let cause_code = if matches!(code, ErrorCode::Unknown(_)) {
            cause_code.or_else(|| Some(machine_code.clone()))
        } else if code.as_str() == machine_code {
            cause_code
        } else {
            Some(machine_code)
        };

        let retryable = retryable || matches!(code, ErrorCode::DeliveryQueuePressure);

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
    use super::{Error, ErrorCategory, ErrorCode};
    use crate::error::{code, ErrorCategory as SdkErrorCategory, SdkError};

    #[test]
    fn maps_runtime_invalid_state_into_app_error() {
        let err = Error::from(SdkError::new(
            code::RUNTIME_INVALID_STATE,
            SdkErrorCategory::Runtime,
            "bad state",
        ));
        assert_eq!(err.code, ErrorCode::RuntimeInvalidState);
        assert_eq!(err.category, ErrorCategory::Runtime);
        assert!(err.terminal);
    }

    #[test]
    fn preserves_original_code_as_cause_when_remapped() {
        let err = Error::from(SdkError::new(
            code::CAPABILITY_DISABLED,
            SdkErrorCategory::Capability,
            "missing capability",
        ));
        assert_eq!(err.code, ErrorCode::CapabilityRequiredFeatureMissing);
        assert_eq!(err.cause_code.as_deref(), Some(code::CAPABILITY_DISABLED));
    }

    #[test]
    fn maps_queue_pressure_into_typed_delivery_error() {
        let err = Error::from(SdkError::new(
            "SDK_RUNTIME_STORE_FORWARD_CAPACITY_REACHED",
            SdkErrorCategory::Runtime,
            "queue pressure",
        ));
        assert_eq!(err.code, ErrorCode::DeliveryQueuePressure);
        assert_eq!(err.category, ErrorCategory::Delivery);
        assert!(err.retryable);
        assert!(!err.terminal);
    }

    #[test]
    fn not_started_is_not_user_action_required() {
        let err = Error::not_started();
        assert!(!err.user_action_required);
        assert!(err.terminal);
    }

    #[test]
    fn degraded_stream_is_terminal_for_the_subscription() {
        let err = Error::from(SdkError::new(
            code::RUNTIME_STREAM_DEGRADED,
            SdkErrorCategory::Runtime,
            "degraded",
        ));
        assert_eq!(err.code, ErrorCode::RuntimeStreamDegraded);
        assert!(err.terminal);
    }
}
