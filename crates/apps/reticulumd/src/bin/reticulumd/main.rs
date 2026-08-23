#![allow(clippy::items_after_test_module)]

mod announce_ingest;
mod announce_persistence;
mod announce_worker;
mod bootstrap;
mod bridge;
mod bridge_helpers;
mod bridge_path_lookup;
mod bridge_rnode_management;
mod bridge_transport_status;
mod bridge_weave_control;
mod direct_backchannel;
mod discovery_publish;
mod inbound_worker;
mod interface_hot_apply;
mod interfaces;
mod outbound_resources;
mod receipt_events;
mod receipt_worker;
mod rpc_loop;
#[cfg(test)]
mod tests;
#[cfg(feature = "zmq-pipeline-rpc")]
mod zmq_rpc_loop;

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
    #[cfg(feature = "zmq-pipeline-rpc")]
    #[arg(long, value_name = "ENDPOINT")]
    zmq_rpc_endpoint: Option<String>,
    /// Legacy dual-endpoint PUSH/PULL command ingress.
    #[cfg(feature = "zmq-pipeline-rpc")]
    #[arg(long)]
    zmq_rpc_command: Option<String>,
}

#[tokio::main(flavor = "multi_thread")]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();
    let args = Args::parse();
    #[cfg(feature = "zmq-pipeline-rpc")]
    let zmq_rpc_command = args.zmq_rpc_command.clone();
    #[cfg(feature = "zmq-pipeline-rpc")]
    let zmq_rpc_endpoint = args.zmq_rpc_endpoint.clone();
    let context = bootstrap::bootstrap(args).await;
    #[cfg(feature = "zmq-pipeline-rpc")]
    {
        run_daemon_loops(context, zmq_rpc_endpoint, zmq_rpc_command).await;
    }
    #[cfg(not(feature = "zmq-pipeline-rpc"))]
    {
        let path_table_persistence = context.path_table_persistence;
        rpc_loop::run_rpc_loop(context.rpc_addr, context.daemon, context.rpc_tls, context.rpc_unix)
            .await;
        announce_persistence::flush_reticulum_path_table_if_configured(path_table_persistence)
            .await;
    }
}

#[cfg(feature = "zmq-pipeline-rpc")]
async fn run_daemon_loops(
    context: bootstrap::BootstrapContext,
    zmq_rpc_endpoint: Option<String>,
    zmq_rpc_command: Option<String>,
) {
    let bootstrap::BootstrapContext { rpc_addr, rpc_unix, daemon, rpc_tls, path_table_persistence } =
        context;
    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
    tokio::spawn(async move {
        match tokio::signal::ctrl_c().await {
            Ok(()) => {
                log::info!("[daemon] shutdown signal received");
                shutdown_tx.send_replace(true);
            }
            Err(err) => {
                log::error!("[daemon] failed to install shutdown signal handler: {}", err);
            }
        }
    });

    if let Some(zmq_rpc_endpoint) = zmq_rpc_endpoint {
        let daemon = daemon.clone();
        let shutdown = shutdown_rx.clone();
        tokio::spawn(async move {
            if let Err(err) =
                zmq_rpc_loop::run_zmq_router_loop_until(zmq_rpc_endpoint, true, daemon, shutdown)
                    .await
            {
                log::error!("[daemon] canonical zmq rpc loop stopped: {}", err);
            }
        });
    }

    if let Some(command_endpoint) = zmq_rpc_command {
        let daemon = daemon.clone();
        let shutdown = shutdown_rx.clone();
        tokio::spawn(async move {
            let config =
                zmq_rpc_loop::ZmqRpcLoopConfig { command_endpoint, require_auth_for_remote: true };
            if let Err(err) = zmq_rpc_loop::run_zmq_rpc_loop_until(config, daemon, shutdown).await {
                log::error!("[daemon] zmq rpc loop stopped: {}", err);
            }
        });
    }

    rpc_loop::run_rpc_loop_until(rpc_addr, daemon, rpc_tls, rpc_unix, shutdown_rx).await;
    announce_persistence::flush_reticulum_path_table_if_configured(path_table_persistence).await;
}
