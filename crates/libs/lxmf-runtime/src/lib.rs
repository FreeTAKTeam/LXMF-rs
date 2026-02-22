//! Runtime boundary crate for application startup and probe snapshots.

#[derive(Clone, Debug, Default)]
pub struct RuntimeConfig {
    pub allow_embedded_runtime: bool,
    pub log_level: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct SendCommandRequest {
    pub command: String,
}

#[derive(Clone, Debug, Default)]
pub struct SendMessageRequest {
    pub command: String,
    pub payload: Vec<u8>,
}

#[derive(Clone, Debug, Default)]
pub struct SendMessageResponse {
    pub ok: bool,
    pub reason: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct RuntimeHandle {
    active: bool,
}

#[derive(Clone, Debug, Default)]
pub struct RuntimeProbeReport {
    pub active: bool,
    pub started_at_epoch_ms: u64,
}

#[derive(Clone, Debug, Default)]
pub struct EventsProbeReport {
    pub queue_depth: usize,
}

#[derive(Clone, Debug, Default)]
pub struct RpcProbeReport {
    pub calls: usize,
    pub errors: usize,
}

pub fn start(config: RuntimeConfig) -> RuntimeHandle {
    let _ = config;
    RuntimeHandle { active: true }
}

impl RuntimeHandle {
    pub fn is_active(&self) -> bool {
        self.active
    }

    pub fn send_command(&self, request: SendCommandRequest) -> SendMessageResponse {
        SendMessageResponse { ok: request.command == "noop", reason: None }
    }

    pub fn send_message(&self, _request: SendMessageRequest) -> SendMessageResponse {
        SendMessageResponse { ok: true, reason: None }
    }

    pub fn events_probe_report(&self) -> EventsProbeReport {
        if self.active {
            EventsProbeReport { queue_depth: 0 }
        } else {
            EventsProbeReport::default()
        }
    }

    pub fn rpc_probe_report(&self) -> RpcProbeReport {
        if self.active {
            RpcProbeReport { calls: 1, errors: 0 }
        } else {
            RpcProbeReport::default()
        }
    }

    pub fn runtime_probe_report(&self) -> RuntimeProbeReport {
        if self.active {
            RuntimeProbeReport { active: true, started_at_epoch_ms: 0 }
        } else {
            RuntimeProbeReport::default()
        }
    }
}
