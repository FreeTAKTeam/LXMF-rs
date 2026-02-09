use ratatui::{
    prelude::*,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, List, ListItem},
};

use super::TuiTheme;

pub fn render(
    frame: &mut Frame<'_>,
    area: Rect,
    events: &[String],
    theme: &TuiTheme,
    active: bool,
) {
    let items = if events.is_empty() {
        vec![ListItem::new(Line::from(Span::styled(
            "No events yet.",
            Style::default().fg(theme.muted),
        )))]
    } else {
        events
            .iter()
            .rev()
            .take(220)
            .map(|line| {
                let color = if line.contains("error") || line.contains("failed") {
                    theme.danger
                } else if line.contains("warning") {
                    theme.warning
                } else if line.contains("receipt") || line.contains("delivered") {
                    theme.success
                } else {
                    theme.text
                };
                ListItem::new(Line::from(vec![
                    Span::styled(
                        "* ",
                        Style::default()
                            .fg(theme.accent_dim)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(short(line, 220), Style::default().fg(color)),
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
            .title(Span::styled("Events", Style::default().fg(theme.accent)))
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
