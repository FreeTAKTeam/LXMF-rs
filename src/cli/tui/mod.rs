mod dashboard;
mod events;
mod interfaces;
mod logs;
mod messages;
mod peers;

use anyhow::{anyhow, Result};
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Modifier, Style};
use ratatui::text::Line;
use ratatui::widgets::{Block, Borders, Paragraph, Tabs};
use ratatui::Terminal;
use serde_json::{json, Value};
use std::io::{self, Write};
use std::time::{Duration, Instant};

use crate::cli::app::{RuntimeContext, TuiCommand};
use crate::cli::daemon::DaemonSupervisor;
use crate::cli::profile::load_reticulum_config;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Pane {
    Dashboard,
    Messages,
    Peers,
    Interfaces,
    Events,
    Logs,
}

impl Pane {
    fn all() -> [Self; 6] {
        [
            Self::Dashboard,
            Self::Messages,
            Self::Peers,
            Self::Interfaces,
            Self::Events,
            Self::Logs,
        ]
    }

    fn title(self) -> &'static str {
        match self {
            Self::Dashboard => "Dashboard",
            Self::Messages => "Messages",
            Self::Peers => "Peers",
            Self::Interfaces => "Interfaces",
            Self::Events => "Events",
            Self::Logs => "Logs",
        }
    }
}

#[derive(Debug, Default)]
pub struct TuiSnapshot {
    pub profile: String,
    pub rpc: String,
    pub daemon_running: bool,
    pub messages: Vec<Value>,
    pub peers: Vec<Value>,
    pub interfaces: Vec<Value>,
    pub events: Vec<String>,
    pub logs: Vec<String>,
}

#[derive(Debug)]
struct TuiState {
    pane: Pane,
    selected_message: usize,
    selected_peer: usize,
    selected_interface: usize,
    status_line: String,
    snapshot: TuiSnapshot,
}

pub fn run_tui(ctx: &RuntimeContext, command: &TuiCommand) -> Result<()> {
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    enable_raw_mode()?;

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    terminal.clear()?;

    let mut state = TuiState {
        pane: Pane::Dashboard,
        selected_message: 0,
        selected_peer: 0,
        selected_interface: 0,
        status_line: "ready".into(),
        snapshot: TuiSnapshot {
            profile: ctx.profile_name.clone(),
            rpc: ctx.profile_settings.rpc.clone(),
            ..TuiSnapshot::default()
        },
    };

    let mut last_refresh = Instant::now() - Duration::from_millis(command.refresh_ms);

    let run_result = loop {
        if last_refresh.elapsed() >= Duration::from_millis(command.refresh_ms.max(100)) {
            if let Err(err) = refresh_snapshot(ctx, &mut state.snapshot) {
                state.status_line = format!("refresh error: {err}");
            }
            last_refresh = Instant::now();
        }

        terminal.draw(|frame| draw(frame, &state))?;

        if event::poll(Duration::from_millis(100))? {
            let event = event::read()?;
            if let Event::Key(key) = event {
                if key.kind != KeyEventKind::Press {
                    continue;
                }

                match key.code {
                    KeyCode::Char('q') => break Ok(()),
                    KeyCode::Tab => {
                        state.pane = next_pane(state.pane);
                    }
                    KeyCode::Char('j') | KeyCode::Down => increment_selection(&mut state),
                    KeyCode::Char('k') | KeyCode::Up => decrement_selection(&mut state),
                    KeyCode::Char('r') => {
                        state.status_line = match restart_daemon(ctx) {
                            Ok(msg) => msg,
                            Err(err) => format!("restart failed: {err}"),
                        };
                        let _ = refresh_snapshot(ctx, &mut state.snapshot);
                    }
                    KeyCode::Char('n') => {
                        state.status_line = match ctx.rpc.call("announce_now", None) {
                            Ok(_) => "announce sent".into(),
                            Err(err) => format!("announce failed: {err}"),
                        };
                    }
                    KeyCode::Char('a') => {
                        state.status_line = match apply_interfaces(ctx) {
                            Ok(msg) => msg,
                            Err(err) => format!("apply failed: {err}"),
                        };
                    }
                    KeyCode::Char('y') => {
                        state.status_line = match sync_selected_peer(ctx, &state) {
                            Ok(msg) => msg,
                            Err(err) => format!("sync failed: {err}"),
                        };
                    }
                    KeyCode::Char('u') => {
                        state.status_line = match unpeer_selected_peer(ctx, &state) {
                            Ok(msg) => msg,
                            Err(err) => format!("unpeer failed: {err}"),
                        };
                        let _ = refresh_snapshot(ctx, &mut state.snapshot);
                    }
                    KeyCode::Char('s') => {
                        state.status_line = match prompt_send_message(&mut terminal, ctx) {
                            Ok(msg) => msg,
                            Err(err) => format!("send failed: {err}"),
                        };
                        let _ = refresh_snapshot(ctx, &mut state.snapshot);
                    }
                    KeyCode::Char('e') => {
                        if let Err(err) = refresh_snapshot(ctx, &mut state.snapshot) {
                            state.status_line = format!("refresh error: {err}");
                        } else {
                            state.status_line = "refreshed".into();
                        }
                    }
                    _ => {}
                }
            }
        }
    };

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    run_result
}

fn draw(frame: &mut ratatui::Frame<'_>, state: &TuiState) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(1),
            Constraint::Length(2),
        ])
        .split(frame.area());

    let tabs = Tabs::new(
        Pane::all()
            .iter()
            .map(|pane| Line::from(pane.title()))
            .collect::<Vec<_>>(),
    )
        .select(index_of(state.pane))
        .block(Block::default().title("lxmf tui").borders(Borders::ALL))
        .highlight_style(Style::default().add_modifier(Modifier::BOLD));
    frame.render_widget(tabs, chunks[0]);

    match state.pane {
        Pane::Dashboard => dashboard::render(frame, chunks[1], &state.snapshot),
        Pane::Messages => messages::render(
            frame,
            chunks[1],
            &state.snapshot.messages,
            state.selected_message,
        ),
        Pane::Peers => peers::render(frame, chunks[1], &state.snapshot.peers, state.selected_peer),
        Pane::Interfaces => interfaces::render(
            frame,
            chunks[1],
            &state.snapshot.interfaces,
            state.selected_interface,
        ),
        Pane::Events => events::render(frame, chunks[1], &state.snapshot.events),
        Pane::Logs => logs::render(frame, chunks[1], &state.snapshot.logs),
    }

    let status = Paragraph::new(state.status_line.as_str())
        .block(Block::default().title("Status").borders(Borders::ALL));
    frame.render_widget(status, chunks[2]);
}

fn refresh_snapshot(ctx: &RuntimeContext, snapshot: &mut TuiSnapshot) -> Result<()> {
    let messages = ctx.rpc.call("list_messages", None)?;
    snapshot.messages = as_vec(messages);

    let peers = ctx.rpc.call("list_peers", None)?;
    snapshot.peers = as_vec(peers);

    let interfaces = match ctx.rpc.call("list_interfaces", None) {
        Ok(v) => as_vec(v),
        Err(_) => {
            let local = load_reticulum_config(&ctx.profile_name)?;
            serde_json::to_value(local.interfaces)
                .ok()
                .and_then(|v| v.as_array().cloned())
                .unwrap_or_default()
        }
    };
    snapshot.interfaces = interfaces;

    let daemon_status = ctx
        .rpc
        .call("daemon_status_ex", None)
        .unwrap_or_else(|_| json!({"running": false}));
    snapshot.daemon_running = daemon_status
        .get("running")
        .and_then(Value::as_bool)
        .unwrap_or(false);

    while let Some(event) = ctx.rpc.poll_event()? {
        snapshot
            .events
            .push(serde_json::to_string(&event).unwrap_or_else(|_| "<event>".into()));
        if snapshot.events.len() > 400 {
            let remove = snapshot.events.len().saturating_sub(400);
            snapshot.events.drain(0..remove);
        }
    }

    let supervisor = DaemonSupervisor::new(&ctx.profile_name, ctx.profile_settings.clone());
    snapshot.logs = supervisor.logs(400).unwrap_or_default();
    snapshot.profile = ctx.profile_name.clone();
    snapshot.rpc = ctx.profile_settings.rpc.clone();

    Ok(())
}

fn prompt_send_message(
    terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
    ctx: &RuntimeContext,
) -> Result<String> {
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;

    let destination = prompt_line("destination (hex)")?;
    let source = prompt_line("source (hex)")?;
    let title = prompt_line("title")?;
    let content = prompt_line("content")?;

    execute!(terminal.backend_mut(), EnterAlternateScreen)?;
    enable_raw_mode()?;

    if destination.is_empty() || source.is_empty() || content.is_empty() {
        return Err(anyhow!(
            "destination, source, and content are required for send"
        ));
    }

    let params = json!({
        "id": format!("tui-{}", chrono_like_now_secs()),
        "destination": destination,
        "source": source,
        "title": title,
        "content": content,
    });

    match ctx.rpc.call("send_message_v2", Some(params.clone())) {
        Ok(_) => Ok("message queued".into()),
        Err(_) => {
            ctx.rpc.call("send_message", Some(params))?;
            Ok("message queued (legacy api)".into())
        }
    }
}

fn prompt_line(label: &str) -> Result<String> {
    print!("{label}: ");
    io::stdout().flush()?;
    let mut line = String::new();
    io::stdin().read_line(&mut line)?;
    Ok(line.trim().to_string())
}

fn restart_daemon(ctx: &RuntimeContext) -> Result<String> {
    let supervisor = DaemonSupervisor::new(&ctx.profile_name, ctx.profile_settings.clone());
    supervisor.restart(None, Some(true), None)?;
    Ok("daemon restarted".into())
}

fn apply_interfaces(ctx: &RuntimeContext) -> Result<String> {
    let config = load_reticulum_config(&ctx.profile_name)?;
    ctx.rpc.call(
        "set_interfaces",
        Some(json!({
            "interfaces": config.interfaces,
        })),
    )?;
    let _ = ctx.rpc.call("reload_config", None);
    Ok("interfaces applied".into())
}

fn sync_selected_peer(ctx: &RuntimeContext, state: &TuiState) -> Result<String> {
    let Some(peer) = selected_peer_name(state) else {
        return Err(anyhow!("no peer selected"));
    };
    ctx.rpc.call("peer_sync", Some(json!({ "peer": peer })))?;
    Ok(format!("synced {peer}"))
}

fn unpeer_selected_peer(ctx: &RuntimeContext, state: &TuiState) -> Result<String> {
    let Some(peer) = selected_peer_name(state) else {
        return Err(anyhow!("no peer selected"));
    };
    ctx.rpc.call("peer_unpeer", Some(json!({ "peer": peer })))?;
    Ok(format!("unpeered {peer}"))
}

fn selected_peer_name(state: &TuiState) -> Option<String> {
    state
        .snapshot
        .peers
        .get(state.selected_peer)
        .and_then(|peer| peer.get("peer"))
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
}

fn as_vec(value: Value) -> Vec<Value> {
    value.as_array().cloned().unwrap_or_default()
}

fn increment_selection(state: &mut TuiState) {
    match state.pane {
        Pane::Messages => {
            if state.selected_message + 1 < state.snapshot.messages.len() {
                state.selected_message += 1;
            }
        }
        Pane::Peers => {
            if state.selected_peer + 1 < state.snapshot.peers.len() {
                state.selected_peer += 1;
            }
        }
        Pane::Interfaces => {
            if state.selected_interface + 1 < state.snapshot.interfaces.len() {
                state.selected_interface += 1;
            }
        }
        _ => {}
    }
}

fn decrement_selection(state: &mut TuiState) {
    match state.pane {
        Pane::Messages => {
            state.selected_message = state.selected_message.saturating_sub(1);
        }
        Pane::Peers => {
            state.selected_peer = state.selected_peer.saturating_sub(1);
        }
        Pane::Interfaces => {
            state.selected_interface = state.selected_interface.saturating_sub(1);
        }
        _ => {}
    }
}

fn next_pane(current: Pane) -> Pane {
    match current {
        Pane::Dashboard => Pane::Messages,
        Pane::Messages => Pane::Peers,
        Pane::Peers => Pane::Interfaces,
        Pane::Interfaces => Pane::Events,
        Pane::Events => Pane::Logs,
        Pane::Logs => Pane::Dashboard,
    }
}

fn index_of(pane: Pane) -> usize {
    match pane {
        Pane::Dashboard => 0,
        Pane::Messages => 1,
        Pane::Peers => 2,
        Pane::Interfaces => 3,
        Pane::Events => 4,
        Pane::Logs => 5,
    }
}

fn chrono_like_now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
