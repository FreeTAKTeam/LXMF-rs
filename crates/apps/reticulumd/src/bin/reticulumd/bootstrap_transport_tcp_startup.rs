use super::InterfaceStartupFailure;
use crate::Args;
use reticulum_daemon::config::DaemonConfig;

pub(super) fn strict_startup(args: &Args, daemon_config: Option<&DaemonConfig>) -> bool {
    args.strict_interface_startup
        || daemon_config.is_some_and(|config| config.panic_on_interface_error)
}

pub(super) fn record_initial_bind_result(
    result: Result<Result<(), String>, tokio::sync::oneshot::error::RecvError>,
    label: String,
    kind: String,
    startup_successes: &mut usize,
    startup_failures: &mut Vec<InterfaceStartupFailure>,
) {
    match result {
        Ok(Ok(())) => *startup_successes += 1,
        Ok(Err(error)) => startup_failures.push(InterfaceStartupFailure { label, kind, error }),
        Err(_) => startup_failures.push(InterfaceStartupFailure {
            label,
            kind,
            error: "TCP listener worker exited before reporting its initial bind result"
                .to_string(),
        }),
    }
}
