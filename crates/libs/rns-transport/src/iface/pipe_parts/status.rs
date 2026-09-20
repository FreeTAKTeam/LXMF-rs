use std::sync::Arc;

use super::PipeRuntimeStatus;

pub(super) fn update_pipe_status(
    runtime_status: &Arc<std::sync::Mutex<PipeRuntimeStatus>>,
    update: impl FnOnce(&mut PipeRuntimeStatus),
) {
    let mut guard = runtime_status.lock().expect("pipe runtime status mutex poisoned");
    update(&mut guard);
}
