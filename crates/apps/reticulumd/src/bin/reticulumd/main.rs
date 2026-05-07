#![allow(clippy::items_after_test_module)]

mod announce_ingest;
mod announce_persistence;
mod announce_worker;
mod bootstrap;
mod bridge;
mod bridge_helpers;
mod inbound_worker;
mod interface_hot_apply;
mod interfaces;
mod outbound_resources;
mod receipt_events;
mod receipt_worker;
mod rpc_loop;
#[cfg(test)]
mod tests;

use clap::Parser;
use std::path::PathBuf;

const DEFAULT_RPC_UNIX_PATH: &str = "/tmp/lxmf-rpc.sock";

#[derive(Parser, Debug)]
#[command(name = "reticulumd")]
struct Args {
    /// Optional TCP RPC bind address. TCP is opt-in; local Unix RPC is enabled by default.
    #[arg(long)]
    rpc: Option<String>,
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
    #[arg(long, default_value_t = false)]
    strict_interface_startup: bool,
    #[arg(long)]
    rpc_tls_cert: Option<PathBuf>,
    #[arg(long)]
    rpc_tls_key: Option<PathBuf>,
    #[arg(long)]
    rpc_tls_client_ca: Option<PathBuf>,
    #[arg(long)]
    rpc_token_issuer: Option<String>,
    #[arg(long)]
    rpc_token_audience: Option<String>,
    /// Environment variable containing the remote RPC token shared secret.
    #[arg(long)]
    rpc_token_secret_env: Option<String>,
    #[arg(long, default_value_t = 60_000)]
    rpc_token_jti_ttl_ms: u64,
    #[arg(long, default_value_t = 5_000)]
    rpc_token_clock_skew_ms: u64,
    #[arg(long, default_value = DEFAULT_RPC_UNIX_PATH)]
    rpc_unix: Option<PathBuf>,
}

#[tokio::main(flavor = "multi_thread")]
async fn main() {
    let args = Args::parse();
    let context = bootstrap::bootstrap(args).await;
    rpc_loop::run_rpc_loop(context.rpc_addr, context.daemon, context.rpc_tls, context.rpc_unix)
        .await;
}
