use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;
use reticulum_daemon::paper_interchange::decode_storage_file;

#[derive(Debug, Parser)]
struct Args {
    #[arg(long)]
    file: PathBuf,
}

fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();
    let args = Args::parse();
    match decode_storage_file(&args.file) {
        Ok(summary) => match serde_json::to_string(&summary) {
            Ok(json) => {
                println!("{json}");
                ExitCode::SUCCESS
            }
            Err(error) => {
                log::error!("failed to encode interchange summary: {error}");
                ExitCode::from(1)
            }
        },
        Err(error) => {
            log::error!("failed to decode interchange file {}: {error}", args.file.display());
            ExitCode::from(1)
        }
    }
}
