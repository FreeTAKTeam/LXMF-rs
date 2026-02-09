use clap::{Parser, Subcommand};
use lxmf::lxmd::config::LxmdConfig;
use lxmf::lxmd::runtime::{execute, LxmdCommand};

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

    if args.verbose && !args.quiet {
        eprintln!("lxmd runtime command={command:?}");
    }

    let output = match execute(command, &config) {
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
