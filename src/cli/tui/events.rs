use ratatui::{prelude::*, widgets::*};

pub fn render(frame: &mut Frame<'_>, area: Rect, events: &[String]) {
    let items = events
        .iter()
        .rev()
        .take(200)
        .map(|line| ListItem::new(line.clone()))
        .collect::<Vec<_>>();

    let list = List::new(items)
        .block(Block::default().title("Events").borders(Borders::ALL))
        .highlight_symbol("> ");
    frame.render_widget(list, area);
}
