use ratatui::{prelude::*, widgets::*};
use serde_json::Value;

pub fn render(frame: &mut Frame<'_>, area: Rect, peers: &[Value], selected: usize) {
    let items = peers
        .iter()
        .enumerate()
        .map(|(idx, peer)| {
            let name = peer.get("peer").and_then(Value::as_str).unwrap_or("<unknown>");
            let last_seen = peer
                .get("last_seen")
                .and_then(Value::as_i64)
                .map(|v| v.to_string())
                .unwrap_or_else(|| "n/a".into());
            let prefix = if idx == selected { ">" } else { " " };
            ListItem::new(format!("{prefix} {name} (last_seen={last_seen})"))
        })
        .collect::<Vec<_>>();

    let list = List::new(items)
        .block(
            Block::default()
                .title("Peers (y sync, u unpeer)")
                .borders(Borders::ALL),
        )
        .highlight_symbol("> ");
    frame.render_widget(list, area);
}
