use super::delivery::{
    BackoffSchedule, DeliveryPlan, QueuePressurePolicy, QueuePressureStrategy, ReconnectPolicy,
    RetryPolicy, TimeoutPolicy,
};
use super::runtime::Profile;

impl Profile {
    pub fn defaults(&self) -> DeliveryPlan {
        match self {
            Self::MobileDefault => DeliveryPlan {
                profile: self.clone(),
                retry: RetryPolicy {
                    max_attempts: 3,
                    backoff: BackoffSchedule::exponential(250, 2, 2_000),
                },
                reconnect: ReconnectPolicy {
                    enabled: true,
                    max_attempts: Some(5),
                    backoff: BackoffSchedule::exponential(500, 2, 10_000),
                },
                queue_pressure: QueuePressurePolicy {
                    strategy: QueuePressureStrategy::Retry,
                    max_attempts: 3,
                    backoff: BackoffSchedule::exponential(100, 2, 750),
                },
                timeout: TimeoutPolicy {
                    send_timeout_ms: Some(5_000),
                    event_next_timeout_ms: Some(1_000),
                    reconnect_grace_ms: Some(15_000),
                },
                durable_queueing: false,
                restart_recovery: false,
                default_event_batch_size: 32,
                redaction_enabled: true,
            },
            Self::DesktopDefault => DeliveryPlan {
                profile: self.clone(),
                retry: RetryPolicy {
                    max_attempts: 5,
                    backoff: BackoffSchedule::exponential(200, 2, 5_000),
                },
                reconnect: ReconnectPolicy {
                    enabled: true,
                    max_attempts: Some(10),
                    backoff: BackoffSchedule::exponential(500, 2, 15_000),
                },
                queue_pressure: QueuePressurePolicy {
                    strategy: QueuePressureStrategy::Retry,
                    max_attempts: 4,
                    backoff: BackoffSchedule::exponential(100, 2, 1_000),
                },
                timeout: TimeoutPolicy {
                    send_timeout_ms: Some(10_000),
                    event_next_timeout_ms: Some(2_000),
                    reconnect_grace_ms: Some(30_000),
                },
                durable_queueing: false,
                restart_recovery: false,
                default_event_batch_size: 64,
                redaction_enabled: true,
            },
            Self::EmbeddedDefault => DeliveryPlan {
                profile: self.clone(),
                retry: RetryPolicy {
                    max_attempts: 2,
                    backoff: BackoffSchedule::exponential(500, 2, 2_000),
                },
                reconnect: ReconnectPolicy {
                    enabled: false,
                    max_attempts: Some(1),
                    backoff: BackoffSchedule::fixed(1_000),
                },
                queue_pressure: QueuePressurePolicy {
                    strategy: QueuePressureStrategy::FailFast,
                    max_attempts: 1,
                    backoff: BackoffSchedule::fixed(0),
                },
                timeout: TimeoutPolicy {
                    send_timeout_ms: Some(3_000),
                    event_next_timeout_ms: Some(500),
                    reconnect_grace_ms: None,
                },
                durable_queueing: false,
                restart_recovery: false,
                default_event_batch_size: 16,
                redaction_enabled: true,
            },
            Self::TestingDefault => DeliveryPlan {
                profile: self.clone(),
                retry: RetryPolicy { max_attempts: 2, backoff: BackoffSchedule::fixed(10) },
                reconnect: ReconnectPolicy {
                    enabled: true,
                    max_attempts: Some(2),
                    backoff: BackoffSchedule::fixed(25),
                },
                queue_pressure: QueuePressurePolicy {
                    strategy: QueuePressureStrategy::FailFast,
                    max_attempts: 1,
                    backoff: BackoffSchedule::fixed(0),
                },
                timeout: TimeoutPolicy {
                    send_timeout_ms: Some(500),
                    event_next_timeout_ms: Some(100),
                    reconnect_grace_ms: Some(250),
                },
                durable_queueing: false,
                restart_recovery: false,
                default_event_batch_size: 16,
                redaction_enabled: true,
            },
        }
    }
}
