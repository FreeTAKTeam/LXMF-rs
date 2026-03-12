#![allow(clippy::items_after_test_module)]

mod announce_worker;
mod bootstrap;
mod bridge;
mod bridge_helpers;
mod inbound_worker;
mod interface_hot_apply;
mod interfaces;
mod receipt_worker;
mod rpc_loop;
#[cfg(test)]
mod tests;

use clap::Parser;
use std::path::PathBuf;

#[derive(Parser, Debug)]
#[command(name = "reticulumd")]
struct Args {
    #[arg(long, default_value = "127.0.0.1:4243")]
    rpc: String,
    #[arg(long)]
    grpc: Option<String>,
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
    grpc_tls_cert: Option<PathBuf>,
    #[arg(long)]
    grpc_tls_key: Option<PathBuf>,
    #[arg(long)]
    grpc_tls_client_ca: Option<PathBuf>,
}

#[tokio::main(flavor = "multi_thread")]
async fn main() {
    let args = Args::parse();
    let context = bootstrap::bootstrap(args).await;
    if let Some(grpc_addr) = context.grpc_addr {
        let rpc_daemon = context.daemon.clone();
        let rpc_tls = context.rpc_tls.clone();
        let grpc_daemon = context.daemon.clone();
        let grpc_tls = context.grpc_tls.clone().map(|config| rns_rpc::grpc::GrpcTlsConfig {
            cert_chain_path: config.cert_chain_path,
            private_key_path: config.private_key_path,
            client_ca_path: config.client_ca_path,
        });
        let rpc_task = tokio::spawn(async move {
            rpc_loop::run_rpc_loop(context.rpc_addr, rpc_daemon, rpc_tls).await
        });
        let grpc_task = tokio::spawn(async move {
            rns_rpc::grpc::serve(grpc_addr, grpc_daemon, grpc_tls)
                .await
                .expect("run gRPC listener");
        });
        tokio::select! {
            rpc_result = rpc_task => {
                rpc_result.expect("rpc loop task panicked");
                panic!("rpc loop exited unexpectedly");
            }
            grpc_result = grpc_task => {
                grpc_result.expect("grpc loop task panicked");
                panic!("grpc loop exited unexpectedly");
            }
        }
    } else {
        rpc_loop::run_rpc_loop(context.rpc_addr, context.daemon, context.rpc_tls).await;
    }
}
