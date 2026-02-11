use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use ratatui::{
    layout::{Constraint, Direction, Layout},
    prelude::*,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, List, ListItem, Paragraph, Wrap},
};
use serde_json::Value;

use super::TuiTheme;

#[derive(Debug, Clone)]
pub struct MessageChannel {
    pub key: String,
    pub indices: Vec<usize>,
    pub message_count: usize,
    pub pending_count: usize,
    pub last_timestamp: Option<i64>,
    pub last_preview: String,
    pub last_status: String,
}

#[derive(Debug, Clone, Default)]
pub struct MessagePaneModel {
    pub channels: Vec<MessageChannel>,
}

impl MessagePaneModel {
    pub fn from_messages(messages: &[Value], self_identity: Option<&str>) -> Self {
        let mut channels: Vec<MessageChannel> = Vec::new();
        let mut by_key: HashMap<String, usize> = HashMap::new();

        for (index, message) in messages.iter().enumerate() {
            let key = channel_key(message, self_identity);
            let timestamp = message_timestamp(message);
            let status = message_status(message);
            let preview = message_preview(message);
            let pending = message_is_pending(message);

            if let Some(channel_index) = by_key.get(&key).copied() {
                let channel = &mut channels[channel_index];
                channel.indices.push(index);
                channel.message_count += 1;
                if pending {
                    channel.pending_count += 1;
                }
                if newer_than(timestamp, channel.last_timestamp) {
                    channel.last_timestamp = timestamp;
                    channel.last_preview = preview;
                    channel.last_status = status;
                }
            } else {
                let mut channel = MessageChannel {
                    key: key.clone(),
                    indices: vec![index],
                    message_count: 1,
                    pending_count: if pending { 1 } else { 0 },
                    last_timestamp: timestamp,
                    last_preview: preview,
                    last_status: status,
                };
                // Keep newest first within a channel to match daemon ordering.
                channel.indices.sort_by_key(|entry| *entry);
                by_key.insert(key, channels.len());
                channels.push(channel);
            }
        }

        channels.sort_by(|a, b| {
            let a_ts = a.last_timestamp.unwrap_or(i64::MIN);
            let b_ts = b.last_timestamp.unwrap_or(i64::MIN);
            b_ts.cmp(&a_ts).then_with(|| a.key.cmp(&b.key))
        });

        Self { channels }
    }

    pub fn channel_len(&self, channel_index: usize) -> usize {
        self.channels
            .get(channel_index)
            .map(|channel| channel.indices.len())
            .unwrap_or(0)
    }

    pub fn message_index(&self, channel_index: usize, message_index: usize) -> Option<usize> {
        self.channels
            .get(channel_index)
            .and_then(|channel| channel.indices.get(message_index))
            .copied()
    }
}

#[allow(clippy::too_many_arguments)]
pub fn render(
    frame: &mut Frame<'_>,
    area: Rect,
    messages: &[Value],
    model: &MessagePaneModel,
    selected_channel: usize,
    selected_message: usize,
    theme: &TuiTheme,
    active: bool,
) {
    let border_color = if active {
        theme.border_active
    } else {
        theme.border
    };

    if messages.is_empty() || model.channels.is_empty() {
        let empty = Paragraph::new(vec![
            Line::from(Span::styled(
                "No messages yet. Press s to compose one.",
                Style::default().fg(theme.muted),
            )),
            Line::from(Span::styled(
                "Messages are grouped into destination channels when available.",
                Style::default().fg(theme.accent_dim),
            )),
        ])
        .block(
            Block::default()
                .title(Span::styled(
                    "Messages  (h/l channels, j/k messages, s compose/reply)",
                    Style::default().fg(theme.accent),
                ))
                .borders(Borders::ALL)
                .border_style(Style::default().fg(border_color))
                .border_type(BorderType::Rounded),
        )
        .wrap(Wrap { trim: true });
        frame.render_widget(empty, area);
        return;
    }

    let selected_channel = selected_channel.min(model.channels.len().saturating_sub(1));
    let channel = &model.channels[selected_channel];
    let selected_message = selected_message.min(channel.indices.len().saturating_sub(1));
    let selected_global_index = channel.indices[selected_message];

    let show_detail = area.width >= 120 && area.height >= 12;
    let (channels_area, list_area, detail_area) = if show_detail {
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(30),
                Constraint::Percentage(36),
                Constraint::Percentage(34),
            ])
            .split(area);
        (chunks[0], chunks[1], Some(chunks[2]))
    } else {
        let chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(36), Constraint::Percentage(64)])
            .split(area);
        (chunks[0], chunks[1], None)
    };

    let channel_items = model
        .channels
        .iter()
        .enumerate()
        .map(|(index, channel)| {
            let pointer = if index == selected_channel { ">" } else { " " };
            let label_style = if index == selected_channel {
                Style::default().fg(theme.text).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(theme.text)
            };
            let pending_label = if channel.pending_count > 0 {
                format!(" pending={}", channel.pending_count)
            } else {
                String::new()
            };
            let ts = relative_time(channel.last_timestamp);
            ListItem::new(vec![
                Line::from(vec![
                    Span::styled(
                        format!("{pointer} "),
                        Style::default()
                            .fg(theme.accent)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(short(&channel_label(&channel.key), 24), label_style),
                    Span::styled(
                        format!("  {}", short(&ts, 7)),
                        Style::default().fg(theme.muted),
                    ),
                ]),
                Line::from(vec![
                    Span::styled("  ", Style::default().fg(theme.muted)),
                    Span::styled(
                        format!("{} msgs{}", channel.message_count, pending_label),
                        Style::default().fg(theme.accent_dim),
                    ),
                    Span::styled(" | ", Style::default().fg(theme.muted)),
                    Span::styled(
                        short(&channel.last_status, 12),
                        Style::default().fg(status_color(&channel.last_status, theme)),
                    ),
                    Span::styled(" | ", Style::default().fg(theme.muted)),
                    Span::styled(
                        short(&channel.last_preview, 24),
                        Style::default().fg(theme.muted),
                    ),
                ]),
            ])
        })
        .collect::<Vec<_>>();
    let channels_widget = List::new(channel_items).block(
        Block::default()
            .title(Span::styled(
                "Channels  (h/l switch)",
                Style::default().fg(theme.accent),
            ))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border_color))
            .border_type(BorderType::Rounded),
    );
    frame.render_widget(channels_widget, channels_area);

    let message_items = channel
        .indices
        .iter()
        .enumerate()
        .map(|(row, global_index)| {
            let message = &messages[*global_index];
            let pointer = if row == selected_message { ">" } else { " " };
            let status = message_status(message);
            let direction = direction_tag(message);
            let timestamp = relative_time(message_timestamp(message));
            let title = field(message, &["title", "subject"]).unwrap_or("untitled");
            let preview = message_preview(message);
            let emphasis = if row == selected_message {
                Style::default().fg(theme.text).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(theme.text)
            };

            ListItem::new(vec![
                Line::from(vec![
                    Span::styled(
                        format!("{pointer} "),
                        Style::default()
                            .fg(theme.accent)
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::styled(short(&timestamp, 7), Style::default().fg(theme.muted)),
                    Span::styled("  ", Style::default().fg(theme.muted)),
                    Span::styled(
                        direction,
                        Style::default().fg(direction_color(message, theme)),
                    ),
                    Span::styled("  ", Style::default().fg(theme.muted)),
                    Span::styled(
                        short(&status, 12),
                        Style::default().fg(status_color(&status, theme)),
                    ),
                ]),
                Line::from(vec![
                    Span::styled("  ", Style::default().fg(theme.muted)),
                    Span::styled(short(title, 24), emphasis),
                    Span::styled(" | ", Style::default().fg(theme.muted)),
                    Span::styled(short(&preview, 34), Style::default().fg(theme.muted)),
                ]),
            ])
        })
        .collect::<Vec<_>>();

    let messages_widget = List::new(message_items).block(
        Block::default()
            .title(Span::styled(
                format!(
                    "Messages in {}  (j/k select, s reply)",
                    short(&channel_label(&channel.key), 22)
                ),
                Style::default().fg(theme.accent),
            ))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border_color))
            .border_type(BorderType::Rounded),
    );
    frame.render_widget(messages_widget, list_area);

    if let Some(detail_area) = detail_area {
        let Some(message) = messages.get(selected_global_index) else {
            return;
        };
        let detail = message_detail(message, &channel.key);
        let detail_widget = Paragraph::new(detail)
            .block(
                Block::default()
                    .title(Span::styled(
                        "Selected Message",
                        Style::default().fg(theme.accent),
                    ))
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(border_color))
                    .border_type(BorderType::Rounded),
            )
            .style(Style::default().fg(theme.text))
            .wrap(Wrap { trim: false });
        frame.render_widget(detail_widget, detail_area);
    }
}

fn channel_key(message: &Value, self_identity: Option<&str>) -> String {
    let source = message_field(message, &["source", "from", "sender"]);
    let destination = message_field(message, &["destination", "to", "recipient"]);
    let self_identity = self_identity
        .map(str::trim)
        .filter(|value| !value.is_empty());

    if let Some(self_identity) = self_identity {
        if destination
            .as_deref()
            .is_some_and(|value| value.eq_ignore_ascii_case(self_identity))
        {
            return source.unwrap_or_else(|| "<unknown>".to_string());
        }
        if source
            .as_deref()
            .is_some_and(|value| value.eq_ignore_ascii_case(self_identity))
        {
            return destination.unwrap_or_else(|| "<unknown>".to_string());
        }
    }

    destination
        .or(source)
        .unwrap_or_else(|| "<unknown>".to_string())
}

fn message_field(message: &Value, keys: &[&str]) -> Option<String> {
    for key in keys {
        if let Some(value) = message
            .get(*key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            return Some(value.to_string());
        }
    }
    None
}

fn field<'a>(message: &'a Value, keys: &[&str]) -> Option<&'a str> {
    for key in keys {
        if let Some(value) = message
            .get(*key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            return Some(value);
        }
    }
    None
}

fn message_preview(message: &Value) -> String {
    if let Some(content) = field(message, &["content", "body", "text"]) {
        let compact = content.split_whitespace().collect::<Vec<_>>().join(" ");
        if !compact.is_empty() {
            return compact;
        }
    }

    if let Some(title) = field(message, &["title", "subject"]) {
        return title.to_string();
    }

    "<no-content>".to_string()
}

fn message_timestamp(message: &Value) -> Option<i64> {
    if let Some(value) = message.get("timestamp") {
        if let Some(ts) = value.as_i64() {
            return Some(ts);
        }
        if let Some(ts) = value.as_u64() {
            return i64::try_from(ts).ok();
        }
        if let Some(text) = value.as_str() {
            return text.trim().parse::<i64>().ok();
        }
    }
    None
}

fn message_status(message: &Value) -> String {
    if let Some(receipt) = field(message, &["receipt_status", "receipt", "delivery_status"]) {
        return receipt.to_string();
    }
    if let Some(status) = field(message, &["status"]) {
        return status.to_string();
    }
    if is_outbound(message) {
        // Daemon message listings may omit receipt/status even after dispatch.
        "sent".to_string()
    } else if is_inbound(message) {
        "received".to_string()
    } else {
        "unknown".to_string()
    }
}

fn message_is_pending(message: &Value) -> bool {
    if !is_outbound(message) {
        return false;
    }
    let status = message_status(message).to_ascii_lowercase();
    !(status.contains("deliver")
        || status.contains("sent")
        || status.contains("ok")
        || status.contains("fail")
        || status.contains("reject")
        || status.contains("cancel")
        || status.contains("error"))
}

fn is_outbound(message: &Value) -> bool {
    field(message, &["direction"])
        .map(|value| value.eq_ignore_ascii_case("out") || value.eq_ignore_ascii_case("outbound"))
        .unwrap_or(false)
}

fn is_inbound(message: &Value) -> bool {
    field(message, &["direction"])
        .map(|value| value.eq_ignore_ascii_case("in") || value.eq_ignore_ascii_case("inbound"))
        .unwrap_or(false)
}

fn direction_tag(message: &Value) -> &'static str {
    if is_outbound(message) {
        "OUT"
    } else if is_inbound(message) {
        "IN"
    } else {
        "?"
    }
}

fn direction_color(message: &Value, theme: &TuiTheme) -> Color {
    if is_outbound(message) {
        theme.accent
    } else if is_inbound(message) {
        theme.success
    } else {
        theme.muted
    }
}

fn message_detail(message: &Value, channel: &str) -> String {
    let id = field(message, &["id"]).unwrap_or("<no-id>");
    let direction = direction_tag(message);
    let status = message_status(message);
    let timestamp = relative_time(message_timestamp(message));
    let source = field(message, &["source", "from", "sender"]).unwrap_or("-");
    let destination = field(message, &["destination", "to", "recipient"]).unwrap_or("-");
    let method = field(message, &["method", "transport"]).unwrap_or("n/a");
    let title = field(message, &["title", "subject"]).unwrap_or("untitled");
    let content = field(message, &["content", "body", "text"]).unwrap_or("<empty>");
    let fields = message
        .get("fields")
        .map(|value| serde_json::to_string_pretty(value).unwrap_or_else(|_| value.to_string()))
        .unwrap_or_else(|| "{}".to_string());

    format!(
        "channel: {channel}\nid: {id}\ndirection: {direction}\nstatus: {status}\ntime: {timestamp}\nsource: {source}\ndestination: {destination}\nmethod: {method}\ntitle: {title}\n\ncontent:\n{content}\n\nfields:\n{fields}"
    )
}

fn status_color(status: &str, theme: &TuiTheme) -> Color {
    let lowered = status.to_ascii_lowercase();
    if lowered.contains("queue")
        || lowered.contains("pending")
        || lowered.contains("defer")
        || lowered.contains("send")
    {
        theme.warning
    } else if lowered.contains("deliver")
        || lowered.contains("sent")
        || lowered.contains("ok")
        || lowered.contains("success")
    {
        theme.success
    } else if lowered.contains("fail")
        || lowered.contains("reject")
        || lowered.contains("cancel")
        || lowered.contains("error")
    {
        theme.danger
    } else {
        theme.muted
    }
}

fn relative_time(timestamp: Option<i64>) -> String {
    let Some(timestamp) = timestamp else {
        return "-".to_string();
    };
    let now = unix_now_i64();
    if timestamp > now {
        return "now".to_string();
    }
    let delta = now.saturating_sub(timestamp);
    if delta < 60 {
        format!("{delta}s")
    } else if delta < 3_600 {
        format!("{}m", delta / 60)
    } else if delta < 86_400 {
        format!("{}h", delta / 3_600)
    } else if delta < 7 * 86_400 {
        format!("{}d", delta / 86_400)
    } else {
        timestamp.to_string()
    }
}

fn channel_label(channel: &str) -> String {
    if channel == "<unknown>" {
        return channel.to_string();
    }
    if channel.len() == 32 && channel.chars().all(|ch| ch.is_ascii_hexdigit()) {
        return format!("{}..{}", &channel[..6], &channel[26..]);
    }
    channel.to_string()
}

fn newer_than(left: Option<i64>, right: Option<i64>) -> bool {
    left.unwrap_or(i64::MIN) > right.unwrap_or(i64::MIN)
}

fn unix_now_i64() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|value| i64::try_from(value.as_secs()).ok())
        .unwrap_or(0)
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

#[cfg(test)]
mod tests {
    use super::{message_status, MessagePaneModel};
    use serde_json::json;

    #[test]
    fn message_model_groups_by_counterparty_when_self_identity_known() {
        let messages = vec![
            json!({
                "id": "out-1",
                "source": "self",
                "destination": "peer-a",
                "direction": "out",
                "timestamp": 100
            }),
            json!({
                "id": "in-1",
                "source": "peer-a",
                "destination": "self",
                "direction": "in",
                "timestamp": 99
            }),
            json!({
                "id": "out-2",
                "source": "self",
                "destination": "peer-b",
                "direction": "out",
                "timestamp": 98
            }),
        ];

        let model = MessagePaneModel::from_messages(&messages, Some("self"));
        assert_eq!(model.channels.len(), 2);
        assert_eq!(model.channels[0].key, "peer-a");
        assert_eq!(model.channels[0].message_count, 2);
        assert_eq!(model.channels[1].key, "peer-b");
        assert_eq!(model.channels[1].message_count, 1);
    }

    #[test]
    fn message_model_resolves_channel_message_indices() {
        let messages = vec![
            json!({"id":"a","source":"self","destination":"peer-a","direction":"out","timestamp":200}),
            json!({"id":"b","source":"self","destination":"peer-a","direction":"out","timestamp":150}),
            json!({"id":"c","source":"self","destination":"peer-b","direction":"out","timestamp":100}),
        ];
        let model = MessagePaneModel::from_messages(&messages, Some("self"));

        let first_channel_len = model.channel_len(0);
        assert_eq!(first_channel_len, 2);
        assert_eq!(model.message_index(0, 0), Some(0));
        assert_eq!(model.message_index(0, 1), Some(1));
        assert_eq!(model.message_index(1, 0), Some(2));
    }

    #[test]
    fn outbound_without_explicit_status_defaults_to_sent() {
        let outbound = json!({
            "id": "out-1",
            "source": "self",
            "destination": "peer-a",
            "direction": "out",
            "timestamp": 100
        });
        assert_eq!(message_status(&outbound), "sent");
    }
}
