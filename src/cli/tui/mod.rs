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
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Clear, Paragraph, Tabs, Wrap};
use ratatui::Terminal;
use serde_json::{json, Value};
use std::io::{self, Write};
use std::time::{Duration, Instant};

use crate::cli::app::{RuntimeContext, TuiCommand};
use crate::cli::daemon::DaemonSupervisor;
use crate::cli::profile::load_reticulum_config;
use crate::cli::rpc_client::RpcClient;

const FAST_RPC_CONNECT_TIMEOUT: Duration = Duration::from_millis(250);
const FAST_RPC_IO_TIMEOUT: Duration = Duration::from_millis(800);
const SPINNER: [char; 4] = ['|', '/', '-', '\\'];

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

#[derive(Debug, Clone, Copy)]
pub struct TuiTheme {
    pub border: Color,
    pub border_active: Color,
    pub accent: Color,
    pub accent_dim: Color,
    pub text: Color,
    pub muted: Color,
    pub success: Color,
    pub warning: Color,
    pub danger: Color,
}

impl Default for TuiTheme {
    fn default() -> Self {
        Self {
            border: Color::DarkGray,
            border_active: Color::Cyan,
            accent: Color::Cyan,
            accent_dim: Color::Blue,
            text: Color::White,
            muted: Color::Gray,
            success: Color::Green,
            warning: Color::Yellow,
            danger: Color::Red,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StatusLevel {
    Info,
    Success,
    Warning,
    Error,
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
    status_level: StatusLevel,
    snapshot: TuiSnapshot,
    theme: TuiTheme,
    connected: bool,
    first_refresh_done: bool,
    spinner_tick: usize,
    started_at: Instant,
    last_refresh_ms: Option<u128>,
}

#[derive(Debug)]
struct RefreshOutcome {
    connected: bool,
    warning: Option<String>,
    new_events: usize,
    elapsed_ms: u128,
}

pub fn run_tui(ctx: &RuntimeContext, command: &TuiCommand) -> Result<()> {
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    enable_raw_mode()?;

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    terminal.clear()?;

    let fast_rpc = RpcClient::new_with_timeouts(
        &ctx.profile_settings.rpc,
        FAST_RPC_CONNECT_TIMEOUT,
        FAST_RPC_IO_TIMEOUT,
        FAST_RPC_IO_TIMEOUT,
    );

    let mut state = TuiState {
        pane: Pane::Dashboard,
        selected_message: 0,
        selected_peer: 0,
        selected_interface: 0,
        status_line: format!("Opening LXMF TUI on {}...", ctx.profile_settings.rpc),
        status_level: StatusLevel::Info,
        snapshot: TuiSnapshot {
            profile: ctx.profile_name.clone(),
            rpc: ctx.profile_settings.rpc.clone(),
            ..TuiSnapshot::default()
        },
        theme: TuiTheme::default(),
        connected: false,
        first_refresh_done: false,
        spinner_tick: 0,
        started_at: Instant::now(),
        last_refresh_ms: None,
    };

    apply_refresh(ctx, &fast_rpc, &mut state, true);

    let mut last_refresh = Instant::now() - Duration::from_millis(command.refresh_ms);

    let run_result = loop {
        state.spinner_tick = state.spinner_tick.wrapping_add(1);

        if last_refresh.elapsed() >= Duration::from_millis(command.refresh_ms.max(150)) {
            apply_refresh(ctx, &fast_rpc, &mut state, false);
            last_refresh = Instant::now();
        }

        terminal.draw(|frame| draw(frame, &state))?;

        if event::poll(Duration::from_millis(60))? {
            let event = event::read()?;
            if let Event::Key(key) = event {
                if key.kind != KeyEventKind::Press {
                    continue;
                }

                match key.code {
                    KeyCode::Char('q') => break Ok(()),
                    KeyCode::Tab => {
                        state.pane = next_pane(state.pane);
                        let pane_title = state.pane.title().to_string();
                        set_status(&mut state, StatusLevel::Info, pane_title);
                    }
                    KeyCode::BackTab => {
                        state.pane = previous_pane(state.pane);
                        let pane_title = state.pane.title().to_string();
                        set_status(&mut state, StatusLevel::Info, pane_title);
                    }
                    KeyCode::Char('j') | KeyCode::Down => increment_selection(&mut state),
                    KeyCode::Char('k') | KeyCode::Up => decrement_selection(&mut state),
                    KeyCode::Char('r') => {
                        set_status(&mut state, StatusLevel::Info, "Restarting daemon...");
                        match restart_daemon(ctx) {
                            Ok(msg) => set_status(&mut state, StatusLevel::Success, msg),
                            Err(err) => set_status(
                                &mut state,
                                StatusLevel::Error,
                                format!("restart failed: {err}"),
                            ),
                        }
                        apply_refresh(ctx, &fast_rpc, &mut state, true);
                    }
                    KeyCode::Char('n') => match fast_rpc.call("announce_now", None) {
                        Ok(_) => set_status(&mut state, StatusLevel::Success, "announce sent"),
                        Err(err) => set_status(
                            &mut state,
                            StatusLevel::Error,
                            format!("announce failed: {err}"),
                        ),
                    },
                    KeyCode::Char('a') => {
                        match apply_interfaces(ctx, &fast_rpc) {
                            Ok(msg) => set_status(&mut state, StatusLevel::Success, msg),
                            Err(err) => set_status(
                                &mut state,
                                StatusLevel::Error,
                                format!("apply failed: {err}"),
                            ),
                        }
                        apply_refresh(ctx, &fast_rpc, &mut state, true);
                    }
                    KeyCode::Char('y') => match sync_selected_peer(&fast_rpc, &state) {
                        Ok(msg) => set_status(&mut state, StatusLevel::Success, msg),
                        Err(err) => set_status(
                            &mut state,
                            StatusLevel::Error,
                            format!("sync failed: {err}"),
                        ),
                    },
                    KeyCode::Char('u') => {
                        match unpeer_selected_peer(&fast_rpc, &state) {
                            Ok(msg) => set_status(&mut state, StatusLevel::Success, msg),
                            Err(err) => set_status(
                                &mut state,
                                StatusLevel::Error,
                                format!("unpeer failed: {err}"),
                            ),
                        }
                        apply_refresh(ctx, &fast_rpc, &mut state, true);
                    }
                    KeyCode::Char('s') => {
                        set_status(&mut state, StatusLevel::Info, "Opening send prompt...");
                        match prompt_send_message(&mut terminal, &fast_rpc) {
                            Ok(msg) => set_status(&mut state, StatusLevel::Success, msg),
                            Err(err) => set_status(
                                &mut state,
                                StatusLevel::Error,
                                format!("send failed: {err}"),
                            ),
                        }
                        apply_refresh(ctx, &fast_rpc, &mut state, true);
                    }
                    KeyCode::Char('e') => {
                        apply_refresh(ctx, &fast_rpc, &mut state, true);
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

fn apply_refresh(ctx: &RuntimeContext, rpc: &RpcClient, state: &mut TuiState, manual: bool) {
    let include_logs = state.pane == Pane::Logs || !state.first_refresh_done;
    let outcome = refresh_snapshot(ctx, rpc, &mut state.snapshot, include_logs);
    state.connected = outcome.connected;
    state.last_refresh_ms = Some(outcome.elapsed_ms);

    clamp_selection(state);

    if !state.first_refresh_done {
        if outcome.connected {
            state.first_refresh_done = true;
            set_status(
                state,
                StatusLevel::Success,
                format!(
                    "Connected to {} ({} messages, {} peers)",
                    state.snapshot.rpc,
                    state.snapshot.messages.len(),
                    state.snapshot.peers.len()
                ),
            );
        } else if let Some(warning) = outcome.warning {
            set_status(state, StatusLevel::Warning, warning);
        }
        return;
    }

    if manual {
        if let Some(warning) = outcome.warning {
            set_status(state, StatusLevel::Warning, warning);
        } else {
            set_status(
                state,
                StatusLevel::Info,
                format!(
                    "Refreshed in {}ms ({} new events)",
                    outcome.elapsed_ms, outcome.new_events
                ),
            );
        }
        return;
    }

    if !outcome.connected {
        if let Some(warning) = outcome.warning {
            set_status(state, StatusLevel::Warning, warning);
        }
    }
}

fn set_status(state: &mut TuiState, level: StatusLevel, message: impl Into<String>) {
    state.status_level = level;
    state.status_line = message.into();
}

fn draw(frame: &mut ratatui::Frame<'_>, state: &TuiState) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Min(1),
            Constraint::Length(3),
        ])
        .split(frame.area());

    draw_header(frame, chunks[0], state);

    let tabs = Tabs::new(
        Pane::all()
            .iter()
            .map(|pane| {
                Line::from(Span::styled(
                    pane.title(),
                    Style::default().fg(state.theme.text),
                ))
            })
            .collect::<Vec<_>>(),
    )
    .select(index_of(state.pane))
    .block(
        Block::default()
            .title(Span::styled("Pane", Style::default().fg(state.theme.muted)))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(state.theme.border))
            .border_type(BorderType::Rounded),
    )
    .style(Style::default().fg(state.theme.muted))
    .highlight_style(
        Style::default()
            .fg(state.theme.accent)
            .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
    )
    .divider(" ");
    frame.render_widget(tabs, chunks[1]);

    match state.pane {
        Pane::Dashboard => dashboard::render(
            frame,
            chunks[2],
            &state.snapshot,
            &state.theme,
            state.connected,
            state.spinner_tick,
        ),
        Pane::Messages => messages::render(
            frame,
            chunks[2],
            &state.snapshot.messages,
            state.selected_message,
            &state.theme,
            true,
        ),
        Pane::Peers => peers::render(
            frame,
            chunks[2],
            &state.snapshot.peers,
            state.selected_peer,
            &state.theme,
            true,
        ),
        Pane::Interfaces => interfaces::render(
            frame,
            chunks[2],
            &state.snapshot.interfaces,
            state.selected_interface,
            &state.theme,
            true,
        ),
        Pane::Events => {
            events::render(frame, chunks[2], &state.snapshot.events, &state.theme, true)
        }
        Pane::Logs => logs::render(frame, chunks[2], &state.snapshot.logs, &state.theme, true),
    }

    draw_status_bar(frame, chunks[3], state);

    if !state.first_refresh_done || state.started_at.elapsed() < Duration::from_secs(2) {
        draw_welcome_overlay(frame, state);
    }
}

fn draw_header(frame: &mut ratatui::Frame<'_>, area: Rect, state: &TuiState) {
    let status_text = if state.connected {
        "CONNECTED"
    } else {
        "OFFLINE"
    };
    let status_color = if state.connected {
        state.theme.success
    } else {
        state.theme.warning
    };

    let refresh = state
        .last_refresh_ms
        .map(|ms| format!("refresh={}ms", ms))
        .unwrap_or_else(|| "refresh=...".into());

    let spinner = SPINNER[state.spinner_tick % SPINNER.len()];
    let line = Line::from(vec![
        Span::styled(
            " LXMF ",
            Style::default()
                .fg(state.theme.accent)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            "Operator TUI  ",
            Style::default()
                .fg(state.theme.text)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("[{status_text}] "),
            Style::default()
                .fg(status_color)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!(
                "{}  {}  {}",
                state.snapshot.profile, state.snapshot.rpc, refresh
            ),
            Style::default().fg(state.theme.muted),
        ),
        Span::styled(
            format!("  {spinner}"),
            Style::default().fg(state.theme.accent_dim),
        ),
    ]);

    let header = Paragraph::new(line)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(state.theme.border_active))
                .border_type(BorderType::Rounded),
        )
        .wrap(Wrap { trim: true });
    frame.render_widget(header, area);
}

fn draw_status_bar(frame: &mut ratatui::Frame<'_>, area: Rect, state: &TuiState) {
    let color = match state.status_level {
        StatusLevel::Info => state.theme.text,
        StatusLevel::Success => state.theme.success,
        StatusLevel::Warning => state.theme.warning,
        StatusLevel::Error => state.theme.danger,
    };

    let keys = "keys: q quit | Tab/Shift+Tab switch | j/k move | s send | y sync | u unpeer | a apply | r restart | n announce | e refresh";
    let content = vec![
        Line::from(Span::styled(
            state.status_line.as_str(),
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(keys, Style::default().fg(state.theme.muted))),
    ];

    let status = Paragraph::new(content)
        .block(
            Block::default()
                .title(Span::styled(
                    "Status",
                    Style::default().fg(state.theme.accent),
                ))
                .borders(Borders::ALL)
                .border_style(Style::default().fg(state.theme.border))
                .border_type(BorderType::Rounded),
        )
        .wrap(Wrap { trim: true });
    frame.render_widget(status, area);
}

fn draw_welcome_overlay(frame: &mut ratatui::Frame<'_>, state: &TuiState) {
    let area = centered_rect(62, 36, frame.area());
    frame.render_widget(Clear, area);

    let lines = vec![
        Line::from(Span::styled(
            "LXMF Operator TUI",
            Style::default()
                .fg(state.theme.accent)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            "Realtime control for messages, peers, interfaces, and daemon state.",
            Style::default().fg(state.theme.text),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "Quick start: Tab switch panes, s send, y sync peer, a apply interfaces.",
            Style::default().fg(state.theme.muted),
        )),
        Line::from(Span::styled(
            "Press q to quit.",
            Style::default().fg(state.theme.muted),
        )),
    ];

    let popup = Paragraph::new(lines)
        .block(
            Block::default()
                .title(Span::styled(
                    "Welcome",
                    Style::default().fg(state.theme.accent),
                ))
                .borders(Borders::ALL)
                .border_style(Style::default().fg(state.theme.border_active))
                .border_type(BorderType::Rounded),
        )
        .wrap(Wrap { trim: true });

    frame.render_widget(popup, area);
}

fn refresh_snapshot(
    ctx: &RuntimeContext,
    rpc: &RpcClient,
    snapshot: &mut TuiSnapshot,
    include_logs: bool,
) -> RefreshOutcome {
    let started = Instant::now();
    let mut warning = None;
    let mut connected = true;
    let mut new_events = 0usize;

    let daemon_status = match rpc.call("daemon_status_ex", None) {
        Ok(value) => value,
        Err(err) => {
            connected = false;
            warning = Some(format!("RPC unreachable: {err}"));
            snapshot.daemon_running = false;
            return RefreshOutcome {
                connected,
                warning,
                new_events,
                elapsed_ms: started.elapsed().as_millis(),
            };
        }
    };

    snapshot.daemon_running = daemon_status
        .get("running")
        .and_then(Value::as_bool)
        .unwrap_or(false);

    match rpc.call("list_messages", None) {
        Ok(messages) => snapshot.messages = as_vec(messages),
        Err(err) => {
            warning.get_or_insert_with(|| format!("messages unavailable: {err}"));
        }
    }

    match rpc.call("list_peers", None) {
        Ok(peers) => snapshot.peers = as_vec(peers),
        Err(err) => {
            warning.get_or_insert_with(|| format!("peers unavailable: {err}"));
        }
    }

    match rpc.call("list_interfaces", None) {
        Ok(interfaces) => snapshot.interfaces = as_vec(interfaces),
        Err(_) => {
            if let Ok(local) = load_reticulum_config(&ctx.profile_name) {
                snapshot.interfaces = serde_json::to_value(local.interfaces)
                    .ok()
                    .and_then(|v| v.as_array().cloned())
                    .unwrap_or_default();
            }
        }
    }

    loop {
        match rpc.poll_event() {
            Ok(Some(event)) => {
                snapshot
                    .events
                    .push(serde_json::to_string(&event).unwrap_or_else(|_| "<event>".into()));
                new_events += 1;
                if snapshot.events.len() > 400 {
                    let remove = snapshot.events.len().saturating_sub(400);
                    snapshot.events.drain(0..remove);
                }
            }
            Ok(None) => break,
            Err(err) => {
                warning.get_or_insert_with(|| format!("events unavailable: {err}"));
                break;
            }
        }
    }

    if include_logs {
        let supervisor = DaemonSupervisor::new(&ctx.profile_name, ctx.profile_settings.clone());
        snapshot.logs = supervisor.logs(400).unwrap_or_default();
    }

    snapshot.profile = ctx.profile_name.clone();
    snapshot.rpc = ctx.profile_settings.rpc.clone();

    RefreshOutcome {
        connected,
        warning,
        new_events,
        elapsed_ms: started.elapsed().as_millis(),
    }
}

fn prompt_send_message(
    terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
    rpc: &RpcClient,
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

    match rpc.call("send_message_v2", Some(params.clone())) {
        Ok(_) => Ok("message queued".into()),
        Err(_) => {
            rpc.call("send_message", Some(params))?;
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

fn apply_interfaces(ctx: &RuntimeContext, rpc: &RpcClient) -> Result<String> {
    let config = load_reticulum_config(&ctx.profile_name)?;
    rpc.call(
        "set_interfaces",
        Some(json!({
            "interfaces": config.interfaces,
        })),
    )?;
    let _ = rpc.call("reload_config", None);
    Ok("interfaces applied".into())
}

fn sync_selected_peer(rpc: &RpcClient, state: &TuiState) -> Result<String> {
    let Some(peer) = selected_peer_name(state) else {
        return Err(anyhow!("no peer selected"));
    };
    rpc.call("peer_sync", Some(json!({ "peer": peer })))?;
    Ok(format!("synced {peer}"))
}

fn unpeer_selected_peer(rpc: &RpcClient, state: &TuiState) -> Result<String> {
    let Some(peer) = selected_peer_name(state) else {
        return Err(anyhow!("no peer selected"));
    };
    rpc.call("peer_unpeer", Some(json!({ "peer": peer })))?;
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

fn clamp_selection(state: &mut TuiState) {
    if state.selected_message >= state.snapshot.messages.len() {
        state.selected_message = state.snapshot.messages.len().saturating_sub(1);
    }
    if state.selected_peer >= state.snapshot.peers.len() {
        state.selected_peer = state.snapshot.peers.len().saturating_sub(1);
    }
    if state.selected_interface >= state.snapshot.interfaces.len() {
        state.selected_interface = state.snapshot.interfaces.len().saturating_sub(1);
    }
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

fn previous_pane(current: Pane) -> Pane {
    match current {
        Pane::Dashboard => Pane::Logs,
        Pane::Messages => Pane::Dashboard,
        Pane::Peers => Pane::Messages,
        Pane::Interfaces => Pane::Peers,
        Pane::Events => Pane::Interfaces,
        Pane::Logs => Pane::Events,
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

fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(area);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}

fn chrono_like_now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
