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

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum ModelState {
        New,
        Starting,
        Running,
        Draining,
        Stopped,
        Failed,
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum ModelOp {
        Start,
        Run,
        Drain,
        Stop,
        Fail,
        Reset,
    }

    const MODEL_OPS: [ModelOp; 6] = [
        ModelOp::Start,
        ModelOp::Run,
        ModelOp::Drain,
        ModelOp::Stop,
        ModelOp::Fail,
        ModelOp::Reset,
    ];

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

    fn apply_model(state: ModelState, op: ModelOp) -> Result<ModelState, ()> {
        match op {
            ModelOp::Start if state == ModelState::New => Ok(ModelState::Starting),
            ModelOp::Run if state == ModelState::Starting => Ok(ModelState::Running),
            ModelOp::Drain if matches!(state, ModelState::Starting | ModelState::Running) => {
                Ok(ModelState::Draining)
            }
            ModelOp::Stop => Ok(ModelState::Stopped),
            ModelOp::Fail => Ok(ModelState::Failed),
            ModelOp::Reset => Ok(ModelState::New),
            _ => Err(()),
        }
    }

    fn apply_lifecycle(lifecycle: &mut Lifecycle, op: ModelOp) -> Result<(), SdkError> {
        match op {
            ModelOp::Start => lifecycle.mark_starting(),
            ModelOp::Run => lifecycle.mark_running(sample_start_request()),
            ModelOp::Drain => lifecycle.mark_draining(),
            ModelOp::Stop => {
                lifecycle.mark_stopped();
                Ok(())
            }
            ModelOp::Fail => {
                lifecycle.mark_failed();
                Ok(())
            }
            ModelOp::Reset => {
                lifecycle.reset_to_new();
                Ok(())
            }
        }
    }

    fn model_to_runtime(state: ModelState) -> RuntimeState {
        match state {
            ModelState::New => RuntimeState::New,
            ModelState::Starting => RuntimeState::Starting,
            ModelState::Running => RuntimeState::Running,
            ModelState::Draining => RuntimeState::Draining,
            ModelState::Stopped => RuntimeState::Stopped,
            ModelState::Failed => RuntimeState::Failed,
        }
    }

    fn model_method_legal(state: ModelState, method: SdkMethod) -> bool {
        match method {
            SdkMethod::Start => matches!(state, ModelState::New | ModelState::Running),
            SdkMethod::Send => state == ModelState::Running,
            SdkMethod::Cancel
            | SdkMethod::Status
            | SdkMethod::Tick
            | SdkMethod::PollEvents
            | SdkMethod::Snapshot
            | SdkMethod::SubscribeEvents => {
                matches!(state, ModelState::Running | ModelState::Draining)
            }
            SdkMethod::Configure => state == ModelState::Running,
            SdkMethod::Shutdown => {
                matches!(
                    state,
                    ModelState::Starting
                        | ModelState::Running
                        | ModelState::Draining
                        | ModelState::Stopped
                        | ModelState::Failed
                )
            }
        }
    }

    fn generate_sequences(max_len: usize) -> Vec<Vec<ModelOp>> {
        fn recurse(target_len: usize, current: &mut Vec<ModelOp>, out: &mut Vec<Vec<ModelOp>>) {
            if current.len() == target_len {
                out.push(current.clone());
                return;
            }
            for op in MODEL_OPS {
                current.push(op);
                recurse(target_len, current, out);
                current.pop();
            }
        }

        let mut out = Vec::new();
        for len in 1..=max_len {
            let mut current = Vec::new();
            recurse(len, &mut current, &mut out);
        }
        out
    }

    fn assert_method_legality_matches_model(lifecycle: &Lifecycle, model_state: ModelState) {
        let all_methods = [
            SdkMethod::Start,
            SdkMethod::Send,
            SdkMethod::Cancel,
            SdkMethod::Status,
            SdkMethod::Configure,
            SdkMethod::Tick,
            SdkMethod::PollEvents,
            SdkMethod::Snapshot,
            SdkMethod::Shutdown,
            SdkMethod::SubscribeEvents,
        ];

        for method in all_methods {
            let expected_ok = model_method_legal(model_state, method);
            let actual_ok = lifecycle.ensure_method_legal(method).is_ok();
            assert_eq!(
                actual_ok, expected_ok,
                "method legality mismatch for state {:?} and method {:?}",
                model_state, method
            );
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

    #[test]
    fn lifecycle_model_transitions_and_method_legality_match_reference() {
        let sequences = generate_sequences(4);
        for sequence in sequences {
            let mut lifecycle = Lifecycle::default();
            let mut model_state = ModelState::New;
            assert_method_legality_matches_model(&lifecycle, model_state);

            for op in &sequence {
                let expected = apply_model(model_state, *op);
                let actual = apply_lifecycle(&mut lifecycle, *op);

                assert_eq!(
                    actual.is_ok(),
                    expected.is_ok(),
                    "operation mismatch: op={:?}, sequence={:?}, runtime_state={:?}, model_state={:?}",
                    op,
                    sequence,
                    lifecycle.state(),
                    model_state
                );

                if let Ok(next_state) = expected {
                    model_state = next_state;
                }

                assert_eq!(
                    lifecycle.state(),
                    model_to_runtime(model_state),
                    "state mismatch after op {:?} in sequence {:?}",
                    op,
                    sequence
                );
                assert_method_legality_matches_model(&lifecycle, model_state);
            }
        }
    }
}
