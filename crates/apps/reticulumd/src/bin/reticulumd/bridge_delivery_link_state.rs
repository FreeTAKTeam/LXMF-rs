use lxmf::message::{DeliverySchedule, DeliveryTiming};
use std::time::Duration;

/// One budget for identity, path discovery and Link closure within a method.
#[derive(Default)]
pub(super) struct Retry {
    pub(super) schedule: DeliverySchedule,
    pub(super) attempts: u32,
    pub(super) never_active_requested: bool,
}
impl Retry {
    pub(super) fn begin(
        &mut self,
        now: Duration,
        ready_path: bool,
        timing: DeliveryTiming,
    ) -> Option<bool> {
        if !self.schedule.due(now, ready_path) {
            return None;
        }
        self.attempts = self.attempts.saturating_add(1);
        self.schedule.begin_attempt();
        self.schedule.schedule_delivery(now, timing);
        Some(self.attempts < self.schedule.max_attempts())
    }
    pub(super) fn closed(&mut self, now: Duration, timing: DeliveryTiming) {
        // Closure consults the stored limit before measuring the next schedule.
        if self.attempts.saturating_add(1) < self.schedule.max_attempts() {
            self.schedule.schedule_delivery(now, timing);
        }
    }
}
