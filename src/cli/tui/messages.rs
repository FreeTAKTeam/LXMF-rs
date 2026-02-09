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
    messages: &[Value],
    selected: usize,
    theme: &TuiTheme,
    active: bool,
) {
    let items = if messages.is_empty() {
        vec![ListItem::new(Line::from(Span::styled(
            "No messages yet. Press s to send one.",
            Style::default().fg(theme.muted),
        )))]
    } else {
        messages
            .iter()
            .enumerate()
            .map(|(idx, message)| {
                let id = message
                    .get("id")
                    .and_then(Value::as_str)
                    .unwrap_or("<no-id>");
                let destination = message
                    .get("destination")
                    .and_then(Value::as_str)
                    .unwrap_or("<no-destination>");
                let title = message
                    .get("title")
                    .and_then(Value::as_str)
                    .filter(|v| !v.is_empty())
                    .unwrap_or("untitled");
                let status = message
                    .get("status")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown");

                let status_color = match status {
                    "queued" | "sending" => theme.warning,
                    "sent" | "delivered" => theme.success,
                    "failed" | "rejected" | "cancelled" => theme.danger,
                    _ => theme.muted,
                };

                let pointer = if idx == selected { ">" } else { " " };
                ListItem::new(Line::from(vec![
                    Span::styled(
                        pointer,
                        Style::default()
                            .fg(theme.accent)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::raw(" "),
                    Span::styled(short(status, 10), Style::default().fg(status_color)),
                    Span::styled(" | ", Style::default().fg(theme.muted)),
                    Span::styled(short(id, 16), Style::default().fg(theme.text)),
                    Span::styled(" -> ", Style::default().fg(theme.muted)),
                    Span::styled(
                        short(destination, 16),
                        Style::default().fg(theme.accent_dim),
                    ),
                    Span::styled(" | ", Style::default().fg(theme.muted)),
                    Span::styled(short(title, 32), Style::default().fg(theme.text)),
                ]))
            })
            .collect::<Vec<_>>()
    };

    let border_color = if active {
        theme.border_active
    } else {
        theme.border
    };
    let list = List::new(items)
        .block(
            Block::default()
                .title(Span::styled(
                    "Messages  (j/k select, s send)",
                    Style::default().fg(theme.accent),
                ))
                .borders(Borders::ALL)
                .border_style(Style::default().fg(border_color))
                .border_type(BorderType::Rounded),
        )
        .highlight_symbol(" > ");
    frame.render_widget(list, area);
}

fn short(input: &str, max: usize) -> String {
    if input.len() <= max {
        return input.to_string();
    }
    let take = max.saturating_sub(1);
    format!("{}~", &input[..take])
}
