use super::capabilities::CapabilitySummary;
use super::operations::{OperationEntry, OperationRegistry, RegistryError};
use crate::{
    DeliverySnapshot, DeliveryState as RawDeliveryState, Profile as CoreProfile, RuntimeState,
    SdkConfig, SendRequest as RawSendRequest, StartRequest,
};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;
use std::collections::BTreeMap;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum Profile {
    MobileDefault,
    DesktopDefault,
    EmbeddedDefault,
    TestingDefault,
}

impl Profile {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::MobileDefault => "mobile_default",
            Self::DesktopDefault => "desktop_default",
            Self::EmbeddedDefault => "embedded_default",
            Self::TestingDefault => "testing_default",
        }
    }

    pub fn to_sdk_profile(&self) -> CoreProfile {
        match self {
            Self::MobileDefault => CoreProfile::DesktopLocalRuntime,
            Self::DesktopDefault => CoreProfile::DesktopFull,
            Self::EmbeddedDefault => CoreProfile::EmbeddedAlloc,
            Self::TestingDefault => CoreProfile::DesktopLocalRuntime,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[non_exhaustive]
pub struct Config {
    pub profile: Profile,
    pub sdk_config: SdkConfig,
    pub supported_contract_versions: Vec<u16>,
    pub requested_capabilities: Vec<String>,
    pub event_batch_size: Option<usize>,
    #[serde(default)]
    pub custom_operations: Vec<OperationEntry>,
}

impl Config {
    pub fn mobile_default() -> Self {
        Self::from_profile(Profile::MobileDefault)
    }

    pub fn desktop_default() -> Self {
        Self::from_profile(Profile::DesktopDefault)
    }

    pub fn embedded_default() -> Self {
        Self::from_profile(Profile::EmbeddedDefault)
    }

    pub fn testing_default() -> Self {
        Self::from_profile(Profile::TestingDefault)
    }

    pub fn with_requested_capability(mut self, capability: impl Into<String>) -> Self {
        self.requested_capabilities.push(capability.into());
        self
    }

    pub fn start_request(&self) -> StartRequest {
        let mut sdk_config = self.sdk_config.clone();
        if !self.custom_operations.is_empty() {
            sdk_config.extensions.insert(
                "custom_operations".to_owned(),
                serde_json::to_value(&self.custom_operations)
                    .expect("custom operation registry should serialize"),
            );
        }
        StartRequest::new(sdk_config)
            .with_supported_contract_versions(self.supported_contract_versions.clone())
            .with_requested_capabilities(self.requested_capabilities.clone())
    }

    pub fn operation_registry(&self) -> Result<OperationRegistry, RegistryError> {
        OperationRegistry::built_in().merged(self.custom_operations.clone())
    }

    pub fn with_custom_operation(mut self, operation: OperationEntry) -> Self {
        self.custom_operations.push(operation);
        self
    }

    pub fn with_custom_operations(
        mut self,
        operations: impl IntoIterator<Item = OperationEntry>,
    ) -> Self {
        self.custom_operations.extend(operations);
        self
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum RunState {
    New,
    Starting,
    Running,
    Degraded,
    Stopping,
    Stopped,
    Failed,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[non_exhaustive]
pub struct Handle {
    pub runtime_id: String,
    pub profile: Profile,
    pub capabilities: CapabilitySummary,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[non_exhaustive]
pub struct RuntimeStatus {
    pub runtime_id: Option<String>,
    pub state: RunState,
    pub profile: Option<Profile>,
    pub capabilities: Option<CapabilitySummary>,
    pub queued_messages: u64,
    pub in_flight_messages: u64,
    pub event_stream_position: u64,
    pub config_revision: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[non_exhaustive]
pub struct SendRequest {
    pub source: String,
    pub destination: String,
    pub payload: JsonValue,
    #[serde(default)]
    pub delivery_method: Option<String>,
    #[serde(default)]
    pub stamp_cost: Option<u32>,
    #[serde(default)]
    pub include_ticket: Option<bool>,
    #[serde(default)]
    pub try_propagation_on_fail: Option<bool>,
    pub idempotency_key: Option<String>,
    pub ttl_ms: Option<u64>,
    pub correlation_id: Option<String>,
    #[serde(default)]
    pub extensions: BTreeMap<String, JsonValue>,
}

impl SendRequest {
    pub fn new(
        source: impl Into<String>,
        destination: impl Into<String>,
        payload: JsonValue,
    ) -> Self {
        Self {
            source: source.into(),
            destination: destination.into(),
            payload,
            delivery_method: None,
            stamp_cost: None,
            include_ticket: None,
            try_propagation_on_fail: None,
            idempotency_key: None,
            ttl_ms: None,
            correlation_id: None,
            extensions: BTreeMap::new(),
        }
    }

    pub fn with_idempotency_key(mut self, key: impl Into<String>) -> Self {
        self.idempotency_key = Some(key.into());
        self
    }

    pub fn with_ttl_ms(mut self, ttl_ms: u64) -> Self {
        self.ttl_ms = Some(ttl_ms);
        self
    }

    pub fn with_correlation_id(mut self, correlation_id: impl Into<String>) -> Self {
        self.correlation_id = Some(correlation_id.into());
        self
    }

    pub fn with_delivery_method(mut self, method: impl Into<String>) -> Self {
        self.delivery_method = Some(method.into());
        self
    }

    pub fn with_stamp_cost(mut self, stamp_cost: u32) -> Self {
        self.stamp_cost = Some(stamp_cost);
        self
    }

    pub fn with_include_ticket(mut self, include_ticket: bool) -> Self {
        self.include_ticket = Some(include_ticket);
        self
    }

    pub fn with_try_propagation_on_fail(mut self, try_propagation_on_fail: bool) -> Self {
        self.try_propagation_on_fail = Some(try_propagation_on_fail);
        self
    }

    pub fn with_extension(mut self, key: impl Into<String>, value: JsonValue) -> Self {
        self.extensions.insert(key.into(), value);
        self
    }

    pub(crate) fn into_raw(self) -> RawSendRequest {
        RawSendRequest {
            source: self.source,
            destination: self.destination,
            payload: self.payload,
            delivery_method: self.delivery_method,
            stamp_cost: self.stamp_cost,
            include_ticket: self.include_ticket,
            try_propagation_on_fail: self.try_propagation_on_fail,
            idempotency_key: self.idempotency_key,
            ttl_ms: self.ttl_ms,
            correlation_id: self.correlation_id,
            extensions: self.extensions,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[non_exhaustive]
pub struct SendReceipt {
    pub runtime_id: String,
    pub message_id: String,
    pub profile: Profile,
    pub correlation_id: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum DeliveryState {
    Queued,
    Dispatching,
    Sent,
    Delivered,
    Failed,
    Cancelled,
    Expired,
    Rejected,
    Unknown,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[non_exhaustive]
pub struct DeliveryStatus {
    pub message_id: String,
    pub state: DeliveryState,
    pub terminal: bool,
    pub last_updated_ms: u64,
    pub attempts: u32,
    pub reason_code: Option<String>,
}

pub(crate) fn map_runtime_state(state: RuntimeState, degraded: bool) -> RunState {
    if degraded && state == RuntimeState::Running {
        return RunState::Degraded;
    }
    match state {
        RuntimeState::New => RunState::New,
        RuntimeState::Starting => RunState::Starting,
        RuntimeState::Running => RunState::Running,
        RuntimeState::Draining => RunState::Stopping,
        RuntimeState::Stopped => RunState::Stopped,
        RuntimeState::Failed | RuntimeState::Unknown => RunState::Failed,
    }
}

pub(crate) fn map_delivery_snapshot(snapshot: DeliverySnapshot) -> DeliveryStatus {
    let state = match snapshot.state {
        RawDeliveryState::Queued => DeliveryState::Queued,
        RawDeliveryState::Dispatching | RawDeliveryState::InFlight => DeliveryState::Dispatching,
        RawDeliveryState::Sent => DeliveryState::Sent,
        RawDeliveryState::Delivered => DeliveryState::Delivered,
        RawDeliveryState::Failed => DeliveryState::Failed,
        RawDeliveryState::Cancelled => DeliveryState::Cancelled,
        RawDeliveryState::Expired => DeliveryState::Expired,
        RawDeliveryState::Rejected => DeliveryState::Rejected,
        RawDeliveryState::Unknown => DeliveryState::Unknown,
    };
    DeliveryStatus {
        message_id: snapshot.message_id.0,
        state,
        terminal: snapshot.terminal,
        last_updated_ms: snapshot.last_updated_ms,
        attempts: snapshot.attempts,
        reason_code: snapshot.reason_code,
    }
}

#[cfg(test)]
mod tests {
    use super::SendRequest;
    use serde_json::json;

    #[test]
    fn app_send_request_preserves_delivery_options_in_raw_request() {
        let raw = SendRequest::new("source", "destination", json!({ "content": "hello" }))
            .with_delivery_method("propagated")
            .with_stamp_cost(8)
            .with_include_ticket(true)
            .with_try_propagation_on_fail(true)
            .into_raw();

        assert_eq!(raw.delivery_method.as_deref(), Some("propagated"));
        assert_eq!(raw.stamp_cost, Some(8));
        assert_eq!(raw.include_ticket, Some(true));
        assert_eq!(raw.try_propagation_on_fail, Some(true));
    }
}
