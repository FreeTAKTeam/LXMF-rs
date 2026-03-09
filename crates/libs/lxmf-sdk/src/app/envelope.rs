use super::operations::OperationId;
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use std::collections::BTreeMap;

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
