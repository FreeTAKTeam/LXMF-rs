#![allow(clippy::items_after_test_module)]

#[path = "reticulumd/bootstrap.rs"]
mod bootstrap;
#[path = "reticulumd/bridge.rs"]
mod bridge;
#[path = "reticulumd/bridge_helpers.rs"]
mod bridge_helpers;
#[path = "reticulumd/rpc_loop.rs"]
mod rpc_loop;
#[cfg(test)]
#[path = "reticulumd/tests.rs"]
mod tests;

use clap::Parser;
use std::path::PathBuf;
use tokio::task::LocalSet;

#[derive(Parser, Debug)]
#[command(name = "reticulumd")]
struct Args {
    #[arg(long, default_value = "127.0.0.1:4243")]
    rpc: String,
    #[arg(long, default_value = "reticulum.db")]
    db: PathBuf,
    #[arg(long)]
    config: Option<PathBuf>,
    #[arg(long)]
    identity: Option<PathBuf>,
    #[arg(long, default_value_t = 0)]
    announce_interval_secs: u64,
    #[arg(long)]
    transport: Option<String>,
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let local = LocalSet::new();
    local
        .run_until(async {
            let args = Args::parse();
            let context = bootstrap::bootstrap(args).await;
            rpc_loop::run_rpc_loop(context.rpc_addr, context.daemon).await;
        })
        .await;
}
