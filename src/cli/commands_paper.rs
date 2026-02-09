use crate::cli::app::{PaperAction, PaperCommand, RuntimeContext};
use anyhow::Result;
use serde_json::{json, Value};

pub fn run(ctx: &RuntimeContext, command: &PaperCommand) -> Result<()> {
    match &command.action {
        PaperAction::IngestUri { uri } => {
            let result = ctx
                .rpc
                .call("paper_ingest_uri", Some(json!({ "uri": uri })))?;
            ctx.output.emit_status(&result)
        }
        PaperAction::Show => {
            let messages = ctx.rpc.call("list_messages", None)?;
            let mut paper = Vec::new();
            if let Some(items) = messages.as_array() {
                for item in items {
                    let is_paper = item
                        .get("fields")
                        .and_then(|fields| fields.get("_paper"))
                        .is_some()
                        || item
                            .get("fields")
                            .and_then(|fields| fields.get("_lxmf"))
                            .and_then(|v| v.get("method"))
                            .and_then(Value::as_str)
                            == Some("paper");
                    if is_paper {
                        paper.push(item.clone());
                    }
                }
            }
            ctx.output.emit_status(&json!({"paper_messages": paper}))
        }
    }
}
