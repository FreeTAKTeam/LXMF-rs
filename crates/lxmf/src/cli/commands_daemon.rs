use crate::cli::app::{DaemonAction, DaemonCommand, RuntimeContext};
use crate::cli::daemon::DaemonSupervisor;
use anyhow::Result;
use serde_json::json;

pub fn run(ctx: &RuntimeContext, command: &DaemonCommand) -> Result<()> {
    let supervisor = DaemonSupervisor::new(&ctx.profile_name, ctx.profile_settings.clone());

    match &command.action {
        DaemonAction::Start { managed, reticulumd, transport } => {
            let managed_override = (*managed).then_some(true);
            let status =
                supervisor.start(reticulumd.clone(), managed_override, transport.clone())?;
            ctx.output.emit_status(&status)
        }
        DaemonAction::Stop => {
            let status = supervisor.stop()?;
            ctx.output.emit_status(&status)
        }
        DaemonAction::Restart { managed, reticulumd, transport } => {
            let managed_override = (*managed).then_some(true);
            let status =
                supervisor.restart(reticulumd.clone(), managed_override, transport.clone())?;
            ctx.output.emit_status(&status)
        }
        DaemonAction::Status => {
            let local = supervisor.status()?;
            let rpc_status = ctx.rpc.call("daemon_status_ex", None).ok();
            ctx.output.emit_status(&json!({
                "profile": ctx.profile_name,
                "local": local,
                "rpc": rpc_status,
            }))
        }
        DaemonAction::Logs { tail } => {
            let lines = supervisor.logs(*tail)?;
            ctx.output.emit_lines(&lines);
            Ok(())
        }
    }
}
