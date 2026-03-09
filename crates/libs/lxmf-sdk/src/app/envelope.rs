use super::operations::{OperationId, OperationRegistry};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use std::collections::BTreeMap;
use std::fmt;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum EnvelopeKind {
    Query,
    Command,
    Result,
    Error,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[non_exhaustive]
pub struct Envelope {
    pub operation_id: OperationId,
    pub kind: EnvelopeKind,
    pub target: Option<String>,
    pub correlation_id: Option<String>,
    pub timeout_ms: Option<u64>,
    pub payload: JsonValue,
    #[serde(default)]
    pub extensions: BTreeMap<String, JsonValue>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum EnvelopeValidationError {
    UnknownOperation { operation_id: OperationId },
    KindMismatch {
        operation_id: OperationId,
        expected: EnvelopeKind,
        actual: EnvelopeKind,
    },
}

impl fmt::Display for EnvelopeValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnknownOperation { operation_id } => {
                write!(f, "unknown operation id '{}'", operation_id.as_str())
            }
            Self::KindMismatch {
                operation_id,
                expected,
                actual,
            } => write!(
                f,
                "operation '{}' requires {:?} envelope, got {:?}",
                operation_id.as_str(),
                expected,
                actual
            ),
        }
    }
}

impl Envelope {
    pub fn query(operation_id: impl Into<OperationId>, payload: JsonValue) -> Self {
        Self {
            operation_id: operation_id.into(),
            kind: EnvelopeKind::Query,
            target: None,
            correlation_id: None,
            timeout_ms: None,
            payload,
            extensions: BTreeMap::new(),
        }
    }

    pub fn command(operation_id: impl Into<OperationId>, payload: JsonValue) -> Self {
        Self {
            operation_id: operation_id.into(),
            kind: EnvelopeKind::Command,
            target: None,
            correlation_id: None,
            timeout_ms: None,
            payload,
            extensions: BTreeMap::new(),
        }
    }

    pub fn with_target(mut self, target: impl Into<String>) -> Self {
        self.target = Some(target.into());
        self
    }

    pub fn with_correlation_id(mut self, correlation_id: impl Into<String>) -> Self {
        self.correlation_id = Some(correlation_id.into());
        self
    }

    pub fn with_timeout_ms(mut self, timeout_ms: u64) -> Self {
        self.timeout_ms = Some(timeout_ms);
        self
    }

    pub fn with_extension(mut self, key: impl Into<String>, value: JsonValue) -> Self {
        self.extensions.insert(key.into(), value);
        self
    }

    pub fn normalized(
        mut self,
        registry: &OperationRegistry,
    ) -> Result<Self, EnvelopeValidationError> {
        let resolved = registry.resolve(self.operation_id.as_str()).ok_or_else(|| {
            EnvelopeValidationError::UnknownOperation { operation_id: self.operation_id.clone() }
        })?;
        let expected = resolved.entry.expected_envelope_kind();
        if !resolved.entry.accepts_envelope_kind(&self.kind) {
            return Err(EnvelopeValidationError::KindMismatch {
                operation_id: resolved.canonical_id.clone(),
                expected,
                actual: self.kind,
            });
        }
        self.operation_id = resolved.canonical_id.clone();
        Ok(self)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[non_exhaustive]
pub struct EnvelopeResponse {
    pub operation_id: OperationId,
    pub kind: EnvelopeKind,
    pub accepted: bool,
    pub correlation_id: Option<String>,
    pub payload: JsonValue,
    #[serde(default)]
    pub extensions: BTreeMap<String, JsonValue>,
}

#[cfg(test)]
mod tests {
    use super::{Envelope, EnvelopeKind, EnvelopeValidationError};
    use crate::app::OperationRegistry;
    use serde_json::json;

    #[test]
    fn normalize_canonicalizes_aliases() {
        let normalized = Envelope::query("sdk_identity_list_v2", json!({}))
            .normalized(OperationRegistry::built_in())
            .expect("normalized envelope");
        assert_eq!(normalized.operation_id.as_str(), "app.identity.list");
    }

    #[test]
    fn normalize_rejects_kind_mismatch() {
        let err = Envelope::command("sdk_identity_list_v2", json!({}))
            .normalized(OperationRegistry::built_in())
            .expect_err("kind mismatch");
        assert!(matches!(
            err,
            EnvelopeValidationError::KindMismatch { operation_id, expected: EnvelopeKind::Query, actual: EnvelopeKind::Command }
            if operation_id.as_str() == "app.identity.list"
        ));
    }
}
