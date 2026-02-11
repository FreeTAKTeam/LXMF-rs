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
    interfaces: &[Value],
    selected: usize,
    theme: &TuiTheme,
    active: bool,
) {
    let items = if interfaces.is_empty() {
        vec![
            ListItem::new(Line::from(Span::styled(
                "No interfaces configured.",
                Style::default().fg(theme.muted),
            ))),
            ListItem::new(Line::from(Span::styled(
                "Press i to add a new interface.",
                Style::default().fg(theme.accent_dim),
            ))),
        ]
    } else {
        interfaces
            .iter()
            .enumerate()
            .map(|(idx, iface)| {
                let name = iface
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or("<unnamed>");
                let kind = iface
                    .get("type")
                    .and_then(Value::as_str)
                    .unwrap_or("<type>");
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

                let pointer = if idx == selected { ">" } else { " " };
                let enabled_text = if enabled { "enabled" } else { "disabled" };
                let enabled_color = if enabled {
                    theme.success
                } else {
                    theme.warning
                };

                ListItem::new(Line::from(vec![
                    Span::styled(
                        pointer,
                        Style::default()
                            .fg(theme.accent)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::raw(" "),
                    Span::styled(short(name, 18), Style::default().fg(theme.text)),
                    Span::styled(" [", Style::default().fg(theme.muted)),
                    Span::styled(short(kind, 12), Style::default().fg(theme.accent_dim)),
                    Span::styled("] ", Style::default().fg(theme.muted)),
                    Span::styled(enabled_text, Style::default().fg(enabled_color)),
                    Span::styled("  ", Style::default().fg(theme.muted)),
                    Span::styled(short(host, 20), Style::default().fg(theme.text)),
                    Span::styled(":", Style::default().fg(theme.muted)),
                    Span::styled(port, Style::default().fg(theme.text)),
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
                "Interfaces  (Enter edit, i add, t toggle, x remove, a apply)",
                Style::default().fg(theme.accent),
            ))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border_color))
            .border_type(BorderType::Rounded),
    );
    frame.render_widget(list, area);
}

fn short(input: &str, max: usize) -> String {
    if max == 0 {
        return String::new();
    }

    let len = input.chars().count();
    if len <= max {
        return input.to_string();
    }

    if max == 1 {
        return "~".to_string();
    }

    input
        .chars()
        .take(max.saturating_sub(1))
        .collect::<String>()
        + "~"
}
