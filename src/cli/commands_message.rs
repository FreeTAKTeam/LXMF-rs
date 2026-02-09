use crate::cli::app::{
    AnnounceAction, AnnounceCommand, DeliveryMethodArg, EventsAction, EventsCommand, MessageAction,
    MessageCommand, MessageSendArgs, RuntimeContext,
};
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
        EventsAction::Watch {
            interval_secs,
            once,
        } => {
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
    let id = args.id.clone().unwrap_or_else(generate_message_id);
    let mut params = json!({
        "id": id,
        "source": args.source,
        "destination": args.destination,
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
    let list = messages.as_array()?;
    for message in list {
        if message.get("id").and_then(Value::as_str) == Some(id) {
            return Some(message.clone());
        }
    }
    None
}

fn generate_message_id() -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    format!("lxmf-{now}")
}

fn delivery_method_to_string(method: DeliveryMethodArg) -> String {
    method.as_str().to_string()
}
