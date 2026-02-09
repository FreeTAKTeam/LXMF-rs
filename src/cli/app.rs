use anyhow::{anyhow, Result};
use clap::{Args, Parser, Subcommand, ValueEnum};

use crate::cli::commands_contact;
use crate::cli::commands_daemon;
use crate::cli::commands_iface;
use crate::cli::commands_message;
use crate::cli::commands_paper;
use crate::cli::commands_peer;
use crate::cli::commands_profile;
use crate::cli::commands_propagation;
use crate::cli::commands_stamp;
use crate::cli::output::Output;
use crate::cli::profile::{
    load_profile_settings, profile_exists, profile_paths, selected_profile_name, ProfilePaths,
    ProfileSettings,
};
use crate::cli::rpc_client::RpcClient;
use crate::cli::tui;

#[derive(Debug, Clone, Parser)]
#[command(name = "lxmf", about = "LXMF operator CLI and TUI", version)]
pub struct Cli {
    #[arg(long, default_value = "default")]
    pub profile: String,
    #[arg(long)]
    pub rpc: Option<String>,
    #[arg(long)]
    pub json: bool,
    #[arg(long)]
    pub no_color: bool,
    #[arg(long)]
    pub quiet: bool,
    #[arg(long, short = 'v', action = clap::ArgAction::Count)]
    pub verbose: u8,
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Clone, Subcommand)]
pub enum Command {
    Profile(ProfileCommand),
    Contact(ContactCommand),
    Daemon(DaemonCommand),
    Iface(IfaceCommand),
    Peer(PeerCommand),
    Message(MessageCommand),
    Propagation(PropagationCommand),
    Paper(PaperCommand),
    Stamp(StampCommand),
    Announce(AnnounceCommand),
    Events(EventsCommand),
    Tui(TuiCommand),
}

#[derive(Debug, Clone, Args)]
pub struct ContactCommand {
    #[command(subcommand)]
    pub action: ContactAction,
}

#[derive(Debug, Clone, Subcommand)]
pub enum ContactAction {
    List {
        #[arg(long)]
        query: Option<String>,
        #[arg(long)]
        limit: Option<usize>,
    },
    Add(ContactUpsertArgs),
    Show {
        selector: String,
        #[arg(long)]
        exact: bool,
    },
    Remove {
        selector: String,
        #[arg(long)]
        exact: bool,
    },
    Import {
        path: String,
        #[arg(long)]
        replace: bool,
    },
    Export {
        path: String,
    },
}

#[derive(Debug, Clone, Args)]
pub struct ContactUpsertArgs {
    pub alias: String,
    pub hash: String,
    #[arg(long)]
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Args)]
pub struct ProfileCommand {
    #[command(subcommand)]
    pub action: ProfileAction,
}

#[derive(Debug, Clone, Subcommand)]
pub enum ProfileAction {
    Init {
        name: String,
        #[arg(long)]
        managed: bool,
        #[arg(long)]
        rpc: Option<String>,
    },
    List,
    Show {
        #[arg(long)]
        name: Option<String>,
    },
    Select {
        name: String,
    },
    Set {
        #[arg(long)]
        display_name: Option<String>,
        #[arg(long)]
        clear_display_name: bool,
        #[arg(long)]
        name: Option<String>,
    },
    ImportIdentity {
        path: String,
        #[arg(long)]
        name: Option<String>,
    },
    ExportIdentity {
        path: String,
        #[arg(long)]
        name: Option<String>,
    },
    Delete {
        name: String,
        #[arg(long)]
        force: bool,
    },
}

#[derive(Debug, Clone, Args)]
pub struct DaemonCommand {
    #[command(subcommand)]
    pub action: DaemonAction,
}

#[derive(Debug, Clone, Subcommand)]
pub enum DaemonAction {
    Start {
        #[arg(long)]
        managed: bool,
        #[arg(long)]
        reticulumd: Option<String>,
        #[arg(long)]
        transport: Option<String>,
    },
    Stop,
    Restart {
        #[arg(long)]
        managed: bool,
        #[arg(long)]
        reticulumd: Option<String>,
        #[arg(long)]
        transport: Option<String>,
    },
    Status,
    Logs {
        #[arg(long, default_value_t = 100)]
        tail: usize,
    },
}

#[derive(Debug, Clone, Args)]
pub struct IfaceCommand {
    #[command(subcommand)]
    pub action: IfaceAction,
}

#[derive(Debug, Clone, Subcommand)]
pub enum IfaceAction {
    List,
    Add(IfaceMutationArgs),
    Remove {
        name: String,
    },
    Enable {
        name: String,
    },
    Disable {
        name: String,
    },
    Apply {
        #[arg(long)]
        restart: bool,
    },
}

#[derive(Debug, Clone, Args)]
pub struct IfaceMutationArgs {
    pub name: String,
    #[arg(long = "type")]
    pub kind: String,
    #[arg(long)]
    pub host: Option<String>,
    #[arg(long)]
    pub port: Option<u16>,
    #[arg(long, default_value_t = true)]
    pub enabled: bool,
}

#[derive(Debug, Clone, Args)]
pub struct PeerCommand {
    #[command(subcommand)]
    pub action: PeerAction,
}

#[derive(Debug, Clone, Subcommand)]
pub enum PeerAction {
    List {
        #[arg(long)]
        query: Option<String>,
        #[arg(long)]
        limit: Option<usize>,
    },
    Show {
        selector: String,
        #[arg(long)]
        exact: bool,
    },
    Watch {
        #[arg(long, default_value_t = 2)]
        interval_secs: u64,
    },
    Sync {
        peer: String,
    },
    Unpeer {
        peer: String,
    },
    Clear,
}

#[derive(Debug, Clone, Args)]
pub struct MessageCommand {
    #[command(subcommand)]
    pub action: MessageAction,
}

#[derive(Debug, Clone, Subcommand)]
pub enum MessageAction {
    Send(MessageSendArgs),
    List,
    Show {
        id: String,
    },
    Watch {
        #[arg(long, default_value_t = 2)]
        interval_secs: u64,
    },
    Clear,
}

#[derive(Debug, Clone, Args)]
pub struct MessageSendArgs {
    #[arg(long)]
    pub id: Option<String>,
    #[arg(long)]
    pub source: String,
    #[arg(long)]
    pub destination: String,
    #[arg(long, default_value = "")]
    pub title: String,
    #[arg(long)]
    pub content: String,
    #[arg(long)]
    pub fields_json: Option<String>,
    #[arg(long)]
    pub method: Option<DeliveryMethodArg>,
    #[arg(long)]
    pub stamp_cost: Option<u32>,
    #[arg(long)]
    pub include_ticket: bool,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
pub enum DeliveryMethodArg {
    Opportunistic,
    Direct,
    Propagated,
    Paper,
}

impl DeliveryMethodArg {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Opportunistic => "opportunistic",
            Self::Direct => "direct",
            Self::Propagated => "propagated",
            Self::Paper => "paper",
        }
    }
}

#[derive(Debug, Clone, Args)]
pub struct PropagationCommand {
    #[command(subcommand)]
    pub action: PropagationAction,
}

#[derive(Debug, Clone, Subcommand)]
pub enum PropagationAction {
    Status,
    Enable {
        #[arg(long)]
        enabled: bool,
        #[arg(long)]
        store_root: Option<String>,
        #[arg(long)]
        target_cost: Option<u32>,
    },
    Ingest {
        #[arg(long)]
        transient_id: Option<String>,
        #[arg(long)]
        payload_hex: Option<String>,
    },
    Fetch {
        transient_id: String,
    },
    Sync,
}

#[derive(Debug, Clone, Args)]
pub struct PaperCommand {
    #[command(subcommand)]
    pub action: PaperAction,
}

#[derive(Debug, Clone, Subcommand)]
pub enum PaperAction {
    IngestUri { uri: String },
    Show,
}

#[derive(Debug, Clone, Args)]
pub struct StampCommand {
    #[command(subcommand)]
    pub action: StampAction,
}

#[derive(Debug, Clone, Subcommand)]
pub enum StampAction {
    Target,
    Get,
    Set {
        #[arg(long)]
        target_cost: Option<u32>,
        #[arg(long)]
        flexibility: Option<u32>,
    },
    GenerateTicket {
        destination: String,
        #[arg(long)]
        ttl_secs: Option<u64>,
    },
    Cache,
}

#[derive(Debug, Clone, Args)]
pub struct AnnounceCommand {
    #[command(subcommand)]
    pub action: AnnounceAction,
}

#[derive(Debug, Clone, Subcommand)]
pub enum AnnounceAction {
    Now,
}

#[derive(Debug, Clone, Args)]
pub struct EventsCommand {
    #[command(subcommand)]
    pub action: EventsAction,
}

#[derive(Debug, Clone, Subcommand)]
pub enum EventsAction {
    Watch {
        #[arg(long, default_value_t = 2)]
        interval_secs: u64,
        #[arg(long)]
        once: bool,
    },
}

#[derive(Debug, Clone, Args)]
pub struct TuiCommand {
    #[arg(long, default_value_t = 1000)]
    pub refresh_ms: u64,
}

#[derive(Debug)]
pub struct RuntimeContext {
    pub cli: Cli,
    pub profile_name: String,
    pub profile_settings: ProfileSettings,
    pub profile_paths: ProfilePaths,
    pub rpc: RpcClient,
    pub output: Output,
}

impl RuntimeContext {
    pub fn load(cli: Cli) -> Result<Self> {
        let profile_name = resolve_profile_name(&cli.profile)?;
        let mut profile_settings = load_profile_settings(&profile_name)?;
        if let Some(rpc) = cli.rpc.clone() {
            profile_settings.rpc = rpc;
        }
        let profile_paths = profile_paths(&profile_name)?;
        let rpc = RpcClient::new(&profile_settings.rpc);
        let output = Output::new(cli.json, cli.quiet, cli.no_color);

        Ok(Self {
            cli,
            profile_name,
            profile_settings,
            profile_paths,
            rpc,
            output,
        })
    }
}

pub fn run_cli(cli: Cli) -> Result<()> {
    let output = Output::new(cli.json, cli.quiet, cli.no_color);
    let command = cli.command.clone();
    match command {
        Command::Profile(command) => commands_profile::run(&cli, &command, &output),
        Command::Contact(command) => {
            let ctx = RuntimeContext::load(cli)?;
            commands_contact::run(&ctx, &command)
        }
        Command::Tui(command) => {
            let ctx = RuntimeContext::load(cli)?;
            tui::run_tui(&ctx, &command)
        }
        Command::Daemon(command) => {
            let ctx = RuntimeContext::load(cli)?;
            commands_daemon::run(&ctx, &command)
        }
        Command::Iface(command) => {
            let ctx = RuntimeContext::load(cli)?;
            commands_iface::run(&ctx, &command)
        }
        Command::Peer(command) => {
            let ctx = RuntimeContext::load(cli)?;
            commands_peer::run(&ctx, &command)
        }
        Command::Message(command) => {
            let ctx = RuntimeContext::load(cli)?;
            commands_message::run(&ctx, &command)
        }
        Command::Propagation(command) => {
            let ctx = RuntimeContext::load(cli)?;
            commands_propagation::run(&ctx, &command)
        }
        Command::Paper(command) => {
            let ctx = RuntimeContext::load(cli)?;
            commands_paper::run(&ctx, &command)
        }
        Command::Stamp(command) => {
            let ctx = RuntimeContext::load(cli)?;
            commands_stamp::run(&ctx, &command)
        }
        Command::Announce(command) => {
            let ctx = RuntimeContext::load(cli)?;
            commands_message::run_announce(&ctx, &command)
        }
        Command::Events(command) => {
            let ctx = RuntimeContext::load(cli)?;
            commands_message::run_events(&ctx, &command)
        }
    }
}

fn resolve_profile_name(cli_profile: &str) -> Result<String> {
    if profile_exists(cli_profile)? {
        return Ok(cli_profile.to_string());
    }

    if let Some(selected) = selected_profile_name()? {
        if profile_exists(&selected)? {
            return Ok(selected);
        }
    }

    if profile_exists("default")? {
        return Ok("default".into());
    }

    Err(anyhow!(
        "no profile found (requested '{}'). run `lxmf profile init <name>` first",
        cli_profile
    ))
}
