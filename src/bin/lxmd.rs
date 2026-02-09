use clap::{Parser, Subcommand};
use lxmf::lxmd::config::LxmdConfig;
use lxmf::lxmd::runtime::{execute_with_runtime, LxmdCommand, LxmdRuntime};

#[derive(Parser)]
struct Args {
    #[arg(long)]
    config: Option<String>,
    #[arg(long)]
    rnsconfig: Option<String>,
    #[arg(short = 'p', long)]
    propagation_node: bool,
    #[arg(short = 'i', long)]
    on_inbound: Option<String>,
    #[arg(short = 'v', long)]
    verbose: bool,
    #[arg(short = 'q', long)]
    quiet: bool,
    #[arg(short = 's', long)]
    service: bool,
    #[arg(long)]
    exampleconfig: bool,
    #[arg(long)]
    announce_interval_secs: Option<u64>,
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    Sync {
        #[arg(long)]
        peer: Option<String>,
    },
    Unpeer {
        #[arg(long)]
        peer: String,
    },
    Status,
}

fn main() {
    let args = Args::parse();

    if args.exampleconfig {
        println!("{}", LxmdConfig::example_toml());
        return;
    }

    let mut config = match args.config.as_ref() {
        Some(path) => match LxmdConfig::load_from_path(std::path::Path::new(path)) {
            Ok(config) => config,
            Err(err) => {
                eprintln!("failed to load config {}: {}", path, err);
                std::process::exit(1);
            }
        },
        None => LxmdConfig::default(),
    };

    if let Some(rnsconfig) = args.rnsconfig {
        config.rnsconfig = Some(rnsconfig);
    }
    if args.propagation_node {
        config.propagation_node = true;
    }
    if let Some(on_inbound) = args.on_inbound {
        config.on_inbound = Some(on_inbound);
    }
    if let Some(announce_interval_secs) = args.announce_interval_secs {
        config.announce_interval_secs = announce_interval_secs;
    }

    let command = match args.command {
        Some(Command::Sync { peer }) => LxmdCommand::Sync { peer },
        Some(Command::Unpeer { peer }) => LxmdCommand::Unpeer { peer },
        Some(Command::Status) => LxmdCommand::Status,
        None => LxmdCommand::Serve,
    };

    let mut runtime = match LxmdRuntime::new(config) {
        Ok(runtime) => runtime,
        Err(err) => {
            eprintln!("lxmd runtime init error: {}", err);
            std::process::exit(1);
        }
    };

    if args.verbose && !args.quiet {
        eprintln!("lxmd runtime command={command:?}");
    }

    if args.service {
        let max_ticks = std::env::var("LXMD_SERVICE_MAX_TICKS")
            .ok()
            .and_then(|value| value.parse::<u64>().ok());
        let status = match runtime.run_service(max_ticks) {
            Ok(status) => status,
            Err(err) => {
                eprintln!("lxmd service error: {}", err);
                std::process::exit(1);
            }
        };
        if !args.quiet {
            println!(
                "lxmd service done peer_count={} jobs_run={} announces_sent={}",
                status.peer_count, status.jobs_run, status.announces_sent
            );
        }
        return;
    }

    let output = match execute_with_runtime(
        &mut runtime,
        command,
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
    ) {
        Ok(output) => output,
        Err(err) => {
            eprintln!("lxmd runtime error: {}", err);
            std::process::exit(1);
        }
    };

    if !args.quiet {
        println!("{output}");
    }
}
