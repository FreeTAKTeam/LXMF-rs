use crate::cli::app::{RuntimeContext, StampAction, StampCommand};
use anyhow::Result;
use serde_json::json;

pub fn run(ctx: &RuntimeContext, command: &StampCommand) -> Result<()> {
    match &command.action {
        StampAction::Target => {
            let policy = ctx.rpc.call("stamp_policy_get", None)?;
            let target = policy
                .get("target_cost")
                .and_then(|v| v.as_u64())
                .unwrap_or_default();
            ctx.output.emit_status(&json!({"target_cost": target}))
        }
        StampAction::Get => {
            let policy = ctx.rpc.call("stamp_policy_get", None)?;
            ctx.output.emit_status(&policy)
        }
        StampAction::Set {
            target_cost,
            flexibility,
        } => {
            let policy = ctx.rpc.call(
                "stamp_policy_set",
                Some(json!({
                    "target_cost": target_cost,
                    "flexibility": flexibility,
                })),
            )?;
            ctx.output.emit_status(&policy)
        }
        StampAction::GenerateTicket {
            destination,
            ttl_secs,
        } => {
            let ticket = ctx.rpc.call(
                "ticket_generate",
                Some(json!({
                    "destination": destination,
                    "ttl_secs": ttl_secs,
                })),
            )?;
            ctx.output.emit_status(&ticket)
        }
        StampAction::Cache => {
            let policy = ctx.rpc.call("stamp_policy_get", None)?;
            ctx.output.emit_status(&json!({
                "cache": "ticket cache introspection is not exposed by daemon yet",
                "policy": policy,
            }))
        }
    }
}
