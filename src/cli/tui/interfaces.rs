use ratatui::{prelude::*, widgets::*};
use serde_json::Value;

pub fn render(frame: &mut Frame<'_>, area: Rect, interfaces: &[Value], selected: usize) {
    let items = interfaces
        .iter()
        .enumerate()
        .map(|(idx, iface)| {
            let name = iface.get("name").and_then(Value::as_str).unwrap_or("<unnamed>");
            let kind = iface.get("type").and_then(Value::as_str).unwrap_or("<type>");
            let enabled = iface
                .get("enabled")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let host = iface.get("host").and_then(Value::as_str).unwrap_or("-");
            let port = iface
                .get("port")
                .and_then(Value::as_u64)
                .map(|v| v.to_string())
                .unwrap_or_else(|| "-".into());
            let prefix = if idx == selected { ">" } else { " " };
            ListItem::new(format!(
                "{prefix} {name} [{kind}] enabled={enabled} {host}:{port}"
            ))
        })
        .collect::<Vec<_>>();

    let list = List::new(items)
        .block(
            Block::default()
                .title("Interfaces (a apply)")
                .borders(Borders::ALL),
        )
        .highlight_symbol("> ");
    frame.render_widget(list, area);
}
