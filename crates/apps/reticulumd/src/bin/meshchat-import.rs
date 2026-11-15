use clap::Parser;
use std::{path::PathBuf, process::ExitCode};

#[derive(Parser)]
#[command(about = "Inspect or import MeshChat history while both clients are closed")]
struct Args {
    #[arg(long)]
    source_identity: PathBuf,
    #[arg(long, requires = "db")]
    identity: Option<PathBuf>,
    #[arg(long, requires = "identity")]
    db: Option<PathBuf>,
}

fn main() -> ExitCode {
    let args = Args::parse();
    let result = match (&args.identity, &args.db) {
        (Some(identity), Some(db)) => {
            reticulum_daemon::meshchat_import::import(&args.source_identity, identity, db)
        }
        _ => reticulum_daemon::meshchat_import::inspect(&args.source_identity),
    };
    match result {
        Ok(summary) => {
            println!("{summary}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}
