use crate::cli::app::{PropagationAction, PropagationCommand, RuntimeContext};
use anyhow::Result;
use serde_json::{json, Value};

pub fn run(ctx: &RuntimeContext, command: &PropagationCommand) -> Result<()> {
    match &command.action {
        PropagationAction::Status => {
            let status = ctx.rpc.call("propagation_status", None)?;
            ctx.output.emit_status(&status)
        }
        PropagationAction::Enable {
            enabled,
            store_root,
            target_cost,
        } => {
            let result = ctx.rpc.call(
                "propagation_enable",
                Some(json!({
                    "enabled": enabled,
                    "store_root": store_root,
                    "target_cost": target_cost,
                })),
            )?;
            ctx.output.emit_status(&result)
        }
        PropagationAction::Ingest {
            transient_id,
            payload_hex,
        } => {
            let result = ctx.rpc.call(
                "propagation_ingest",
                Some(json!({
                    "transient_id": transient_id,
                    "payload_hex": payload_hex,
                })),
            )?;
            ctx.output.emit_status(&result)
        }
        PropagationAction::Fetch { transient_id } => {
            let result = ctx.rpc.call(
                "propagation_fetch",
                Some(json!({
                    "transient_id": transient_id,
                })),
            )?;
            ctx.output.emit_status(&result)
        }
        PropagationAction::Sync => sync(ctx),
    }
}

fn sync(ctx: &RuntimeContext) -> Result<()> {
    let peers = ctx.rpc.call("list_peers", None)?;
    let mut synced = Vec::new();

    if let Some(items) = peers.as_array() {
        for item in items {
            if let Some(peer) = item.get("peer").and_then(Value::as_str) {
                let result = ctx
                    .rpc
                    .call("peer_sync", Some(json!({"peer": peer})))
                    .unwrap_or_else(|_| json!({"ok": false, "peer": peer}));
                synced.push(result);
            }
        }
    }

    ctx.output.emit_status(&json!({"synced": synced}))
}
