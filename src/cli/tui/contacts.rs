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
    contacts: &[Value],
    selected: usize,
    theme: &TuiTheme,
    active: bool,
) {
    let items = if contacts.is_empty() {
        vec![
            ListItem::new(Line::from(Span::styled(
                "No contacts yet.",
                Style::default().fg(theme.muted),
            ))),
            ListItem::new(Line::from(Span::styled(
                "Press a to add one, or c on Peers to save selected peer.",
                Style::default().fg(theme.accent_dim),
            ))),
        ]
    } else {
        contacts
            .iter()
            .enumerate()
            .map(|(idx, contact)| {
                let alias = contact
                    .get("alias")
                    .and_then(Value::as_str)
                    .unwrap_or("<alias>");
                let hash = contact
                    .get("hash")
                    .and_then(Value::as_str)
                    .unwrap_or("<hash>");
                let notes = contact
                    .get("notes")
                    .and_then(Value::as_str)
                    .filter(|value| !value.trim().is_empty())
                    .unwrap_or("-");
                let pointer = if idx == selected { ">" } else { " " };

                ListItem::new(Line::from(vec![
                    Span::styled(
                        pointer,
                        Style::default()
                            .fg(theme.accent)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::raw(" "),
                    Span::styled(short(alias, 22), Style::default().fg(theme.text)),
                    Span::styled(" -> ", Style::default().fg(theme.muted)),
                    Span::styled(short(hash, 20), Style::default().fg(theme.accent_dim)),
                    Span::styled("  ", Style::default().fg(theme.muted)),
                    Span::styled(short(notes, 26), Style::default().fg(theme.muted)),
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
                "Contacts  (s send, a add, Enter edit, x remove)",
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
