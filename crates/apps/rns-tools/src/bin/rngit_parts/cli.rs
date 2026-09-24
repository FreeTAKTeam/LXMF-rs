#[derive(Debug, Parser)]
#[command(name = "rngit", about = "Run local Git workflows or a Reticulum Git network service/client")]
struct Cli {
    #[arg(long)]
    root: PathBuf,
    #[arg(long, value_name = "HOST:PORT", action = clap::ArgAction::Append)]
    listen: Vec<String>,
    #[arg(long, value_name = "HOST:PORT", action = clap::ArgAction::Append)]
    connect: Vec<String>,
    #[arg(long, value_name = "SEED")]
    identity_seed: Option<String>,
    #[arg(long, value_name = "PATH")]
    identity: Option<PathBuf>,
    #[arg(long)]
    print_identity: bool,
    #[arg(long)]
    silent: bool,
    #[arg(long)]
    no_media_conversion: bool,
    #[arg(long, default_value_t = 85, value_parser = clap::value_parser!(u8).range(1..=100))]
    media_quality: u8,
    #[arg(long)]
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
