#![allow(clippy::items_after_test_module)]

mod announce_ingest;
mod announce_persistence;
mod announce_worker;
mod bootstrap;
mod bridge;
mod bridge_helpers;
mod control_router_mode;
mod inbound_worker;
mod interface_hot_apply;
mod interface_worker_mode;
mod interfaces;
mod outbound_resources;
mod receipt_events;
mod receipt_worker;
mod rpc_loop;
#[cfg(test)]
mod tests;
mod worker_mode;

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
    /// Disable the default local Unix RPC listener.
    #[arg(long, hide = true, default_value_t = false)]
    no_rpc_unix: bool,
    /// Run the framed worker process protocol on stdin/stdout.
    #[arg(long, hide = true, default_value_t = false)]
    worker_stdio: bool,
    /// Run the framed interface worker process protocol on stdin.
    #[arg(long, hide = true, default_value_t = false)]
    interface_worker_stdio: bool,
    /// Run the framed router/control process protocol on stdin/stdout.
    #[arg(long, hide = true, default_value_t = false)]
    control_router_stdio: bool,
    /// UDP bind address for hidden framed interface-worker child mode.
    #[arg(long, hide = true)]
    interface_worker_udp_bind: Option<String>,
    /// UDP forward address for hidden framed interface-worker child mode.
    #[arg(long, hide = true)]
    interface_worker_udp_forward: Option<String>,
    /// TCP connect address for hidden framed interface-worker child mode.
    #[arg(long, hide = true)]
    interface_worker_tcp_connect: Option<String>,
    /// TCP listen address for hidden framed interface-worker child mode.
    #[arg(long, hide = true)]
    interface_worker_tcp_listen: Option<String>,
    /// Interface address hex for hidden framed interface-worker child mode.
    #[arg(long, hide = true)]
    interface_worker_address: Option<String>,
    /// Serial device for hidden framed interface-worker child mode.
    #[arg(long, hide = true)]
    interface_worker_serial_device: Option<String>,
    /// Serial baud rate for hidden framed interface-worker child mode.
    #[arg(long, hide = true)]
    interface_worker_serial_baud_rate: Option<u32>,
    /// Serial data bits for hidden framed interface-worker child mode.
    #[arg(long, hide = true)]
    interface_worker_serial_data_bits: Option<u8>,
    /// Serial stop bits for hidden framed interface-worker child mode.
    #[arg(long, hide = true)]
    interface_worker_serial_stop_bits: Option<u8>,
    /// Serial parity for hidden framed interface-worker child mode.
    #[arg(long, hide = true)]
    interface_worker_serial_parity: Option<String>,
    /// Serial flow control for hidden framed interface-worker child mode.
    #[arg(long, hide = true)]
    interface_worker_serial_flow_control: Option<String>,
    /// Serial MTU for hidden framed interface-worker child mode.
    #[arg(long, hide = true)]
    interface_worker_serial_mtu: Option<usize>,
    /// Serial reconnect backoff in ms for hidden framed interface-worker child mode.
    #[arg(long, hide = true)]
    interface_worker_serial_reconnect_backoff_ms: Option<u64>,
    /// Serial max reconnect backoff in ms for hidden framed interface-worker child mode.
    #[arg(long, hide = true)]
    interface_worker_serial_max_reconnect_backoff_ms: Option<u64>,
    /// BLE adapter for hidden framed interface-worker child mode.
    #[arg(long, hide = true)]
    interface_worker_ble_adapter: Option<String>,
    /// BLE peripheral id for hidden framed interface-worker child mode.
    #[arg(long, hide = true)]
    interface_worker_ble_peripheral_id: Option<String>,
    /// BLE service UUID for hidden framed interface-worker child mode.
    #[arg(long, hide = true)]
    interface_worker_ble_service_uuid: Option<String>,
    /// BLE write characteristic UUID for hidden framed interface-worker child mode.
    #[arg(long, hide = true)]
    interface_worker_ble_write_char_uuid: Option<String>,
    /// BLE notify characteristic UUID for hidden framed interface-worker child mode.
    #[arg(long, hide = true)]
    interface_worker_ble_notify_char_uuid: Option<String>,
    /// BLE MTU for hidden framed interface-worker child mode.
    #[arg(long, hide = true)]
    interface_worker_ble_mtu: Option<usize>,
    /// BLE scan timeout in ms for hidden framed interface-worker child mode.
    #[arg(long, hide = true)]
    interface_worker_ble_scan_timeout_ms: Option<u64>,
    /// BLE connect timeout in ms for hidden framed interface-worker child mode.
    #[arg(long, hide = true)]
    interface_worker_ble_connect_timeout_ms: Option<u64>,
    /// BLE reconnect backoff in ms for hidden framed interface-worker child mode.
    #[arg(long, hide = true)]
    interface_worker_ble_reconnect_backoff_ms: Option<u64>,
    /// BLE max reconnect backoff in ms for hidden framed interface-worker child mode.
    #[arg(long, hide = true)]
    interface_worker_ble_max_reconnect_backoff_ms: Option<u64>,
    /// Number of hidden framed worker child processes to start for process-backed hot-path work.
    #[arg(long, hide = true, default_value_t = 0)]
    worker_process_count: usize,
    /// Per-job timeout for hidden framed worker child-process requests.
    #[arg(long, hide = true, default_value_t = 1_000)]
    worker_process_timeout_ms: u64,
    /// Worker executable to spawn for hidden framed child-process requests.
    #[arg(long, hide = true)]
    worker_process_command: Option<PathBuf>,
    /// Unix socket for externally managed framed worker requests.
    #[cfg(unix)]
    #[arg(long, hide = true)]
    worker_process_unix_socket: Option<PathBuf>,
    /// TCP address for externally managed framed worker requests.
    #[arg(long, hide = true)]
    worker_process_tcp: Option<std::net::SocketAddr>,
    /// Number of hidden framed interface-worker child processes to register.
    #[arg(long, hide = true, default_value_t = 0)]
    interface_worker_process_count: usize,
    /// Interface worker executable to spawn for hidden framed interface child processes.
    #[arg(long, hide = true)]
    interface_worker_process_command: Option<PathBuf>,
    /// Shutdown wait for hidden framed interface-worker child processes.
    #[arg(long, hide = true, default_value_t = 1_000)]
    interface_worker_process_shutdown_ms: u64,
    /// Restart backoff for supervised hidden framed interface-worker child processes.
    #[arg(
        long,
        hide = true,
        default_value_t = interface_worker_mode::DEFAULT_INTERFACE_WORKER_RESTART_BACKOFF_MS
    )]
    interface_worker_process_restart_backoff_ms: u64,
    /// Number of hidden framed router/control child processes to start.
    #[arg(long, hide = true, default_value_t = 0)]
    control_router_process_count: usize,
    /// Per-request timeout for hidden framed router/control child-process requests.
    #[arg(long, hide = true, default_value_t = 1_000)]
    control_router_process_timeout_ms: u64,
    /// Router/control executable to spawn for hidden framed child-process requests.
    #[arg(long, hide = true)]
    control_router_process_command: Option<PathBuf>,
}

#[tokio::main(flavor = "multi_thread")]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();
    let args = Args::parse();
    if args.worker_stdio {
        worker_mode::run_worker_stdio().await;
        return;
    }
    if args.interface_worker_stdio {
        interface_worker_mode::run_interface_worker_stdio(&args).await;
        return;
    }
    if args.control_router_stdio {
        control_router_mode::run_control_router_stdio(args).await;
        return;
    }
    worker_mode::validate_worker_process_options(
        args.worker_process_count,
        args.worker_process_timeout_ms,
    )
    .expect("invalid worker process options");
    control_router_mode::validate_control_router_process_options(
        args.control_router_process_count,
        args.control_router_process_timeout_ms,
    )
    .expect("invalid control router process options");
    let context = bootstrap::bootstrap(args).await;
    rpc_loop::run_rpc_loop(
        context.rpc_addr,
        context.daemon,
        context.rpc_tls,
        context.rpc_unix,
        context.control_router_process_pool,
        context.control_router_process_runtime.timeout_ms,
    )
    .await;
}
