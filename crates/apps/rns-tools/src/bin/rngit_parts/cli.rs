#[derive(Debug, Parser)]
#[command(name = "rngit", about = "Run local Git workflows or a Reticulum Git network service/client")]
struct Cli {
    #[arg(long)]
    root: PathBuf,
    #[arg(long, value_name = "DIRECTORY", help = "Read rngit configuration from this directory (config file)")]
    config: Option<PathBuf>,
    #[arg(long, value_name = "HOST:PORT", action = clap::ArgAction::Append)]
    listen: Vec<String>,
    #[arg(long, value_name = "HOST:PORT", action = clap::ArgAction::Append)]
    connect: Vec<String>,
    #[arg(long, value_name = "SEED")]
    identity_seed: Option<String>,
    #[arg(long, value_name = "PATH")]
    identity: Option<PathBuf>,
    #[arg(long, value_name = "HASH", action = clap::ArgAction::Append)]
    blocked_identity_hash: Vec<String>,
    #[arg(long)]
    print_identity: bool,
    #[arg(long, help = "Suppress routine status output; cleanup failures remain visible")]
    silent: bool,
    #[arg(long, help = "Skip optional WebP conversion and serve the original media bytes")]
    no_media_conversion: bool,
    #[arg(
        long,
        default_value_t = 85,
        value_parser = clap::value_parser!(u8).range(1..=100),
        help = "WebP quality from 1 to 100 (requires an available converter; default: 85)"
    )]
    media_quality: u8,
    #[arg(long, help = "Maximum WebP width or height in pixels (requires an available converter)")]
    media_max_dimension: Option<u32>,
    #[command(subcommand)]
    command: Option<GitCommand>,
}

#[derive(Debug, Subcommand)]
enum GitCommand {
    Init {
        path: PathBuf,
    },
    Status {
        path: PathBuf,
    },
    Bundle {
        path: PathBuf,
        output: PathBuf,
        #[arg(default_value = "--all")]
        revision: String,
    },
    Fetch {
        remote: String,
        reference: String,
        destination_ref: String,
    },
    Push {
        remote: String,
        local_ref: String,
        remote_ref: String,
        #[arg(long)]
        force: bool,
    },
    Unbundle {
        path: PathBuf,
        bundle: PathBuf,
    },
}
