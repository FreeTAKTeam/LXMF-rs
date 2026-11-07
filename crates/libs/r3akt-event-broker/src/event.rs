//! Generic publishing and processing contracts. No transport field authenticates a caller.
use crate::{EventPosition, Limits};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Application-authenticated authority; never construct this from untrusted request identity.
#[derive(Debug, Clone, Copy)]
pub struct Context<'a> {
    pub owner: &'a str,
    pub scope: &'a str,
}

/// Exact topic/subject routing within an application-authorized scope.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Route {
    pub topic: String,
    pub subject: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EventEnvelope {
    pub version: u16,
    pub occurred_at: i64,
    /// Publisher-provided provenance; this never grants authority.
    pub origin: String,
    pub correlation_id: Option<String>,
    pub event_id: String,
    pub event_type: String,
    pub route: Route,
    pub payload: Value,
}

/// Immutable publication. Reuse this operation ID after a timeout/uncertain response.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PublishRequest {
    pub operation_id: String,
    pub event: EventEnvelope,
    /// Maximum encoded completion event accounting (payload, route and journal overhead).
    pub completion_reserve: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProcessingState {
    Queued,
    Claimed,
    /// A worker's lease expired. Its business effect may have happened; inspect before recovery.
    Unknown,
    Completed,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Publication {
    pub operation_id: String,
    pub event_id: String,
    pub position: EventPosition,
    pub state: ProcessingState,
}

/// Lease capability bound to the authenticated owner, scope and operation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Claim {
    pub operation_id: String,
    pub token: String,
    pub expires_at: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Recovery {
    /// Application has established replay is safe (or the business handler is idempotent).
    Requeue,
    /// Application has established that no further processing should occur.
    Fail,
}

#[derive(Debug, Clone, Copy)]
pub struct Config {
    pub budget_bytes: i64,
    pub limits: Limits,
    pub max_payload_bytes: usize,
    pub max_completion_bytes: usize,
    pub max_lease_seconds: i64,
}
impl Default for Config {
    fn default() -> Self {
        Self {
            budget_bytes: 64 * 1024 * 1024,
            limits: Limits::default(),
            max_payload_bytes: 1024 * 1024,
            max_completion_bytes: 64 * 1024,
            max_lease_seconds: 3600,
        }
    }
}
impl Config {
    pub fn validate(self) -> rusqlite::Result<()> {
        self.limits.validate()?;
        if self.budget_bytes <= 0
            || self.max_payload_bytes == 0
            || self.max_payload_bytes > self.limits.max_batch_bytes.saturating_sub(4096)
            || self.max_completion_bytes > self.limits.max_batch_bytes.saturating_sub(4096)
            || self.max_lease_seconds <= 0
        {
            return Err(crate::storage::error(
                "BROKER_CONFIG_INVALID",
                "invalid standalone bounds",
            ));
        }
        Ok(())
    }
}
