use super::bootstrap::RpcTlsConfig;
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
                println!("[daemon] shutdown signal received");
                let _ = shutdown_tx.send(true);
            }
            Err(err) => {
                eprintln!("[daemon] failed to install shutdown signal handler: {}", err);
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
    println!("reticulumd listening on http://{}", addr);

    loop {
        tokio::select! {
            shutdown_result = shutdown.changed() => {
                if shutdown_result.is_err() || *shutdown.borrow() {
                    println!("[daemon] rpc tcp listener shutting down");
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
    println!("reticulumd listening on unix:{}", path.display());
    let peer_addr = SocketAddr::from(([127, 0, 0, 1], 0));

    loop {
        tokio::select! {
            shutdown_result = shutdown.changed() => {
                if shutdown_result.is_err() || *shutdown.borrow() {
                    println!("[daemon] rpc unix listener shutting down");
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
    eprintln!(
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
    println!("reticulumd listening on https://{}", addr);

    loop {
        tokio::select! {
            shutdown_result = shutdown.changed() => {
                if shutdown_result.is_err() || *shutdown.borrow() {
                    println!("[daemon] rpc tls listener shutting down");
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
                            eprintln!(
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
            eprintln!("[daemon] rpc read error peer={} err={}", peer_addr, err);
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
                eprintln!(
                    "[daemon] event stream catch-up error peer={} code={} message={}",
                    peer_addr, err.code, err.message
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
                eprintln!("[daemon] event stream encode error peer={} err={}", peer_addr, err);
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
    use tokio::io::{duplex, AsyncReadExt, AsyncWriteExt};
    #[cfg(unix)]
    use tokio::net::UnixStream;

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
            let (shutdown_tx, shutdown_rx) = watch::channel(false);
            let task_path = path.clone();
            let task =
                tokio::spawn(
                    async move { run_unix_rpc_loop(task_path, daemon, shutdown_rx).await },
                );

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
}
