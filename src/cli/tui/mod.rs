mod contacts;
mod dashboard;
mod events;
mod interfaces;
mod logs;
mod messages;
mod peers;

use anyhow::{anyhow, Result};
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind};
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
use std::io;
use std::time::{Duration, Instant};

use crate::cli::app::{RuntimeContext, TuiCommand};
use crate::cli::contacts::{
    find_contact_by_hash, load_contacts, remove_contact_by_alias, resolve_contact_hash,
    save_contacts, upsert_contact, validate_contact, ContactEntry,
};
use crate::cli::daemon::DaemonSupervisor;
use crate::cli::profile::{
    load_profile_settings, load_reticulum_config, normalize_optional_display_name,
    remove_interface, save_profile_settings, save_reticulum_config, set_interface_enabled,
    upsert_interface, InterfaceEntry, ProfileSettings,
};
use crate::cli::rpc_client::{RpcClient, RpcEvent};

const FAST_RPC_CONNECT_TIMEOUT: Duration = Duration::from_millis(350);
const FAST_RPC_IO_TIMEOUT: Duration = Duration::from_millis(900);
const OFFLINE_REFRESH_BACKOFF: Duration = Duration::from_millis(2_500);
const DISCOVERY_ANNOUNCE_BURST: usize = 3;
const DISCOVERY_ANNOUNCE_GAP: Duration = Duration::from_millis(220);
const DISCOVERY_WINDOW: Duration = Duration::from_secs(3);
const DISCOVERY_POLL_INTERVAL: Duration = Duration::from_millis(320);
const WELCOME_TIMEOUT: Duration = Duration::from_secs(6);
const SPINNER: [char; 4] = ['|', '/', '-', '\\'];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Pane {
    Dashboard,
    Messages,
    Peers,
    Contacts,
    Interfaces,
    Events,
    Logs,
}

impl Pane {
    fn all() -> [Self; 7] {
        [
            Self::Dashboard,
            Self::Messages,
            Self::Peers,
            Self::Contacts,
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
            Self::Contacts => "Contacts",
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
    pub identity_hash: Option<String>,
    pub daemon_running: bool,
    pub messages: Vec<Value>,
    pub peers: Vec<Value>,
    pub contacts: Vec<Value>,
    pub interfaces: Vec<Value>,
    pub events: Vec<String>,
    pub logs: Vec<String>,
}

#[derive(Debug)]
struct TuiState {
    pane: Pane,
    selected_message: usize,
    selected_channel: usize,
    selected_peer: usize,
    selected_contact: usize,
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
    welcome_dismissed: bool,
    composer: Option<ComposeState>,
    contact_editor: Option<ContactEditorState>,
    interface_editor: Option<InterfaceEditorState>,
    profile_editor: Option<ProfileEditorState>,
    peer_filter: String,
    peer_filter_editing: bool,
    peer_details_open: bool,
    profile_managed: bool,
}

#[derive(Debug)]
struct RefreshOutcome {
    connected: bool,
    warning: Option<String>,
    new_events: usize,
    elapsed_ms: u128,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProfileField {
    Managed,
    Rpc,
    DisplayName,
    ReticulumdPath,
    DbPath,
    IdentityPath,
    Transport,
}

impl ProfileField {
    fn next(self) -> Self {
        match self {
            Self::Managed => Self::Rpc,
            Self::Rpc => Self::DisplayName,
            Self::DisplayName => Self::ReticulumdPath,
            Self::ReticulumdPath => Self::DbPath,
            Self::DbPath => Self::IdentityPath,
            Self::IdentityPath => Self::Transport,
            Self::Transport => Self::Managed,
        }
    }

    fn prev(self) -> Self {
        match self {
            Self::Managed => Self::Transport,
            Self::Rpc => Self::Managed,
            Self::DisplayName => Self::Rpc,
            Self::ReticulumdPath => Self::DisplayName,
            Self::DbPath => Self::ReticulumdPath,
            Self::IdentityPath => Self::DbPath,
            Self::Transport => Self::IdentityPath,
        }
    }
}

#[derive(Debug, Clone)]
struct ProfileEditorState {
    managed: bool,
    rpc: String,
    display_name: String,
    reticulumd_path: String,
    db_path: String,
    identity_path: String,
    transport: String,
    active: Option<ProfileField>,
}

impl ProfileEditorState {
    fn from_settings(settings: &ProfileSettings) -> Self {
        Self {
            managed: settings.managed,
            rpc: settings.rpc.clone(),
            display_name: settings.display_name.clone().unwrap_or_default(),
            reticulumd_path: settings.reticulumd_path.clone().unwrap_or_default(),
            db_path: settings.db_path.clone().unwrap_or_default(),
            identity_path: settings.identity_path.clone().unwrap_or_default(),
            transport: settings.transport.clone().unwrap_or_default(),
            active: Some(ProfileField::Managed),
        }
    }

    fn active_field(&self) -> ProfileField {
        self.active.unwrap_or(ProfileField::Managed)
    }

    fn next_field(&mut self) {
        self.active = Some(self.active_field().next());
    }

    fn prev_field(&mut self) {
        self.active = Some(self.active_field().prev());
    }

    fn active_value_mut(&mut self) -> Option<&mut String> {
        match self.active_field() {
            ProfileField::Managed => None,
            ProfileField::Rpc => Some(&mut self.rpc),
            ProfileField::DisplayName => Some(&mut self.display_name),
            ProfileField::ReticulumdPath => Some(&mut self.reticulumd_path),
            ProfileField::DbPath => Some(&mut self.db_path),
            ProfileField::IdentityPath => Some(&mut self.identity_path),
            ProfileField::Transport => Some(&mut self.transport),
        }
    }
}

#[derive(Debug, Clone)]
struct ProfileSaveResult {
    rpc: String,
    managed: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ContactField {
    Alias,
    Hash,
    Notes,
}

impl ContactField {
    fn next(self) -> Self {
        match self {
            Self::Alias => Self::Hash,
            Self::Hash => Self::Notes,
            Self::Notes => Self::Alias,
        }
    }

    fn prev(self) -> Self {
        match self {
            Self::Alias => Self::Notes,
            Self::Hash => Self::Alias,
            Self::Notes => Self::Hash,
        }
    }
}

#[derive(Debug, Clone)]
struct ContactEditorState {
    alias: String,
    hash: String,
    notes: String,
    active: Option<ContactField>,
    mode: ContactEditorMode,
}

#[derive(Debug, Clone)]
enum ContactEditorMode {
    Add,
    Edit { original_alias: String },
}

impl ContactEditorState {
    fn new() -> Self {
        Self {
            alias: String::new(),
            hash: String::new(),
            notes: String::new(),
            active: Some(ContactField::Alias),
            mode: ContactEditorMode::Add,
        }
    }

    fn from_existing(contact: &Value) -> Self {
        Self {
            alias: contact
                .get("alias")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            hash: contact
                .get("hash")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            notes: contact
                .get("notes")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            active: Some(ContactField::Alias),
            mode: ContactEditorMode::Edit {
                original_alias: contact
                    .get("alias")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
            },
        }
    }

    fn from_peer(peer: &Value) -> Self {
        let alias = peer_display_name(peer).unwrap_or_default().to_string();
        let hash = peer
            .get("peer")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        Self {
            alias,
            hash,
            notes: String::new(),
            active: Some(ContactField::Alias),
            mode: ContactEditorMode::Add,
        }
    }

    fn active_field(&self) -> ContactField {
        self.active.unwrap_or(ContactField::Alias)
    }

    fn next_field(&mut self) {
        self.active = Some(self.active_field().next());
    }

    fn prev_field(&mut self) {
        self.active = Some(self.active_field().prev());
    }

    fn active_value_mut(&mut self) -> &mut String {
        match self.active_field() {
            ContactField::Alias => &mut self.alias,
            ContactField::Hash => &mut self.hash,
            ContactField::Notes => &mut self.notes,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InterfaceField {
    Name,
    Type,
    Host,
    Port,
    Enabled,
}

impl InterfaceField {
    fn next(self) -> Self {
        match self {
            Self::Name => Self::Type,
            Self::Type => Self::Host,
            Self::Host => Self::Port,
            Self::Port => Self::Enabled,
            Self::Enabled => Self::Name,
        }
    }

    fn prev(self) -> Self {
        match self {
            Self::Name => Self::Enabled,
            Self::Type => Self::Name,
            Self::Host => Self::Type,
            Self::Port => Self::Host,
            Self::Enabled => Self::Port,
        }
    }
}

#[derive(Debug, Clone)]
struct InterfaceEditorState {
    name: String,
    kind: String,
    host: String,
    port: String,
    enabled: bool,
    active: Option<InterfaceField>,
    mode: InterfaceEditorMode,
}

#[derive(Debug, Clone)]
enum InterfaceEditorMode {
    Add,
    Edit { original_name: String },
}

impl InterfaceEditorState {
    fn new() -> Self {
        Self {
            name: String::new(),
            kind: "tcp_client".to_string(),
            host: String::new(),
            port: String::new(),
            enabled: true,
            active: Some(InterfaceField::Name),
            mode: InterfaceEditorMode::Add,
        }
    }

    fn from_existing(iface: &Value) -> Self {
        Self {
            name: iface
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            kind: iface
                .get("type")
                .and_then(Value::as_str)
                .unwrap_or("tcp_client")
                .to_string(),
            host: iface
                .get("host")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string(),
            port: iface
                .get("port")
                .and_then(Value::as_u64)
                .map(|port| port.to_string())
                .unwrap_or_default(),
            enabled: iface
                .get("enabled")
                .and_then(Value::as_bool)
                .unwrap_or(true),
            active: Some(InterfaceField::Name),
            mode: InterfaceEditorMode::Edit {
                original_name: iface
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_string(),
            },
        }
    }

    fn active_field(&self) -> InterfaceField {
        self.active.unwrap_or(InterfaceField::Name)
    }

    fn next_field(&mut self) {
        self.active = Some(self.active_field().next());
    }

    fn prev_field(&mut self) {
        self.active = Some(self.active_field().prev());
    }

    fn active_value_mut(&mut self) -> Option<&mut String> {
        match self.active_field() {
            InterfaceField::Name => Some(&mut self.name),
            InterfaceField::Type => Some(&mut self.kind),
            InterfaceField::Host => Some(&mut self.host),
            InterfaceField::Port => Some(&mut self.port),
            InterfaceField::Enabled => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ComposeField {
    Destination,
    Source,
    Title,
    Content,
}

impl ComposeField {
    fn next(self) -> Self {
        match self {
            Self::Destination => Self::Source,
            Self::Source => Self::Title,
            Self::Title => Self::Content,
            Self::Content => Self::Destination,
        }
    }

    fn prev(self) -> Self {
        match self {
            Self::Destination => Self::Content,
            Self::Source => Self::Destination,
            Self::Title => Self::Source,
            Self::Content => Self::Title,
        }
    }
}

#[derive(Debug, Clone, Default)]
struct ComposeState {
    destination: String,
    source: String,
    title: String,
    content: String,
    active: Option<ComposeField>,
    send_armed: bool,
}

impl ComposeState {
    fn new_with_source(source: Option<String>) -> Self {
        let normalized_source = source.and_then(normalize_identity_hash);
        Self {
            destination: String::new(),
            source: normalized_source.unwrap_or_default(),
            title: String::new(),
            content: String::new(),
            active: Some(ComposeField::Destination),
            send_armed: false,
        }
    }

    fn with_destination_and_source(destination: impl Into<String>, source: Option<String>) -> Self {
        let normalized_source = source.and_then(normalize_identity_hash);
        let has_source = normalized_source.is_some();
        Self {
            destination: destination.into(),
            source: normalized_source.unwrap_or_default(),
            title: String::new(),
            content: String::new(),
            active: Some(if has_source {
                ComposeField::Content
            } else {
                ComposeField::Source
            }),
            send_armed: false,
        }
    }

    fn active_field(&self) -> ComposeField {
        self.active.unwrap_or(ComposeField::Destination)
    }

    fn next_field(&mut self) {
        self.active = Some(self.active_field().next());
        self.send_armed = false;
    }

    fn prev_field(&mut self) {
        self.active = Some(self.active_field().prev());
        self.send_armed = false;
    }

    fn active_value_mut(&mut self) -> &mut String {
        match self.active_field() {
            ComposeField::Destination => &mut self.destination,
            ComposeField::Source => &mut self.source,
            ComposeField::Title => &mut self.title,
            ComposeField::Content => &mut self.content,
        }
    }

    fn disarm_send(&mut self) {
        self.send_armed = false;
    }
}

pub fn run_tui(ctx: &RuntimeContext, command: &TuiCommand) -> Result<()> {
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    enable_raw_mode()?;

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    terminal.clear()?;

    let mut fast_rpc = build_fast_rpc(&ctx.profile_settings.rpc);

    let mut state = TuiState {
        pane: Pane::Dashboard,
        selected_message: 0,
        selected_channel: 0,
        selected_peer: 0,
        selected_contact: 0,
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
        welcome_dismissed: false,
        composer: None,
        contact_editor: None,
        interface_editor: None,
        profile_editor: None,
        peer_filter: String::new(),
        peer_filter_editing: false,
        peer_details_open: false,
        profile_managed: ctx.profile_settings.managed,
    };

    apply_refresh(ctx, &fast_rpc, &mut state, true);
    if state.profile_managed && !state.connected {
        auto_start_managed_daemon(ctx, &mut state);
        apply_refresh(ctx, &fast_rpc, &mut state, true);
    }

    let mut next_refresh_due = next_refresh_deadline(command.refresh_ms, state.connected);

    let run_result = loop {
        state.spinner_tick = state.spinner_tick.wrapping_add(1);

        let now = Instant::now();
        if state.composer.is_none()
            && state.contact_editor.is_none()
            && state.interface_editor.is_none()
            && state.profile_editor.is_none()
            && now >= next_refresh_due
        {
            apply_refresh(ctx, &fast_rpc, &mut state, false);
            next_refresh_due = next_refresh_deadline(command.refresh_ms, state.connected);
        }

        terminal.draw(|frame| draw(frame, &state))?;

        if event::poll(Duration::from_millis(60))? {
            let event = event::read()?;
            if let Event::Key(key) = event {
                if key.kind != KeyEventKind::Press {
                    continue;
                }

                state.welcome_dismissed = true;

                if state.composer.is_some() {
                    handle_compose_key(key, ctx, &fast_rpc, &mut state)?;
                    continue;
                }

                if state.contact_editor.is_some() {
                    handle_contact_editor_key(key, ctx, &mut state)?;
                    continue;
                }

                if state.interface_editor.is_some() {
                    handle_interface_editor_key(key, ctx, &mut state)?;
                    continue;
                }

                if state.profile_editor.is_some() {
                    if let Some(updated) = handle_profile_editor_key(key, ctx, &mut state)? {
                        let rpc_changed = state.snapshot.rpc != updated.rpc;
                        state.profile_managed = updated.managed;
                        state.snapshot.rpc = updated.rpc.clone();
                        if rpc_changed {
                            fast_rpc = build_fast_rpc(&updated.rpc);
                        }
                        apply_refresh(ctx, &fast_rpc, &mut state, true);
                        next_refresh_due =
                            next_refresh_deadline(command.refresh_ms, state.connected);
                    }
                    continue;
                }

                if state.peer_filter_editing {
                    handle_peer_filter_key(key, &mut state);
                    continue;
                }

                if state.peer_details_open {
                    handle_peer_details_key(key, &mut state);
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
                    KeyCode::Char('h') | KeyCode::Left => {
                        if state.pane == Pane::Messages {
                            select_previous_message_channel(&mut state);
                        }
                    }
                    KeyCode::Char('l') | KeyCode::Right => {
                        if state.pane == Pane::Messages {
                            select_next_message_channel(&mut state);
                        }
                    }
                    KeyCode::Char('j') | KeyCode::Down => increment_selection(&mut state),
                    KeyCode::Char('k') | KeyCode::Up => decrement_selection(&mut state),
                    KeyCode::Esc => {
                        if state.pane == Pane::Peers && !state.peer_filter.is_empty() {
                            state.peer_filter.clear();
                            clamp_selection(&mut state);
                            set_status(&mut state, StatusLevel::Info, "Peer filter cleared");
                        }
                    }
                    KeyCode::Char('/') => {
                        if state.pane == Pane::Peers {
                            state.peer_filter_editing = true;
                            set_status(
                                &mut state,
                                StatusLevel::Info,
                                "Peer filter: type to search by hash/name, Enter to close, Esc to clear",
                            );
                        }
                    }
                    KeyCode::Char('r') => {
                        set_status(
                            &mut state,
                            StatusLevel::Info,
                            "Starting/restarting daemon...",
                        );
                        match restart_daemon(ctx) {
                            Ok(msg) => set_status(&mut state, StatusLevel::Success, msg),
                            Err(err) => set_status(
                                &mut state,
                                StatusLevel::Error,
                                format!("restart failed: {err}"),
                            ),
                        }
                        apply_refresh(ctx, &fast_rpc, &mut state, true);
                        next_refresh_due =
                            next_refresh_deadline(command.refresh_ms, state.connected);
                    }
                    KeyCode::Char('d') => {
                        set_status(
                            &mut state,
                            StatusLevel::Info,
                            "Running peer discovery sweep...",
                        );
                        match discover_peers(ctx, &fast_rpc, &mut state) {
                            Ok(msg) => set_status(&mut state, StatusLevel::Success, msg),
                            Err(err) => set_status(
                                &mut state,
                                StatusLevel::Error,
                                format!("discovery failed: {err}"),
                            ),
                        }
                        next_refresh_due =
                            next_refresh_deadline(command.refresh_ms, state.connected);
                    }
                    KeyCode::Char('n') => match fast_rpc.call("announce_now", None) {
                        Ok(_) => {
                            apply_refresh(ctx, &fast_rpc, &mut state, true);
                            next_refresh_due =
                                next_refresh_deadline(command.refresh_ms, state.connected);
                            let peer_count = state.snapshot.peers.len();
                            if peer_count == 0 {
                                set_status(
                                    &mut state,
                                    StatusLevel::Success,
                                    "announce sent; waiting for peers...",
                                );
                            } else {
                                set_status(
                                    &mut state,
                                    StatusLevel::Success,
                                    format!("announce sent; {peer_count} peer(s) visible"),
                                );
                            }
                        }
                        Err(err) => set_status(
                            &mut state,
                            StatusLevel::Error,
                            format!("announce failed: {err}"),
                        ),
                    },
                    KeyCode::Char('a') => {
                        if state.pane == Pane::Interfaces {
                            match apply_interfaces(ctx, &fast_rpc) {
                                Ok(msg) => set_status(&mut state, StatusLevel::Success, msg),
                                Err(err) => set_status(
                                    &mut state,
                                    StatusLevel::Error,
                                    format!("apply failed: {err}"),
                                ),
                            }
                            apply_refresh(ctx, &fast_rpc, &mut state, true);
                            next_refresh_due =
                                next_refresh_deadline(command.refresh_ms, state.connected);
                        } else if state.pane == Pane::Contacts {
                            state.contact_editor = Some(ContactEditorState::new());
                            set_status(
                                &mut state,
                                StatusLevel::Info,
                                "New contact: Enter to advance/save, Esc to cancel",
                            );
                        }
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
                        next_refresh_due =
                            next_refresh_deadline(command.refresh_ms, state.connected);
                    }
                    KeyCode::Char('s') => {
                        if state.pane == Pane::Contacts {
                            match open_compose_from_selected_contact(&mut state) {
                                Ok(msg) => set_status(&mut state, StatusLevel::Info, msg),
                                Err(err) => set_status(
                                    &mut state,
                                    StatusLevel::Error,
                                    format!("compose failed: {err}"),
                                ),
                            }
                        } else if state.pane == Pane::Peers {
                            match open_compose_from_selected_peer(&mut state) {
                                Ok(msg) => set_status(&mut state, StatusLevel::Info, msg),
                                Err(err) => set_status(
                                    &mut state,
                                    StatusLevel::Error,
                                    format!("compose failed: {err}"),
                                ),
                            }
                        } else if state.pane == Pane::Messages {
                            match open_compose_from_selected_message(&mut state) {
                                Ok(msg) => set_status(&mut state, StatusLevel::Info, msg),
                                Err(err) => set_status(
                                    &mut state,
                                    StatusLevel::Error,
                                    format!("compose failed: {err}"),
                                ),
                            }
                        } else {
                            let source = preferred_source_identity(&state);
                            state.composer = Some(ComposeState::new_with_source(source));
                            set_status(
                                &mut state,
                                StatusLevel::Info,
                                "Compose message: destination/content required, source auto-fills when daemon identity is available (Enter on content twice to send)",
                            );
                        }
                    }
                    KeyCode::Char('p') => {
                        let mut settings = load_profile_settings(&ctx.profile_name)
                            .unwrap_or_else(|_| ctx.profile_settings.clone());
                        settings.managed = state.profile_managed;
                        settings.rpc = state.snapshot.rpc.clone();
                        state.profile_editor = Some(ProfileEditorState::from_settings(&settings));
                        set_status(
                            &mut state,
                            StatusLevel::Info,
                            "Profile settings: Enter to advance/save, Esc to cancel",
                        );
                    }
                    KeyCode::Char('i') => {
                        if state.pane == Pane::Interfaces {
                            state.interface_editor = Some(InterfaceEditorState::new());
                            set_status(
                                &mut state,
                                StatusLevel::Info,
                                "New interface: Enter to advance/save, Esc to cancel",
                            );
                        }
                    }
                    KeyCode::Enter => {
                        if state.pane == Pane::Interfaces {
                            match open_selected_interface_editor(&mut state) {
                                Ok(msg) => set_status(&mut state, StatusLevel::Info, msg),
                                Err(err) => set_status(
                                    &mut state,
                                    StatusLevel::Error,
                                    format!("edit failed: {err}"),
                                ),
                            }
                        } else if state.pane == Pane::Contacts {
                            match open_selected_contact_editor(&mut state) {
                                Ok(msg) => set_status(&mut state, StatusLevel::Info, msg),
                                Err(err) => set_status(
                                    &mut state,
                                    StatusLevel::Error,
                                    format!("edit failed: {err}"),
                                ),
                            }
                        } else if state.pane == Pane::Peers {
                            match open_selected_peer_details(&mut state) {
                                Ok(msg) => set_status(&mut state, StatusLevel::Info, msg),
                                Err(err) => set_status(
                                    &mut state,
                                    StatusLevel::Error,
                                    format!("peer details unavailable: {err}"),
                                ),
                            }
                        }
                    }
                    KeyCode::Char('t') => {
                        if state.pane == Pane::Interfaces {
                            match toggle_selected_interface(ctx, &mut state) {
                                Ok(msg) => set_status(&mut state, StatusLevel::Success, msg),
                                Err(err) => set_status(
                                    &mut state,
                                    StatusLevel::Error,
                                    format!("toggle failed: {err}"),
                                ),
                            }
                        }
                    }
                    KeyCode::Char('x') => {
                        if state.pane == Pane::Interfaces {
                            match remove_selected_interface(ctx, &mut state) {
                                Ok(msg) => set_status(&mut state, StatusLevel::Success, msg),
                                Err(err) => set_status(
                                    &mut state,
                                    StatusLevel::Error,
                                    format!("remove failed: {err}"),
                                ),
                            }
                        } else if state.pane == Pane::Contacts {
                            match remove_selected_contact(ctx, &mut state) {
                                Ok(msg) => set_status(&mut state, StatusLevel::Success, msg),
                                Err(err) => set_status(
                                    &mut state,
                                    StatusLevel::Error,
                                    format!("remove failed: {err}"),
                                ),
                            }
                        }
                    }
                    KeyCode::Char('c') => {
                        if state.pane == Pane::Peers {
                            match open_contact_editor_from_selected_peer(&mut state) {
                                Ok(msg) => set_status(&mut state, StatusLevel::Info, msg),
                                Err(err) => set_status(
                                    &mut state,
                                    StatusLevel::Error,
                                    format!("contact add failed: {err}"),
                                ),
                            }
                        }
                    }
                    KeyCode::Char('e') => {
                        apply_refresh(ctx, &fast_rpc, &mut state, true);
                        next_refresh_due =
                            next_refresh_deadline(command.refresh_ms, state.connected);
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

fn build_fast_rpc(rpc: &str) -> RpcClient {
    RpcClient::new_with_timeouts(
        rpc,
        FAST_RPC_CONNECT_TIMEOUT,
        FAST_RPC_IO_TIMEOUT,
        FAST_RPC_IO_TIMEOUT,
    )
}

fn apply_refresh(ctx: &RuntimeContext, rpc: &RpcClient, state: &mut TuiState, manual: bool) {
    let include_logs = state.pane == Pane::Logs || !state.first_refresh_done;
    let rpc_display = state.snapshot.rpc.clone();
    let outcome = refresh_snapshot(
        ctx,
        rpc,
        &mut state.snapshot,
        include_logs,
        state.profile_managed,
        &rpc_display,
    );
    state.connected = outcome.connected;
    state.last_refresh_ms = Some(outcome.elapsed_ms);

    clamp_selection(state);

    if !state.first_refresh_done {
        state.first_refresh_done = true;
        if outcome.connected {
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
            if state.status_level != StatusLevel::Error {
                set_status(state, StatusLevel::Warning, warning);
            }
        }
    }
}

fn set_status(state: &mut TuiState, level: StatusLevel, message: impl Into<String>) {
    state.status_level = level;
    state.status_line = message.into();
}

fn next_refresh_deadline(refresh_ms: u64, connected: bool) -> Instant {
    let base = Duration::from_millis(refresh_ms.max(150));
    let interval = if connected {
        base
    } else {
        std::cmp::max(base, OFFLINE_REFRESH_BACKOFF)
    };
    Instant::now() + interval
}

fn rpc_unreachable_warning(managed: bool, err: &anyhow::Error) -> String {
    let detail = err.to_string();
    let mut suffix = if managed {
        "Press r to start/restart daemon."
    } else {
        "Press r to enable managed mode and start local daemon, or set --rpc endpoint."
    }
    .to_string();

    if detail.to_ascii_lowercase().contains("status line") {
        suffix.push_str(" Endpoint appears reachable but is not speaking LXMF RPC.");
    }

    format!("RPC unreachable ({detail}). {suffix}")
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

    let filtered_peers = filtered_peers(state);
    let message_model = messages::MessagePaneModel::from_messages(
        &state.snapshot.messages,
        state.snapshot.identity_hash.as_deref(),
    );

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
            &message_model,
            state.selected_channel,
            state.selected_message,
            &state.theme,
            true,
        ),
        Pane::Peers => peers::render(
            frame,
            chunks[2],
            &filtered_peers,
            state.selected_peer,
            &state.theme,
            true,
            &state.peer_filter,
            state.peer_filter_editing,
        ),
        Pane::Contacts => contacts::render(
            frame,
            chunks[2],
            &state.snapshot.contacts,
            state.selected_contact,
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

    if should_show_welcome_overlay(state) {
        draw_welcome_overlay(frame, state);
    }

    if state.composer.is_some() {
        draw_compose_overlay(frame, state);
    }

    if state.contact_editor.is_some() {
        draw_contact_editor_overlay(frame, state);
    }

    if state.interface_editor.is_some() {
        draw_interface_editor_overlay(frame, state);
    }

    if state.profile_editor.is_some() {
        draw_profile_editor_overlay(frame, state);
    }

    if state.peer_filter_editing {
        draw_peer_filter_overlay(frame, state);
    }

    if state.peer_details_open {
        draw_peer_details_overlay(frame, state);
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

    let keys = "keys: q quit | Tab/Shift+Tab panes | h/l message channels | j/k select rows | s compose/reply (Messages/Peers/Contacts prefilled) | c save peer->contact | p profile | / peer filter | Enter peer details/contact edit/iface edit | a add contact or apply iface | x remove contact/iface | d discover | y sync | u unpeer | r start/restart | n announce | e refresh";
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
            "Quick start: Tab panes, h/l switch message channels, j/k move, d discover, n announce, s compose/reply (Messages/Peers/Contacts prefill destination), c save peer as contact, / filter peers, Enter peer details/contact edit/iface edit, p profile, i/t/x/a interfaces.",
            Style::default().fg(state.theme.muted),
        )),
        Line::from(Span::styled(
            "Press any key to dismiss.",
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

fn should_show_welcome_overlay(state: &TuiState) -> bool {
    !state.welcome_dismissed && state.started_at.elapsed() < WELCOME_TIMEOUT
}

fn draw_compose_overlay(frame: &mut ratatui::Frame<'_>, state: &TuiState) {
    let Some(compose) = state.composer.as_ref() else {
        return;
    };

    let area = centered_rect(74, 52, frame.area());
    frame.render_widget(Clear, area);

    let mut lines = Vec::new();
    lines.push(compose_line(
        "destination",
        &compose.destination,
        compose.active_field() == ComposeField::Destination,
        true,
        &state.theme,
    ));
    lines.push(compose_line(
        "source",
        &compose.source,
        compose.active_field() == ComposeField::Source,
        true,
        &state.theme,
    ));
    lines.push(compose_line(
        "title",
        &compose.title,
        compose.active_field() == ComposeField::Title,
        false,
        &state.theme,
    ));
    lines.push(compose_line(
        "content",
        &compose.content,
        compose.active_field() == ComposeField::Content,
        true,
        &state.theme,
    ));
    lines.push(Line::from(""));
    if compose.send_armed {
        lines.push(Line::from(Span::styled(
            "Send armed: press Enter again on content to queue message. Any edit cancels arm.",
            Style::default()
                .fg(state.theme.warning)
                .add_modifier(Modifier::BOLD),
        )));
    }
    lines.push(Line::from(Span::styled(
        "Enter: next field (on content: arm/send)  Tab: next  Shift+Tab: previous  Backspace: delete  Esc: cancel  (source auto-fills from daemon identity)",
        Style::default().fg(state.theme.muted),
    )));

    let popup = Paragraph::new(lines)
        .block(
            Block::default()
                .title(Span::styled(
                    "Compose Message",
                    Style::default()
                        .fg(state.theme.accent)
                        .add_modifier(Modifier::BOLD),
                ))
                .borders(Borders::ALL)
                .border_style(Style::default().fg(state.theme.border_active))
                .border_type(BorderType::Rounded),
        )
        .wrap(Wrap { trim: true });

    frame.render_widget(popup, area);
}

fn draw_contact_editor_overlay(frame: &mut ratatui::Frame<'_>, state: &TuiState) {
    let Some(editor) = state.contact_editor.as_ref() else {
        return;
    };

    let area = centered_rect(72, 48, frame.area());
    frame.render_widget(Clear, area);

    let mut lines = Vec::new();
    lines.push(interface_line(
        "alias",
        &editor.alias,
        editor.active_field() == ContactField::Alias,
        true,
        &state.theme,
    ));
    lines.push(interface_line(
        "hash",
        &editor.hash,
        editor.active_field() == ContactField::Hash,
        true,
        &state.theme,
    ));
    lines.push(interface_line(
        "notes",
        &editor.notes,
        editor.active_field() == ContactField::Notes,
        false,
        &state.theme,
    ));
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "Enter: next/save  Tab: next field  Shift+Tab: previous  Esc: cancel",
        Style::default().fg(state.theme.muted),
    )));

    let title = match editor.mode {
        ContactEditorMode::Add => "Add Contact",
        ContactEditorMode::Edit { .. } => "Edit Contact",
    };

    let popup = Paragraph::new(lines)
        .block(
            Block::default()
                .title(Span::styled(
                    title,
                    Style::default()
                        .fg(state.theme.accent)
                        .add_modifier(Modifier::BOLD),
                ))
                .borders(Borders::ALL)
                .border_style(Style::default().fg(state.theme.border_active))
                .border_type(BorderType::Rounded),
        )
        .wrap(Wrap { trim: true });
    frame.render_widget(popup, area);
}

fn compose_line(
    label: &str,
    value: &str,
    active: bool,
    required: bool,
    theme: &TuiTheme,
) -> Line<'static> {
    let label_color = if active { theme.accent } else { theme.muted };
    let value_style = if active {
        Style::default()
            .fg(theme.text)
            .add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
    } else {
        Style::default().fg(theme.text)
    };

    let prompt = if active { "> " } else { "  " };
    let required_tag = if required { " *" } else { "" };
    let shown = if value.is_empty() {
        "<empty>".to_string()
    } else {
        value.to_string()
    };

    Line::from(vec![
        Span::styled(prompt, Style::default().fg(theme.accent_dim)),
        Span::styled(
            format!("{label}{required_tag}: "),
            Style::default().fg(label_color),
        ),
        Span::styled(shown, value_style),
    ])
}

fn draw_interface_editor_overlay(frame: &mut ratatui::Frame<'_>, state: &TuiState) {
    let Some(editor) = state.interface_editor.as_ref() else {
        return;
    };

    let area = centered_rect(72, 54, frame.area());
    frame.render_widget(Clear, area);

    let mut lines = Vec::new();
    lines.push(interface_line(
        "name",
        &editor.name,
        editor.active_field() == InterfaceField::Name,
        true,
        &state.theme,
    ));
    lines.push(interface_line(
        "type",
        &editor.kind,
        editor.active_field() == InterfaceField::Type,
        true,
        &state.theme,
    ));
    lines.push(interface_line(
        "host",
        &editor.host,
        editor.active_field() == InterfaceField::Host,
        false,
        &state.theme,
    ));
    lines.push(interface_line(
        "port",
        &editor.port,
        editor.active_field() == InterfaceField::Port,
        false,
        &state.theme,
    ));
    lines.push(interface_bool_line(
        "enabled",
        editor.enabled,
        editor.active_field() == InterfaceField::Enabled,
        &state.theme,
    ));
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "Enter: next/save  Tab: next field  Shift+Tab: previous  Space on enabled toggles  Esc: cancel",
        Style::default().fg(state.theme.muted),
    )));
    lines.push(Line::from(Span::styled(
        "Supported type values: tcp_client, tcp_server",
        Style::default().fg(state.theme.muted),
    )));

    let title = match editor.mode {
        InterfaceEditorMode::Add => "Add Interface",
        InterfaceEditorMode::Edit { .. } => "Edit Interface",
    };

    let popup = Paragraph::new(lines)
        .block(
            Block::default()
                .title(Span::styled(
                    title,
                    Style::default()
                        .fg(state.theme.accent)
                        .add_modifier(Modifier::BOLD),
                ))
                .borders(Borders::ALL)
                .border_style(Style::default().fg(state.theme.border_active))
                .border_type(BorderType::Rounded),
        )
        .wrap(Wrap { trim: true });
    frame.render_widget(popup, area);
}

fn interface_line(
    label: &str,
    value: &str,
    active: bool,
    required: bool,
    theme: &TuiTheme,
) -> Line<'static> {
    let label_color = if active { theme.accent } else { theme.muted };
    let value_style = if active {
        Style::default()
            .fg(theme.text)
            .add_modifier(Modifier::BOLD | Modifier::UNDERLINED)
    } else {
        Style::default().fg(theme.text)
    };

    let prompt = if active { "> " } else { "  " };
    let required_tag = if required { " *" } else { "" };
    let shown = if value.is_empty() {
        "<empty>".to_string()
    } else {
        value.to_string()
    };

    Line::from(vec![
        Span::styled(prompt, Style::default().fg(theme.accent_dim)),
        Span::styled(
            format!("{label}{required_tag}: "),
            Style::default().fg(label_color),
        ),
        Span::styled(shown, value_style),
    ])
}

fn interface_bool_line(label: &str, value: bool, active: bool, theme: &TuiTheme) -> Line<'static> {
    let prompt = if active { "> " } else { "  " };
    let label_color = if active { theme.accent } else { theme.muted };
    let bool_text = if value { "true" } else { "false" };
    let bool_color = if value { theme.success } else { theme.warning };

    Line::from(vec![
        Span::styled(prompt, Style::default().fg(theme.accent_dim)),
        Span::styled(format!("{label}: "), Style::default().fg(label_color)),
        Span::styled(
            bool_text,
            Style::default().fg(bool_color).add_modifier(Modifier::BOLD),
        ),
    ])
}

fn draw_profile_editor_overlay(frame: &mut ratatui::Frame<'_>, state: &TuiState) {
    let Some(editor) = state.profile_editor.as_ref() else {
        return;
    };

    let area = centered_rect(78, 68, frame.area());
    frame.render_widget(Clear, area);

    let mut lines = Vec::new();
    lines.push(Line::from(Span::styled(
        format!("profile: {}", state.snapshot.profile),
        Style::default().fg(state.theme.muted),
    )));
    lines.push(Line::from(""));
    lines.push(interface_bool_line(
        "managed",
        editor.managed,
        editor.active_field() == ProfileField::Managed,
        &state.theme,
    ));
    lines.push(interface_line(
        "rpc",
        &editor.rpc,
        editor.active_field() == ProfileField::Rpc,
        true,
        &state.theme,
    ));
    lines.push(interface_line(
        "display_name",
        &editor.display_name,
        editor.active_field() == ProfileField::DisplayName,
        false,
        &state.theme,
    ));
    lines.push(interface_line(
        "reticulumd_path",
        &editor.reticulumd_path,
        editor.active_field() == ProfileField::ReticulumdPath,
        false,
        &state.theme,
    ));
    lines.push(interface_line(
        "db_path",
        &editor.db_path,
        editor.active_field() == ProfileField::DbPath,
        false,
        &state.theme,
    ));
    lines.push(interface_line(
        "identity_path",
        &editor.identity_path,
        editor.active_field() == ProfileField::IdentityPath,
        false,
        &state.theme,
    ));
    lines.push(interface_line(
        "transport",
        &editor.transport,
        editor.active_field() == ProfileField::Transport,
        false,
        &state.theme,
    ));
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "Enter: next/save  Tab: next field  Shift+Tab: previous  Space on managed toggles  Esc: cancel",
        Style::default().fg(state.theme.muted),
    )));
    lines.push(Line::from(Span::styled(
        "Saving updates profile.toml immediately; use n (announce) or r (restart) so peers see display-name changes.",
        Style::default().fg(state.theme.muted),
    )));

    let popup = Paragraph::new(lines)
        .block(
            Block::default()
                .title(Span::styled(
                    "Profile Settings",
                    Style::default()
                        .fg(state.theme.accent)
                        .add_modifier(Modifier::BOLD),
                ))
                .borders(Borders::ALL)
                .border_style(Style::default().fg(state.theme.border_active))
                .border_type(BorderType::Rounded),
        )
        .wrap(Wrap { trim: true });
    frame.render_widget(popup, area);
}

fn draw_peer_filter_overlay(frame: &mut ratatui::Frame<'_>, state: &TuiState) {
    if state.pane != Pane::Peers {
        return;
    }

    let area = centered_rect(70, 20, frame.area());
    frame.render_widget(Clear, area);

    let shown = if state.peer_filter.trim().is_empty() {
        "<type hash or name>".to_string()
    } else {
        state.peer_filter.clone()
    };
    let filtered = filtered_peer_indices(state).len();
    let total = state.snapshot.peers.len();

    let lines = vec![
        Line::from(vec![
            Span::styled("> ", Style::default().fg(state.theme.accent)),
            Span::styled(
                shown,
                Style::default()
                    .fg(state.theme.text)
                    .add_modifier(Modifier::BOLD | Modifier::UNDERLINED),
            ),
        ]),
        Line::from(Span::styled(
            format!("showing {filtered} of {total} peer(s)"),
            Style::default().fg(state.theme.muted),
        )),
        Line::from(Span::styled(
            "Enter: keep filter  Esc: clear filter  Backspace/Delete: edit",
            Style::default().fg(state.theme.muted),
        )),
    ];

    let popup = Paragraph::new(lines)
        .block(
            Block::default()
                .title(Span::styled(
                    "Peer Search",
                    Style::default()
                        .fg(state.theme.accent)
                        .add_modifier(Modifier::BOLD),
                ))
                .borders(Borders::ALL)
                .border_style(Style::default().fg(state.theme.border_active))
                .border_type(BorderType::Rounded),
        )
        .wrap(Wrap { trim: true });

    frame.render_widget(popup, area);
}

fn draw_peer_details_overlay(frame: &mut ratatui::Frame<'_>, state: &TuiState) {
    let Some(peer) = selected_peer_value(state) else {
        return;
    };

    let area = centered_rect(70, 50, frame.area());
    frame.render_widget(Clear, area);

    let name = peer_display_name(peer).unwrap_or("<unknown>");
    let hash = peer
        .get("peer")
        .and_then(Value::as_str)
        .unwrap_or("<unknown>");
    let source = peer
        .get("name_source")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let first_seen = format_timestamp(peer.get("first_seen").and_then(Value::as_i64));
    let last_seen = format_timestamp(peer.get("last_seen").and_then(Value::as_i64));
    let seen_count = peer
        .get("seen_count")
        .and_then(Value::as_u64)
        .map(|value| value.to_string())
        .unwrap_or_else(|| "0".to_string());

    let lines = vec![
        Line::from(vec![
            Span::styled("name: ", Style::default().fg(state.theme.muted)),
            Span::styled(name, Style::default().fg(state.theme.text)),
        ]),
        Line::from(vec![
            Span::styled("hash: ", Style::default().fg(state.theme.muted)),
            Span::styled(hash, Style::default().fg(state.theme.text)),
        ]),
        Line::from(vec![
            Span::styled("source: ", Style::default().fg(state.theme.muted)),
            Span::styled(source, Style::default().fg(state.theme.text)),
        ]),
        Line::from(vec![
            Span::styled("first seen: ", Style::default().fg(state.theme.muted)),
            Span::styled(first_seen, Style::default().fg(state.theme.text)),
        ]),
        Line::from(vec![
            Span::styled("last seen: ", Style::default().fg(state.theme.muted)),
            Span::styled(last_seen, Style::default().fg(state.theme.text)),
        ]),
        Line::from(vec![
            Span::styled("seen count: ", Style::default().fg(state.theme.muted)),
            Span::styled(seen_count, Style::default().fg(state.theme.text)),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            "Esc or Enter closes this panel.",
            Style::default().fg(state.theme.muted),
        )),
    ];

    let popup = Paragraph::new(lines)
        .block(
            Block::default()
                .title(Span::styled(
                    "Peer Details",
                    Style::default()
                        .fg(state.theme.accent)
                        .add_modifier(Modifier::BOLD),
                ))
                .borders(Borders::ALL)
                .border_style(Style::default().fg(state.theme.border_active))
                .border_type(BorderType::Rounded),
        )
        .wrap(Wrap { trim: true });

    frame.render_widget(popup, area);
}

fn format_timestamp(value: Option<i64>) -> String {
    value
        .map(|ts| ts.to_string())
        .unwrap_or_else(|| "n/a".to_string())
}

fn handle_profile_editor_key(
    key: KeyEvent,
    ctx: &RuntimeContext,
    state: &mut TuiState,
) -> Result<Option<ProfileSaveResult>> {
    let mut close = false;
    let mut submit = false;
    let mut status: Option<(StatusLevel, String)> = None;
    let mut saved = None;

    let Some(editor) = state.profile_editor.as_mut() else {
        return Ok(None);
    };

    match key.code {
        KeyCode::Esc => {
            close = true;
            status = Some((StatusLevel::Info, "profile settings edit canceled".into()));
        }
        KeyCode::Tab | KeyCode::Down => editor.next_field(),
        KeyCode::BackTab | KeyCode::Up => editor.prev_field(),
        KeyCode::Enter => {
            if editor.active_field() == ProfileField::Transport {
                submit = true;
            } else {
                editor.next_field();
            }
        }
        KeyCode::Backspace => {
            if let Some(value) = editor.active_value_mut() {
                value.pop();
            }
        }
        KeyCode::Delete => {
            if let Some(value) = editor.active_value_mut() {
                value.clear();
            }
        }
        KeyCode::Char(' ') => {
            if editor.active_field() == ProfileField::Managed {
                editor.managed = !editor.managed;
            } else if let Some(value) = editor.active_value_mut() {
                value.push(' ');
            }
        }
        KeyCode::Char(c) => {
            if editor.active_field() == ProfileField::Managed {
                match c {
                    't' | 'y' | '1' => editor.managed = true,
                    'f' | 'n' | '0' => editor.managed = false,
                    _ => {}
                }
            } else if let Some(value) = editor.active_value_mut() {
                value.push(c);
            }
        }
        _ => {}
    }

    if submit {
        let payload = state
            .profile_editor
            .clone()
            .ok_or_else(|| anyhow!("profile editor state disappeared"))?;
        match save_profile_from_editor(ctx, &payload) {
            Ok(result) => {
                let managed_text = if result.managed {
                    "managed"
                } else {
                    "external"
                };
                status = Some((
                    StatusLevel::Success,
                    format!("profile settings saved ({managed_text} mode); announce or restart to publish name"),
                ));
                close = true;
                saved = Some(result);
            }
            Err(err) => {
                status = Some((StatusLevel::Error, format!("profile save failed: {err}")));
            }
        }
    }

    if close {
        state.profile_editor = None;
    }
    if let Some((level, message)) = status {
        set_status(state, level, message);
    }

    Ok(saved)
}

fn save_profile_from_editor(
    ctx: &RuntimeContext,
    editor: &ProfileEditorState,
) -> Result<ProfileSaveResult> {
    let rpc = editor.rpc.trim();
    if rpc.is_empty() {
        return Err(anyhow!("rpc endpoint is required"));
    }

    let mut settings = load_profile_settings(&ctx.profile_name)?;
    settings.managed = editor.managed;
    settings.rpc = rpc.to_string();
    let display_name = optional_trimmed_string(&editor.display_name);
    settings.display_name = normalize_optional_display_name(display_name.as_deref())?;
    settings.reticulumd_path = optional_trimmed_string(&editor.reticulumd_path);
    settings.db_path = optional_trimmed_string(&editor.db_path);
    settings.identity_path = optional_trimmed_string(&editor.identity_path);
    settings.transport = optional_trimmed_string(&editor.transport);
    save_profile_settings(&settings)?;

    Ok(ProfileSaveResult {
        rpc: settings.rpc,
        managed: settings.managed,
    })
}

fn optional_trimmed_string(value: &str) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn handle_contact_editor_key(
    key: KeyEvent,
    ctx: &RuntimeContext,
    state: &mut TuiState,
) -> Result<()> {
    let mut close = false;
    let mut submit = false;
    let mut status: Option<(StatusLevel, String)> = None;

    let Some(editor) = state.contact_editor.as_mut() else {
        return Ok(());
    };

    match key.code {
        KeyCode::Esc => {
            close = true;
            status = Some((StatusLevel::Info, "contact edit canceled".into()));
        }
        KeyCode::Tab | KeyCode::Down => editor.next_field(),
        KeyCode::BackTab | KeyCode::Up => editor.prev_field(),
        KeyCode::Enter => {
            if editor.active_field() == ContactField::Notes {
                submit = true;
            } else {
                editor.next_field();
            }
        }
        KeyCode::Backspace => {
            editor.active_value_mut().pop();
        }
        KeyCode::Delete => {
            editor.active_value_mut().clear();
        }
        KeyCode::Char(c) => {
            editor.active_value_mut().push(c);
        }
        _ => {}
    }

    if submit {
        let payload = state
            .contact_editor
            .clone()
            .ok_or_else(|| anyhow!("contact editor state disappeared"))?;
        match save_contact_from_editor(ctx, &payload) {
            Ok(msg) => {
                close = true;
                status = Some((StatusLevel::Success, msg));
                reload_contacts_from_local(ctx, &mut state.snapshot)?;
                annotate_snapshot_peers_with_contacts(&mut state.snapshot);
                clamp_selection(state);
            }
            Err(err) => {
                status = Some((StatusLevel::Error, format!("contact save failed: {err}")));
            }
        }
    }

    if close {
        state.contact_editor = None;
    }

    if let Some((level, message)) = status {
        set_status(state, level, message);
    }

    Ok(())
}

fn handle_interface_editor_key(
    key: KeyEvent,
    ctx: &RuntimeContext,
    state: &mut TuiState,
) -> Result<()> {
    let mut close = false;
    let mut submit = false;
    let mut status: Option<(StatusLevel, String)> = None;

    let Some(editor) = state.interface_editor.as_mut() else {
        return Ok(());
    };

    match key.code {
        KeyCode::Esc => {
            close = true;
            status = Some((StatusLevel::Info, "interface creation canceled".into()));
        }
        KeyCode::Tab | KeyCode::Down => editor.next_field(),
        KeyCode::BackTab | KeyCode::Up => editor.prev_field(),
        KeyCode::Enter => {
            if editor.active_field() == InterfaceField::Enabled {
                submit = true;
            } else {
                editor.next_field();
            }
        }
        KeyCode::Backspace => {
            if let Some(value) = editor.active_value_mut() {
                value.pop();
            }
        }
        KeyCode::Delete => {
            if let Some(value) = editor.active_value_mut() {
                value.clear();
            }
        }
        KeyCode::Char(' ') => {
            if editor.active_field() == InterfaceField::Enabled {
                editor.enabled = !editor.enabled;
            } else if let Some(value) = editor.active_value_mut() {
                value.push(' ');
            }
        }
        KeyCode::Char(c) => {
            if editor.active_field() == InterfaceField::Enabled {
                match c {
                    't' | 'y' | '1' => editor.enabled = true,
                    'f' | 'n' | '0' => editor.enabled = false,
                    _ => {}
                }
            } else if let Some(value) = editor.active_value_mut() {
                value.push(c);
            }
        }
        _ => {}
    }

    if submit {
        let payload = state
            .interface_editor
            .clone()
            .ok_or_else(|| anyhow!("interface editor state disappeared"))?;
        match save_interface_from_editor(ctx, &payload) {
            Ok(msg) => {
                close = true;
                status = Some((StatusLevel::Success, msg));
                reload_interfaces_from_local(ctx, &mut state.snapshot)?;
                clamp_selection(state);
            }
            Err(err) => {
                status = Some((StatusLevel::Error, format!("interface save failed: {err}")));
            }
        }
    }

    if close {
        state.interface_editor = None;
    }

    if let Some((level, message)) = status {
        set_status(state, level, message);
    }

    Ok(())
}

fn save_interface_from_editor(
    ctx: &RuntimeContext,
    editor: &InterfaceEditorState,
) -> Result<String> {
    let name = editor.name.trim();
    let kind = editor.kind.trim();
    let host = editor.host.trim();
    let port_raw = editor.port.trim();

    if name.is_empty() {
        return Err(anyhow!("interface name is required"));
    }
    if kind != "tcp_client" && kind != "tcp_server" {
        return Err(anyhow!("interface type must be tcp_client or tcp_server"));
    }

    let port = if port_raw.is_empty() {
        None
    } else {
        Some(
            port_raw
                .parse::<u16>()
                .map_err(|_| anyhow!("port must be a valid u16"))?,
        )
    };

    let mut config = load_reticulum_config(&ctx.profile_name)?;
    if let InterfaceEditorMode::Edit { original_name } = &editor.mode {
        if original_name != name {
            remove_interface(&mut config, original_name);
        }
    }
    upsert_interface(
        &mut config,
        InterfaceEntry {
            name: name.to_string(),
            kind: kind.to_string(),
            enabled: editor.enabled,
            host: if host.is_empty() {
                None
            } else {
                Some(host.to_string())
            },
            port,
        },
    );
    save_reticulum_config(&ctx.profile_name, &config)?;
    let verb = match editor.mode {
        InterfaceEditorMode::Add => "added",
        InterfaceEditorMode::Edit { .. } => "updated",
    };
    Ok(format!("interface '{name}' {verb} (run a to apply)"))
}

fn toggle_selected_interface(ctx: &RuntimeContext, state: &mut TuiState) -> Result<String> {
    let Some(name) = selected_interface_name(state) else {
        return Err(anyhow!("no interface selected"));
    };

    let mut config = load_reticulum_config(&ctx.profile_name)?;
    let current = config
        .interfaces
        .iter()
        .find(|iface| iface.name == name)
        .map(|iface| iface.enabled)
        .ok_or_else(|| anyhow!("selected interface not found in profile config"))?;
    let new_value = !current;
    set_interface_enabled(&mut config, &name, new_value);
    save_reticulum_config(&ctx.profile_name, &config)?;

    reload_interfaces_from_local(ctx, &mut state.snapshot)?;
    clamp_selection(state);
    Ok(format!(
        "interface '{}' {} (run a to apply)",
        name,
        if new_value { "enabled" } else { "disabled" }
    ))
}

fn remove_selected_interface(ctx: &RuntimeContext, state: &mut TuiState) -> Result<String> {
    let Some(name) = selected_interface_name(state) else {
        return Err(anyhow!("no interface selected"));
    };

    let mut config = load_reticulum_config(&ctx.profile_name)?;
    if !remove_interface(&mut config, &name) {
        return Err(anyhow!("selected interface not found in profile config"));
    }
    save_reticulum_config(&ctx.profile_name, &config)?;

    reload_interfaces_from_local(ctx, &mut state.snapshot)?;
    clamp_selection(state);
    Ok(format!("interface '{}' removed (run a to apply)", name))
}

fn open_selected_interface_editor(state: &mut TuiState) -> Result<String> {
    let Some(iface) = state.snapshot.interfaces.get(state.selected_interface) else {
        return Err(anyhow!("no interface selected"));
    };
    let name = iface
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or("<unnamed>");
    state.interface_editor = Some(InterfaceEditorState::from_existing(iface));
    Ok(format!(
        "Edit interface '{name}': Enter to save, Esc to cancel"
    ))
}

fn reload_interfaces_from_local(ctx: &RuntimeContext, snapshot: &mut TuiSnapshot) -> Result<()> {
    let local = load_reticulum_config(&ctx.profile_name)?;
    snapshot.interfaces = serde_json::to_value(local.interfaces)
        .ok()
        .and_then(|v| v.as_array().cloned())
        .unwrap_or_default();
    Ok(())
}

fn refresh_snapshot(
    ctx: &RuntimeContext,
    rpc: &RpcClient,
    snapshot: &mut TuiSnapshot,
    include_logs: bool,
    managed_profile: bool,
    rpc_display: &str,
) -> RefreshOutcome {
    let started = Instant::now();
    let mut warning = None;
    let mut connected = true;
    let mut new_events = 0usize;
    let mut peers_changed_from_events = false;
    let mut contacts = Vec::new();

    match load_contacts(&ctx.profile_name) {
        Ok(value) => {
            contacts = value;
            snapshot.contacts = contacts_to_values(&contacts);
        }
        Err(err) => {
            warning.get_or_insert_with(|| format!("contacts unavailable: {err}"));
            snapshot.contacts.clear();
        }
    }

    match rpc.call("daemon_status_ex", None) {
        Ok(value) => {
            snapshot.daemon_running = value
                .get("running")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            snapshot.identity_hash = identity_hash_from_status(&value);
        }
        Err(daemon_status_err) => match rpc.call("status", None) {
            Ok(value) => {
                snapshot.daemon_running = value
                    .get("running")
                    .and_then(Value::as_bool)
                    .unwrap_or(true);
                snapshot.identity_hash = identity_hash_from_status(&value);
                warning.get_or_insert_with(|| {
                    format!("daemon_status_ex unavailable ({daemon_status_err}); using status")
                });
            }
            Err(status_err) => {
                connected = false;
                warning = Some(rpc_unreachable_warning(managed_profile, &status_err));
                snapshot.identity_hash = None;

                if managed_profile {
                    let runtime_settings = load_profile_settings(&ctx.profile_name)
                        .unwrap_or_else(|_| ctx.profile_settings.clone());
                    if let Ok(local_status) =
                        DaemonSupervisor::new(&ctx.profile_name, runtime_settings).status()
                    {
                        snapshot.daemon_running = local_status.running;
                    } else {
                        snapshot.daemon_running = false;
                    }
                } else {
                    snapshot.daemon_running = false;
                }
            }
        },
    }

    if connected {
        match rpc.call("list_messages", None) {
            Ok(messages) => snapshot.messages = as_vec(messages, "messages"),
            Err(err) => {
                warning.get_or_insert_with(|| format!("messages unavailable: {err}"));
            }
        }

        match rpc.call("list_peers", None) {
            Ok(peers) => {
                snapshot.peers = as_vec(peers, "peers");
                annotate_peers_with_contacts(&mut snapshot.peers, &contacts);
            }
            Err(err) => {
                warning.get_or_insert_with(|| format!("peers unavailable: {err}"));
            }
        }

        match rpc.call("list_interfaces", None) {
            Ok(interfaces) => snapshot.interfaces = as_vec(interfaces, "interfaces"),
            Err(err) => {
                warning.get_or_insert_with(|| format!("interfaces unavailable: {err}"));
            }
        }

        loop {
            match rpc.poll_event() {
                Ok(Some(event)) => {
                    snapshot
                        .events
                        .push(serde_json::to_string(&event).unwrap_or_else(|_| "<event>".into()));
                    new_events += 1;
                    peers_changed_from_events |= merge_peer_from_event(&mut snapshot.peers, &event);
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

        if peers_changed_from_events {
            snapshot.peers.sort_by(|a, b| {
                let a_last_seen = a.get("last_seen").and_then(Value::as_i64).unwrap_or(0);
                let b_last_seen = b.get("last_seen").and_then(Value::as_i64).unwrap_or(0);
                let a_hash = a.get("peer").and_then(Value::as_str).unwrap_or("");
                let b_hash = b.get("peer").and_then(Value::as_str).unwrap_or("");
                b_last_seen
                    .cmp(&a_last_seen)
                    .then_with(|| a_hash.cmp(b_hash))
            });
            annotate_peers_with_contacts(&mut snapshot.peers, &contacts);
        }
    } else {
        snapshot.messages.clear();
        snapshot.peers.clear();
    }

    if let Ok(local) = load_reticulum_config(&ctx.profile_name) {
        if !connected || snapshot.interfaces.is_empty() {
            snapshot.interfaces = serde_json::to_value(local.interfaces)
                .ok()
                .and_then(|v| v.as_array().cloned())
                .unwrap_or_default();
        }
    }

    if include_logs {
        let runtime_settings = load_profile_settings(&ctx.profile_name)
            .unwrap_or_else(|_| ctx.profile_settings.clone());
        let supervisor = DaemonSupervisor::new(&ctx.profile_name, runtime_settings);
        snapshot.logs = supervisor.logs(400).unwrap_or_default();
    }

    snapshot.profile = ctx.profile_name.clone();
    snapshot.rpc = rpc_display.to_string();

    RefreshOutcome {
        connected,
        warning,
        new_events,
        elapsed_ms: started.elapsed().as_millis(),
    }
}

fn handle_compose_key(
    key: KeyEvent,
    ctx: &RuntimeContext,
    rpc: &RpcClient,
    state: &mut TuiState,
) -> Result<()> {
    let mut close = false;
    let mut submit = false;
    let mut status: Option<(StatusLevel, String)> = None;

    {
        let Some(composer) = state.composer.as_mut() else {
            return Ok(());
        };

        match key.code {
            KeyCode::Esc => {
                close = true;
                status = Some((StatusLevel::Info, "message composition canceled".into()));
            }
            KeyCode::Tab | KeyCode::Down => composer.next_field(),
            KeyCode::BackTab | KeyCode::Up => composer.prev_field(),
            KeyCode::Enter => {
                if composer.active_field() == ComposeField::Content {
                    if composer.send_armed {
                        submit = true;
                    } else {
                        composer.send_armed = true;
                        status = Some((
                            StatusLevel::Info,
                            "Message ready. Press Enter again on content to send.".into(),
                        ));
                    }
                } else {
                    composer.next_field();
                }
            }
            KeyCode::Backspace => {
                composer.active_value_mut().pop();
                composer.disarm_send();
            }
            KeyCode::Delete => {
                composer.active_value_mut().clear();
                composer.disarm_send();
            }
            KeyCode::Char(c) => {
                composer.active_value_mut().push(c);
                composer.disarm_send();
            }
            _ => {}
        }
    }

    if submit {
        let payload = state
            .composer
            .clone()
            .ok_or_else(|| anyhow!("composer state disappeared"))?;
        match send_message_from_composer(ctx, rpc, &payload) {
            Ok(result) => {
                close = true;
                refresh_after_compose_send(ctx, rpc, state, &result.message_id);
                let receipt_status =
                    message_receipt_status(&state.snapshot.messages, &result.message_id);
                status = Some((
                    StatusLevel::Success,
                    compose_send_status(&result, receipt_status),
                ));
            }
            Err(err) => {
                if let Some(composer) = state.composer.as_mut() {
                    composer.disarm_send();
                }
                status = Some((StatusLevel::Error, format!("send failed: {err}")));
            }
        }
    }

    if close {
        state.composer = None;
    }

    if let Some((level, message)) = status {
        set_status(state, level, message);
    }

    Ok(())
}

#[derive(Debug, Clone)]
struct ComposeSendResult {
    message_id: String,
    destination: String,
    used_legacy_api: bool,
}

fn compose_send_status(result: &ComposeSendResult, receipt_status: Option<&str>) -> String {
    if let Some(receipt_status) = receipt_status.filter(|value| !value.trim().is_empty()) {
        if result.used_legacy_api {
            format!(
                "message {}: {} -> {} (legacy api)",
                receipt_status, result.message_id, result.destination
            )
        } else {
            format!(
                "message {}: {} -> {}",
                receipt_status, result.message_id, result.destination
            )
        }
    } else if result.used_legacy_api {
        format!(
            "message queued: {} -> {} (legacy api)",
            result.message_id, result.destination
        )
    } else {
        format!(
            "message queued: {} -> {}",
            result.message_id, result.destination
        )
    }
}

fn message_receipt_status<'a>(messages: &'a [Value], message_id: &str) -> Option<&'a str> {
    messages.iter().find_map(|message| {
        if message
            .get("id")
            .and_then(Value::as_str)
            .is_some_and(|value| value == message_id)
        {
            return message.get("receipt_status").and_then(Value::as_str);
        }
        None
    })
}

fn refresh_after_compose_send(
    ctx: &RuntimeContext,
    rpc: &RpcClient,
    state: &mut TuiState,
    sent_message_id: &str,
) {
    let rpc_display = state.snapshot.rpc.clone();
    let outcome = refresh_snapshot(
        ctx,
        rpc,
        &mut state.snapshot,
        false,
        state.profile_managed,
        &rpc_display,
    );
    state.connected = outcome.connected;
    state.last_refresh_ms = Some(outcome.elapsed_ms);

    if let Some((channel_index, message_index)) = message_selection_for_message_id(
        &state.snapshot.messages,
        state.snapshot.identity_hash.as_deref(),
        sent_message_id,
    ) {
        state.selected_channel = channel_index;
        state.selected_message = message_index;
    }

    clamp_selection(state);
}

fn message_selection_for_message_id(
    messages: &[Value],
    self_identity: Option<&str>,
    sent_message_id: &str,
) -> Option<(usize, usize)> {
    let model = messages::MessagePaneModel::from_messages(messages, self_identity);
    for (channel_index, channel) in model.channels.iter().enumerate() {
        for (message_index, source_index) in channel.indices.iter().enumerate() {
            let Some(message) = messages.get(*source_index) else {
                continue;
            };
            if message
                .get("id")
                .and_then(Value::as_str)
                .is_some_and(|value| value == sent_message_id)
            {
                return Some((channel_index, message_index));
            }
        }
    }
    None
}

fn send_message_from_composer(
    ctx: &RuntimeContext,
    rpc: &RpcClient,
    compose: &ComposeState,
) -> Result<ComposeSendResult> {
    let contacts = load_contacts(&ctx.profile_name).unwrap_or_default();
    let destination = compose.destination.trim();
    let content = compose.content.trim();

    if destination.is_empty() || content.is_empty() {
        return Err(anyhow!(
            "destination and content are required before sending"
        ));
    }

    let source = if compose.source.trim().is_empty() {
        resolve_runtime_identity_hash(rpc)?
    } else {
        compose.source.trim().to_string()
    };

    let resolved_destination =
        resolve_contact_hash(&contacts, destination).unwrap_or_else(|| destination.to_string());
    let resolved_source = resolve_contact_hash(&contacts, &source).unwrap_or(source);
    let message_id = format!("tui-{}", chrono_like_now_secs());

    let params = json!({
        "id": message_id.clone(),
        "destination": resolved_destination.clone(),
        "source": resolved_source,
        "title": compose.title.trim(),
        "content": content,
    });

    let used_legacy_api = match rpc.call("send_message_v2", Some(params.clone())) {
        Ok(_) => false,
        Err(_) => {
            rpc.call("send_message", Some(params))?;
            true
        }
    };

    Ok(ComposeSendResult {
        message_id,
        destination: resolved_destination,
        used_legacy_api,
    })
}

fn resolve_runtime_identity_hash(rpc: &RpcClient) -> Result<String> {
    for method in ["daemon_status_ex", "status"] {
        if let Ok(response) = rpc.call(method, None) {
            if let Some(identity_hash) = identity_hash_from_status(&response) {
                return Ok(identity_hash);
            }
        }
    }
    Err(anyhow!(
        "source missing and daemon identity unavailable; start daemon or set source explicitly"
    ))
}

fn restart_daemon(ctx: &RuntimeContext) -> Result<String> {
    let mut runtime_settings =
        load_profile_settings(&ctx.profile_name).unwrap_or_else(|_| ctx.profile_settings.clone());
    if let Some(rpc_override) = ctx.cli.rpc.clone() {
        runtime_settings.rpc = rpc_override;
    }

    if runtime_settings.managed {
        let supervisor = DaemonSupervisor::new(&ctx.profile_name, runtime_settings.clone());
        let status = supervisor.restart(None, Some(true), None)?;
        return Ok(match status.pid {
            Some(pid) => format!("daemon started/restarted (pid {pid})"),
            None => "daemon start/restart requested".to_string(),
        } + &transport_suffix(&status));
    }

    runtime_settings.managed = true;
    save_profile_settings(&runtime_settings)?;
    let supervisor = DaemonSupervisor::new(&ctx.profile_name, runtime_settings);
    let status = supervisor.restart(None, Some(true), None)?;
    Ok(match status.pid {
        Some(pid) => format!("managed mode enabled; daemon started (pid {pid})"),
        None => "managed mode enabled; daemon start requested".to_string(),
    } + &transport_suffix(&status))
}

fn discover_peers(ctx: &RuntimeContext, rpc: &RpcClient, state: &mut TuiState) -> Result<String> {
    let baseline_peers = state.snapshot.peers.len();
    let baseline_events = state.snapshot.events.len();

    let mut announces_sent = 0usize;
    let mut announce_failures = 0usize;
    let mut first_error = None;

    for _ in 0..DISCOVERY_ANNOUNCE_BURST {
        match rpc.call("announce_now", None) {
            Ok(_) => announces_sent += 1,
            Err(err) => {
                announce_failures += 1;
                if first_error.is_none() {
                    first_error = Some(err.to_string());
                }
            }
        }
        std::thread::sleep(DISCOVERY_ANNOUNCE_GAP);
    }

    if announces_sent == 0 {
        return Err(anyhow!(
            "all announce attempts failed: {}",
            first_error.unwrap_or_else(|| "unknown error".to_string())
        ));
    }

    let deadline = Instant::now() + DISCOVERY_WINDOW;
    while Instant::now() < deadline {
        let rpc_display = state.snapshot.rpc.clone();
        let outcome = refresh_snapshot(
            ctx,
            rpc,
            &mut state.snapshot,
            false,
            state.profile_managed,
            &rpc_display,
        );
        state.connected = outcome.connected;
        state.last_refresh_ms = Some(outcome.elapsed_ms);
        clamp_selection(state);

        if !outcome.connected {
            return Err(anyhow!(
                "rpc unreachable during discovery sweep; verify daemon and rpc endpoint"
            ));
        }
        if state.snapshot.peers.len() > baseline_peers {
            break;
        }
        std::thread::sleep(DISCOVERY_POLL_INTERVAL);
    }

    let peer_delta = state.snapshot.peers.len().saturating_sub(baseline_peers);
    let event_delta = state.snapshot.events.len().saturating_sub(baseline_events);
    let announce_event_delta = state
        .snapshot
        .events
        .iter()
        .skip(baseline_events)
        .filter(|event| event.contains("announce"))
        .count();

    let mut message = if peer_delta > 0 {
        format!(
            "discovery done: {peer_delta} new peer(s), {} total",
            state.snapshot.peers.len()
        )
    } else {
        format!(
            "discovery done: no new peers yet ({} total)",
            state.snapshot.peers.len()
        )
    };
    message.push_str(&format!(
        ", announces sent={announces_sent}, announce events={announce_event_delta}, new events={event_delta}"
    ));
    if announce_failures > 0 {
        message.push_str(&format!(", failed announces={announce_failures}"));
    }

    Ok(message)
}

fn auto_start_managed_daemon(ctx: &RuntimeContext, state: &mut TuiState) {
    let supervisor = DaemonSupervisor::new(&ctx.profile_name, ctx.profile_settings.clone());
    set_status(
        state,
        StatusLevel::Info,
        "Managed profile is offline; starting daemon...",
    );
    match supervisor.start(None, None, None) {
        Ok(status) => {
            if let Some(pid) = status.pid {
                set_status(
                    state,
                    StatusLevel::Success,
                    format!(
                        "Managed daemon started (pid {pid}); connecting...{}",
                        transport_suffix(&status)
                    ),
                );
            } else {
                set_status(
                    state,
                    StatusLevel::Success,
                    format!(
                        "Managed daemon start requested; connecting...{}",
                        transport_suffix(&status)
                    ),
                );
            }
        }
        Err(err) => {
            set_status(
                state,
                StatusLevel::Warning,
                format!("Managed daemon auto-start failed: {err}. Press r to retry."),
            );
        }
    }
}

fn transport_suffix(status: &crate::cli::daemon::DaemonStatus) -> String {
    if status.transport_inferred {
        let transport = status.transport.as_deref().unwrap_or("127.0.0.1:0");
        format!(" (inferred transport: {transport})")
    } else {
        String::new()
    }
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

fn save_contact_from_editor(ctx: &RuntimeContext, editor: &ContactEditorState) -> Result<String> {
    let mut contacts = load_contacts(&ctx.profile_name)?;
    let alias = editor.alias.trim().to_string();
    let hash = editor.hash.trim().to_string();
    let notes = optional_trimmed_string(&editor.notes);

    let contact = validate_contact(ContactEntry { alias, hash, notes })?;
    let saved_alias = contact.alias.clone();
    let verb = match &editor.mode {
        ContactEditorMode::Add => "saved",
        ContactEditorMode::Edit { original_alias } => {
            if !original_alias.eq_ignore_ascii_case(&contact.alias) {
                let _ = remove_contact_by_alias(&mut contacts, original_alias);
            }
            "updated"
        }
    };
    upsert_contact(&mut contacts, contact);
    save_contacts(&ctx.profile_name, &contacts)?;
    Ok(format!("contact '{saved_alias}' {verb}"))
}

fn reload_contacts_from_local(ctx: &RuntimeContext, snapshot: &mut TuiSnapshot) -> Result<()> {
    let contacts = load_contacts(&ctx.profile_name)?;
    snapshot.contacts = contacts_to_values(&contacts);
    Ok(())
}

fn annotate_snapshot_peers_with_contacts(snapshot: &mut TuiSnapshot) {
    let contacts = snapshot
        .contacts
        .iter()
        .filter_map(|value| serde_json::from_value::<ContactEntry>(value.clone()).ok())
        .collect::<Vec<_>>();
    annotate_peers_with_contacts(&mut snapshot.peers, &contacts);
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

fn open_compose_from_selected_peer(state: &mut TuiState) -> Result<String> {
    let Some(peer_hash) = selected_peer_name(state) else {
        return Err(anyhow!("no peer selected"));
    };

    let peer_label = selected_peer_value(state)
        .and_then(peer_display_name)
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| peer_hash.clone());

    open_compose_with_destination(state, peer_hash, peer_label)
}

fn open_compose_from_selected_contact(state: &mut TuiState) -> Result<String> {
    let Some(contact) = selected_contact_value(state) else {
        return Err(anyhow!("no contact selected"));
    };
    let hash = contact
        .get("hash")
        .and_then(Value::as_str)
        .ok_or_else(|| anyhow!("selected contact has no hash"))?
        .to_string();
    let label = contact
        .get("alias")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| hash.clone());
    open_compose_with_destination(state, hash, label)
}

fn open_compose_from_selected_message(state: &mut TuiState) -> Result<String> {
    let Some(message) = selected_message_value(state) else {
        return Err(anyhow!("no message selected"));
    };

    let source = preferred_source_identity(state);
    let Some(destination_hash) = reply_destination_from_message(message, source.as_deref()) else {
        return Err(anyhow!("selected message has no source/destination"));
    };

    let label = destination_hash.clone();
    let mut composer =
        ComposeState::with_destination_and_source(destination_hash.clone(), source.clone());
    if let Some(reply_title) = message_reply_title(message) {
        composer.title = reply_title;
    }

    state.pane = Pane::Messages;
    state.composer = Some(composer);

    let source_hint = if source.is_some() {
        "source auto-filled"
    } else {
        "source required"
    };
    Ok(format!(
        "Reply compose to {label}: destination prefilled ({destination_hash}), {source_hint}. Fill content, Enter twice to send"
    ))
}

fn open_compose_with_destination(
    state: &mut TuiState,
    destination_hash: String,
    label: String,
) -> Result<String> {
    let source = preferred_source_identity(state);
    state.pane = Pane::Messages;
    state.composer = Some(ComposeState::with_destination_and_source(
        destination_hash.clone(),
        source.clone(),
    ));
    let source_hint = if source.is_some() {
        "source auto-filled"
    } else {
        "source required"
    };
    Ok(format!(
        "Compose to {label}: destination prefilled ({destination_hash}), {source_hint}. Fill content, Enter twice to send"
    ))
}

fn selected_message_source_index(state: &TuiState) -> Option<usize> {
    let message_model = messages::MessagePaneModel::from_messages(
        &state.snapshot.messages,
        state.snapshot.identity_hash.as_deref(),
    );
    message_model.message_index(state.selected_channel, state.selected_message)
}

fn selected_message_value(state: &TuiState) -> Option<&Value> {
    let index = selected_message_source_index(state)?;
    state.snapshot.messages.get(index)
}

fn message_identity_field(message: &Value, keys: &[&str]) -> Option<String> {
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

fn reply_destination_from_message(message: &Value, self_identity: Option<&str>) -> Option<String> {
    let source = message_identity_field(message, &["source", "from", "sender"]);
    let destination = message_identity_field(message, &["destination", "to", "recipient"]);

    if let Some(self_id) = self_identity
        .map(str::trim)
        .filter(|value| !value.is_empty())
    {
        if destination
            .as_deref()
            .is_some_and(|value| value.eq_ignore_ascii_case(self_id))
        {
            if let Some(source_value) = source.clone() {
                return Some(source_value);
            }
        }

        if source
            .as_deref()
            .is_some_and(|value| value.eq_ignore_ascii_case(self_id))
        {
            if let Some(destination_value) = destination.clone() {
                return Some(destination_value);
            }
        }
    }

    destination.or(source)
}

fn message_reply_title(message: &Value) -> Option<String> {
    let title = message_identity_field(message, &["title", "subject"])?;
    if title.to_ascii_lowercase().starts_with("re:") {
        Some(title)
    } else {
        Some(format!("Re: {title}"))
    }
}

fn open_contact_editor_from_selected_peer(state: &mut TuiState) -> Result<String> {
    let Some(peer) = selected_peer_value(state) else {
        return Err(anyhow!("no peer selected"));
    };
    state.contact_editor = Some(ContactEditorState::from_peer(peer));
    Ok("New contact from peer: set alias if needed, Enter to save".into())
}

fn open_selected_contact_editor(state: &mut TuiState) -> Result<String> {
    let Some(contact) = selected_contact_value(state) else {
        return Err(anyhow!("no contact selected"));
    };
    let alias = contact
        .get("alias")
        .and_then(Value::as_str)
        .unwrap_or("<unknown>")
        .to_string();
    state.contact_editor = Some(ContactEditorState::from_existing(contact));
    Ok(format!("Edit contact '{alias}'"))
}

fn remove_selected_contact(ctx: &RuntimeContext, state: &mut TuiState) -> Result<String> {
    let Some(alias) = selected_contact_alias(state) else {
        return Err(anyhow!("no contact selected"));
    };

    let mut contacts = load_contacts(&ctx.profile_name)?;
    if !remove_contact_by_alias(&mut contacts, &alias) {
        return Err(anyhow!("selected contact not found"));
    }
    save_contacts(&ctx.profile_name, &contacts)?;
    reload_contacts_from_local(ctx, &mut state.snapshot)?;
    annotate_snapshot_peers_with_contacts(&mut state.snapshot);
    clamp_selection(state);
    Ok(format!("contact '{alias}' removed"))
}

fn selected_peer_name(state: &TuiState) -> Option<String> {
    let index = selected_peer_source_index(state)?;
    state
        .snapshot
        .peers
        .get(index)
        .and_then(|peer| peer.get("peer"))
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
}

fn selected_peer_source_index(state: &TuiState) -> Option<usize> {
    filtered_peer_indices(state)
        .get(state.selected_peer)
        .copied()
}

fn selected_peer_value(state: &TuiState) -> Option<&Value> {
    let index = selected_peer_source_index(state)?;
    state.snapshot.peers.get(index)
}

pub(super) fn peer_display_name(peer: &Value) -> Option<&str> {
    for key in ["name", "display_name", "alias"] {
        if let Some(name) = peer
            .get(key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            return Some(name);
        }
    }
    None
}

fn normalize_identity_hash(value: String) -> Option<String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn identity_hash_from_status(value: &Value) -> Option<String> {
    for key in ["delivery_destination_hash", "identity_hash"] {
        if let Some(hash) = value
            .get(key)
            .and_then(Value::as_str)
            .map(ToOwned::to_owned)
            .and_then(normalize_identity_hash)
        {
            return Some(hash);
        }
    }
    None
}

fn preferred_source_identity(state: &TuiState) -> Option<String> {
    state
        .snapshot
        .identity_hash
        .as_ref()
        .cloned()
        .and_then(normalize_identity_hash)
}

fn selected_contact_value(state: &TuiState) -> Option<&Value> {
    state.snapshot.contacts.get(state.selected_contact)
}

fn selected_contact_alias(state: &TuiState) -> Option<String> {
    selected_contact_value(state)
        .and_then(|contact| contact.get("alias"))
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
}

fn filtered_peers(state: &TuiState) -> Vec<Value> {
    filtered_peer_indices(state)
        .into_iter()
        .filter_map(|index| state.snapshot.peers.get(index).cloned())
        .collect()
}

fn filtered_peer_indices(state: &TuiState) -> Vec<usize> {
    let query = state.peer_filter.trim().to_ascii_lowercase();
    if query.is_empty() {
        return (0..state.snapshot.peers.len()).collect();
    }

    state
        .snapshot
        .peers
        .iter()
        .enumerate()
        .filter_map(|(index, peer)| {
            let hash = peer
                .get("peer")
                .and_then(Value::as_str)
                .map(str::to_ascii_lowercase)
                .unwrap_or_default();
            let name = peer_display_name(peer)
                .map(str::to_ascii_lowercase)
                .unwrap_or_default();
            let contact_alias = peer
                .get("contact_alias")
                .and_then(Value::as_str)
                .map(str::to_ascii_lowercase)
                .unwrap_or_default();
            if hash.contains(&query) || name.contains(&query) || contact_alias.contains(&query) {
                Some(index)
            } else {
                None
            }
        })
        .collect()
}

fn open_selected_peer_details(state: &mut TuiState) -> Result<String> {
    let Some(index) = selected_peer_source_index(state) else {
        return Err(anyhow!("no peer selected"));
    };
    state.peer_details_open = true;
    let label = state
        .snapshot
        .peers
        .get(index)
        .and_then(|peer| {
            peer_display_name(peer).map(ToOwned::to_owned).or_else(|| {
                peer.get("peer")
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned)
            })
        })
        .unwrap_or_else(|| "<unknown>".to_string());
    Ok(format!("Peer details: {label} (Esc/Enter to close)"))
}

fn handle_peer_details_key(key: KeyEvent, state: &mut TuiState) {
    match key.code {
        KeyCode::Esc | KeyCode::Enter => {
            state.peer_details_open = false;
            set_status(state, StatusLevel::Info, "Peer details closed");
        }
        _ => {}
    }
}

fn handle_peer_filter_key(key: KeyEvent, state: &mut TuiState) {
    match key.code {
        KeyCode::Esc => {
            state.peer_filter.clear();
            state.peer_filter_editing = false;
            clamp_selection(state);
            set_status(state, StatusLevel::Info, "Peer filter cleared");
        }
        KeyCode::Enter => {
            state.peer_filter_editing = false;
            clamp_selection(state);
            set_status(
                state,
                StatusLevel::Info,
                format!(
                    "Peer filter active: {}",
                    if state.peer_filter.trim().is_empty() {
                        "<none>"
                    } else {
                        state.peer_filter.trim()
                    }
                ),
            );
        }
        KeyCode::Backspace => {
            state.peer_filter.pop();
            clamp_selection(state);
        }
        KeyCode::Delete => {
            state.peer_filter.clear();
            clamp_selection(state);
        }
        KeyCode::Char(c) => {
            state.peer_filter.push(c);
            clamp_selection(state);
        }
        _ => {}
    }
}

fn selected_interface_name(state: &TuiState) -> Option<String> {
    state
        .snapshot
        .interfaces
        .get(state.selected_interface)
        .and_then(|iface| iface.get("name"))
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
}

fn contacts_to_values(contacts: &[ContactEntry]) -> Vec<Value> {
    serde_json::to_value(contacts)
        .ok()
        .and_then(|value| value.as_array().cloned())
        .unwrap_or_default()
}

fn annotate_peers_with_contacts(peers: &mut [Value], contacts: &[ContactEntry]) {
    for peer in peers.iter_mut() {
        let Some(hash) = peer.get("peer").and_then(Value::as_str) else {
            continue;
        };
        if let Some(contact) = find_contact_by_hash(contacts, hash) {
            peer["contact_alias"] = Value::String(contact.alias.clone());
        }
    }
}

fn merge_peer_from_event(peers: &mut Vec<Value>, event: &RpcEvent) -> bool {
    let event_type = event.event_type.as_str();
    if !(event_type.contains("peer") || event_type.contains("announce")) {
        return false;
    }

    let payload = &event.payload;
    let (peer_hash, peer_payload) = if let Some(hash) = payload.get("peer").and_then(Value::as_str)
    {
        (hash.to_string(), payload)
    } else if let Some(peer_obj) = payload.get("peer").and_then(Value::as_object) {
        let Some(hash) = peer_obj.get("peer").and_then(Value::as_str) else {
            return false;
        };
        let Some(peer_payload) = payload.get("peer") else {
            return false;
        };
        (hash.to_string(), peer_payload)
    } else {
        return false;
    };

    let index = if let Some(index) = peers.iter().position(|peer| {
        peer.get("peer")
            .and_then(Value::as_str)
            .is_some_and(|existing| existing.eq_ignore_ascii_case(&peer_hash))
    }) {
        index
    } else {
        peers.push(json!({ "peer": peer_hash }));
        peers.len().saturating_sub(1)
    };

    let Some(peer) = peers.get_mut(index) else {
        return false;
    };

    let mut changed = false;
    if !peer
        .get("peer")
        .and_then(Value::as_str)
        .is_some_and(|existing| existing.eq_ignore_ascii_case(&peer_hash))
    {
        peer["peer"] = Value::String(peer_hash);
        changed = true;
    }

    if let Some(name) = peer_display_name(peer_payload) {
        if peer_display_name(peer) != Some(name) {
            peer["name"] = Value::String(name.to_string());
            changed = true;
        }
    }

    for key in ["name_source", "first_seen", "seen_count"] {
        if let Some(value) = peer_payload.get(key) {
            if peer.get(key) != Some(value) {
                peer[key] = value.clone();
                changed = true;
            }
        }
    }

    if let Some(last_seen) = peer_payload
        .get("last_seen")
        .cloned()
        .or_else(|| peer_payload.get("timestamp").cloned())
    {
        if peer.get("last_seen") != Some(&last_seen) {
            peer["last_seen"] = last_seen;
            changed = true;
        }
    }

    changed
}

fn as_vec(value: Value, key: &str) -> Vec<Value> {
    if let Some(items) = value.get(key).and_then(Value::as_array) {
        return items.clone();
    }
    value.as_array().cloned().unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::{
        as_vec, filtered_peer_indices, merge_peer_from_event, open_compose_from_selected_contact,
        open_compose_from_selected_message, open_compose_from_selected_peer,
        reply_destination_from_message, selected_peer_name, Pane, StatusLevel, TuiSnapshot,
        TuiState, TuiTheme,
    };
    use crate::cli::rpc_client::RpcEvent;
    use serde_json::json;
    use std::time::Instant;

    #[test]
    fn as_vec_extracts_wrapped_array() {
        let value = json!({
            "peers": [
                { "peer": "abc" },
                { "peer": "def" }
            ]
        });
        let peers = as_vec(value, "peers");
        assert_eq!(peers.len(), 2);
    }

    #[test]
    fn as_vec_extracts_direct_array() {
        let value = json!([{ "id": "m1" }]);
        let messages = as_vec(value, "messages");
        assert_eq!(messages.len(), 1);
    }

    #[test]
    fn as_vec_returns_empty_for_missing_key() {
        let value = json!({ "items": [] });
        let peers = as_vec(value, "peers");
        assert!(peers.is_empty());
    }

    #[test]
    fn filtered_peer_indices_matches_hash_and_name() {
        let mut state = sample_state(vec![
            json!({"peer": "abc123", "name": "Alice Node"}),
            json!({"peer": "def456", "name": "Bob Node"}),
            json!({"peer": "0011ff", "name": "Relay"}),
        ]);

        state.peer_filter = "alice".into();
        assert_eq!(filtered_peer_indices(&state), vec![0]);

        state.peer_filter = "def".into();
        assert_eq!(filtered_peer_indices(&state), vec![1]);

        state.peer_filter = String::new();
        assert_eq!(filtered_peer_indices(&state), vec![0, 1, 2]);
    }

    #[test]
    fn filtered_peer_indices_matches_display_name_and_contact_alias() {
        let mut state = sample_state(vec![
            json!({"peer": "abc123", "display_name": "Alpha Relay", "contact_alias": "work-relay"}),
            json!({"peer": "def456", "name": "Beta"}),
        ]);

        state.peer_filter = "alpha".into();
        assert_eq!(filtered_peer_indices(&state), vec![0]);

        state.peer_filter = "work".into();
        assert_eq!(filtered_peer_indices(&state), vec![0]);
    }

    #[test]
    fn selected_peer_name_uses_filtered_selection() {
        let mut state = sample_state(vec![
            json!({"peer": "hash-a", "name": "Alice"}),
            json!({"peer": "hash-b", "name": "Bob"}),
            json!({"peer": "hash-c", "name": "Bob Two"}),
        ]);
        state.peer_filter = "bob".into();
        state.selected_peer = 1;

        let selected = selected_peer_name(&state);
        assert_eq!(selected.as_deref(), Some("hash-c"));
    }

    #[test]
    fn compose_from_peer_prefills_destination_and_switches_to_messages() {
        let mut state = sample_state(vec![json!({
            "peer": "deadbeefcafe0011",
            "name": "Alice Node"
        })]);
        let message = open_compose_from_selected_peer(&mut state).expect("compose opens");

        assert_eq!(state.pane, Pane::Messages);
        let composer = state.composer.expect("composer state");
        assert_eq!(composer.destination, "deadbeefcafe0011");
        assert_eq!(composer.source, "self-hash");
        assert!(message.contains("destination prefilled"));
    }

    #[test]
    fn compose_from_contact_prefills_destination_and_switches_to_messages() {
        let mut state = sample_state(vec![]);
        state.snapshot.contacts = vec![json!({
            "alias": "alice",
            "hash": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
        })];

        let message = open_compose_from_selected_contact(&mut state).expect("compose opens");
        assert_eq!(state.pane, Pane::Messages);
        let composer = state.composer.expect("composer state");
        assert_eq!(composer.destination, "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
        assert_eq!(composer.source, "self-hash");
        assert!(message.contains("destination prefilled"));
    }

    #[test]
    fn compose_from_selected_message_prefills_reply_target_and_title() {
        let mut state = sample_state(vec![]);
        state.pane = Pane::Messages;
        state.snapshot.messages = vec![json!({
            "id": "msg-1",
            "source": "peer-hash",
            "destination": "self-hash",
            "title": "status update",
            "content": "hello"
        })];
        state.selected_message = 0;

        let message = open_compose_from_selected_message(&mut state).expect("compose opens");
        let composer = state.composer.expect("composer state");
        assert_eq!(composer.destination, "peer-hash");
        assert_eq!(composer.source, "self-hash");
        assert_eq!(composer.title, "Re: status update");
        assert!(!composer.send_armed);
        assert!(message.contains("Enter twice"));
    }

    #[test]
    fn reply_destination_prefers_counterparty_when_self_identity_is_present() {
        let inbound = json!({
            "source": "peer-a",
            "destination": "self-hash"
        });
        assert_eq!(
            reply_destination_from_message(&inbound, Some("self-hash")).as_deref(),
            Some("peer-a")
        );

        let outbound = json!({
            "source": "self-hash",
            "destination": "peer-b"
        });
        assert_eq!(
            reply_destination_from_message(&outbound, Some("self-hash")).as_deref(),
            Some("peer-b")
        );
    }

    #[test]
    fn merge_peer_from_event_applies_name_fields() {
        let mut peers = vec![json!({
            "peer": "deadbeef",
            "last_seen": 1
        })];
        let event = RpcEvent {
            event_type: "announce_received".into(),
            payload: json!({
                "peer": "deadbeef",
                "name": "Alice Node",
                "name_source": "pn_meta",
                "first_seen": 1,
                "timestamp": 10,
                "seen_count": 2
            }),
        };

        assert!(merge_peer_from_event(&mut peers, &event));
        assert_eq!(peers[0]["name"], "Alice Node");
        assert_eq!(peers[0]["name_source"], "pn_meta");
        assert_eq!(peers[0]["last_seen"], 10);
        assert_eq!(peers[0]["seen_count"], 2);
    }

    fn sample_state(peers: Vec<serde_json::Value>) -> TuiState {
        TuiState {
            pane: Pane::Peers,
            selected_message: 0,
            selected_channel: 0,
            selected_peer: 0,
            selected_contact: 0,
            selected_interface: 0,
            status_line: String::new(),
            status_level: StatusLevel::Info,
            snapshot: TuiSnapshot {
                profile: "default".into(),
                rpc: "127.0.0.1:4243".into(),
                identity_hash: Some("self-hash".into()),
                daemon_running: false,
                messages: Vec::new(),
                peers,
                contacts: Vec::new(),
                interfaces: Vec::new(),
                events: Vec::new(),
                logs: Vec::new(),
            },
            theme: TuiTheme::default(),
            connected: false,
            first_refresh_done: false,
            spinner_tick: 0,
            started_at: Instant::now(),
            last_refresh_ms: None,
            welcome_dismissed: false,
            composer: None,
            contact_editor: None,
            interface_editor: None,
            profile_editor: None,
            peer_filter: String::new(),
            peer_filter_editing: false,
            peer_details_open: false,
            profile_managed: false,
        }
    }
}

fn clamp_selection(state: &mut TuiState) {
    let message_model = messages::MessagePaneModel::from_messages(
        &state.snapshot.messages,
        state.snapshot.identity_hash.as_deref(),
    );
    if state.selected_channel >= message_model.channels.len() {
        state.selected_channel = message_model.channels.len().saturating_sub(1);
    }
    let channel_messages = message_model.channel_len(state.selected_channel);
    if state.selected_message >= channel_messages {
        state.selected_message = channel_messages.saturating_sub(1);
    }

    let filtered_len = filtered_peer_indices(state).len();
    if state.selected_peer >= filtered_len {
        state.selected_peer = filtered_len.saturating_sub(1);
    }
    if filtered_len == 0 {
        state.peer_details_open = false;
    }
    if state.selected_contact >= state.snapshot.contacts.len() {
        state.selected_contact = state.snapshot.contacts.len().saturating_sub(1);
    }
    if state.selected_interface >= state.snapshot.interfaces.len() {
        state.selected_interface = state.snapshot.interfaces.len().saturating_sub(1);
    }
}

fn increment_selection(state: &mut TuiState) {
    match state.pane {
        Pane::Messages => {
            let message_model = messages::MessagePaneModel::from_messages(
                &state.snapshot.messages,
                state.snapshot.identity_hash.as_deref(),
            );
            if state.selected_message + 1 < message_model.channel_len(state.selected_channel) {
                state.selected_message += 1;
            }
        }
        Pane::Peers => {
            if state.selected_peer + 1 < filtered_peer_indices(state).len() {
                state.selected_peer += 1;
            }
        }
        Pane::Contacts => {
            if state.selected_contact + 1 < state.snapshot.contacts.len() {
                state.selected_contact += 1;
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
        Pane::Contacts => {
            state.selected_contact = state.selected_contact.saturating_sub(1);
        }
        Pane::Interfaces => {
            state.selected_interface = state.selected_interface.saturating_sub(1);
        }
        _ => {}
    }
}

fn select_next_message_channel(state: &mut TuiState) {
    let message_model = messages::MessagePaneModel::from_messages(
        &state.snapshot.messages,
        state.snapshot.identity_hash.as_deref(),
    );
    if message_model.channels.is_empty() {
        state.selected_channel = 0;
        state.selected_message = 0;
        return;
    }
    if state.selected_channel + 1 < message_model.channels.len() {
        state.selected_channel += 1;
    }
    state.selected_message = 0;
    clamp_selection(state);
}

fn select_previous_message_channel(state: &mut TuiState) {
    let message_model = messages::MessagePaneModel::from_messages(
        &state.snapshot.messages,
        state.snapshot.identity_hash.as_deref(),
    );
    if message_model.channels.is_empty() {
        state.selected_channel = 0;
        state.selected_message = 0;
        return;
    }
    state.selected_channel = state.selected_channel.saturating_sub(1);
    state.selected_message = 0;
    clamp_selection(state);
}

fn next_pane(current: Pane) -> Pane {
    match current {
        Pane::Dashboard => Pane::Messages,
        Pane::Messages => Pane::Peers,
        Pane::Peers => Pane::Contacts,
        Pane::Contacts => Pane::Interfaces,
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
        Pane::Contacts => Pane::Peers,
        Pane::Interfaces => Pane::Contacts,
        Pane::Events => Pane::Interfaces,
        Pane::Logs => Pane::Events,
    }
}

fn index_of(pane: Pane) -> usize {
    match pane {
        Pane::Dashboard => 0,
        Pane::Messages => 1,
        Pane::Peers => 2,
        Pane::Contacts => 3,
        Pane::Interfaces => 4,
        Pane::Events => 5,
        Pane::Logs => 6,
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
