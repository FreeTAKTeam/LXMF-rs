use ratatui::{prelude::*, widgets::*};

use super::TuiSnapshot;

pub fn render(frame: &mut Frame<'_>, area: Rect, snapshot: &TuiSnapshot) {
    let mut lines = Vec::new();
    lines.push(Line::from(format!("Profile: {}", snapshot.profile)));
    lines.push(Line::from(format!("RPC: {}", snapshot.rpc)));
    lines.push(Line::from(format!("Daemon running: {}", snapshot.daemon_running)));
    lines.push(Line::from(format!("Messages: {}", snapshot.messages.len())));
    lines.push(Line::from(format!("Peers: {}", snapshot.peers.len())));
    lines.push(Line::from(format!("Interfaces: {}", snapshot.interfaces.len())));
    lines.push(Line::from(format!("Events buffered: {}", snapshot.events.len())));
    lines.push(Line::from(""));
    lines.push(Line::from("Keys: q quit | Tab switch | r restart daemon | n announce"));

    let paragraph = Paragraph::new(lines)
        .block(Block::default().title("Dashboard").borders(Borders::ALL))
        .wrap(Wrap { trim: true });
    frame.render_widget(paragraph, area);
}
