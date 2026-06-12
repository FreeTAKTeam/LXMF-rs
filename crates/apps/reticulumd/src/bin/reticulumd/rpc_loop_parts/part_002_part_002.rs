use rns_rpc::rpc::codec;

use rns_rpc::{http, RpcDaemon, RpcError, RpcRequest, RpcResponse};

use rpc_access_log::{emit_rpc_access_log, parse_request_log_meta};

use rustls::server::WebPkiClientVerifier;

use rustls::{RootCertStore, ServerConfig};

use rustls_pemfile::private_key;

use serde_json::json;

use std::fs::File;

use std::io::{self, BufReader};

use std::net::{IpAddr, SocketAddr};

use std::path::{Path, PathBuf};

use std::sync::Arc;

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

#[cfg(unix)]
use tokio::net::UnixListener;

use tokio::net::{TcpListener, TcpStream};

use tokio::sync::watch;

use tokio::time::{timeout, Duration};

use tokio_rustls::server::TlsStream;

use tokio_rustls::TlsAcceptor;

use x509_parser::extensions::ParsedExtension;

use x509_parser::prelude::{FromDer, GeneralName, X509Certificate};

const RPC_READ_TIMEOUT: Duration = Duration::from_secs(5);

const RPC_MAX_HEADER_BYTES: usize = 16 * 1024;

const RPC_MAX_BODY_BYTES: usize = 1024 * 1024;

type ShutdownReceiver = watch::Receiver<bool>;

fn rpc_ready_line(scheme: &str, addr: impl std::fmt::Display) -> String {
    format!("reticulumd listening on {scheme}://{addr}")
}

#[cfg_attr(feature = "zmq-pipeline-rpc", allow(dead_code))]
pub(super) async fn run_rpc_loop(
    addr: Option<SocketAddr>,
    daemon: Arc<RpcDaemon>,
    tls: Option<RpcTlsConfig>,
    unix_socket: Option<PathBuf>,
) {
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    tokio::spawn(async move {
        match tokio::signal::ctrl_c().await {
            Ok(()) => {
                log::info!("[daemon] shutdown signal received");
                let _ = shutdown_tx.send(true);
            }
            Err(err) => {
                log::error!("[daemon] failed to install shutdown signal handler: {}", err);
            }
        }
    });
    run_rpc_loop_until(addr, daemon, tls, unix_socket, shutdown_rx).await;
}

pub(super) async fn run_rpc_loop_until(
    addr: Option<SocketAddr>,
    daemon: Arc<RpcDaemon>,
    tls: Option<RpcTlsConfig>,
    unix_socket: Option<PathBuf>,
    shutdown: ShutdownReceiver,
) {
    match (addr, tls, unix_socket) {
        (Some(addr), tls, unix_socket) => {
            let unix_handle = if let Some(path) = unix_socket {
                let daemon_for_unix = daemon.clone();
                let shutdown_for_unix = shutdown.clone();
                Some(tokio::spawn(async move {
                    run_unix_rpc_loop(path, daemon_for_unix, shutdown_for_unix).await;
                }))
            } else {
                None
            };
            match tls {
                Some(config) => run_tls_rpc_loop(addr, daemon, config, shutdown).await,
                None => run_plain_rpc_loop(addr, daemon, shutdown).await,
            }
            if let Some(handle) = unix_handle {
                let _ = handle.await;
            }
        }
        (None, None, Some(path)) => run_unix_rpc_loop(path, daemon, shutdown).await,
        (None, Some(_), Some(_)) => {
            panic!("--rpc is required when TLS RPC options are configured");
        }
        (None, _, None) => {
            panic!("no RPC listener configured; use --rpc-unix or --rpc");
        }
    }
}

async fn run_plain_rpc_loop(
    addr: SocketAddr,
    daemon: Arc<RpcDaemon>,
    mut shutdown: ShutdownReceiver,
) {
    let listener = TcpListener::bind(addr).await.expect("bind rpc listener");
    println!("{}", rpc_ready_line("http", addr));

    loop {
        tokio::select! {
            shutdown_result = shutdown.changed() => {
                if shutdown_result.is_err() || *shutdown.borrow() {
                    log::info!("[daemon] rpc tcp listener shutting down");
                    break;
                }
            }
            accepted = listener.accept() => {
                let (stream, peer_addr) = accepted.expect("accept rpc socket");
                let daemon = daemon.clone();
                tokio::spawn(async move {
                    handle_connection(stream, peer_addr, daemon.as_ref(), None).await;
                });
            }
        }
    }
}

#[cfg(unix)]
async fn run_unix_rpc_loop(path: PathBuf, daemon: Arc<RpcDaemon>, mut shutdown: ShutdownReceiver) {
    prepare_rpc_unix_socket_path(&path).expect("prepare rpc unix socket path");
    let listener = UnixListener::bind(&path).expect("bind rpc unix socket");
    log::info!("reticulumd listening on unix:{}", path.display());
    let peer_addr = SocketAddr::from(([127, 0, 0, 1], 0));

    loop {
        tokio::select! {
            shutdown_result = shutdown.changed() => {
                if shutdown_result.is_err() || *shutdown.borrow() {
                    log::info!("[daemon] rpc unix listener shutting down");
                    break;
                }
            }
            accepted = listener.accept() => {
                let (stream, _) = accepted.expect("accept rpc unix socket");
                let daemon = daemon.clone();
                tokio::spawn(async move {
                    handle_connection(stream, peer_addr, daemon.as_ref(), None).await;
                });
            }
        }
    }
    cleanup_rpc_unix_socket_path(&path).expect("cleanup rpc unix socket path");
}

#[cfg(unix)]
fn prepare_rpc_unix_socket_path(path: &Path) -> io::Result<()> {
    if let Ok(metadata) = std::fs::metadata(path) {
        use std::os::unix::fs::FileTypeExt;
        if metadata.file_type().is_socket() {
            std::fs::remove_file(path)?;
        } else {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!("refusing to remove non-socket rpc unix path {}", path.display()),
            ));
        }
    }
    Ok(())
}

#[cfg(unix)]
fn cleanup_rpc_unix_socket_path(path: &Path) -> io::Result<()> {
    if let Ok(metadata) = std::fs::metadata(path) {
        use std::os::unix::fs::FileTypeExt;
        if metadata.file_type().is_socket() {
            std::fs::remove_file(path)?;
        }
    }
    Ok(())
}

#[cfg(not(unix))]
async fn run_unix_rpc_loop(path: PathBuf, _daemon: Arc<RpcDaemon>, _shutdown: ShutdownReceiver) {
    log::warn!(
        "[daemon] ignoring --rpc-unix {} because Unix sockets are not supported on this platform",
        path.display()
    );
}

async fn run_tls_rpc_loop(
    addr: SocketAddr,
    daemon: Arc<RpcDaemon>,
    config: RpcTlsConfig,
    mut shutdown: ShutdownReceiver,
) {
    let tls_server = build_tls_server_config(&config).expect("build rpc tls server config");
    let acceptor = TlsAcceptor::from(tls_server);
    let listener = TcpListener::bind(addr).await.expect("bind tls rpc listener");
    println!("{}", rpc_ready_line("https", addr));

    loop {
        tokio::select! {
            shutdown_result = shutdown.changed() => {
                if shutdown_result.is_err() || *shutdown.borrow() {
                    log::info!("[daemon] rpc tls listener shutting down");
                    break;
                }
            }
            accepted = listener.accept() => {
                let (stream, peer_addr) = accepted.expect("accept tls rpc socket");
                let daemon = daemon.clone();
                let acceptor = acceptor.clone();
                tokio::spawn(async move {
                    match acceptor.accept(stream).await {
                        Ok(tls_stream) => {
                            let transport_auth = extract_transport_auth(&tls_stream);
                            handle_connection(
                                tls_stream,
                                peer_addr,
                                daemon.as_ref(),
                                Some(transport_auth),
                            )
                            .await;
                        }
                        Err(err) => {
                            log::error!(
                                "[daemon] rpc tls handshake failed peer={} err={}",
                                peer_addr, err
                            );
                        }
                    }
                });
            }
        }
    }
}

#[tracing::instrument(name = "rpc_conn", skip(stream, daemon, transport_auth))]
async fn handle_connection<S>(
    mut stream: S,
    peer_addr: SocketAddr,
    daemon: &RpcDaemon,
    transport_auth: Option<http::TransportAuthContext>,
) where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let buffer = match read_http_request(&mut stream).await {
        Ok(buffer) => buffer,
        Err(err) => {
            log::error!("[daemon] rpc read error peer={} err={}", peer_addr, err);
            let _ = stream.write_all(request_read_error_response(&err)).await;
            let _ = stream.shutdown().await;
            return;
        }
    };

    if buffer.is_empty() {
        let _ = stream.shutdown().await;
        return;
    }

    if let Ok((method, path, headers)) = http::request_method_path_headers(&buffer) {
        if method == "GET" && path.split('?').next() == Some("/events/stream") {
            handle_event_stream(stream, peer_addr, daemon, path, headers, transport_auth).await;
            return;
        }
    }

    let request_meta = parse_request_log_meta(&buffer);
    let started_at = std::time::Instant::now();
    let response_result = http::handle_http_request_with_transport_auth(
        daemon,
        &buffer,
        Some(peer_addr),
        transport_auth,
    );
    let elapsed_ms = started_at.elapsed().as_millis() as u64;
    let (response, error_text) = match response_result {
        Ok(response) => (response, None),
        Err(err) => {
            let err_text = err.to_string();
            (http::build_error_response(&format!("rpc error: {err_text}")), Some(err_text))
        }
    };
    emit_rpc_access_log(peer_addr, &request_meta, &response, elapsed_ms, error_text.as_deref());
    let _ = stream.write_all(&response).await;
    let _ = stream.shutdown().await;
}

async fn handle_event_stream<S>(
    mut stream: S,
    peer_addr: SocketAddr,
    daemon: &RpcDaemon,
    path: String,
    headers: Vec<(String, String)>,
    transport_auth: Option<http::TransportAuthContext>,
) where
    S: AsyncWrite + Unpin,
{
    let peer_ip = peer_addr.ip().to_string();
    if let Err(error) = daemon.authorize_http_request_with_transport(
        &headers,
        Some(peer_ip.as_str()),
        transport_auth.as_ref(),
    ) {
        let response = http::build_rpc_error_response(0, error)
            .unwrap_or_else(|err| http::build_error_response(&format!("rpc auth error: {err}")));
        let _ = stream.write_all(&response).await;
        let _ = stream.shutdown().await;
        return;
    }

    let mut live_events = daemon.subscribe_sdk_events();
    let mut cursor = event_stream_query_cursor(path.as_str());
    let first_batch = match poll_sdk_event_stream_batch(daemon, cursor.as_deref(), 256) {
        Ok(batch) => batch,
        Err(err) => {
            let response = http::build_rpc_error_response(0, *err).unwrap_or_else(|encode_err| {
                http::build_error_response(&format!("event stream error: {encode_err}"))
            });
            let _ = stream.write_all(&response).await;
            let _ = stream.shutdown().await;
            return;
        }
    };

    if stream.write_all(&http::streaming_event_response_header()).await.is_err() {
        let _ = stream.shutdown().await;
        return;
    }

    let mut last_sent_seq = 0_u64;
    if !write_sdk_event_batch(&mut stream, &first_batch, &mut cursor, &mut last_sent_seq).await {
        let _ = stream.shutdown().await;
        return;
    }

    loop {
        let batch = match poll_sdk_event_stream_batch(daemon, cursor.as_deref(), 256) {
            Ok(batch) => batch,
            Err(err) => {
                log::error!(
                    "[daemon] event stream catch-up error peer={} code={} message={}",
                    peer_addr,
                    err.code,
                    err.message
                );
                let response = RpcResponse { id: 0, result: None, error: Some(*err) };
                if let Ok(frame) = codec::encode_frame(&response) {
                    let _ = stream.write_all(&frame).await;
                }
                break;
            }
        };
        let event_count =
            batch.get("events").and_then(serde_json::Value::as_array).map_or(0, Vec::len);
        if !write_sdk_event_batch(&mut stream, &batch, &mut cursor, &mut last_sent_seq).await {
            let _ = stream.shutdown().await;
            return;
        }
        if event_count == 0 {
            break;
        }
    }

    loop {
        let event = match live_events.recv().await {
            Ok(event) if event.seq_no <= last_sent_seq => continue,
            Ok(event) => daemon.sdk_stream_event_frame(&event),
            Err(tokio::sync::broadcast::error::RecvError::Lagged(dropped_count)) => {
                let expected_seq_no = last_sent_seq.saturating_add(1);
                let observed_seq_no = expected_seq_no.saturating_add(dropped_count);
                daemon.sdk_stream_gap_frame(expected_seq_no, observed_seq_no, dropped_count)
            }
            Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
        };
        if let Some(seq_no) = event.get("seq_no").and_then(serde_json::Value::as_u64) {
            last_sent_seq = last_sent_seq.max(seq_no);
        }
        let frame = match codec::encode_frame(&event) {
            Ok(frame) => frame,
            Err(err) => {
                log::error!("[daemon] event stream encode error peer={} err={}", peer_addr, err);
                break;
            }
        };
        if stream.write_all(&frame).await.is_err() {
            break;
        }
    }
    let _ = stream.shutdown().await;
}

fn event_stream_query_cursor(path: &str) -> Option<String> {
    let query = path.split_once('?')?.1;
    query.split('&').find_map(|part| {
        let (name, value) = part.split_once('=')?;
        (name == "cursor" && !value.is_empty()).then(|| value.to_string())
    })
}

fn poll_sdk_event_stream_batch(
    daemon: &RpcDaemon,
    cursor: Option<&str>,
    max: usize,
) -> Result<serde_json::Value, Box<RpcError>> {
    let response = daemon
        .handle_rpc(RpcRequest {
            id: 0,
            method: "sdk_poll_events_v2".to_string(),
            params: Some(json!({ "cursor": cursor, "max": max })),
        })
        .map_err(|err| Box::new(RpcError::new("SDK_INTERNAL", err.to_string())))?;
    if let Some(error) = response.error {
        return Err(Box::new(error));
    }
    Ok(response.result.unwrap_or(serde_json::Value::Null))
}

async fn write_sdk_event_batch<S>(
    stream: &mut S,
    batch: &serde_json::Value,
    cursor: &mut Option<String>,
    last_sent_seq: &mut u64,
) -> bool
where
    S: AsyncWrite + Unpin,
{
    if let Some(next_cursor) = batch.get("next_cursor").and_then(serde_json::Value::as_str) {
        *cursor = Some(next_cursor.to_string());
    }
    let Some(events) = batch.get("events").and_then(serde_json::Value::as_array) else {
        return true;
    };
    for event in events {
        if let Some(seq_no) = event.get("seq_no").and_then(serde_json::Value::as_u64) {
            *last_sent_seq = (*last_sent_seq).max(seq_no);
        }
        let frame = match codec::encode_frame(event) {
            Ok(frame) => frame,
            Err(_) => return false,
        };
        if stream.write_all(&frame).await.is_err() {
            return false;
        }
    }
    true
}
