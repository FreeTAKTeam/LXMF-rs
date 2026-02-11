use crate::cli::app::{
    AnnounceAction, AnnounceCommand, DeliveryMethodArg, EventsAction, EventsCommand, MessageAction,
    MessageCommand, MessageSendArgs, RuntimeContext,
};
use crate::cli::contacts::{load_contacts, resolve_contact_hash};
use anyhow::{anyhow, Context, Result};
use serde_json::{json, Value};
use std::time::{SystemTime, UNIX_EPOCH};

pub fn run(ctx: &RuntimeContext, command: &MessageCommand) -> Result<()> {
    match &command.action {
        MessageAction::Send(args) => send_message(ctx, args),
        MessageAction::List => {
            let messages = ctx.rpc.call("list_messages", None)?;
            ctx.output.emit_status(&json!({ "messages": messages }))
        }
        MessageAction::Show { id } => {
            let messages = ctx.rpc.call("list_messages", None)?;
            let Some(record) = find_message(&messages, id) else {
                return Err(anyhow!("message '{}' not found", id));
            };
            ctx.output.emit_status(&record)
        }
        MessageAction::Watch { interval_secs } => watch_messages(ctx, *interval_secs),
        MessageAction::Clear => {
            let result = ctx.rpc.call("clear_messages", None)?;
            ctx.output.emit_status(&result)
        }
    }
}

pub fn run_announce(ctx: &RuntimeContext, command: &AnnounceCommand) -> Result<()> {
    match command.action {
        AnnounceAction::Now => {
            let result = ctx.rpc.call("announce_now", None)?;
            ctx.output.emit_status(&result)
        }
    }
}

pub fn run_events(ctx: &RuntimeContext, command: &EventsCommand) -> Result<()> {
    match command.action {
        EventsAction::Watch { interval_secs, once } => {
            loop {
                if let Some(event) = ctx.rpc.poll_event()? {
                    ctx.output.emit_status(&event)?;
                }

                if once {
                    break;
                }

                std::thread::sleep(std::time::Duration::from_secs(interval_secs.max(1)));
            }
            Ok(())
        }
    }
}

fn send_message(ctx: &RuntimeContext, args: &MessageSendArgs) -> Result<()> {
    let contacts = load_contacts(&ctx.profile_name)?;
    let source_input =
        match args.source.as_deref().and_then(trimmed_nonempty).map(ToOwned::to_owned) {
            Some(value) => value,
            None => resolve_runtime_identity_hash(&ctx.rpc)?,
        };
    let source = resolve_contact_hash(&contacts, &source_input).unwrap_or(source_input.clone());
    let destination = resolve_contact_hash(&contacts, &args.destination)
        .unwrap_or_else(|| args.destination.clone());
    let id = args.id.clone().unwrap_or_else(generate_message_id);
    let mut params = json!({
        "id": id,
        "source": source,
        "destination": destination,
        "title": args.title,
        "content": args.content,
    });

    if let Some(fields_json) = args.fields_json.as_ref() {
        let parsed: Value = serde_json::from_str(fields_json)
            .with_context(|| "--fields-json must be valid JSON")?;
        params["fields"] = parsed;
    }

    if let Some(method) = args.method {
        params["method"] = Value::String(delivery_method_to_string(method));
    }
    if let Some(stamp_cost) = args.stamp_cost {
        params["stamp_cost"] = Value::from(stamp_cost);
    }
    if args.include_ticket {
        params["include_ticket"] = Value::Bool(true);
    }

    let result = match ctx.rpc.call("send_message_v2", Some(params.clone())) {
        Ok(v) => v,
        Err(_) => ctx.rpc.call("send_message", Some(params))?,
    };

    let source_changed = args.source.as_deref().map(|raw| raw != source).unwrap_or(true);
    if source_changed || destination != args.destination {
        return ctx.output.emit_status(&json!({
            "result": result,
            "resolved": {
                "source": source,
                "destination": destination,
            }
        }));
    }

    ctx.output.emit_status(&result)
}

fn watch_messages(ctx: &RuntimeContext, interval_secs: u64) -> Result<()> {
    loop {
        while let Some(event) = ctx.rpc.poll_event()? {
            if event.event_type.contains("message") || event.event_type.contains("outbound") {
                ctx.output.emit_status(&event)?;
            }
        }

        let messages = ctx.rpc.call("list_messages", None)?;
        ctx.output.emit_status(&json!({"messages": messages}))?;
        std::thread::sleep(std::time::Duration::from_secs(interval_secs.max(1)));
    }
}

fn find_message(messages: &Value, id: &str) -> Option<Value> {
    let list = if let Some(list) = messages.as_array() {
        list
    } else {
        messages.get("messages")?.as_array()?
    };
    for message in list {
        if message.get("id").and_then(Value::as_str) == Some(id) {
            return Some(message.clone());
        }
    }
    None
}

fn generate_message_id() -> String {
    let now = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis();
    format!("lxmf-{now}")
}

fn delivery_method_to_string(method: DeliveryMethodArg) -> String {
    method.as_str().to_string()
}

fn resolve_runtime_identity_hash(rpc: &crate::cli::rpc_client::RpcClient) -> Result<String> {
    for method in ["daemon_status_ex", "status"] {
        if let Ok(response) = rpc.call(method, None) {
            if let Some(source_hash) = source_hash_from_status(&response) {
                return Ok(source_hash);
            }
        }
    }
    Err(anyhow!(
        "source not provided and daemon did not report delivery/identity hash; pass --source or start daemon"
    ))
}

fn source_hash_from_status(value: &Value) -> Option<String> {
    for key in ["delivery_destination_hash", "identity_hash"] {
        if let Some(hash) = value
            .get(key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|candidate| !candidate.is_empty())
        {
            return Some(hash.to_string());
        }
    }
    None
}

fn trimmed_nonempty(value: &str) -> Option<&str> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}

#[cfg(test)]
mod tests {
    use super::{find_message, source_hash_from_status};
    use serde_json::json;

    #[test]
    fn find_message_accepts_top_level_array() {
        let payload = json!([
            {"id": "a", "content": "x"},
            {"id": "b", "content": "y"}
        ]);
        let found = find_message(&payload, "b");
        assert_eq!(found.and_then(|v| v.get("id").cloned()), Some(json!("b")));
    }

    #[test]
    fn find_message_accepts_wrapped_messages_array() {
        let payload = json!({
            "messages": [
                {"id": "a", "content": "x"},
                {"id": "b", "content": "y"}
            ]
        });
        let found = find_message(&payload, "b");
        assert_eq!(found.and_then(|v| v.get("id").cloned()), Some(json!("b")));
    }

    #[test]
    fn source_hash_prefers_delivery_destination_hash() {
        let status = json!({
            "identity_hash": "identity",
            "delivery_destination_hash": "delivery"
        });
        assert_eq!(source_hash_from_status(&status), Some("delivery".into()));
    }

    #[test]
    fn source_hash_falls_back_to_identity_hash() {
        let status = json!({ "identity_hash": "identity" });
        assert_eq!(source_hash_from_status(&status), Some("identity".into()));
    }
}
