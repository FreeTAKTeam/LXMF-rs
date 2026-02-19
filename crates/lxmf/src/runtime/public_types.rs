use crate::cli::daemon::DaemonStatus;
use crate::payload_fields::CommandEntry;
use serde::Serialize;
use serde_json::Value;

#[derive(Debug, Clone, Default)]
pub struct SendMessageRequest {
    pub id: Option<String>,
    pub source: Option<String>,
    pub source_private_key: Option<String>,
    pub destination: String,
    pub title: String,
    pub content: String,
    pub fields: Option<Value>,
    pub method: Option<String>,
    pub stamp_cost: Option<u32>,
    pub include_ticket: bool,
    pub try_propagation_on_fail: bool,
}

impl SendMessageRequest {
    pub fn new(destination: impl Into<String>, content: impl Into<String>) -> Self {
        Self { destination: destination.into(), content: content.into(), ..Self::default() }
    }
}

#[derive(Debug, Clone, Default)]
pub struct SendCommandRequest {
    pub message: SendMessageRequest,
    pub commands: Vec<CommandEntry>,
}

impl SendCommandRequest {
    pub fn new(
        destination: impl Into<String>,
        content: impl Into<String>,
        commands: Vec<CommandEntry>,
    ) -> Self {
        Self { message: SendMessageRequest::new(destination, content), commands }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct SendMessageResponse {
    pub id: String,
    pub source: String,
    pub destination: String,
    pub result: Value,
}

#[derive(Debug, Clone, Serialize)]
pub struct RuntimeProbeReport {
    pub profile: String,
    pub local: DaemonStatus,
    pub rpc: RpcProbeReport,
    pub events: EventsProbeReport,
}

#[derive(Debug, Clone, Serialize)]
pub struct RpcProbeReport {
    pub reachable: bool,
    pub endpoint: String,
    pub method: Option<String>,
    pub roundtrip_ms: Option<u128>,
    pub identity_hash: Option<String>,
    pub status: Option<serde_json::Value>,
    pub errors: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct EventsProbeReport {
    pub reachable: bool,
    pub endpoint: String,
    pub roundtrip_ms: Option<u128>,
    pub event_type: Option<String>,
    pub payload: Option<serde_json::Value>,
    pub error: Option<String>,
}
