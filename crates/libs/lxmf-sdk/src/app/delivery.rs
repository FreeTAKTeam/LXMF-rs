use super::errors::{Error, ErrorCategory, ErrorCode};
use super::node::{Profile, SendReceipt};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[non_exhaustive]
pub struct BackoffSchedule {
    pub initial_delay_ms: u64,
    pub multiplier: u32,
    pub max_delay_ms: u64,
}

impl BackoffSchedule {
    pub const fn fixed(delay_ms: u64) -> Self {
        Self { initial_delay_ms: delay_ms, multiplier: 1, max_delay_ms: delay_ms }
    }

    pub const fn exponential(initial_delay_ms: u64, multiplier: u32, max_delay_ms: u64) -> Self {
        Self { initial_delay_ms, multiplier, max_delay_ms }
    }

    pub fn delay_for_attempt(&self, attempt: u32) -> u64 {
        if attempt <= 1 {
            return self.initial_delay_ms.min(self.max_delay_ms);
        }
        let mut delay = self.initial_delay_ms.max(1);
        for _ in 1..attempt {
            delay = delay.saturating_mul(u64::from(self.multiplier.max(1)));
            if delay >= self.max_delay_ms {
                return self.max_delay_ms;
            }
        }
        delay.min(self.max_delay_ms)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum QueuePressureStrategy {
    FailFast,
    Retry,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[non_exhaustive]
pub struct RetryPolicy {
    pub max_attempts: u32,
    pub backoff: BackoffSchedule,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[non_exhaustive]
pub struct ReconnectPolicy {
    pub enabled: bool,
    pub max_attempts: Option<u32>,
    pub backoff: BackoffSchedule,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[non_exhaustive]
pub struct QueuePressurePolicy {
    pub strategy: QueuePressureStrategy,
    pub max_attempts: u32,
    pub backoff: BackoffSchedule,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[non_exhaustive]
pub struct TimeoutPolicy {
    pub send_timeout_ms: Option<u64>,
    pub event_next_timeout_ms: Option<u64>,
    pub reconnect_grace_ms: Option<u64>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[non_exhaustive]
pub struct DeliveryPlan {
    pub profile: Profile,
    pub retry: RetryPolicy,
    pub reconnect: ReconnectPolicy,
    pub queue_pressure: QueuePressurePolicy,
    pub timeout: TimeoutPolicy,
    pub durable_queueing: bool,
    pub restart_recovery: bool,
    pub default_event_batch_size: usize,
    pub redaction_enabled: bool,
}

impl DeliveryPlan {
    pub(crate) fn resolve(&self, options: &DeliveryOptions) -> ResolvedSendPolicy {
        ResolvedSendPolicy {
            max_attempts: options.max_attempts.unwrap_or(self.retry.max_attempts).max(1),
            timeout_ms: options.timeout_ms.or(self.timeout.send_timeout_ms),
            retry_backoff: self.retry.backoff.clone(),
            queue_pressure_max_attempts: options
                .max_attempts
                .unwrap_or(self.queue_pressure.max_attempts)
                .max(1),
            queue_pressure_strategy: options
                .queue_pressure_strategy
                .clone()
                .unwrap_or(self.queue_pressure.strategy.clone()),
            queue_pressure_backoff: self.queue_pressure.backoff.clone(),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct DeliveryOptions {
    pub max_attempts: Option<u32>,
    pub timeout_ms: Option<u64>,
    pub queue_pressure_strategy: Option<QueuePressureStrategy>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum AttemptDisposition {
    Retried,
    Failed,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[non_exhaustive]
pub struct DeliveryAttempt {
    pub attempt: u32,
    pub disposition: AttemptDisposition,
    pub error_code: String,
    pub retryable: bool,
    pub queue_pressure: bool,
    pub scheduled_delay_ms: Option<u64>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[non_exhaustive]
pub struct SendReport {
    pub receipt: SendReceipt,
    pub attempts: Vec<DeliveryAttempt>,
    pub total_delay_ms: u64,
    pub plan: DeliveryPlan,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ResolvedSendPolicy {
    pub max_attempts: u32,
    pub timeout_ms: Option<u64>,
    pub retry_backoff: BackoffSchedule,
    pub queue_pressure_max_attempts: u32,
    pub queue_pressure_strategy: QueuePressureStrategy,
    pub queue_pressure_backoff: BackoffSchedule,
}

impl ResolvedSendPolicy {
    pub(crate) fn classify_failure(
        &self,
        attempt: u32,
        err: &Error,
        elapsed_ms: u64,
    ) -> AttemptDecision {
        if let Some(timeout_ms) = self.timeout_ms {
            if elapsed_ms >= timeout_ms {
                return AttemptDecision::Timeout(Error {
                    code: ErrorCode::TimeoutOperationExpired,
                    category: ErrorCategory::Timeout,
                    retryable: false,
                    terminal: true,
                    user_action_required: false,
                    message: format!("send helper exceeded timeout budget of {timeout_ms}ms"),
                    details: [("timeout_ms".to_owned(), serde_json::Value::from(timeout_ms))]
                        .into_iter()
                        .collect(),
                    cause_code: Some(root_cause_code(err)),
                });
            }
        }

        if matches!(err.code, ErrorCode::DeliveryQueuePressure) {
            if self.queue_pressure_strategy == QueuePressureStrategy::Retry
                && attempt < self.queue_pressure_max_attempts
            {
                return AttemptDecision::Retry(
                    self.queue_pressure_backoff.delay_for_attempt(attempt),
                );
            }
            return AttemptDecision::Stop(err.clone());
        }

        if err.retryable && attempt < self.max_attempts {
            return AttemptDecision::Retry(self.retry_backoff.delay_for_attempt(attempt));
        }

        if err.retryable && attempt >= self.max_attempts {
            return AttemptDecision::Stop(Error {
                code: ErrorCode::DeliveryRetryExhausted,
                category: ErrorCategory::Delivery,
                retryable: false,
                terminal: true,
                user_action_required: false,
                message: format!("send helper exhausted retry policy after {attempt} attempts"),
                details: [("attempts".to_owned(), serde_json::Value::from(attempt))]
                    .into_iter()
                    .collect(),
                cause_code: Some(root_cause_code(err)),
            });
        }

        AttemptDecision::Stop(err.clone())
    }
}

fn root_cause_code(err: &Error) -> String {
    err.cause_code.clone().unwrap_or_else(|| err.code.as_str().to_owned())
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) enum AttemptDecision {
    Retry(u64),
    Stop(Error),
    Timeout(Error),
}
