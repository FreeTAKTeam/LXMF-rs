use super::bootstrap::RpcTlsConfig;
use super::control_router_mode::{ControlRouterProcessError, ControlRouterStdioPool};
#[path = "rpc_access_log.rs"]
mod rpc_access_log;
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

#[derive(Clone)]
pub(super) struct RpcRouteContext {
    daemon: Arc<RpcDaemon>,
    control_router_process_pool: Option<Arc<ControlRouterStdioPool>>,
    control_router_process_timeout: Duration,
}

impl RpcRouteContext {
    pub(super) fn new(
        daemon: Arc<RpcDaemon>,
        control_router_process_pool: Option<Arc<ControlRouterStdioPool>>,
        control_router_process_timeout_ms: u64,
    ) -> Self {
        Self {
            daemon,
            control_router_process_pool,
            control_router_process_timeout: Duration::from_millis(
                control_router_process_timeout_ms,
            ),
        }
    }

    #[cfg(test)]
    fn local(daemon: Arc<RpcDaemon>) -> Self {
        Self::new(daemon, None, 0)
    }
}

pub(super) async fn run_rpc_loop(
    addr: Option<SocketAddr>,
    daemon: Arc<RpcDaemon>,
    tls: Option<RpcTlsConfig>,
    unix_socket: Option<PathBuf>,
    control_router_process_pool: Option<Arc<ControlRouterStdioPool>>,
    control_router_process_timeout_ms: u64,
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
    let route_context = RpcRouteContext::new(
        daemon,
        control_router_process_pool,
        control_router_process_timeout_ms,
    );
    run_rpc_loop_until(addr, route_context, tls, unix_socket, shutdown_rx).await;
}

pub(super) async fn run_rpc_loop_until(
    addr: Option<SocketAddr>,
    route_context: RpcRouteContext,
    tls: Option<RpcTlsConfig>,
    unix_socket: Option<PathBuf>,
    shutdown: ShutdownReceiver,
) {
    match (addr, tls, unix_socket) {
        (Some(addr), tls, unix_socket) => {
            let unix_handle = if let Some(path) = unix_socket {
                let route_context_for_unix = route_context.clone();
                let shutdown_for_unix = shutdown.clone();
                Some(tokio::spawn(async move {
                    run_unix_rpc_loop(path, route_context_for_unix, shutdown_for_unix).await;
                }))
            } else {
                None
            };
            match tls {
                Some(config) => run_tls_rpc_loop(addr, route_context, config, shutdown).await,
                None => run_plain_rpc_loop(addr, route_context, shutdown).await,
            }
            if let Some(handle) = unix_handle {
                let _ = handle.await;
            }
        }
        (None, None, Some(path)) => run_unix_rpc_loop(path, route_context, shutdown).await,
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
    route_context: RpcRouteContext,
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
                let route_context = route_context.clone();
                tokio::spawn(async move {
                    handle_connection(stream, peer_addr, route_context, None).await;
                });
            }
        }
    }
}

#[cfg(unix)]
async fn run_unix_rpc_loop(
    path: PathBuf,
    route_context: RpcRouteContext,
    mut shutdown: ShutdownReceiver,
) {
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
                let route_context = route_context.clone();
                tokio::spawn(async move {
                    handle_connection(stream, peer_addr, route_context, None).await;
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
async fn run_unix_rpc_loop(
    path: PathBuf,
    _route_context: RpcRouteContext,
    _shutdown: ShutdownReceiver,
) {
    eprintln!(
        "[daemon] ignoring --rpc-unix {} because Unix sockets are not supported on this platform",
        path.display()
    );
}

async fn run_tls_rpc_loop(
    addr: SocketAddr,
    route_context: RpcRouteContext,
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
                let route_context = route_context.clone();
                let acceptor = acceptor.clone();
                tokio::spawn(async move {
                    match acceptor.accept(stream).await {
                        Ok(tls_stream) => {
                            let transport_auth = extract_transport_auth(&tls_stream);
                            handle_connection(
                                tls_stream,
                                peer_addr,
                                route_context,
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
    route_context: RpcRouteContext,
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
            handle_event_stream(
                stream,
                peer_addr,
                route_context.daemon.as_ref(),
                path,
                headers,
                transport_auth,
            )
            .await;
            return;
        }
    }

    let request_meta = parse_request_log_meta(&buffer);
    let started_at = std::time::Instant::now();
    let response_result =
        handle_http_request_with_route_context(&route_context, &buffer, peer_addr, transport_auth)
            .await;
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

async fn handle_http_request_with_route_context(
    route_context: &RpcRouteContext,
    buffer: &[u8],
    peer_addr: SocketAddr,
    transport_auth: Option<http::TransportAuthContext>,
) -> io::Result<Vec<u8>> {
    if let Some(response) =
        try_route_control_router_rpc(route_context, buffer, peer_addr, transport_auth.as_ref())
            .await?
    {
        return Ok(response);
    }
    http::handle_http_request_with_transport_auth(
        route_context.daemon.as_ref(),
        buffer,
        Some(peer_addr),
        transport_auth,
    )
}

async fn try_route_control_router_rpc(
    route_context: &RpcRouteContext,
    buffer: &[u8],
    peer_addr: SocketAddr,
    transport_auth: Option<&http::TransportAuthContext>,
) -> io::Result<Option<Vec<u8>>> {
    let Some(pool) = route_context.control_router_process_pool.as_ref() else {
        return Ok(None);
    };
    let (method, path, headers) = http::request_method_path_headers(buffer)?;
    if method != "POST" || path != "/rpc" {
        return Ok(None);
    }
    let header_end = http::find_header_end(buffer)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "missing headers"))?;
    let headers_raw = &buffer[..header_end];
    let content_length = http::parse_content_length(headers_raw)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "missing content-length"))?;
    if content_length > codec::MAX_FRAME_PAYLOAD_LEN + 4 {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "body too large"));
    }
    let body_start = header_end + 4;
    let body_end = body_start
        .checked_add(content_length)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "body too large"))?;
    if buffer.len() < body_end {
        return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "body incomplete"));
    }
    let body = &buffer[body_start..body_end];
    let rpc_request: RpcRequest = codec::decode_frame(body)?;
    if !control_router_process_route_allowed(rpc_request.method.as_str()) {
        return Ok(None);
    }
    if let Err(error) = route_context.daemon.authorize_http_request_with_transport(
        &headers,
        Some(peer_addr.ip().to_string().as_str()),
        transport_auth,
    ) {
        return http::build_rpc_error_response(rpc_request.id, error).map(Some);
    }
    let request_id = rpc_request.id;
    let rpc_response =
        match pool.request(rpc_request, route_context.control_router_process_timeout).await {
            Ok(response) => response,
            Err(err) => control_router_rpc_error_response(request_id, err),
        };
    let response_body = codec::encode_frame(&rpc_response).map_err(io::Error::other)?;
    Ok(Some(build_msgpack_ok_response(&response_body)))
}

fn control_router_process_route_allowed(method: &str) -> bool {
    matches!(method, "status")
}

fn control_router_rpc_error_response(id: u64, err: ControlRouterProcessError) -> RpcResponse {
    RpcResponse {
        id,
        result: None,
        error: Some(RpcError::new(
            "CONTROL_ROUTER_PROCESS_UNAVAILABLE",
            format!("control router process request failed: {err:?}"),
        )),
    }
}

fn build_msgpack_ok_response(body: &[u8]) -> Vec<u8> {
    let mut response = Vec::new();
    response.extend_from_slice(b"HTTP/1.1 200 OK\r\nContent-Type: application/msgpack\r\n");
    response.extend_from_slice(format!("Content-Length: {}\r\n\r\n", body.len()).as_bytes());
    response.extend_from_slice(body);
    response
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

async fn read_http_request<S>(stream: &mut S) -> io::Result<Vec<u8>>
where
    S: AsyncRead + Unpin,
{
    let mut buffer = Vec::new();
    let mut expected_len: Option<usize> = None;

    loop {
        let mut chunk = [0_u8; 4096];
        let read = timeout(RPC_READ_TIMEOUT, stream.read(&mut chunk))
            .await
            .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "rpc read timed out"))??;
        if read == 0 {
            break;
        }
        buffer.extend_from_slice(&chunk[..read]);

        if let Some(header_end) = http::find_header_end(&buffer) {
            if header_end > RPC_MAX_HEADER_BYTES {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "rpc headers exceed maximum size",
                ));
            }
            let headers = &buffer[..header_end];
            let content_length = http::parse_content_length(headers).unwrap_or(0);
            if content_length > RPC_MAX_BODY_BYTES {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "rpc body exceeds maximum size",
                ));
            }
            let total_len = header_end
                .checked_add(4)
                .and_then(|body_start| body_start.checked_add(content_length))
                .ok_or_else(|| {
                    io::Error::new(io::ErrorKind::InvalidData, "rpc request too large")
                })?;
            expected_len = Some(total_len);
            if buffer.len() >= total_len {
                break;
            }
        } else if buffer.len() > RPC_MAX_HEADER_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "rpc headers exceed maximum size",
            ));
        }

        if let Some(total_len) = expected_len {
            if buffer.len() > total_len {
                break;
            }
        }
    }

    Ok(buffer)
}

fn request_read_error_response(error: &io::Error) -> &'static [u8] {
    match error.kind() {
        io::ErrorKind::TimedOut => {
            b"HTTP/1.1 408 Request Timeout\r\nContent-Length: 18\r\n\r\nrpc read timed out"
        }
        _ => b"HTTP/1.1 400 Bad Request\r\nContent-Length: 19\r\n\r\ninvalid rpc request",
    }
}

fn build_tls_server_config(config: &RpcTlsConfig) -> io::Result<std::sync::Arc<ServerConfig>> {
    let server_chain = load_cert_chain(config.cert_chain_path.as_path())?;
    let private_key = load_private_key(config.private_key_path.as_path())?;

    let builder = ServerConfig::builder();
    let server_config = if let Some(client_ca_path) = config.client_ca_path.as_ref() {
        let roots = load_root_store(client_ca_path.as_path())?;
        let verifier =
            WebPkiClientVerifier::builder(std::sync::Arc::new(roots)).build().map_err(|err| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!(
                        "failed to build client verifier from {}: {}",
                        client_ca_path.display(),
                        err
                    ),
                )
            })?;
        builder
            .with_client_cert_verifier(verifier)
            .with_single_cert(server_chain, private_key)
            .map_err(|err| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("invalid rpc tls server certificate/key configuration: {}", err),
                )
            })?
    } else {
        builder.with_no_client_auth().with_single_cert(server_chain, private_key).map_err(
            |err| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("invalid rpc tls server certificate/key configuration: {}", err),
                )
            },
        )?
    };

    Ok(std::sync::Arc::new(server_config))
}

fn load_cert_chain(path: &Path) -> io::Result<Vec<rustls::pki_types::CertificateDer<'static>>> {
    let file = File::open(path)?;
    let mut reader = BufReader::new(file);
    let certificates =
        rustls_pemfile::certs(&mut reader).collect::<Result<Vec<_>, _>>().map_err(|err| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("failed to parse PEM certs from {}: {}", path.display(), err),
            )
        })?;
    if certificates.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("no certificates found in {}", path.display()),
        ));
    }
    Ok(certificates)
}

fn load_private_key(path: &Path) -> io::Result<rustls::pki_types::PrivateKeyDer<'static>> {
    let file = File::open(path)?;
    let mut reader = BufReader::new(file);
    let key = private_key(&mut reader).map_err(|err| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("failed to parse private key {}: {}", path.display(), err),
        )
    })?;
    key.ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("no private key found in {}", path.display()),
        )
    })
}

fn load_root_store(path: &Path) -> io::Result<RootCertStore> {
    let certificates = load_cert_chain(path)?;
    let mut roots = RootCertStore::empty();
    let (added, _ignored) = roots.add_parsable_certificates(certificates);
    if added == 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("no valid CA certificates found in {}", path.display()),
        ));
    }
    Ok(roots)
}

fn extract_transport_auth(stream: &TlsStream<TcpStream>) -> http::TransportAuthContext {
    let mut context = http::TransportAuthContext::default();
    let (_tcp_stream, session) = stream.get_ref();
    let Some(peer_certs) = session.peer_certificates() else {
        return context;
    };
    let Some(leaf) = peer_certs.first() else {
        return context;
    };
    context.client_cert_present = true;
    let (subject, sans) = parse_client_identity(leaf.as_ref());
    context.client_subject = subject;
    context.client_sans = sans;
    context
}

fn parse_client_identity(cert_der: &[u8]) -> (Option<String>, Vec<String>) {
    let Ok((_remaining, cert)) = X509Certificate::from_der(cert_der) else {
        return (None, Vec::new());
    };
    let subject = cert
        .subject()
        .iter_common_name()
        .find_map(|name| name.as_str().ok().map(str::to_string))
        .or_else(|| Some(cert.subject().to_string()));
    let sans = parse_subject_alt_names(&cert);
    (subject, sans)
}

fn parse_subject_alt_names(cert: &X509Certificate<'_>) -> Vec<String> {
    let mut sans = Vec::new();
    for extension in cert.extensions() {
        if let ParsedExtension::SubjectAlternativeName(subject_alt_name) =
            extension.parsed_extension()
        {
            for name in &subject_alt_name.general_names {
                let value = match name {
                    GeneralName::DNSName(value) => Some((*value).to_string()),
                    GeneralName::URI(value) => Some((*value).to_string()),
                    GeneralName::RFC822Name(value) => Some((*value).to_string()),
                    GeneralName::IPAddress(raw) if raw.len() == 4 => {
                        Some(IpAddr::from([raw[0], raw[1], raw[2], raw[3]]).to_string())
                    }
                    GeneralName::IPAddress(raw) if raw.len() == 16 => {
                        let mut octets = [0_u8; 16];
                        octets.copy_from_slice(raw);
                        Some(IpAddr::from(octets).to_string())
                    }
                    _ => None,
                };
                if let Some(value) = value {
                    let value = value.trim();
                    if !value.is_empty() {
                        sans.push(value.to_string());
                    }
                }
            }
        }
    }
    sans
}

#[cfg(test)]
mod rpc_loop_tests {
    use super::*;
    use std::fs;
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;
    use tokio::io::{duplex, AsyncReadExt, AsyncWriteExt};
    #[cfg(unix)]
    use tokio::net::UnixStream;

    #[test]
    fn rpc_ready_line_matches_e2e_harness_marker() {
        let line = rpc_ready_line("http", "127.0.0.1:4242");

        assert!(line.contains("listening on http://127.0.0.1:4242"));
    }

    #[test]
    fn read_http_request_collects_complete_post_body() {
        let runtime =
            tokio::runtime::Builder::new_current_thread().enable_all().build().expect("runtime");
        runtime.block_on(async {
            let (mut client, mut server) = duplex(4096);
            let body = b"hello";
            let request = format!(
                "POST /rpc HTTP/1.1\r\nHost: localhost\r\nContent-Length: {}\r\n\r\n",
                body.len()
            );
            let writer = tokio::spawn(async move {
                client.write_all(request.as_bytes()).await.expect("write headers");
                client.write_all(body).await.expect("write body");
            });

            let raw = read_http_request(&mut server).await.expect("read request");
            writer.await.expect("join writer");
            assert!(raw.ends_with(body));
        });
    }

    #[cfg(unix)]
    #[test]
    fn control_router_rpc_route_handles_status_request_via_pool() {
        let runtime =
            tokio::runtime::Builder::new_current_thread().enable_all().build().expect("runtime");
        runtime.block_on(async {
            let temp = tempfile::tempdir().expect("tempdir");
            let script = temp.path().join("control-router-rpc-route.py");
            let routed_response = rns_rpc::rpc::control_boundary::ControlEnvelope::response(
                1,
                RpcResponse { id: 77, result: Some(json!({"routed": true})), error: None },
            )
            .encode_frame()
            .expect("control response frame");
            fs::write(
                &script,
                format!(
                    r#"#!/usr/bin/env python3
import struct
import sys

header = sys.stdin.buffer.read(4)
if len(header) != 4:
    sys.exit(2)
length = struct.unpack(">I", header)[0]
payload = sys.stdin.buffer.read(length)
if len(payload) != length:
    sys.exit(3)
sys.stdout.buffer.write(bytes.fromhex({response_hex:?}))
sys.stdout.buffer.flush()
"#,
                    response_hex = hex::encode(routed_response),
                ),
            )
            .expect("write route worker");
            let mut permissions = fs::metadata(&script).expect("script metadata").permissions();
            permissions.set_mode(0o755);
            fs::set_permissions(&script, permissions).expect("script permissions");

            let pool = Arc::new(ControlRouterStdioPool::spawn(&script, 1).expect("spawn pool"));
            let route_context =
                RpcRouteContext::new(Arc::new(RpcDaemon::test_instance()), Some(pool), 2_000);
            let request = RpcRequest { id: 77, method: "status".to_string(), params: None };
            let request_body = codec::encode_frame(&request).expect("request frame");
            let http_request = build_test_rpc_post(&request_body);
            let response = handle_http_request_with_route_context(
                &route_context,
                &http_request,
                SocketAddr::from(([127, 0, 0, 1], 4242)),
                None,
            )
            .await
            .expect("routed http response");
            let rpc_response = decode_test_http_rpc_response(&response);
            assert_eq!(rpc_response.id, 77);
            assert_eq!(rpc_response.result, Some(json!({"routed": true})));
        });
    }

    #[cfg(unix)]
    #[test]
    fn stalled_control_router_route_does_not_block_local_rpc() {
        let runtime =
            tokio::runtime::Builder::new_current_thread().enable_all().build().expect("runtime");
        runtime.block_on(async {
            let temp = tempfile::tempdir().expect("tempdir");
            let script = temp.path().join("control-router-stalled-route.py");
            let stall_token = temp.path().join("control-router-stalled-once");
            let replacement_response = rns_rpc::rpc::control_boundary::ControlEnvelope::response(
                1,
                RpcResponse { id: 93, result: Some(json!({"replacement": true})), error: None },
            )
            .encode_frame()
            .expect("replacement response frame");
            fs::write(
                &script,
                format!(
                    r#"#!/usr/bin/env python3
import struct
import sys
import time

stall_token = {stall_token_path:?}
try:
    token = open(stall_token, "x")
    token.write("stalled")
    token.close()
    should_stall = True
except FileExistsError:
    should_stall = False
header = sys.stdin.buffer.read(4)
if len(header) != 4:
    sys.exit(2)
length = struct.unpack(">I", header)[0]
payload = sys.stdin.buffer.read(length)
if len(payload) != length:
    sys.exit(3)
if should_stall:
    time.sleep(60)
else:
    sys.stdout.buffer.write(bytes.fromhex({response_hex:?}))
    sys.stdout.buffer.flush()
"#,
                    stall_token_path = stall_token.to_string_lossy(),
                    response_hex = hex::encode(replacement_response),
                ),
            )
            .expect("write stalled route worker");
            let mut permissions = fs::metadata(&script).expect("script metadata").permissions();
            permissions.set_mode(0o755);
            fs::set_permissions(&script, permissions).expect("script permissions");

            let pool = Arc::new(ControlRouterStdioPool::spawn(&script, 1).expect("spawn pool"));
            let route_context =
                RpcRouteContext::new(Arc::new(RpcDaemon::test_instance()), Some(pool), 1_500);
            let status_body = codec::encode_frame(&RpcRequest {
                id: 91,
                method: "status".to_string(),
                params: None,
            })
            .expect("status request frame");
            let status_request = build_test_rpc_post(&status_body);
            let status_context = route_context.clone();
            let status_task = tokio::spawn(async move {
                handle_http_request_with_route_context(
                    &status_context,
                    &status_request,
                    SocketAddr::from(([127, 0, 0, 1], 4242)),
                    None,
                )
                .await
            });

            tokio::time::sleep(Duration::from_millis(10)).await;
            let local_body = codec::encode_frame(&RpcRequest {
                id: 92,
                method: "list_interfaces".to_string(),
                params: None,
            })
            .expect("local request frame");
            let local_request = build_test_rpc_post(&local_body);
            let local_response = timeout(
                Duration::from_millis(100),
                handle_http_request_with_route_context(
                    &route_context,
                    &local_request,
                    SocketAddr::from(([127, 0, 0, 1], 4242)),
                    None,
                ),
            )
            .await
            .expect("local rpc should not wait for stalled control router")
            .expect("local rpc response");
            let local_rpc_response = decode_test_http_rpc_response(&local_response);
            assert_eq!(local_rpc_response.id, 92);
            assert!(local_rpc_response.error.is_none());

            let configure_body = codec::encode_frame(&RpcRequest {
                id: 94,
                method: "sdk_configure_v2".to_string(),
                params: Some(json!({
                    "expected_revision": 0,
                    "patch": { "event_stream": { "max_poll_events": 64 } }
                })),
            })
            .expect("configure request frame");
            let configure_request = build_test_rpc_post(&configure_body);
            let configure_response = timeout(
                Duration::from_millis(100),
                handle_http_request_with_route_context(
                    &route_context,
                    &configure_request,
                    SocketAddr::from(([127, 0, 0, 1], 4242)),
                    None,
                ),
            )
            .await
            .expect("local config rpc should not wait for stalled control router")
            .expect("local config rpc response");
            let configure_rpc_response = decode_test_http_rpc_response(&configure_response);
            assert_eq!(configure_rpc_response.id, 94);
            assert_eq!(
                configure_rpc_response.result.expect("configure result")["revision"],
                json!(1)
            );

            let status_response =
                status_task.await.expect("status route task").expect("status response");
            let status_rpc_response = decode_test_http_rpc_response(&status_response);
            assert_eq!(status_rpc_response.id, 91);
            assert_eq!(
                status_rpc_response.error.expect("status route error").code,
                "CONTROL_ROUTER_PROCESS_UNAVAILABLE"
            );

            let replacement_body = codec::encode_frame(&RpcRequest {
                id: 93,
                method: "status".to_string(),
                params: None,
            })
            .expect("replacement status request frame");
            let replacement_request = build_test_rpc_post(&replacement_body);
            let replacement_response = handle_http_request_with_route_context(
                &route_context,
                &replacement_request,
                SocketAddr::from(([127, 0, 0, 1], 4242)),
                None,
            )
            .await
            .expect("replacement routed response");
            let replacement_rpc_response = decode_test_http_rpc_response(&replacement_response);
            assert_eq!(replacement_rpc_response.id, 93);
            if let Some(error) = replacement_rpc_response.error.as_ref() {
                panic!("replacement routed status failed: {error:?}");
            }
            assert_eq!(
                replacement_rpc_response.result.expect("replacement result"),
                json!({"replacement": true})
            );

            let snapshot_body = codec::encode_frame(&RpcRequest {
                id: 95,
                method: "sdk_snapshot_v2".to_string(),
                params: Some(json!({ "include_counts": true })),
            })
            .expect("snapshot request frame");
            let snapshot_request = build_test_rpc_post(&snapshot_body);
            let snapshot_response = handle_http_request_with_route_context(
                &route_context,
                &snapshot_request,
                SocketAddr::from(([127, 0, 0, 1], 4242)),
                None,
            )
            .await
            .expect("local snapshot response");
            let snapshot_rpc_response = decode_test_http_rpc_response(&snapshot_response);
            assert_eq!(snapshot_rpc_response.id, 95);
            assert_eq!(
                snapshot_rpc_response.result.expect("snapshot result")["config_revision"],
                json!(1)
            );
        });
    }

    #[test]
    fn control_router_process_route_rejects_mutating_methods() {
        assert!(control_router_process_route_allowed("status"));
        assert!(!control_router_process_route_allowed("daemon_status_ex"));
        assert!(!control_router_process_route_allowed("send_message"));
        assert!(!control_router_process_route_allowed("sdk_send_v2"));
    }

    #[test]
    fn read_http_request_rejects_oversized_body() {
        let runtime =
            tokio::runtime::Builder::new_current_thread().enable_all().build().expect("runtime");
        runtime.block_on(async {
            let (mut client, mut server) = duplex(4096);
            let request = format!(
                "POST /rpc HTTP/1.1\r\nHost: localhost\r\nContent-Length: {}\r\n\r\n",
                RPC_MAX_BODY_BYTES + 1
            );
            client.write_all(request.as_bytes()).await.expect("write headers");

            let err = read_http_request(&mut server).await.expect_err("oversized request");
            assert_eq!(err.kind(), io::ErrorKind::InvalidData);
            assert!(err.to_string().contains("maximum size"));
        });
    }

    #[test]
    fn event_stream_invalid_cursor_returns_framed_rpc_error() {
        let runtime =
            tokio::runtime::Builder::new_current_thread().enable_all().build().expect("runtime");
        runtime.block_on(async {
            let daemon = RpcDaemon::test_instance();
            let (mut client, server) = duplex(4096);
            let peer_addr = SocketAddr::from(([127, 0, 0, 1], 4242));

            let server_task = tokio::spawn(async move {
                handle_event_stream(
                    server,
                    peer_addr,
                    &daemon,
                    "/events/stream?cursor=bad-cursor".to_string(),
                    Vec::new(),
                    None,
                )
                .await;
            });

            let mut response = Vec::new();
            client.read_to_end(&mut response).await.expect("read stream rejection");
            server_task.await.expect("event stream task");
            assert!(response.starts_with(b"HTTP/1.1 200 OK\r\n"));
            let header_end = http::find_header_end(&response).expect("response header end");
            let frame = &response[(header_end + 4)..];
            let rpc_response =
                codec::decode_frame::<RpcResponse>(frame).expect("decode framed rpc error");
            let error = rpc_response.error.expect("rpc error");
            assert_eq!(error.code, "SDK_RUNTIME_INVALID_CURSOR");
            assert_eq!(error.machine_code.as_deref(), Some("SDK_RUNTIME_INVALID_CURSOR"));
        });
    }

    #[cfg(unix)]
    #[test]
    fn prepare_rpc_unix_socket_path_refuses_regular_file() {
        let temp = tempfile::tempdir().expect("tempdir");
        let path = temp.path().join("reticulumd.sock");
        std::fs::write(&path, b"not a socket").expect("write regular file");

        let err = prepare_rpc_unix_socket_path(&path).expect_err("regular file rejected");
        assert_eq!(err.kind(), io::ErrorKind::AlreadyExists);
        assert!(err.to_string().contains("non-socket"));
        assert!(path.exists());
    }

    #[cfg(unix)]
    #[test]
    fn unix_rpc_loop_removes_stale_socket_and_cleans_up_on_shutdown() {
        let runtime =
            tokio::runtime::Builder::new_current_thread().enable_all().build().expect("runtime");
        runtime.block_on(async {
            let temp = tempfile::tempdir().expect("tempdir");
            let path = temp.path().join("reticulumd.sock");
            let stale_listener = UnixListener::bind(&path).expect("bind stale socket");
            drop(stale_listener);
            assert!(path.exists());

            let daemon = Arc::new(RpcDaemon::test_instance());
            let route_context = RpcRouteContext::local(daemon);
            let (shutdown_tx, shutdown_rx) = watch::channel(false);
            let task_path = path.clone();
            let task = tokio::spawn(async move {
                run_unix_rpc_loop(task_path, route_context, shutdown_rx).await
            });

            assert!(wait_for_unix_connect(&path).await, "unix listener did not become ready");
            shutdown_tx.send(true).expect("send shutdown");
            timeout(Duration::from_secs(2), task)
                .await
                .expect("unix loop shutdown timeout")
                .expect("join unix loop");
            assert!(wait_for_path_removed(&path).await, "unix socket was not cleaned up");
        });
    }

    #[cfg(unix)]
    async fn wait_for_unix_connect(path: &Path) -> bool {
        for _ in 0..50 {
            if UnixStream::connect(path).await.is_ok() {
                return true;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        false
    }

    #[cfg(unix)]
    async fn wait_for_path_removed(path: &Path) -> bool {
        for _ in 0..50 {
            if !path.exists() {
                return true;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        false
    }

    fn build_test_rpc_post(body: &[u8]) -> Vec<u8> {
        let mut request = Vec::new();
        request.extend_from_slice(b"POST /rpc HTTP/1.1\r\nHost: localhost\r\nContent-Length: ");
        request.extend_from_slice(body.len().to_string().as_bytes());
        request.extend_from_slice(b"\r\n\r\n");
        request.extend_from_slice(body);
        request
    }

    fn decode_test_http_rpc_response(response: &[u8]) -> RpcResponse {
        assert!(response.starts_with(b"HTTP/1.1 200 OK\r\n"));
        let header_end = http::find_header_end(response).expect("response header end");
        codec::decode_frame::<RpcResponse>(&response[(header_end + 4)..])
            .expect("decode rpc response")
    }
}
