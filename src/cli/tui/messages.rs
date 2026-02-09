use ratatui::{prelude::*, widgets::*};
use serde_json::Value;

pub fn render(frame: &mut Frame<'_>, area: Rect, messages: &[Value], selected: usize) {
    let items = messages
        .iter()
        .enumerate()
        .map(|(idx, message)| {
            let id = message.get("id").and_then(Value::as_str).unwrap_or("<no-id>");
            let destination = message
                .get("destination")
                .and_then(Value::as_str)
                .unwrap_or("<no-destination>");
            let status = message
                .get("status")
                .and_then(Value::as_str)
                .unwrap_or("unknown");
            let prefix = if idx == selected { ">" } else { " " };
            ListItem::new(format!("{prefix} {id} -> {destination} ({status})"))
        })
        .collect::<Vec<_>>();

    let list = List::new(items)
        .block(
            Block::default()
                .title("Messages (j/k select, s send)")
                .borders(Borders::ALL),
        )
        .highlight_symbol("> ");
    frame.render_widget(list, area);
}
