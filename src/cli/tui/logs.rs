use ratatui::{
    prelude::*,
    style::Style,
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, List, ListItem},
};

use super::TuiTheme;

pub fn render(frame: &mut Frame<'_>, area: Rect, logs: &[String], theme: &TuiTheme, active: bool) {
    let items = if logs.is_empty() {
        vec![ListItem::new(Line::from(Span::styled(
            "No daemon logs available.",
            Style::default().fg(theme.muted),
        )))]
    } else {
        logs.iter()
            .rev()
            .take(420)
            .map(|line| {
                let color = if line.contains("ERROR") || line.contains("error") {
                    theme.danger
                } else if line.contains("WARN") || line.contains("warning") {
                    theme.warning
                } else {
                    theme.text
                };
                ListItem::new(Line::from(vec![Span::styled(
                    short(line, 220),
                    Style::default().fg(color),
                )]))
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
                "Daemon Logs",
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
