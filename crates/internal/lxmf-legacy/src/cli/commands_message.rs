use crate::cli::app::{
    AnnounceAction, AnnounceCommand, DeliveryMethodArg, EventsAction, EventsCommand, MessageAction,
    MessageCommand, MessageSendArgs, MessageSendCommandArgs, RuntimeContext,
};
use crate::cli::contacts::{load_contacts, resolve_contact_hash};
use crate::payload_fields::{CommandEntry, WireFields};
use anyhow::{anyhow, Context, Result};
use serde_json::{json, Value};
use std::time::{SystemTime, UNIX_EPOCH};

pub fn run(ctx: &RuntimeContext, command: &MessageCommand) -> Result<()> {
    match &command.action {
        MessageAction::Send(args) => send_message(ctx, args),
        MessageAction::SendCommand(args) => send_command_message(ctx, args),
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
    let prepared = prepare_send_params(ctx, args, None)?;
    emit_send_result(ctx, args, prepared)
}

fn send_command_message(ctx: &RuntimeContext, args: &MessageSendCommandArgs) -> Result<()> {
    if args.message.fields_json.is_some() {
        return Err(anyhow!(
            "--fields-json is not supported with `message send-command`; use --command/--command-hex only"
        ));
    }

    let command_entries = parse_command_entries(&args.commands, &args.commands_hex)?;
    if command_entries.is_empty() {
        return Err(anyhow!("provide at least one --command or --command-hex entry"));
    }

    let mut fields = WireFields::new();
    fields.set_commands(command_entries);
    let transport_fields = fields.to_transport_json().map_err(|err| anyhow!("{err}"))?;

    let prepared = prepare_send_params(ctx, &args.message, Some(transport_fields))?;
    emit_send_result(ctx, &args.message, prepared)
}

struct PreparedSend {
    params: Value,
    source: String,
    destination: String,
    source_changed: bool,
}

fn prepare_send_params(
    ctx: &RuntimeContext,
    args: &MessageSendArgs,
    fields_override: Option<Value>,
) -> Result<PreparedSend> {
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

    if let Some(fields_value) = fields_override {
        params["fields"] = fields_value;
    } else if let Some(fields_json) = args.fields_json.as_ref() {
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

    let source_changed = args.source.as_deref().map(|raw| raw != source).unwrap_or(true);
    Ok(PreparedSend { params, source, destination, source_changed })
}

fn emit_send_result(
    ctx: &RuntimeContext,
    args: &MessageSendArgs,
    prepared: PreparedSend,
) -> Result<()> {
    let PreparedSend { params, source, destination, source_changed } = prepared;
    let result = ctx.rpc.call("send_message_v2", Some(params))?;

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

fn parse_command_entries(
    commands: &[String],
    commands_hex: &[String],
) -> Result<Vec<CommandEntry>> {
    let mut out = Vec::new();

    for value in commands {
        let (command_id, payload) = split_command_spec(value)?;
        out.push(CommandEntry::from_text(command_id, payload));
    }

    for value in commands_hex {
        let (command_id, payload_hex) = split_command_spec(value)?;
        let payload = hex::decode(payload_hex)
            .with_context(|| format!("invalid command hex payload '{payload_hex}'"))?;
        out.push(CommandEntry::from_bytes(command_id, payload));
    }

    Ok(out)
}

fn split_command_spec(value: &str) -> Result<(u8, &str)> {
    let (id_raw, payload_raw) = value
        .split_once(':')
        .ok_or_else(|| anyhow!("invalid command spec '{value}', expected ID:PAYLOAD"))?;
    let id = id_raw
        .trim()
        .parse::<u8>()
        .with_context(|| format!("invalid command id '{id_raw}' in '{value}'"))?;
    let payload = payload_raw.trim();
    if payload.is_empty() {
        return Err(anyhow!("command payload cannot be empty in '{value}'"));
    }
    Ok((id, payload))
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
    use super::{find_message, parse_command_entries, source_hash_from_status};
    use crate::constants::FIELD_COMMANDS;
    use crate::payload_fields::WireFields;
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

    #[test]
    fn parse_command_entries_supports_text_and_hex() {
        let parsed = parse_command_entries(&["1:ping".into()], &["2:deadbeef".into()])
            .expect("command parse");

        let mut fields = WireFields::new();
        fields.set_commands(parsed);
        let rmpv::Value::Map(entries) = fields.to_rmpv() else { panic!("expected map") };
        let commands = entries
            .iter()
            .find_map(|(key, value)| (key.as_i64() == Some(FIELD_COMMANDS as i64)).then_some(value))
            .expect("commands field");
        let Some(items) = commands.as_array() else { panic!("commands array expected") };
        assert_eq!(items.len(), 2);
    }

    #[test]
    fn parse_command_entries_rejects_bad_specs() {
        let err = parse_command_entries(&["oops".into()], &[]).expect_err("invalid spec");
        assert!(err.to_string().contains("ID:PAYLOAD"));
    }
}
