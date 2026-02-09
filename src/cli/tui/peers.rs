use ratatui::{
    prelude::*,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, List, ListItem},
};
use serde_json::Value;

use super::{peer_display_name, TuiTheme};

pub fn render(
    frame: &mut Frame<'_>,
    area: Rect,
    peers: &[Value],
    selected: usize,
    theme: &TuiTheme,
    active: bool,
    filter: &str,
    filter_editing: bool,
) {
    let items = if peers.is_empty() {
        let mut lines = vec![
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
        ];
        if !filter.trim().is_empty() {
            lines.insert(
                0,
                ListItem::new(Line::from(Span::styled(
                    format!("No peers match filter '{}'.", filter.trim()),
                    Style::default().fg(theme.warning),
                ))),
            );
        }
        lines
    } else {
        peers
            .iter()
            .enumerate()
            .map(|(idx, peer)| {
                let hash = peer
                    .get("peer")
                    .and_then(Value::as_str)
                    .unwrap_or("<unknown>");
                let name = peer_display_name(peer);
                let contact_alias = peer
                    .get("contact_alias")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|value| !value.is_empty());
                let primary = contact_alias.or(name).unwrap_or(hash);
                let secondary = if name.is_some() {
                    format!("hash={} ", short(hash, 20))
                } else {
                    String::new()
                };
                let seen_count = peer
                    .get("seen_count")
                    .and_then(Value::as_u64)
                    .unwrap_or_default();
                let pointer = if idx == selected { ">" } else { " " };

                ListItem::new(Line::from(vec![
                    Span::styled(
                        pointer,
                        Style::default()
                            .fg(theme.accent)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::raw(" "),
                    Span::styled(short(primary, 28), Style::default().fg(theme.text)),
                    Span::styled("  ", Style::default().fg(theme.text)),
                    Span::styled(secondary, Style::default().fg(theme.muted)),
                    Span::styled(
                        format!("seen={seen_count}"),
                        Style::default().fg(theme.accent_dim),
                    ),
                ]))
            })
            .collect::<Vec<_>>()
    };

    let border_color = if active {
        theme.border_active
    } else {
        theme.border
    };
    let filter_label = if filter.trim().is_empty() {
        "all".to_string()
    } else {
        format!("'{}'", short(filter.trim(), 28))
    };
    let edit_suffix = if filter_editing { " (typing)" } else { "" };

    let list = List::new(items).block(
        Block::default()
            .title(Span::styled(
                format!(
                    "Peers  (s send, c add contact, / filter{edit_suffix}, Enter details, y sync, u unpeer)  filter={filter_label}"
                ),
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
