use crate::cli::app::{IfaceAction, IfaceCommand, RuntimeContext};
use crate::cli::daemon::DaemonSupervisor;
use crate::cli::profile::{
    load_reticulum_config, remove_interface, save_reticulum_config, set_interface_enabled,
    upsert_interface, InterfaceEntry,
};
use anyhow::{anyhow, Result};
use serde_json::json;

pub fn run(ctx: &RuntimeContext, command: &IfaceCommand) -> Result<()> {
    match &command.action {
        IfaceAction::List => {
            let config = load_reticulum_config(&ctx.profile_name)?;
            let rpc_value = ctx.rpc.call("list_interfaces", None).ok();
            ctx.output.emit_status(&json!({
                "profile": ctx.profile_name,
                "config_interfaces": config.interfaces,
                "rpc_interfaces": rpc_value,
            }))
        }
        IfaceAction::Add(args) => {
            validate_kind(&args.kind)?;
            let mut config = load_reticulum_config(&ctx.profile_name)?;
            upsert_interface(
                &mut config,
                InterfaceEntry {
                    name: args.name.clone(),
                    kind: args.kind.clone(),
                    enabled: args.enabled,
                    host: args.host.clone(),
                    port: args.port,
                },
            );
            save_reticulum_config(&ctx.profile_name, &config)?;
            ctx.output.emit_status(&json!({
                "updated": args.name,
                "interfaces": config.interfaces,
            }))
        }
        IfaceAction::Remove { name } => {
            let mut config = load_reticulum_config(&ctx.profile_name)?;
            let removed = remove_interface(&mut config, name);
            save_reticulum_config(&ctx.profile_name, &config)?;
            ctx.output.emit_status(&json!({
                "removed": removed,
                "name": name,
                "interfaces": config.interfaces,
            }))
        }
        IfaceAction::Enable { name } => {
            let mut config = load_reticulum_config(&ctx.profile_name)?;
            let updated = set_interface_enabled(&mut config, name, true);
            save_reticulum_config(&ctx.profile_name, &config)?;
            ctx.output.emit_status(&json!({
                "updated": updated,
                "name": name,
                "enabled": true,
            }))
        }
        IfaceAction::Disable { name } => {
            let mut config = load_reticulum_config(&ctx.profile_name)?;
            let updated = set_interface_enabled(&mut config, name, false);
            save_reticulum_config(&ctx.profile_name, &config)?;
            ctx.output.emit_status(&json!({
                "updated": updated,
                "name": name,
                "enabled": false,
            }))
        }
        IfaceAction::Apply { restart } => apply_interfaces(ctx, *restart),
    }
}

fn apply_interfaces(ctx: &RuntimeContext, force_restart: bool) -> Result<()> {
    let config = load_reticulum_config(&ctx.profile_name)?;

    let set_result = ctx.rpc.call(
        "set_interfaces",
        Some(json!({
            "interfaces": config.interfaces,
        })),
    );

    let mut applied_via = "set_interfaces";
    let mut reload_result = None;
    if set_result.is_ok() && !force_restart {
        reload_result = ctx.rpc.call("reload_config", None).ok();
        if reload_result.is_none() {
            applied_via = "set_interfaces_only";
        }
    } else {
        applied_via = "daemon_restart";
        let supervisor = DaemonSupervisor::new(&ctx.profile_name, ctx.profile_settings.clone());
        let _ = supervisor.restart(None, Some(true), None)?;
    }

    ctx.output.emit_status(&json!({
        "profile": ctx.profile_name,
        "applied_via": applied_via,
        "set_interfaces_ok": set_result.is_ok(),
        "reload_result": reload_result,
    }))
}

fn validate_kind(kind: &str) -> Result<()> {
    match kind {
        "tcp_client" | "tcp_server" => Ok(()),
        other => Err(anyhow!(
            "unsupported interface type '{}' (supported: tcp_client, tcp_server)",
            other
        )),
    }
}
