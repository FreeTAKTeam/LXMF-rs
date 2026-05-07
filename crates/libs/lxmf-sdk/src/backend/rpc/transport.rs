use super::*;
use hmac::{Hmac, Mac};
use rns_rpc::e2e_harness::{build_rpc_frame, parse_http_response_body, parse_rpc_frame};
#[cfg(feature = "sdk-async")]
use rns_rpc::rpc::codec;
use rns_rpc::RpcError;
use rustls::pki_types::ServerName;
use rustls::{ClientConfig, RootCertStore};
use rustls_pemfile::private_key;
use sha2::Sha256;
use std::fs::File;
use std::io::{self, BufReader, Read, Write};
use std::net::{IpAddr, Shutdown, TcpStream};
#[cfg(unix)]
use std::os::unix::net::UnixStream as StdUnixStream;
use std::path::Path;
#[cfg(feature = "sdk-async")]
use std::pin::Pin;
use std::sync::Arc;
#[cfg(feature = "sdk-async")]
use std::task::{Context, Poll};
#[cfg(feature = "sdk-async")]
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
#[cfg(feature = "sdk-async")]
use tokio::sync::mpsc;
#[cfg(feature = "sdk-async")]
use tokio::task::JoinHandle;
#[cfg(feature = "sdk-async")]
use tokio_rustls::TlsConnector;
#[cfg(feature = "sdk-async")]
use tokio_stream::wrappers::ReceiverStream;
#[cfg(feature = "sdk-async")]
use tokio_stream::Stream;
use zeroize::{Zeroize, Zeroizing};

#[cfg(feature = "sdk-async")]
const RPC_EVENT_STREAM_MAX_FRAME_BYTES: usize = 2 * 1024 * 1024;

#[derive(Clone, Copy)]
enum RpcEndpoint<'a> {
    Tcp(&'a str),
    Unix(&'a str),
}

impl RpcEndpoint<'_> {
    fn host_header(&self) -> &str {
        match self {
            Self::Tcp(authority) => authority,
            Self::Unix(_) => "localhost",
        }
    }
}

impl RpcBackendClient {
    pub(super) fn call_rpc(
        &self,
        method: &str,
        params: Option<JsonValue>,
    ) -> Result<JsonValue, SdkError> {
        let (headers, mtls_auth) = {
            let auth_guard = self.session_auth.read().expect("session_auth rwlock poisoned");
            (self.headers_for_session_auth(&auth_guard), Self::mtls_for_session_auth(&auth_guard))
        };
        self.call_rpc_with_headers(method, params, mtls_auth.as_ref(), headers)
    }

    pub(super) fn call_rpc_with_headers(
        &self,
        method: &str,
        params: Option<JsonValue>,
        mtls_auth: Option<&MtlsRequestAuth>,
        mut headers: Vec<(String, String)>,
    ) -> Result<JsonValue, SdkError> {
        let request_id = self.next_request_id();
        let frame = build_rpc_frame(request_id, method, params).map_err(|err| {
            SdkError::new(code::INTERNAL, ErrorCategory::Internal, err.to_string())
        })?;
        let endpoint = Self::parse_endpoint(&self.endpoint)?;
        let mut request = Self::build_http_post_with_headers(
            "/rpc",
            endpoint.host_header(),
            &frame,
            headers.as_slice(),
        );
        let response_result = match (endpoint, mtls_auth) {
            (RpcEndpoint::Tcp(authority), Some(mtls_auth)) => self.send_mtls_request(
                authority,
                request.as_slice(),
                mtls_auth.ca_bundle_path.as_str(),
                mtls_auth.client_cert_path.as_deref(),
                mtls_auth.client_key_path.as_deref(),
            ),
            (RpcEndpoint::Tcp(authority), None) => {
                self.send_plain_request(authority, request.as_slice())
            }
            (RpcEndpoint::Unix(_), Some(_)) => Err(SdkError::new(
                code::VALIDATION_INVALID_ARGUMENT,
                ErrorCategory::Validation,
                "mTLS transport auth is not supported over unix RPC endpoints",
            )),
            (RpcEndpoint::Unix(path), None) => Self::send_unix_request(path, request.as_slice()),
        };
        request.zeroize();
        Self::zeroize_header_values(headers.as_mut_slice());
        let mut response = response_result?;
        let body = parse_http_response_body(response.as_mut_slice()).map_err(|err| {
            SdkError::new(code::INTERNAL, ErrorCategory::Transport, err.to_string())
        })?;
        let rpc_response = parse_rpc_frame(&body).map_err(|err| {
            SdkError::new(code::INTERNAL, ErrorCategory::Transport, err.to_string())
        })?;
        if let Some(error) = rpc_response.error {
            return Err(Self::map_rpc_error(error));
        }
        Ok(rpc_response.result.unwrap_or(JsonValue::Null))
    }

    #[cfg(feature = "sdk-async")]
    pub(super) async fn call_rpc_async(
        &self,
        method: &str,
        params: Option<JsonValue>,
    ) -> Result<JsonValue, SdkError> {
        let (headers, mtls_auth) = {
            let auth_guard = self.session_auth.read().expect("session_auth rwlock poisoned");
            (self.headers_for_session_auth(&auth_guard), Self::mtls_for_session_auth(&auth_guard))
        };
        self.call_rpc_async_with_headers(method, params, mtls_auth.as_ref(), headers).await
    }

    #[cfg(feature = "sdk-async")]
    pub(super) async fn call_rpc_async_with_headers(
        &self,
        method: &str,
        params: Option<JsonValue>,
        mtls_auth: Option<&MtlsRequestAuth>,
        mut headers: Vec<(String, String)>,
    ) -> Result<JsonValue, SdkError> {
        let request_id = self.next_request_id();
        let frame = build_rpc_frame(request_id, method, params).map_err(|err| {
            SdkError::new(code::INTERNAL, ErrorCategory::Internal, err.to_string())
        })?;
        let endpoint = Self::parse_endpoint(&self.endpoint)?;
        let mut request = Self::build_http_post_with_headers(
            "/rpc",
            endpoint.host_header(),
            &frame,
            headers.as_slice(),
        );
        let mut response = match (endpoint, mtls_auth) {
            (RpcEndpoint::Tcp(authority), Some(mtls_auth)) => {
                Self::send_mtls_request_async(
                    authority,
                    request.as_slice(),
                    mtls_auth.ca_bundle_path.as_str(),
                    mtls_auth.client_cert_path.as_deref(),
                    mtls_auth.client_key_path.as_deref(),
                )
                .await
            }
            (RpcEndpoint::Tcp(authority), None) => {
                Self::send_plain_request_async(authority, request.as_slice()).await
            }
            (RpcEndpoint::Unix(_path), Some(_)) => Err(SdkError::new(
                code::VALIDATION_INVALID_ARGUMENT,
                ErrorCategory::Validation,
                "mTLS transport auth is not supported over unix RPC endpoints",
            )),
            (RpcEndpoint::Unix(path), None) => {
                Self::send_unix_request_async(path, request.as_slice()).await
            }
        }?;
        request.zeroize();
        Self::zeroize_header_values(headers.as_mut_slice());
        let body = parse_http_response_body(response.as_mut_slice()).map_err(|err| {
            SdkError::new(code::INTERNAL, ErrorCategory::Transport, err.to_string())
        })?;
        let rpc_response = parse_rpc_frame(&body).map_err(|err| {
            SdkError::new(code::INTERNAL, ErrorCategory::Transport, err.to_string())
        })?;
        if let Some(error) = rpc_response.error {
            return Err(Self::map_rpc_error(error));
        }
        Ok(rpc_response.result.unwrap_or(JsonValue::Null))
    }

    fn send_plain_request(&self, authority: &str, request: &[u8]) -> Result<Vec<u8>, SdkError> {
        let mut stream = TcpStream::connect(authority).map_err(|err| {
            SdkError::new(code::INTERNAL, ErrorCategory::Transport, err.to_string())
        })?;
        stream.write_all(request).map_err(|err| {
            SdkError::new(code::INTERNAL, ErrorCategory::Transport, err.to_string())
        })?;
        stream.shutdown(Shutdown::Write).map_err(|err| {
            SdkError::new(code::INTERNAL, ErrorCategory::Transport, err.to_string())
        })?;
        let mut response = Vec::new();
        stream.read_to_end(&mut response).map_err(|err| {
            SdkError::new(code::INTERNAL, ErrorCategory::Transport, err.to_string())
        })?;
        Ok(response)
    }

    #[cfg(unix)]
    fn send_unix_request(path: &str, request: &[u8]) -> Result<Vec<u8>, SdkError> {
        let mut stream = StdUnixStream::connect(path).map_err(|err| {
            SdkError::new(code::INTERNAL, ErrorCategory::Transport, err.to_string())
        })?;
        stream.write_all(request).map_err(|err| {
            SdkError::new(code::INTERNAL, ErrorCategory::Transport, err.to_string())
        })?;
        stream.shutdown(Shutdown::Write).map_err(|err| {
            SdkError::new(code::INTERNAL, ErrorCategory::Transport, err.to_string())
        })?;
        let mut response = Vec::new();
        stream.read_to_end(&mut response).map_err(|err| {
            SdkError::new(code::INTERNAL, ErrorCategory::Transport, err.to_string())
        })?;
        Ok(response)
    }

    #[cfg(not(unix))]
    fn send_unix_request(_path: &str, _request: &[u8]) -> Result<Vec<u8>, SdkError> {
        Err(SdkError::new(
            code::VALIDATION_INVALID_ARGUMENT,
            ErrorCategory::Validation,
            "unix RPC endpoints are not supported on this platform",
        ))
    }

    fn send_mtls_request(
        &self,
        authority: &str,
        request: &[u8],
        ca_bundle_path: &str,
        client_cert_path: Option<&str>,
        client_key_path: Option<&str>,
    ) -> Result<Vec<u8>, SdkError> {
        let roots = Self::load_root_store(Path::new(ca_bundle_path))?;
        let builder = ClientConfig::builder().with_root_certificates(roots);
        let client_config = match (client_cert_path, client_key_path) {
            (Some(cert_path), Some(key_path)) => {
                let cert_chain = Self::load_cert_chain(Path::new(cert_path))?;
                let private_key = Self::load_private_key(Path::new(key_path))?;
                builder.with_client_auth_cert(cert_chain, private_key).map_err(|err| {
                    SdkError::new(
                        code::INTERNAL,
                        ErrorCategory::Transport,
                        format!("invalid mtls client certificate/key configuration: {}", err),
                    )
                })?
            }
            (None, None) => builder.with_no_client_auth(),
            _ => {
                return Err(SdkError::new(
                    code::SECURITY_AUTH_REQUIRED,
                    ErrorCategory::Security,
                    "mtls client certificate and key paths must be configured together",
                ))
            }
        };
        let server_name = Self::server_name_for_authority(authority)?;
        let connection = rustls::ClientConnection::new(Arc::new(client_config), server_name)
            .map_err(|err| {
                SdkError::new(
                    code::INTERNAL,
                    ErrorCategory::Transport,
                    format!("failed to start tls client connection: {}", err),
                )
            })?;
        let stream = TcpStream::connect(authority).map_err(|err| {
            SdkError::new(code::INTERNAL, ErrorCategory::Transport, err.to_string())
        })?;
        let mut tls = rustls::StreamOwned::new(connection, stream);
        tls.write_all(request).map_err(|err| {
            SdkError::new(code::INTERNAL, ErrorCategory::Transport, err.to_string())
        })?;
        tls.flush().map_err(|err| {
            SdkError::new(code::INTERNAL, ErrorCategory::Transport, err.to_string())
        })?;
        let mut response = Vec::new();
        tls.read_to_end(&mut response).map_err(|err| {
            SdkError::new(code::INTERNAL, ErrorCategory::Transport, err.to_string())
        })?;
        Ok(response)
    }

    #[cfg(feature = "sdk-async")]
    async fn send_plain_request_async(
        authority: &str,
        request: &[u8],
    ) -> Result<Vec<u8>, SdkError> {
        let mut stream = tokio::net::TcpStream::connect(authority).await.map_err(|err| {
            SdkError::new(code::INTERNAL, ErrorCategory::Transport, err.to_string())
        })?;
        stream.write_all(request).await.map_err(|err| {
            SdkError::new(code::INTERNAL, ErrorCategory::Transport, err.to_string())
        })?;
        let mut response = Vec::new();
        stream.read_to_end(&mut response).await.map_err(|err| {
            SdkError::new(code::INTERNAL, ErrorCategory::Transport, err.to_string())
        })?;
        Ok(response)
    }

    #[cfg(all(feature = "sdk-async", unix))]
    async fn send_unix_request_async(path: &str, request: &[u8]) -> Result<Vec<u8>, SdkError> {
        let mut stream = tokio::net::UnixStream::connect(path).await.map_err(|err| {
            SdkError::new(code::INTERNAL, ErrorCategory::Transport, err.to_string())
        })?;
        stream.write_all(request).await.map_err(|err| {
            SdkError::new(code::INTERNAL, ErrorCategory::Transport, err.to_string())
        })?;
        let mut response = Vec::new();
        stream.read_to_end(&mut response).await.map_err(|err| {
            SdkError::new(code::INTERNAL, ErrorCategory::Transport, err.to_string())
        })?;
        Ok(response)
    }

    #[cfg(all(feature = "sdk-async", not(unix)))]
    async fn send_unix_request_async(_path: &str, _request: &[u8]) -> Result<Vec<u8>, SdkError> {
        Err(SdkError::new(
            code::VALIDATION_INVALID_ARGUMENT,
            ErrorCategory::Validation,
            "unix RPC endpoints are not supported on this platform",
        ))
    }

    #[cfg(feature = "sdk-async")]
    async fn send_mtls_request_async(
        authority: &str,
        request: &[u8],
        ca_bundle_path: &str,
        client_cert_path: Option<&str>,
        client_key_path: Option<&str>,
    ) -> Result<Vec<u8>, SdkError> {
        let roots = Self::load_root_store(Path::new(ca_bundle_path))?;
        let builder = ClientConfig::builder().with_root_certificates(roots);
        let client_config = match (client_cert_path, client_key_path) {
            (Some(cert_path), Some(key_path)) => {
                let cert_chain = Self::load_cert_chain(Path::new(cert_path))?;
                let private_key = Self::load_private_key(Path::new(key_path))?;
                builder.with_client_auth_cert(cert_chain, private_key).map_err(|err| {
                    SdkError::new(
                        code::INTERNAL,
                        ErrorCategory::Transport,
                        format!("invalid mtls client certificate/key configuration: {}", err),
                    )
                })?
            }
            (None, None) => builder.with_no_client_auth(),
            _ => {
                return Err(SdkError::new(
                    code::SECURITY_AUTH_REQUIRED,
                    ErrorCategory::Security,
                    "mtls client certificate and key paths must be configured together",
                ))
            }
        };
        let server_name = Self::server_name_for_authority(authority)?;
        let connector = TlsConnector::from(Arc::new(client_config));
        let stream = tokio::net::TcpStream::connect(authority).await.map_err(|err| {
            SdkError::new(code::INTERNAL, ErrorCategory::Transport, err.to_string())
        })?;
        let mut stream = connector.connect(server_name, stream).await.map_err(|err| {
            SdkError::new(
                code::INTERNAL,
                ErrorCategory::Transport,
                format!("failed to start tls client connection: {}", err),
            )
        })?;
        stream.write_all(request).await.map_err(|err| {
            SdkError::new(code::INTERNAL, ErrorCategory::Transport, err.to_string())
        })?;
        let mut response = Vec::new();
        stream.read_to_end(&mut response).await.map_err(|err| {
            SdkError::new(code::INTERNAL, ErrorCategory::Transport, err.to_string())
        })?;
        Ok(response)
    }

    fn parse_endpoint(endpoint: &str) -> Result<RpcEndpoint<'_>, SdkError> {
        if let Some(path) = endpoint
            .strip_prefix("unix://")
            .or_else(|| endpoint.strip_prefix("unix:"))
            .map(str::trim)
            .filter(|path| !path.is_empty())
        {
            return Ok(RpcEndpoint::Unix(path));
        }
        Self::endpoint_authority(endpoint).map(RpcEndpoint::Tcp)
    }

    fn endpoint_authority(endpoint: &str) -> Result<&str, SdkError> {
        let without_scheme = endpoint
            .strip_prefix("http://")
            .or_else(|| endpoint.strip_prefix("https://"))
            .or_else(|| endpoint.strip_prefix("tls://"))
            .unwrap_or(endpoint);
        let authority = without_scheme.split('/').next().unwrap_or(without_scheme).trim();
        if authority.is_empty() {
            return Err(SdkError::new(
                code::VALIDATION_INVALID_ARGUMENT,
                ErrorCategory::Validation,
                "rpc endpoint must include host:port authority",
            ));
        }
        Ok(authority)
    }

    fn endpoint_host(authority: &str) -> Result<String, SdkError> {
        let host = if let Some(stripped) = authority.strip_prefix('[') {
            let Some(end) = stripped.find(']') else {
                return Err(SdkError::new(
                    code::VALIDATION_INVALID_ARGUMENT,
                    ErrorCategory::Validation,
                    "invalid bracketed rpc endpoint host",
                ));
            };
            stripped[..end].to_string()
        } else if let Some((host, _port)) = authority.rsplit_once(':') {
            host.to_string()
        } else {
            authority.to_string()
        };
        let host = host.trim();
        if host.is_empty() {
            return Err(SdkError::new(
                code::VALIDATION_INVALID_ARGUMENT,
                ErrorCategory::Validation,
                "rpc endpoint host must not be empty",
            ));
        }
        Ok(host.to_string())
    }

    fn server_name_for_authority(authority: &str) -> Result<ServerName<'static>, SdkError> {
        let host = Self::endpoint_host(authority)?;
        if let Ok(server_name) = ServerName::try_from(host.clone()) {
            return Ok(server_name);
        }
        let ip = host.parse::<IpAddr>().map_err(|_| {
            SdkError::new(
                code::VALIDATION_INVALID_ARGUMENT,
                ErrorCategory::Validation,
                "rpc tls endpoint host must be a valid DNS name or IP address",
            )
        })?;
        Ok(ServerName::IpAddress(ip.into()))
    }

    fn load_cert_chain(
        path: &Path,
    ) -> Result<Vec<rustls::pki_types::CertificateDer<'static>>, SdkError> {
        let file = File::open(path).map_err(|err| {
            SdkError::new(
                code::SECURITY_AUTH_REQUIRED,
                ErrorCategory::Security,
                format!("failed to open mtls certificate chain {}: {}", path.display(), err),
            )
        })?;
        let mut reader = BufReader::new(file);
        let certificates = rustls_pemfile::certs(&mut reader)
            .collect::<Result<Vec<_>, io::Error>>()
            .map_err(|err| {
                SdkError::new(
                    code::SECURITY_AUTH_REQUIRED,
                    ErrorCategory::Security,
                    format!("failed to parse mtls certificate chain {}: {}", path.display(), err),
                )
            })?;
        if certificates.is_empty() {
            return Err(SdkError::new(
                code::SECURITY_AUTH_REQUIRED,
                ErrorCategory::Security,
                format!("mtls certificate chain {} is empty", path.display()),
            ));
        }
        Ok(certificates)
    }

    fn load_private_key(
        path: &Path,
    ) -> Result<rustls::pki_types::PrivateKeyDer<'static>, SdkError> {
        let file = File::open(path).map_err(|err| {
            SdkError::new(
                code::SECURITY_AUTH_REQUIRED,
                ErrorCategory::Security,
                format!("failed to open mtls private key {}: {}", path.display(), err),
            )
        })?;
        let mut reader = BufReader::new(file);
        let key = private_key(&mut reader).map_err(|err| {
            SdkError::new(
                code::SECURITY_AUTH_REQUIRED,
                ErrorCategory::Security,
                format!("failed to parse mtls private key {}: {}", path.display(), err),
            )
        })?;
        key.ok_or_else(|| {
            SdkError::new(
                code::SECURITY_AUTH_REQUIRED,
                ErrorCategory::Security,
                format!("mtls private key {} is empty", path.display()),
            )
        })
    }

    fn load_root_store(path: &Path) -> Result<RootCertStore, SdkError> {
        let certificates = Self::load_cert_chain(path)?;
        let mut roots = RootCertStore::empty();
        let (added, _ignored) = roots.add_parsable_certificates(certificates);
        if added == 0 {
            return Err(SdkError::new(
                code::SECURITY_AUTH_REQUIRED,
                ErrorCategory::Security,
                format!("no valid CA certificates found in {}", path.display()),
            ));
        }
        Ok(roots)
    }

    pub(super) fn build_http_post_with_headers(
        path: &str,
        host: &str,
        body: &[u8],
        headers: &[(String, String)],
    ) -> Vec<u8> {
        let mut request = Vec::new();
        request.extend_from_slice(format!("POST {path} HTTP/1.1\r\n").as_bytes());
        request.extend_from_slice(format!("Host: {host}\r\n").as_bytes());
        request.extend_from_slice(b"Content-Type: application/msgpack\r\n");
        for (name, value) in headers {
            request.extend_from_slice(format!("{name}: {value}\r\n").as_bytes());
        }
        request.extend_from_slice(format!("Content-Length: {}\r\n", body.len()).as_bytes());
        request.extend_from_slice(b"\r\n");
        request.extend_from_slice(body);
        request
    }

    #[cfg(feature = "sdk-async")]
    pub(super) fn open_event_stream_impl(
        &self,
        subscription: &EventSubscription,
    ) -> Result<Option<SdkEventStream>, SdkError> {
        let (headers, mtls_auth) = {
            let auth_guard = self.session_auth.read().expect("session_auth rwlock poisoned");
            (self.headers_for_session_auth(&auth_guard), Self::mtls_for_session_auth(&auth_guard))
        };
        let handle = tokio::runtime::Handle::try_current().map_err(|_| {
            SdkError::new(
                code::INTERNAL,
                ErrorCategory::Runtime,
                "rpc event stream requires an active Tokio runtime",
            )
        })?;
        let (tx, rx) = mpsc::channel(256);
        let endpoint = self.endpoint.clone();
        let cursor = subscription.cursor.clone();
        let task = handle.spawn(async move {
            run_rpc_http_event_stream(endpoint, headers, mtls_auth, cursor, tx).await;
        });
        Ok(Some(Box::pin(AbortOnDropStream::new(ReceiverStream::new(rx), task))))
    }

    #[cfg(feature = "sdk-async")]
    fn build_http_get_with_headers(
        path: &str,
        host: &str,
        headers: &[(String, String)],
    ) -> Vec<u8> {
        let mut request = Vec::new();
        request.extend_from_slice(format!("GET {path} HTTP/1.1\r\n").as_bytes());
        request.extend_from_slice(format!("Host: {host}\r\n").as_bytes());
        request.extend_from_slice(b"Accept: application/msgpack\r\n");
        for (name, value) in headers {
            request.extend_from_slice(format!("{name}: {value}\r\n").as_bytes());
        }
        request.extend_from_slice(b"\r\n");
        request
    }

    pub(super) fn map_rpc_error(error: RpcError) -> SdkError {
        let machine_code = error
            .machine_code
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| error.code.clone());
        let category = error
            .category
            .as_deref()
            .and_then(Self::parse_error_category)
            .unwrap_or_else(|| Self::map_category(machine_code.as_str()));
        let mut sdk_error = SdkError::new(machine_code, category, error.message);
        if let Some(retryable) = error.retryable {
            sdk_error = sdk_error.with_retryable(retryable);
        }
        if let Some(is_user_actionable) = error.is_user_actionable {
            sdk_error = sdk_error.with_user_actionable(is_user_actionable);
        }
        if let Some(cause_code) = error.cause_code {
            sdk_error = sdk_error.with_cause_code(cause_code);
        }
        if let Some(details) = error.details {
            for (key, value) in *details {
                sdk_error = sdk_error.with_detail(key, value);
            }
        }
        if let Some(extensions) = error.extensions {
            for (key, value) in *extensions {
                sdk_error.extensions.insert(key, value);
            }
        }
        sdk_error
    }

    pub(super) fn map_category(code: &str) -> ErrorCategory {
        if code.contains("_VALIDATION_") {
            return ErrorCategory::Validation;
        }
        if code.contains("_CAPABILITY_") {
            return ErrorCategory::Capability;
        }
        if code.contains("_CONFIG_") {
            return ErrorCategory::Config;
        }
        if code.contains("_POLICY_") {
            return ErrorCategory::Policy;
        }
        if code.contains("_TRANSPORT_") {
            return ErrorCategory::Transport;
        }
        if code.contains("_STORAGE_") {
            return ErrorCategory::Storage;
        }
        if code.contains("_CRYPTO_") {
            return ErrorCategory::Crypto;
        }
        if code.contains("_TIMEOUT_") {
            return ErrorCategory::Timeout;
        }
        if code.contains("_RUNTIME_") {
            return ErrorCategory::Runtime;
        }
        if code.contains("_SECURITY_") {
            return ErrorCategory::Security;
        }
        ErrorCategory::Internal
    }

    fn parse_error_category(raw: &str) -> Option<ErrorCategory> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "validation" => Some(ErrorCategory::Validation),
            "capability" => Some(ErrorCategory::Capability),
            "config" => Some(ErrorCategory::Config),
            "policy" => Some(ErrorCategory::Policy),
            "transport" => Some(ErrorCategory::Transport),
            "storage" => Some(ErrorCategory::Storage),
            "crypto" => Some(ErrorCategory::Crypto),
            "timeout" => Some(ErrorCategory::Timeout),
            "runtime" => Some(ErrorCategory::Runtime),
            "security" => Some(ErrorCategory::Security),
            "internal" => Some(ErrorCategory::Internal),
            _ => None,
        }
    }

    pub(super) fn profile_to_wire(profile: crate::types::Profile) -> &'static str {
        match profile {
            crate::types::Profile::DesktopFull => "desktop-full",
            crate::types::Profile::DesktopLocalRuntime => "desktop-local-runtime",
            crate::types::Profile::EmbeddedAlloc => "embedded-alloc",
        }
    }

    pub(super) fn bind_mode_to_wire(bind_mode: crate::types::BindMode) -> &'static str {
        match bind_mode {
            crate::types::BindMode::LocalOnly => "local_only",
            crate::types::BindMode::Remote => "remote",
        }
    }

    pub(super) fn auth_mode_to_wire(auth_mode: crate::types::AuthMode) -> &'static str {
        match auth_mode {
            crate::types::AuthMode::LocalTrusted => "local_trusted",
            crate::types::AuthMode::Token => "token",
            crate::types::AuthMode::Mtls => "mtls",
        }
    }

    pub(super) fn overflow_policy_to_wire(
        overflow_policy: crate::types::OverflowPolicy,
    ) -> &'static str {
        match overflow_policy {
            crate::types::OverflowPolicy::Reject => "reject",
            crate::types::OverflowPolicy::DropOldest => "drop_oldest",
            crate::types::OverflowPolicy::Block => "block",
        }
    }

    pub(super) fn session_auth_from_request(
        &self,
        req: &NegotiationRequest,
    ) -> Result<SessionAuth, SdkError> {
        match req.auth_mode {
            AuthMode::LocalTrusted => Ok(SessionAuth::LocalTrusted),
            AuthMode::Mtls => {
                let mtls_auth = req
                    .rpc_backend
                    .as_ref()
                    .and_then(|config| config.mtls_auth.as_ref())
                    .ok_or_else(|| {
                        SdkError::new(
                            code::SECURITY_AUTH_REQUIRED,
                            ErrorCategory::Security,
                            "mtls auth mode requires rpc_backend.mtls_auth",
                        )
                    })?;
                if mtls_auth.ca_bundle_path.trim().is_empty() {
                    return Err(SdkError::new(
                        code::SECURITY_AUTH_REQUIRED,
                        ErrorCategory::Security,
                        "mtls auth mode requires non-empty rpc_backend.mtls_auth.ca_bundle_path",
                    ));
                }
                let client_cert_path = mtls_auth
                    .client_cert_path
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_string);
                let client_key_path = mtls_auth
                    .client_key_path
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .map(str::to_string);
                if client_cert_path.is_some() ^ client_key_path.is_some() {
                    return Err(SdkError::new(
                        code::VALIDATION_INVALID_ARGUMENT,
                        ErrorCategory::Validation,
                        "mtls client certificate and key paths must be configured together",
                    ));
                }
                if mtls_auth.require_client_cert
                    && (client_cert_path.is_none() || client_key_path.is_none())
                {
                    return Err(SdkError::new(
                        code::SECURITY_AUTH_REQUIRED,
                        ErrorCategory::Security,
                        "mtls auth mode with require_client_cert=true requires client_cert_path and client_key_path",
                    ));
                }
                Ok(SessionAuth::Mtls {
                    ca_bundle_path: mtls_auth.ca_bundle_path.clone(),
                    client_cert_path,
                    client_key_path,
                })
            }
            AuthMode::Token => {
                let token_auth = req
                    .rpc_backend
                    .as_ref()
                    .and_then(|config| config.token_auth.as_ref())
                    .ok_or_else(|| {
                        SdkError::new(
                            code::SECURITY_AUTH_REQUIRED,
                            ErrorCategory::Security,
                            "token auth mode requires rpc_backend.token_auth",
                        )
                    })?;
                if token_auth.shared_secret.trim().is_empty() {
                    return Err(SdkError::new(
                        code::SECURITY_AUTH_REQUIRED,
                        ErrorCategory::Security,
                        "token auth shared_secret must be configured",
                    ));
                }
                Ok(SessionAuth::Token {
                    issuer: token_auth.issuer.clone(),
                    audience: token_auth.audience.clone(),
                    shared_secret: Zeroizing::new(token_auth.shared_secret.clone()),
                    ttl_secs: (token_auth.jti_cache_ttl_ms / 1000).max(1),
                })
            }
        }
    }

    pub(super) fn token_signature(secret: &str, payload: &str) -> String {
        let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes())
            .expect("token shared secret must be non-empty");
        mac.update(payload.as_bytes());
        hex::encode(mac.finalize().into_bytes())
    }

    pub(super) fn headers_for_session_auth(&self, auth: &SessionAuth) -> Vec<(String, String)> {
        match auth {
            SessionAuth::LocalTrusted => Vec::new(),
            SessionAuth::Mtls { .. } => Vec::new(),
            SessionAuth::Token { issuer, audience, shared_secret, ttl_secs } => {
                let jti = format!("sdk-jti-{}", self.next_request_id());
                let iat = Self::now_seconds();
                let exp = iat.saturating_add(*ttl_secs);
                let payload = Zeroizing::new(format!(
                    "iss={issuer};aud={audience};jti={jti};sub=sdk-client;iat={iat};exp={exp}"
                ));
                let sig =
                    Zeroizing::new(Self::token_signature(shared_secret.as_str(), payload.as_str()));
                let token = Zeroizing::new(format!("{};sig={}", payload.as_str(), sig.as_str()));
                vec![("Authorization".to_owned(), format!("Bearer {}", token.as_str()))]
            }
        }
    }

    pub(super) fn mtls_for_session_auth(auth: &SessionAuth) -> Option<MtlsRequestAuth> {
        match auth {
            SessionAuth::Mtls { ca_bundle_path, client_cert_path, client_key_path } => {
                Some(MtlsRequestAuth {
                    ca_bundle_path: ca_bundle_path.clone(),
                    client_cert_path: client_cert_path.clone(),
                    client_key_path: client_key_path.clone(),
                })
            }
            SessionAuth::LocalTrusted | SessionAuth::Token { .. } => None,
        }
    }

    fn zeroize_header_values(headers: &mut [(String, String)]) {
        for (_, value) in headers {
            value.zeroize();
        }
    }
}

#[cfg(feature = "sdk-async")]
trait RpcEventStreamIo: AsyncRead + AsyncWrite + Unpin + Send {}

#[cfg(feature = "sdk-async")]
impl<T> RpcEventStreamIo for T where T: AsyncRead + AsyncWrite + Unpin + Send {}

#[cfg(feature = "sdk-async")]
struct AbortOnDropStream<S> {
    inner: S,
    task: JoinHandle<()>,
}

#[cfg(feature = "sdk-async")]
impl<S> AbortOnDropStream<S> {
    fn new(inner: S, task: JoinHandle<()>) -> Self {
        Self { inner, task }
    }
}

#[cfg(feature = "sdk-async")]
impl<S> Stream for AbortOnDropStream<S>
where
    S: Stream<Item = Result<SdkEvent, SdkError>> + Unpin,
{
    type Item = Result<SdkEvent, SdkError>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        Pin::new(&mut self.inner).poll_next(cx)
    }
}

#[cfg(feature = "sdk-async")]
impl<S> Drop for AbortOnDropStream<S> {
    fn drop(&mut self) {
        self.task.abort();
    }
}

#[cfg(feature = "sdk-async")]
async fn run_rpc_http_event_stream(
    endpoint: String,
    headers: Vec<(String, String)>,
    mtls_auth: Option<MtlsRequestAuth>,
    mut cursor: Option<EventCursor>,
    tx: mpsc::Sender<Result<SdkEvent, SdkError>>,
) {
    loop {
        let parsed_endpoint = match RpcBackendClient::parse_endpoint(&endpoint) {
            Ok(endpoint) => endpoint.to_owned(),
            Err(err) => {
                let _ = tx.send(Err(err)).await;
                return;
            }
        };
        let mut stream = match connect_rpc_http_event_stream(
            parsed_endpoint.as_ref(),
            &headers,
            cursor.as_ref(),
            mtls_auth.as_ref(),
        )
        .await
        {
            Ok(stream) => stream,
            Err(err) => {
                let _ = tx.send(Err(err)).await;
                return;
            }
        };
        loop {
            match read_rpc_http_event_frame(&mut stream).await {
                Ok(event) => {
                    cursor = Some(EventCursor(format!(
                        "v2:{}:{}:{}",
                        event.runtime_id, event.stream_id, event.seq_no
                    )));
                    if tx.send(Ok(event)).await.is_err() {
                        return;
                    }
                }
                Err(_) => {
                    tokio::time::sleep(std::time::Duration::from_millis(250)).await;
                    break;
                }
            }
        }
    }
}

#[cfg(feature = "sdk-async")]
enum OwnedRpcEndpoint {
    Tcp(String),
    Unix(String),
}

#[cfg(feature = "sdk-async")]
impl<'a> RpcEndpoint<'a> {
    fn to_owned(self) -> OwnedRpcEndpoint {
        match self {
            Self::Tcp(authority) => OwnedRpcEndpoint::Tcp(authority.to_string()),
            Self::Unix(path) => OwnedRpcEndpoint::Unix(path.to_string()),
        }
    }
}

#[cfg(feature = "sdk-async")]
impl OwnedRpcEndpoint {
    fn as_ref(&self) -> RpcEndpoint<'_> {
        match self {
            Self::Tcp(authority) => RpcEndpoint::Tcp(authority.as_str()),
            Self::Unix(path) => RpcEndpoint::Unix(path.as_str()),
        }
    }
}

#[cfg(feature = "sdk-async")]
async fn connect_rpc_http_event_stream(
    endpoint: RpcEndpoint<'_>,
    headers: &[(String, String)],
    cursor: Option<&EventCursor>,
    mtls_auth: Option<&MtlsRequestAuth>,
) -> Result<Box<dyn RpcEventStreamIo>, SdkError> {
    let path = match cursor {
        Some(cursor) => format!("/events/stream?cursor={}", cursor.0),
        None => "/events/stream".to_string(),
    };
    let request = RpcBackendClient::build_http_get_with_headers(
        path.as_str(),
        endpoint.host_header(),
        headers,
    );
    match endpoint {
        RpcEndpoint::Tcp(authority) => {
            connect_tcp_rpc_http_event_stream(authority, request.as_slice(), mtls_auth).await
        }
        RpcEndpoint::Unix(_) if mtls_auth.is_some() => Err(SdkError::new(
            code::VALIDATION_INVALID_ARGUMENT,
            ErrorCategory::Validation,
            "mTLS transport auth is not supported over unix RPC endpoints",
        )),
        RpcEndpoint::Unix(path) => {
            connect_unix_rpc_http_event_stream(path, request.as_slice()).await
        }
    }
}

#[cfg(feature = "sdk-async")]
async fn connect_tcp_rpc_http_event_stream(
    authority: &str,
    request: &[u8],
    mtls_auth: Option<&MtlsRequestAuth>,
) -> Result<Box<dyn RpcEventStreamIo>, SdkError> {
    let mut stream = tokio::net::TcpStream::connect(authority)
        .await
        .map_err(|err| SdkError::new(code::INTERNAL, ErrorCategory::Transport, err.to_string()))?;
    if let Some(mtls_auth) = mtls_auth {
        let roots =
            RpcBackendClient::load_root_store(Path::new(mtls_auth.ca_bundle_path.as_str()))?;
        let builder = ClientConfig::builder().with_root_certificates(roots);
        let client_config =
            match (mtls_auth.client_cert_path.as_deref(), mtls_auth.client_key_path.as_deref()) {
                (Some(cert_path), Some(key_path)) => {
                    let cert_chain = RpcBackendClient::load_cert_chain(Path::new(cert_path))?;
                    let private_key = RpcBackendClient::load_private_key(Path::new(key_path))?;
                    builder.with_client_auth_cert(cert_chain, private_key).map_err(|err| {
                        SdkError::new(
                            code::INTERNAL,
                            ErrorCategory::Transport,
                            format!("invalid mtls client certificate/key configuration: {}", err),
                        )
                    })?
                }
                (None, None) => builder.with_no_client_auth(),
                _ => {
                    return Err(SdkError::new(
                        code::SECURITY_AUTH_REQUIRED,
                        ErrorCategory::Security,
                        "mtls client certificate and key paths must be configured together",
                    ))
                }
            };
        let server_name = RpcBackendClient::server_name_for_authority(authority)?;
        let connector = TlsConnector::from(Arc::new(client_config));
        let mut stream = connector.connect(server_name, stream).await.map_err(|err| {
            SdkError::new(
                code::INTERNAL,
                ErrorCategory::Transport,
                format!("failed to start event stream tls connection: {}", err),
            )
        })?;
        stream.write_all(request).await.map_err(|err| {
            SdkError::new(code::INTERNAL, ErrorCategory::Transport, err.to_string())
        })?;
        read_rpc_http_event_header(&mut stream).await?;
        return Ok(Box::new(stream));
    }
    stream
        .write_all(request)
        .await
        .map_err(|err| SdkError::new(code::INTERNAL, ErrorCategory::Transport, err.to_string()))?;
    read_rpc_http_event_header(&mut stream).await?;
    Ok(Box::new(stream))
}

#[cfg(all(feature = "sdk-async", unix))]
async fn connect_unix_rpc_http_event_stream(
    path: &str,
    request: &[u8],
) -> Result<Box<dyn RpcEventStreamIo>, SdkError> {
    let mut stream = tokio::net::UnixStream::connect(path)
        .await
        .map_err(|err| SdkError::new(code::INTERNAL, ErrorCategory::Transport, err.to_string()))?;
    stream
        .write_all(request)
        .await
        .map_err(|err| SdkError::new(code::INTERNAL, ErrorCategory::Transport, err.to_string()))?;
    read_rpc_http_event_header(&mut stream).await?;
    Ok(Box::new(stream))
}

#[cfg(all(feature = "sdk-async", not(unix)))]
async fn connect_unix_rpc_http_event_stream(
    _path: &str,
    _request: &[u8],
) -> Result<Box<dyn RpcEventStreamIo>, SdkError> {
    Err(SdkError::new(
        code::VALIDATION_INVALID_ARGUMENT,
        ErrorCategory::Validation,
        "unix RPC endpoints are not supported on this platform",
    ))
}

#[cfg(feature = "sdk-async")]
async fn read_rpc_http_event_header<S>(stream: &mut S) -> Result<(), SdkError>
where
    S: AsyncRead + Unpin + ?Sized,
{
    let mut header = Vec::with_capacity(512);
    let mut byte = [0_u8; 1];
    while !header.ends_with(b"\r\n\r\n") {
        stream.read_exact(&mut byte).await.map_err(|err| {
            SdkError::new(code::INTERNAL, ErrorCategory::Transport, err.to_string())
        })?;
        header.push(byte[0]);
        if header.len() > 16 * 1024 {
            return Err(SdkError::new(
                code::INTERNAL,
                ErrorCategory::Transport,
                "event stream response header exceeded 16 KiB",
            ));
        }
    }
    if !header.starts_with(b"HTTP/1.1 200") {
        return Err(SdkError::new(
            code::INTERNAL,
            ErrorCategory::Transport,
            "event stream request was rejected",
        ));
    }
    Ok(())
}

#[cfg(feature = "sdk-async")]
async fn read_rpc_http_event_frame<S>(stream: &mut S) -> Result<SdkEvent, SdkError>
where
    S: AsyncRead + Unpin + ?Sized,
{
    let mut frame_len = [0_u8; 4];
    stream
        .read_exact(&mut frame_len)
        .await
        .map_err(|err| SdkError::new(code::INTERNAL, ErrorCategory::Transport, err.to_string()))?;
    let len = u32::from_be_bytes(frame_len) as usize;
    if len > RPC_EVENT_STREAM_MAX_FRAME_BYTES {
        return Err(SdkError::new(
            code::INTERNAL,
            ErrorCategory::Transport,
            format!("event stream frame exceeded {} bytes", RPC_EVENT_STREAM_MAX_FRAME_BYTES),
        ));
    }
    let mut frame = Vec::with_capacity(4 + len);
    frame.extend_from_slice(&frame_len);
    frame.resize(4 + len, 0);
    stream
        .read_exact(&mut frame[4..])
        .await
        .map_err(|err| SdkError::new(code::INTERNAL, ErrorCategory::Transport, err.to_string()))?;
    codec::decode_frame::<SdkEvent>(&frame)
        .map_err(|err| SdkError::new(code::INTERNAL, ErrorCategory::Transport, err.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(feature = "sdk-async")]
    fn test_sdk_event(seq_no: u64) -> SdkEvent {
        SdkEvent {
            event_id: format!("evt-{seq_no}"),
            runtime_id: "rt-test".to_string(),
            stream_id: "sdk-events-v2".to_string(),
            seq_no,
            contract_version: 2,
            ts_ms: seq_no,
            event_type: "RuntimeStateChanged".to_string(),
            severity: Severity::Info,
            source_component: "transport-test".to_string(),
            operation_id: None,
            message_id: None,
            peer_id: None,
            correlation_id: None,
            trace_id: None,
            payload: serde_json::json!({ "to": "running" }),
            extensions: BTreeMap::new(),
        }
    }

    #[cfg(feature = "sdk-async")]
    async fn read_event_stream_request(socket: &mut tokio::net::TcpStream) -> String {
        use tokio::io::AsyncReadExt as _;

        let mut request = Vec::new();
        let mut byte = [0_u8; 1];
        while !request.ends_with(b"\r\n\r\n") {
            socket.read_exact(&mut byte).await.expect("read event stream request");
            request.push(byte[0]);
        }
        String::from_utf8(request).expect("request should be valid utf8")
    }

    #[cfg(feature = "sdk-async")]
    async fn accept_event_stream_request(
        listener: &tokio::net::TcpListener,
        event: SdkEvent,
    ) -> String {
        use tokio::io::AsyncWriteExt as _;

        let (mut socket, _) = listener.accept().await.expect("accept event stream client");
        let request = read_event_stream_request(&mut socket).await;
        socket
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Type: application/msgpack\r\n\r\n")
            .await
            .expect("write response header");
        let frame = codec::encode_frame(&event).expect("encode event frame");
        socket.write_all(&frame).await.expect("write event frame");
        request
    }

    #[cfg(feature = "sdk-async")]
    async fn accept_event_stream_request_with_events(
        listener: &tokio::net::TcpListener,
        events: impl IntoIterator<Item = SdkEvent>,
    ) -> String {
        use tokio::io::AsyncWriteExt as _;

        let (mut socket, _) = listener.accept().await.expect("accept event stream client");
        let request = read_event_stream_request(&mut socket).await;
        socket
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Type: application/msgpack\r\n\r\n")
            .await
            .expect("write response header");
        for event in events {
            let frame = codec::encode_frame(&event).expect("encode event frame");
            socket.write_all(&frame).await.expect("write event frame");
        }
        request
    }

    #[cfg(feature = "sdk-async")]
    #[tokio::test]
    async fn call_rpc_async_uses_async_http_post_transport() {
        use rns_rpc::rpc::{RpcRequest, RpcResponse};
        use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind listener");
        let authority = listener.local_addr().expect("listener address").to_string();

        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.expect("accept async rpc client");
            let mut request = Vec::new();
            let mut byte = [0_u8; 1];
            while !request.ends_with(b"\r\n\r\n") {
                socket.read_exact(&mut byte).await.expect("read header byte");
                request.push(byte[0]);
            }
            let headers = String::from_utf8(request.clone()).expect("headers utf8");
            assert!(headers.starts_with("POST /rpc HTTP/1.1"));
            let content_length = headers
                .lines()
                .find_map(|line| line.strip_prefix("Content-Length: "))
                .and_then(|value| value.trim().parse::<usize>().ok())
                .expect("content length");
            let mut body = vec![0_u8; content_length];
            socket.read_exact(&mut body).await.expect("read rpc body");
            let rpc_request =
                codec::decode_frame::<RpcRequest>(&body).expect("decode async rpc request");
            assert_eq!(rpc_request.method, "probe_async");

            let response = RpcResponse {
                id: rpc_request.id,
                result: Some(serde_json::json!({ "ok": true })),
                error: None,
            };
            let response_frame = codec::encode_frame(&response).expect("encode response");
            let http_response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/msgpack\r\nContent-Length: {}\r\n\r\n",
                response_frame.len()
            );
            socket.write_all(http_response.as_bytes()).await.expect("write response header");
            socket.write_all(&response_frame).await.expect("write response body");
            socket.shutdown().await.expect("shutdown server response");
        });

        let client = RpcBackendClient::new(authority);
        let result = client
            .call_rpc_async("probe_async", Some(serde_json::json!({ "value": 7 })))
            .await
            .expect("async rpc call");
        assert_eq!(result.get("ok").and_then(JsonValue::as_bool), Some(true));
        server.await.expect("server task");
    }

    #[cfg(all(feature = "sdk-async", unix))]
    fn test_unix_socket_path(label: &str) -> std::path::PathBuf {
        let mut path = std::env::temp_dir();
        path.push(format!(
            "lxmf-sdk-{label}-{}-{}.sock",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("unix epoch")
                .as_nanos()
        ));
        path
    }

    #[cfg(all(feature = "sdk-async", unix))]
    #[tokio::test]
    async fn call_rpc_async_supports_unix_socket_endpoint() {
        use rns_rpc::rpc::{RpcRequest, RpcResponse};
        use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

        let path = test_unix_socket_path("rpc");
        let listener = tokio::net::UnixListener::bind(&path).expect("bind unix listener");
        let server = tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.expect("accept unix rpc client");
            let mut request = Vec::new();
            let mut byte = [0_u8; 1];
            while !request.ends_with(b"\r\n\r\n") {
                socket.read_exact(&mut byte).await.expect("read header byte");
                request.push(byte[0]);
            }
            let headers = String::from_utf8(request.clone()).expect("headers utf8");
            assert!(headers.starts_with("POST /rpc HTTP/1.1"));
            assert!(headers.contains("Host: localhost\r\n"));
            let content_length = headers
                .lines()
                .find_map(|line| line.strip_prefix("Content-Length: "))
                .and_then(|value| value.trim().parse::<usize>().ok())
                .expect("content length");
            let mut body = vec![0_u8; content_length];
            socket.read_exact(&mut body).await.expect("read rpc body");
            let rpc_request =
                codec::decode_frame::<RpcRequest>(&body).expect("decode async rpc request");
            assert_eq!(rpc_request.method, "probe_unix_async");

            let response = RpcResponse {
                id: rpc_request.id,
                result: Some(serde_json::json!({ "ok": true })),
                error: None,
            };
            let response_frame = codec::encode_frame(&response).expect("encode response");
            let http_response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/msgpack\r\nContent-Length: {}\r\n\r\n",
                response_frame.len()
            );
            socket.write_all(http_response.as_bytes()).await.expect("write response header");
            socket.write_all(&response_frame).await.expect("write response body");
            socket.shutdown().await.expect("shutdown server response");
        });

        let client = RpcBackendClient::new(format!("unix:{}", path.display()));
        let result = client
            .call_rpc_async("probe_unix_async", Some(serde_json::json!({ "value": 7 })))
            .await
            .expect("async unix rpc call");
        assert_eq!(result.get("ok").and_then(JsonValue::as_bool), Some(true));
        server.await.expect("server task");
        let _ = std::fs::remove_file(path);
    }

    #[cfg(all(feature = "sdk-async", unix))]
    #[tokio::test]
    async fn native_event_stream_supports_unix_socket_endpoint() {
        use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};

        let path = test_unix_socket_path("events");
        let listener = tokio::net::UnixListener::bind(&path).expect("bind unix listener");
        let (tx, mut rx) = mpsc::channel::<Result<SdkEvent, SdkError>>(4);
        let endpoint = format!("unix:{}", path.display());
        let client_task = tokio::spawn(async move {
            run_rpc_http_event_stream(endpoint, Vec::new(), None, None, tx).await;
        });

        let (mut socket, _) = listener.accept().await.expect("accept unix event stream client");
        let mut request = Vec::new();
        let mut byte = [0_u8; 1];
        while !request.ends_with(b"\r\n\r\n") {
            socket.read_exact(&mut byte).await.expect("read event stream request");
            request.push(byte[0]);
        }
        let request = String::from_utf8(request).expect("event stream request utf8");
        assert!(request.starts_with("GET /events/stream HTTP/1.1"));
        assert!(request.contains("Host: localhost\r\n"));
        socket
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Type: application/msgpack\r\n\r\n")
            .await
            .expect("write response header");
        let frame = codec::encode_frame(&test_sdk_event(1)).expect("encode event frame");
        socket.write_all(&frame).await.expect("write event frame");

        let event = tokio::time::timeout(std::time::Duration::from_secs(1), rx.recv())
            .await
            .expect("unix event stream should deliver event")
            .expect("stream should stay open")
            .expect("event should decode");
        assert_eq!(event.seq_no, 1);

        client_task.abort();
        let _ = std::fs::remove_file(path);
    }

    #[cfg(feature = "sdk-async")]
    #[tokio::test]
    async fn native_event_stream_reconnects_with_last_event_cursor() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind listener");
        let authority = listener.local_addr().expect("listener address").to_string();

        let (tx, mut rx) = mpsc::channel::<Result<SdkEvent, SdkError>>(4);
        let endpoint = authority.clone();
        let client_task = tokio::spawn(async move {
            run_rpc_http_event_stream(endpoint, Vec::new(), None, None, tx).await;
        });

        let first_request = accept_event_stream_request(&listener, test_sdk_event(1)).await;
        assert!(first_request.starts_with("GET /events/stream HTTP/1.1"));

        let first = tokio::time::timeout(std::time::Duration::from_secs(1), rx.recv())
            .await
            .expect("first event should arrive")
            .expect("stream should stay open")
            .expect("first event should decode");
        assert_eq!(first.seq_no, 1);

        let second_request = accept_event_stream_request(&listener, test_sdk_event(2)).await;
        assert!(second_request
            .starts_with("GET /events/stream?cursor=v2:rt-test:sdk-events-v2:1 HTTP/1.1"));

        let second = tokio::time::timeout(std::time::Duration::from_secs(1), rx.recv())
            .await
            .expect("second event should arrive")
            .expect("stream should stay open")
            .expect("second event should decode");
        assert_eq!(second.seq_no, 2);

        client_task.abort();
    }

    #[cfg(feature = "sdk-async")]
    #[tokio::test]
    async fn native_event_stream_backpressures_when_consumer_is_slow() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.expect("bind listener");
        let authority = listener.local_addr().expect("listener address").to_string();

        let (tx, mut rx) = mpsc::channel::<Result<SdkEvent, SdkError>>(1);
        let endpoint = authority.clone();
        let client_task = tokio::spawn(async move {
            run_rpc_http_event_stream(endpoint, Vec::new(), None, None, tx).await;
        });

        let first_request = accept_event_stream_request_with_events(
            &listener,
            [test_sdk_event(1), test_sdk_event(2), test_sdk_event(3)],
        )
        .await;
        assert!(first_request.starts_with("GET /events/stream HTTP/1.1"));

        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(150), listener.accept())
                .await
                .is_err(),
            "bounded channel should stall the reader before it reconnects"
        );

        let first = rx.recv().await.expect("first queued event").expect("first event");
        assert_eq!(first.seq_no, 1);
        let second = rx.recv().await.expect("second queued event").expect("second event");
        assert_eq!(second.seq_no, 2);

        client_task.abort();
    }

    #[cfg(feature = "sdk-async")]
    #[tokio::test]
    async fn native_event_stream_task_aborts_when_receiver_stream_is_dropped() {
        struct DropNotify(Option<tokio::sync::oneshot::Sender<()>>);

        impl Drop for DropNotify {
            fn drop(&mut self) {
                if let Some(tx) = self.0.take() {
                    let _ = tx.send(());
                }
            }
        }

        let (_tx, rx) = mpsc::channel::<Result<SdkEvent, SdkError>>(1);
        let (dropped_tx, dropped_rx) = tokio::sync::oneshot::channel();
        let task = tokio::spawn(async move {
            let _notify = DropNotify(Some(dropped_tx));
            std::future::pending::<()>().await;
        });

        let stream = AbortOnDropStream::new(ReceiverStream::new(rx), task);
        tokio::task::yield_now().await;
        drop(stream);

        tokio::time::timeout(std::time::Duration::from_secs(1), dropped_rx)
            .await
            .expect("background stream task should abort on drop")
            .expect("drop notification should be delivered");
    }

    #[cfg(feature = "sdk-async")]
    #[tokio::test]
    async fn native_event_stream_rejects_oversized_frame_before_allocation() {
        let len = (RPC_EVENT_STREAM_MAX_FRAME_BYTES as u32) + 1;
        let bytes = len.to_be_bytes();
        let mut stream = &bytes[..];

        let err = read_rpc_http_event_frame(&mut stream)
            .await
            .expect_err("oversized frame should fail before payload allocation");
        assert_eq!(err.category, ErrorCategory::Transport);
        assert!(err.message.contains("event stream frame exceeded"));
    }

    #[cfg(feature = "sdk-async")]
    #[tokio::test]
    async fn open_event_stream_uses_native_stream_for_mtls_auth() {
        let client = RpcBackendClient::new("127.0.0.1:9");
        {
            let mut auth = client.session_auth.write().expect("session auth");
            *auth = SessionAuth::Mtls {
                ca_bundle_path: "/definitely/missing/ca.pem".to_string(),
                client_cert_path: None,
                client_key_path: None,
            };
        }

        let stream = client
            .open_event_stream_impl(&EventSubscription {
                start: SubscriptionStart::Head,
                cursor: None,
            })
            .expect("stream creation should not fall back for mtls");

        assert!(stream.is_some(), "mTLS sessions should use the native stream connector");
    }

    #[test]
    fn zeroize_header_values_clears_sensitive_header_contents() {
        let mut headers = vec![
            ("Authorization".to_string(), "Bearer super-secret-token".to_string()),
            ("X-Correlation-Id".to_string(), "trace-123".to_string()),
        ];

        RpcBackendClient::zeroize_header_values(headers.as_mut_slice());

        assert!(headers.iter().all(|(_, value)| value.is_empty()));
    }

    #[test]
    fn mtls_for_session_auth_returns_mtls_paths_only() {
        let mtls_auth = SessionAuth::Mtls {
            ca_bundle_path: "/tmp/ca.pem".to_string(),
            client_cert_path: Some("/tmp/client.pem".to_string()),
            client_key_path: Some("/tmp/client.key".to_string()),
        };
        let extracted =
            RpcBackendClient::mtls_for_session_auth(&mtls_auth).expect("mtls config expected");
        assert_eq!(extracted.ca_bundle_path, "/tmp/ca.pem");
        assert_eq!(extracted.client_cert_path.as_deref(), Some("/tmp/client.pem"));
        assert_eq!(extracted.client_key_path.as_deref(), Some("/tmp/client.key"));

        assert!(RpcBackendClient::mtls_for_session_auth(&SessionAuth::LocalTrusted).is_none());
        assert!(RpcBackendClient::mtls_for_session_auth(&SessionAuth::Token {
            issuer: "issuer".to_string(),
            audience: "audience".to_string(),
            shared_secret: Zeroizing::new("secret".to_string()),
            ttl_secs: 60,
        })
        .is_none());
    }
}
