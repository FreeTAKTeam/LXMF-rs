use super::bootstrap::RpcTlsConfig;
use rns_rpc::rpc::codec;
use rns_rpc::{http, RpcDaemon, RpcRequest};
use rustls::server::WebPkiClientVerifier;
use rustls::{RootCertStore, ServerConfig};
use rustls_pemfile::private_key;
use serde_json::json;
use std::fs::File;
use std::io::{self, BufReader};
use std::net::{IpAddr, SocketAddr};
use std::path::Path;
use std::rc::Rc;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio_rustls::server::TlsStream;
use tokio_rustls::TlsAcceptor;
use x509_parser::extensions::ParsedExtension;
use x509_parser::prelude::{FromDer, GeneralName, X509Certificate};

#[derive(Debug, Default, Clone)]
struct RpcRequestLogMeta {
    http_method: String,
    path: String,
    rpc_method: Option<String>,
    rpc_request_id: Option<u64>,
    trace_ref: Option<String>,
}

pub(super) async fn run_rpc_loop(
    addr: SocketAddr,
    daemon: Rc<RpcDaemon>,
    tls: Option<RpcTlsConfig>,
) {
    match tls {
        Some(config) => run_tls_rpc_loop(addr, daemon, config).await,
        None => run_plain_rpc_loop(addr, daemon).await,
    }
}

async fn run_plain_rpc_loop(addr: SocketAddr, daemon: Rc<RpcDaemon>) {
    let listener = TcpListener::bind(addr).await.expect("bind rpc listener");
    println!("reticulumd listening on http://{}", addr);

    loop {
        let (stream, peer_addr) = listener.accept().await.expect("accept rpc socket");
        handle_connection(stream, peer_addr, daemon.as_ref(), None).await;
    }
}

async fn run_tls_rpc_loop(addr: SocketAddr, daemon: Rc<RpcDaemon>, config: RpcTlsConfig) {
    let tls_server = build_tls_server_config(&config).expect("build rpc tls server config");
    let acceptor = TlsAcceptor::from(tls_server);
    let listener = TcpListener::bind(addr).await.expect("bind tls rpc listener");
    println!("reticulumd listening on https://{}", addr);

    loop {
        let (stream, peer_addr) = listener.accept().await.expect("accept tls rpc socket");
        match acceptor.accept(stream).await {
            Ok(tls_stream) => {
                let transport_auth = extract_transport_auth(&tls_stream);
                handle_connection(tls_stream, peer_addr, daemon.as_ref(), Some(transport_auth))
                    .await;
            }
            Err(err) => {
                eprintln!("[daemon] rpc tls handshake failed peer={} err={}", peer_addr, err);
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
    let mut buffer = Vec::new();
    loop {
        let mut chunk = [0_u8; 4096];
        let read = match stream.read(&mut chunk).await {
            Ok(read) => read,
            Err(err) => {
                eprintln!("[daemon] rpc read error peer={} err={}", peer_addr, err);
                return;
            }
        };
        if read == 0 {
            break;
        }
        buffer.extend_from_slice(&chunk[..read]);
        if let Some(header_end) = http::find_header_end(&buffer) {
            let headers = &buffer[..header_end];
            if let Some(length) = http::parse_content_length(headers) {
                let body_start = header_end + 4;
                if buffer.len() >= body_start + length {
                    break;
                }
            } else {
                break;
            }
        }
    }

    if buffer.is_empty() {
        return;
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

fn parse_request_log_meta(request: &[u8]) -> RpcRequestLogMeta {
    let mut meta = RpcRequestLogMeta::default();
    let Some(header_end) = http::find_header_end(request) else {
        return meta;
    };
    let headers = &request[..header_end];
    let Some((http_method, path)) = parse_http_request_line(headers) else {
        return meta;
    };
    meta.http_method = http_method.to_string();
    meta.path = path.to_string();

    if http_method != "POST" || path != "/rpc" {
        return meta;
    }
    let Some(content_length) = http::parse_content_length(headers) else {
        return meta;
    };
    let body_start = header_end + 4;
    if request.len() < body_start.saturating_add(content_length) {
        return meta;
    }
    let body = &request[body_start..body_start + content_length];
    let Ok(rpc_request) = codec::decode_frame::<RpcRequest>(body) else {
        return meta;
    };
    meta.trace_ref = Some(format!("rpc:{}:{:016x}", rpc_request.method, rpc_request.id));
    meta.rpc_method = Some(rpc_request.method);
    meta.rpc_request_id = Some(rpc_request.id);
    meta
}

fn parse_http_request_line(headers: &[u8]) -> Option<(&str, &str)> {
    let text = std::str::from_utf8(headers).ok()?;
    let line = text.lines().next()?;
    let mut parts = line.split_whitespace();
    let method = parts.next()?;
    let path = parts.next()?;
    Some((method, path))
}

fn parse_status_code(response: &[u8]) -> Option<u16> {
    let text = std::str::from_utf8(response).ok()?;
    let line = text.lines().next()?;
    let mut parts = line.split_whitespace();
    let _http_version = parts.next()?;
    let code = parts.next()?;
    code.parse::<u16>().ok()
}

fn emit_rpc_access_log(
    peer_addr: SocketAddr,
    meta: &RpcRequestLogMeta,
    response: &[u8],
    elapsed_ms: u64,
    error_text: Option<&str>,
) {
    let status_code = parse_status_code(response).unwrap_or(0);
    let payload = json!({
        "event": "rpc_request",
        "peer": peer_addr.to_string(),
        "http_method": meta.http_method,
        "path": meta.path,
        "rpc_method": meta.rpc_method,
        "rpc_request_id": meta.rpc_request_id,
        "trace_ref": meta.trace_ref,
        "status_code": status_code,
        "elapsed_ms": elapsed_ms,
        "ok": error_text.is_none(),
        "error": error_text,
    });
    eprintln!("{}", payload);
}

fn build_tls_server_config(config: &RpcTlsConfig) -> io::Result<std::sync::Arc<ServerConfig>> {
    let server_chain = load_cert_chain(config.cert_chain_path.as_path())?;
    let private_key = load_private_key(config.private_key_path.as_path())?;

    let builder = ServerConfig::builder();
    let server_config = if let Some(client_ca_path) = config.client_ca_path.as_ref() {
        let roots = load_root_store(client_ca_path.as_path())?;
        let verifier = WebPkiClientVerifier::builder(std::sync::Arc::new(roots))
            .allow_unauthenticated()
            .build()
            .map_err(|err| {
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

    #[test]
    fn parse_request_log_meta_extracts_rpc_fields() {
        let rpc_body = codec::encode_frame(&RpcRequest {
            id: 44,
            method: "sdk_poll_events_v2".to_string(),
            params: Some(json!({ "cursor": null, "max": 1 })),
        })
        .expect("encode rpc body");
        let request = format!(
            "POST /rpc HTTP/1.1\r\nHost: localhost\r\nContent-Length: {}\r\n\r\n",
            rpc_body.len()
        );
        let mut raw = request.into_bytes();
        raw.extend_from_slice(&rpc_body);

        let meta = parse_request_log_meta(&raw);
        assert_eq!(meta.http_method, "POST");
        assert_eq!(meta.path, "/rpc");
        assert_eq!(meta.rpc_method.as_deref(), Some("sdk_poll_events_v2"));
        assert_eq!(meta.rpc_request_id, Some(44));
        assert!(meta
            .trace_ref
            .as_deref()
            .is_some_and(|value| value.contains("sdk_poll_events_v2")));
    }

    #[test]
    fn parse_request_log_meta_keeps_non_rpc_requests_lightweight() {
        let raw = b"GET /healthz HTTP/1.1\r\nHost: localhost\r\n\r\n";
        let meta = parse_request_log_meta(raw);
        assert_eq!(meta.http_method, "GET");
        assert_eq!(meta.path, "/healthz");
        assert!(meta.rpc_method.is_none());
        assert!(meta.rpc_request_id.is_none());
        assert!(meta.trace_ref.is_none());
    }

    #[test]
    fn parse_status_code_extracts_numeric_status() {
        let response = b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n";
        assert_eq!(parse_status_code(response), Some(200));
    }
}
