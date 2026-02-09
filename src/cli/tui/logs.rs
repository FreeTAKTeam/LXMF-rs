use ratatui::{prelude::*, widgets::*};

pub fn render(frame: &mut Frame<'_>, area: Rect, logs: &[String]) {
    let items = logs
        .iter()
        .rev()
        .take(400)
        .map(|line| ListItem::new(line.clone()))
        .collect::<Vec<_>>();

    let list = List::new(items)
        .block(Block::default().title("Daemon Logs").borders(Borders::ALL))
        .highlight_symbol("> ");
    frame.render_widget(list, area);
}
