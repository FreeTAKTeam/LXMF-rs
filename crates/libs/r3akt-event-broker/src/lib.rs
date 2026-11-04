//! Transport-independent durable event journal extracted from LXMF-rs.
mod contracts;
pub use contracts::*;
pub mod event;
pub mod standalone;
pub mod storage;
pub use event::*;
pub use standalone::{Broker, Session};

/// Bounded response and scan limits. LXMF retains the historical defaults.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    pub max_batch_bytes: usize,
    pub max_batch_events: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self { max_batch_bytes: MAX_BATCH_BYTES, max_batch_events: MAX_BATCH_EVENTS }
    }
}
impl Limits {
    pub fn validate(self) -> rusqlite::Result<()> {
        if self.max_batch_bytes < 4096
            || self.max_batch_bytes > i64::MAX as usize / 2
            || self.max_batch_events == 0
            || self.max_batch_events > i64::MAX as usize
        {
            return Err(storage::error("SDK_VALIDATION_BROKER_LIMIT", "invalid journal limits"));
        }
        Ok(())
    }
}
