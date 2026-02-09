use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    prelude::*,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Paragraph, Wrap},
};

use super::{TuiSnapshot, TuiTheme};

const SPINNER: [char; 4] = ['|', '/', '-', '\\'];

pub fn render(
    frame: &mut Frame<'_>,
    area: Rect,
    snapshot: &TuiSnapshot,
    theme: &TuiTheme,
    connected: bool,
    spinner_tick: usize,
) {
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(52), Constraint::Percentage(48)])
        .split(area);

    let daemon_text = if connected {
        if snapshot.daemon_running {
            Span::styled(
                "running",
                Style::default()
                    .fg(theme.success)
                    .add_modifier(Modifier::BOLD),
            )
        } else {
            Span::styled(
                "stopped",
                Style::default()
                    .fg(theme.warning)
                    .add_modifier(Modifier::BOLD),
            )
        }
    } else if snapshot.daemon_running {
        Span::styled(
            "rpc degraded",
            Style::default()
                .fg(theme.warning)
                .add_modifier(Modifier::BOLD),
        )
    } else {
        Span::styled(
            "offline",
            Style::default()
                .fg(theme.danger)
                .add_modifier(Modifier::BOLD),
        )
    };

    let pulse = SPINNER[spinner_tick % SPINNER.len()];
    let left_lines = vec![
        Line::from(vec![
            Span::styled("Daemon: ", Style::default().fg(theme.muted)),
            daemon_text,
            Span::styled(
                format!("  {}", pulse),
                Style::default().fg(theme.accent_dim),
            ),
        ]),
        Line::from(vec![
            Span::styled("Messages: ", Style::default().fg(theme.muted)),
            Span::styled(
                snapshot.messages.len().to_string(),
                Style::default().fg(theme.text).add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            Span::styled("Peers: ", Style::default().fg(theme.muted)),
            Span::styled(
                snapshot.peers.len().to_string(),
                Style::default().fg(theme.text).add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            Span::styled("Interfaces: ", Style::default().fg(theme.muted)),
            Span::styled(
                snapshot.interfaces.len().to_string(),
                Style::default().fg(theme.text).add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            Span::styled("Contacts: ", Style::default().fg(theme.muted)),
            Span::styled(
                snapshot.contacts.len().to_string(),
                Style::default().fg(theme.text).add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            Span::styled("Events buffered: ", Style::default().fg(theme.muted)),
            Span::styled(
                snapshot.events.len().to_string(),
                Style::default().fg(theme.text).add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(vec![
            Span::styled("Profile: ", Style::default().fg(theme.muted)),
            Span::styled(&snapshot.profile, Style::default().fg(theme.accent)),
        ]),
        Line::from(vec![
            Span::styled("RPC: ", Style::default().fg(theme.muted)),
            Span::styled(&snapshot.rpc, Style::default().fg(theme.accent_dim)),
        ]),
    ];

    let left = Paragraph::new(left_lines)
        .block(
            Block::default()
                .title(Span::styled(
                    "Live State",
                    Style::default().fg(theme.accent),
                ))
                .borders(Borders::ALL)
                .border_style(Style::default().fg(theme.border_active))
                .border_type(BorderType::Rounded),
        )
        .wrap(Wrap { trim: true });
    frame.render_widget(left, columns[0]);

    let right_lines = vec![
        Line::from(Span::styled(
            "Actions",
            Style::default().fg(theme.text).add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(vec![
            Span::styled(
                "s",
                Style::default()
                    .fg(theme.accent)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                " send message (Peers/Contacts: prefilled destination)",
                Style::default().fg(theme.text),
            ),
        ]),
        Line::from(vec![
            Span::styled(
                "c",
                Style::default()
                    .fg(theme.accent)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                " save selected peer as contact",
                Style::default().fg(theme.text),
            ),
        ]),
        Line::from(vec![
            Span::styled(
                "p",
                Style::default()
                    .fg(theme.accent)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" edit profile settings", Style::default().fg(theme.text)),
        ]),
        Line::from(vec![
            Span::styled(
                "y",
                Style::default()
                    .fg(theme.accent)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" sync selected peer", Style::default().fg(theme.text)),
        ]),
        Line::from(vec![
            Span::styled(
                "/",
                Style::default()
                    .fg(theme.accent)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" peer filter/search", Style::default().fg(theme.text)),
        ]),
        Line::from(vec![
            Span::styled(
                "n",
                Style::default()
                    .fg(theme.accent)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" announce now", Style::default().fg(theme.text)),
        ]),
        Line::from(vec![
            Span::styled(
                "d",
                Style::default()
                    .fg(theme.accent)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" discover peers (burst)", Style::default().fg(theme.text)),
        ]),
        Line::from(vec![
            Span::styled(
                "u",
                Style::default()
                    .fg(theme.accent)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" unpeer selected peer", Style::default().fg(theme.text)),
        ]),
        Line::from(vec![
            Span::styled(
                "a",
                Style::default()
                    .fg(theme.accent)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" apply interface config", Style::default().fg(theme.text)),
        ]),
        Line::from(vec![
            Span::styled(
                "Enter",
                Style::default()
                    .fg(theme.accent)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                " peer details / edit selected interface",
                Style::default().fg(theme.text),
            ),
        ]),
        Line::from(vec![
            Span::styled(
                "r",
                Style::default()
                    .fg(theme.accent)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" start/restart daemon", Style::default().fg(theme.text)),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            "Use Tab / Shift+Tab to move between panes (including Contacts).",
            Style::default().fg(theme.muted),
        )),
    ];

    let right = Paragraph::new(right_lines)
        .block(
            Block::default()
                .title(Span::styled(
                    "Operator Controls",
                    Style::default().fg(theme.accent),
                ))
                .borders(Borders::ALL)
                .border_style(Style::default().fg(theme.border))
                .border_type(BorderType::Rounded),
        )
        .wrap(Wrap { trim: true });
    frame.render_widget(right, columns[1]);
}
