use crate::error::{code, ErrorCategory, SdkError};
use crate::types::{RuntimeState, StartRequest};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum SdkMethod {
    Start,
    Send,
    Cancel,
    Status,
    Configure,
    Tick,
    PollEvents,
    Snapshot,
    Shutdown,
    SubscribeEvents,
}

impl SdkMethod {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Start => "start",
            Self::Send => "send",
            Self::Cancel => "cancel",
            Self::Status => "status",
            Self::Configure => "configure",
            Self::Tick => "tick",
            Self::PollEvents => "poll_events",
            Self::Snapshot => "snapshot",
            Self::Shutdown => "shutdown",
            Self::SubscribeEvents => "subscribe_events",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Lifecycle {
    state: RuntimeState,
    active_start_request: Option<StartRequest>,
}

impl Default for Lifecycle {
    fn default() -> Self {
        Self { state: RuntimeState::New, active_start_request: None }
    }
}

impl Lifecycle {
    pub fn state(&self) -> RuntimeState {
        self.state.clone()
    }

    pub fn ensure_method_legal(&self, method: SdkMethod) -> Result<(), SdkError> {
        if legal_states_for_method(method).contains(&self.state) {
            return Ok(());
        }
        Err(SdkError::invalid_state(method.as_str(), self.state_name()))
    }

    pub fn check_start_reentry(&self, req: &StartRequest) -> Result<bool, SdkError> {
        match self.state {
            RuntimeState::New => Ok(false),
            RuntimeState::Running => match &self.active_start_request {
                Some(active) if active == req => Ok(true),
                _ => Err(SdkError::new(
                    code::RUNTIME_ALREADY_RUNNING_WITH_DIFFERENT_CONFIG,
                    ErrorCategory::Runtime,
                    "runtime is already running with a different start request",
                )
                .with_user_actionable(true)
                .with_detail("method", JsonValue::String("start".to_owned()))),
            },
            _ => Err(SdkError::invalid_state("start", self.state_name())),
        }
    }

    pub fn mark_starting(&mut self) -> Result<(), SdkError> {
        if self.state != RuntimeState::New {
            return Err(SdkError::invalid_state("start", self.state_name()));
        }
        self.state = RuntimeState::Starting;
        Ok(())
    }

    pub fn mark_running(&mut self, req: StartRequest) -> Result<(), SdkError> {
        if self.state != RuntimeState::Starting {
            return Err(SdkError::invalid_state("start", self.state_name()));
        }
        self.state = RuntimeState::Running;
        self.active_start_request = Some(req);
        Ok(())
    }

    pub fn mark_draining(&mut self) -> Result<(), SdkError> {
        if !matches!(self.state, RuntimeState::Running | RuntimeState::Starting) {
            return Err(SdkError::invalid_state("shutdown", self.state_name()));
        }
        self.state = RuntimeState::Draining;
        Ok(())
    }

    pub fn mark_stopped(&mut self) {
        self.state = RuntimeState::Stopped;
    }

    pub fn mark_failed(&mut self) {
        self.state = RuntimeState::Failed;
    }

    pub fn reset_to_new(&mut self) {
        self.state = RuntimeState::New;
        self.active_start_request = None;
    }

    fn state_name(&self) -> &'static str {
        match self.state {
            RuntimeState::New => "new",
            RuntimeState::Starting => "starting",
            RuntimeState::Running => "running",
            RuntimeState::Draining => "draining",
            RuntimeState::Stopped => "stopped",
            RuntimeState::Failed => "failed",
            RuntimeState::Unknown => "unknown",
        }
    }
}

fn legal_states_for_method(method: SdkMethod) -> &'static [RuntimeState] {
    use RuntimeState as S;
    match method {
        SdkMethod::Start => &[S::New, S::Running],
        SdkMethod::Send => &[S::Running],
        SdkMethod::Cancel => &[S::Running, S::Draining],
        SdkMethod::Status => &[S::Running, S::Draining],
        SdkMethod::Configure => &[S::Running],
        SdkMethod::Tick => &[S::Running, S::Draining],
        SdkMethod::PollEvents => &[S::Running, S::Draining],
        SdkMethod::Snapshot => &[S::Running, S::Draining],
        SdkMethod::Shutdown => &[S::Starting, S::Running, S::Draining, S::Stopped, S::Failed],
        SdkMethod::SubscribeEvents => &[S::Running, S::Draining],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{
        AuthMode, BindMode, EventStreamConfig, OverflowPolicy, Profile, RedactionConfig,
        RedactionTransform, SdkConfig,
    };
    use std::collections::BTreeMap;

    fn sample_start_request() -> StartRequest {
        StartRequest {
            supported_contract_versions: vec![2, 1],
            requested_capabilities: vec!["sdk.capability.cursor_replay".to_owned()],
            config: SdkConfig {
                profile: Profile::DesktopFull,
                bind_mode: BindMode::LocalOnly,
                auth_mode: AuthMode::LocalTrusted,
                overflow_policy: OverflowPolicy::Reject,
                block_timeout_ms: None,
                event_stream: EventStreamConfig {
                    max_poll_events: 256,
                    max_event_bytes: 65_536,
                    max_batch_bytes: 1_048_576,
                    max_extension_keys: 32,
                },
                idempotency_ttl_ms: 86_400_000,
                redaction: RedactionConfig {
                    enabled: true,
                    sensitive_transform: RedactionTransform::Hash,
                    break_glass_allowed: false,
                    break_glass_ttl_ms: None,
                },
                rpc_backend: None,
                extensions: BTreeMap::new(),
            },
        }
    }

    #[test]
    fn method_legality_matrix_enforced() {
        let mut lifecycle = Lifecycle::default();
        assert!(lifecycle.ensure_method_legal(SdkMethod::Start).is_ok());
        assert!(lifecycle.ensure_method_legal(SdkMethod::Send).is_err());

        lifecycle.mark_starting().expect("new -> starting");
        lifecycle.mark_running(sample_start_request()).expect("starting -> running");
        assert!(lifecycle.ensure_method_legal(SdkMethod::Send).is_ok());
        assert!(lifecycle.ensure_method_legal(SdkMethod::Configure).is_ok());
        assert!(lifecycle.ensure_method_legal(SdkMethod::Shutdown).is_ok());
    }

    #[test]
    fn start_reentry_same_request_reuses_running_session() {
        let request = sample_start_request();
        let mut lifecycle = Lifecycle::default();
        lifecycle.mark_starting().expect("new -> starting");
        lifecycle.mark_running(request.clone()).expect("starting -> running");
        let reused = lifecycle.check_start_reentry(&request).expect("same request should reuse");
        assert!(reused);
    }

    #[test]
    fn start_reentry_different_request_is_rejected() {
        let request = sample_start_request();
        let mut other_request = sample_start_request();
        other_request.requested_capabilities = vec!["sdk.capability.async_events".to_owned()];
        let mut lifecycle = Lifecycle::default();
        lifecycle.mark_starting().expect("new -> starting");
        lifecycle.mark_running(request).expect("starting -> running");
        let err = lifecycle.check_start_reentry(&other_request).expect_err("must reject mismatch");
        assert_eq!(err.machine_code, code::RUNTIME_ALREADY_RUNNING_WITH_DIFFERENT_CONFIG);
    }
}
