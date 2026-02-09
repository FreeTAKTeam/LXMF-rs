use ratatui::{
    prelude::*,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, List, ListItem},
};
use serde_json::Value;

use super::TuiTheme;

pub fn render(
    frame: &mut Frame<'_>,
    area: Rect,
    peers: &[Value],
    selected: usize,
    theme: &TuiTheme,
    active: bool,
) {
    let items = if peers.is_empty() {
        vec![
            ListItem::new(Line::from(Span::styled(
                "No peers discovered yet.",
                Style::default().fg(theme.muted),
            ))),
            ListItem::new(Line::from(Span::styled(
                "Press d for discovery sweep or n for single announce.",
                Style::default().fg(theme.accent_dim),
            ))),
            ListItem::new(Line::from(Span::styled(
                "Verify interfaces are enabled and applied (i/t/x/a).",
                Style::default().fg(theme.accent_dim),
            ))),
        ]
    } else {
        peers
            .iter()
            .enumerate()
            .map(|(idx, peer)| {
                let name = peer
                    .get("peer")
                    .and_then(Value::as_str)
                    .unwrap_or("<unknown>");
                let last_seen = peer
                    .get("last_seen")
                    .and_then(Value::as_i64)
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| "n/a".into());
                let pointer = if idx == selected { ">" } else { " " };

                ListItem::new(Line::from(vec![
                    Span::styled(
                        pointer,
                        Style::default()
                            .fg(theme.accent)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::raw(" "),
                    Span::styled(short(name, 28), Style::default().fg(theme.text)),
                    Span::styled("  last_seen=", Style::default().fg(theme.muted)),
                    Span::styled(last_seen, Style::default().fg(theme.accent_dim)),
                ]))
            })
            .collect::<Vec<_>>()
    };

    let border_color = if active {
        theme.border_active
    } else {
        theme.border
    };
    let list = List::new(items).block(
        Block::default()
            .title(Span::styled(
                "Peers  (d discover, n announce, y sync, u unpeer)",
                Style::default().fg(theme.accent),
            ))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border_color))
            .border_type(BorderType::Rounded),
    );
    frame.render_widget(list, area);
}

fn short(input: &str, max: usize) -> String {
    if input.len() <= max {
        return input.to_string();
    }
    let take = max.saturating_sub(1);
    format!("{}~", &input[..take])
}
