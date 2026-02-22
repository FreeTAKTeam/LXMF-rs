#![allow(clippy::items_after_test_module)]

mod announce_worker;
mod bootstrap;
mod bridge;
mod bridge_helpers;
mod inbound_worker;
mod interfaces;
mod receipt_worker;
mod rpc_loop;
#[cfg(test)]
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
    #[arg(long)]
    rpc_tls_cert: Option<PathBuf>,
    #[arg(long)]
    rpc_tls_key: Option<PathBuf>,
    #[arg(long)]
    rpc_tls_client_ca: Option<PathBuf>,
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let local = LocalSet::new();
    local
        .run_until(async {
            let args = Args::parse();
            let context = bootstrap::bootstrap(args).await;
            rpc_loop::run_rpc_loop(context.rpc_addr, context.daemon, context.rpc_tls).await;
        })
        .await;
}
