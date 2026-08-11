mod independent_interop_node;

use clap::Parser;
use independent_interop_node::{run, Cli};
use std::process::ExitCode;

#[tokio::main]
async fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_writer(std::io::stderr)
        .init();
    match run(Cli::parse()).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("independent-interop-node: {error}");
            ExitCode::FAILURE
        }
    }
}
