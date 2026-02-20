use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum RuntimeState {
    New,
    Starting,
    Running,
    Draining,
    Stopped,
    Failed,
    #[serde(other)]
    Unknown,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[non_exhaustive]
pub struct RuntimeSnapshot {
    pub runtime_id: String,
    pub state: RuntimeState,
    pub active_contract_version: u16,
    pub event_stream_position: u64,
    pub config_revision: u64,
    pub queued_messages: u64,
    pub in_flight_messages: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ShutdownMode {
    Graceful,
    Immediate,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[non_exhaustive]
pub struct TickBudget {
    pub max_work_items: usize,
    pub max_duration_ms: Option<u64>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[non_exhaustive]
pub struct TickResult {
    pub processed_items: usize,
    pub yielded: bool,
    pub next_recommended_delay_ms: Option<u64>,
}
