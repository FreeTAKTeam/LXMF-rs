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
    mut auth: EventStreamRequestAuth,
    mut cursor: Option<EventCursor>,
    tx: mpsc::Sender<Result<SdkEvent, SdkError>>,
) {
    let mut last_delivered = cursor.as_ref().and_then(EventStreamPosition::from_cursor);
    let mut had_connected_stream = false;
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
            cursor.as_ref(),
            &mut auth,
        )
        .await
        {
            Ok(stream) => stream,
            Err(err) => {
                if had_connected_stream && err.category == ErrorCategory::Transport {
                    tokio::time::sleep(std::time::Duration::from_millis(250)).await;
                    continue;
                }
                let _ = tx.send(Err(err)).await;
                return;
            }
        };
        had_connected_stream = true;
        loop {
            match read_rpc_http_event_frame(&mut stream).await {
                Ok(event) => {
                    if last_delivered.as_ref().is_some_and(|position| position.covers(&event)) {
                        continue;
                    }
                    cursor = Some(EventCursor(format!(
                        "v2:{}:{}:{}",
                        event.runtime_id, event.stream_id, event.seq_no
                    )));
                    last_delivered = Some(EventStreamPosition::from_event(&event));
                    if tx.send(Ok(event)).await.is_err() {
                        return;
                    }
                }
                Err(err) if err.category != ErrorCategory::Transport => {
                    let _ = tx.send(Err(err)).await;
                    return;
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
enum EventStreamRequestAuth {
    LocalTrusted,
    Token {
        issuer: String,
        audience: String,
        shared_secret: Zeroizing<String>,
        ttl_secs: u64,
        stream_id: u64,
        next_jti: u64,
    },
    Mtls {
        ca_bundle_path: String,
        client_cert_path: Option<String>,
        client_key_path: Option<String>,
    },
}

#[cfg(feature = "sdk-async")]
impl EventStreamRequestAuth {
    fn from_session_auth(auth: &SessionAuth, stream_id: u64) -> Self {
        match auth {
            SessionAuth::LocalTrusted => Self::LocalTrusted,
            SessionAuth::Token { issuer, audience, shared_secret, ttl_secs } => Self::Token {
                issuer: issuer.clone(),
                audience: audience.clone(),
                shared_secret: Zeroizing::new(shared_secret.as_str().to_string()),
                ttl_secs: *ttl_secs,
                stream_id,
                next_jti: 0,
            },
            SessionAuth::Mtls { ca_bundle_path, client_cert_path, client_key_path } => Self::Mtls {
                ca_bundle_path: ca_bundle_path.clone(),
                client_cert_path: client_cert_path.clone(),
                client_key_path: client_key_path.clone(),
            },
        }
    }

    fn headers(&mut self) -> Vec<(String, String)> {
        match self {
            Self::LocalTrusted | Self::Mtls { .. } => Vec::new(),
            Self::Token { issuer, audience, shared_secret, ttl_secs, stream_id, next_jti } => {
                let jti = format!("sdk-stream-jti-{stream_id}-{next_jti}");
                *next_jti = next_jti.saturating_add(1);
                let iat = RpcBackendClient::now_seconds();
                let exp = iat.saturating_add(*ttl_secs);
                let payload = Zeroizing::new(format!(
                    "iss={issuer};aud={audience};jti={jti};sub=sdk-client;iat={iat};exp={exp}"
                ));
                let sig = Zeroizing::new(RpcBackendClient::token_signature(
                    shared_secret.as_str(),
                    payload.as_str(),
                ));
                let token = Zeroizing::new(format!("{};sig={}", payload.as_str(), sig.as_str()));
                vec![("Authorization".to_owned(), format!("Bearer {}", token.as_str()))]
            }
        }
    }

    fn mtls_auth(&self) -> Option<MtlsRequestAuth> {
        match self {
            Self::Mtls { ca_bundle_path, client_cert_path, client_key_path } => {
                Some(MtlsRequestAuth {
                    ca_bundle_path: ca_bundle_path.clone(),
                    client_cert_path: client_cert_path.clone(),
                    client_key_path: client_key_path.clone(),
                })
            }
            Self::LocalTrusted | Self::Token { .. } => None,
        }
    }
}

#[cfg(feature = "sdk-async")]
struct EventStreamPosition {
    runtime_id: String,
    stream_id: String,
    seq_no: u64,
}

#[cfg(feature = "sdk-async")]
impl EventStreamPosition {
    fn from_cursor(cursor: &EventCursor) -> Option<Self> {
        let body = cursor.0.strip_prefix("v2:")?;
        let (runtime_id, rest) = body.split_once(':')?;
        let (stream_id, seq_no) = rest.rsplit_once(':')?;
        Some(Self {
            runtime_id: runtime_id.to_string(),
            stream_id: stream_id.to_string(),
            seq_no: seq_no.parse().ok()?,
        })
    }

    fn from_event(event: &SdkEvent) -> Self {
        Self {
            runtime_id: event.runtime_id.clone(),
            stream_id: event.stream_id.clone(),
            seq_no: event.seq_no,
        }
    }

    fn covers(&self, event: &SdkEvent) -> bool {
        self.runtime_id == event.runtime_id
            && self.stream_id == event.stream_id
            && self.seq_no >= event.seq_no
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
    cursor: Option<&EventCursor>,
    auth: &mut EventStreamRequestAuth,
) -> Result<Box<dyn RpcEventStreamIo>, SdkError> {
    let path = match cursor {
        Some(cursor) => format!("/events/stream?cursor={}", cursor.0),
        None => "/events/stream".to_string(),
    };
    let mut headers = auth.headers();
    let mut request = RpcBackendClient::build_http_get_with_headers(
        path.as_str(),
        endpoint.host_header(),
        &headers,
    );
    RpcBackendClient::zeroize_header_values(&mut headers);
    let mtls_auth = auth.mtls_auth();
    let result = match endpoint {
        RpcEndpoint::Tcp(authority) => {
            connect_tcp_rpc_http_event_stream(authority, request.as_slice(), mtls_auth.as_ref())
                .await
        }
        RpcEndpoint::Unix(_) if mtls_auth.is_some() => Err(SdkError::new(
            code::VALIDATION_INVALID_ARGUMENT,
            ErrorCategory::Validation,
            "mTLS transport auth is not supported over unix RPC endpoints",
        )),
        RpcEndpoint::Unix(path) => {
            connect_unix_rpc_http_event_stream(path, request.as_slice()).await
        }
    };
    request.zeroize();
    result
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
        if let Some(error) = read_event_stream_rejection_error(stream, header.as_slice()).await? {
            return Err(error);
        }
        return Err(SdkError::new(
            code::INTERNAL,
            ErrorCategory::Transport,
            "event stream request was rejected",
        ));
    }
    Ok(())
}

#[cfg(feature = "sdk-async")]
async fn read_event_stream_rejection_error<S>(
    stream: &mut S,
    header: &[u8],
) -> Result<Option<SdkError>, SdkError>
where
    S: AsyncRead + Unpin + ?Sized,
{
    let Some(content_length) = http::parse_content_length(header) else {
        return Ok(None);
    };
    if content_length == 0 {
        return Ok(None);
    }
    if content_length > RPC_EVENT_STREAM_MAX_FRAME_BYTES {
        return Err(SdkError::new(
            code::INTERNAL,
            ErrorCategory::Transport,
            format!(
                "event stream rejection body exceeded {} bytes",
                RPC_EVENT_STREAM_MAX_FRAME_BYTES
            ),
        ));
    }
    let mut body = vec![0_u8; content_length];
    stream
        .read_exact(&mut body)
        .await
        .map_err(|err| SdkError::new(code::INTERNAL, ErrorCategory::Transport, err.to_string()))?;
    let Ok(rpc_response) = codec::decode_frame::<RpcResponse>(&body) else {
        return Ok(None);
    };
    Ok(rpc_response.error.map(RpcBackendClient::map_rpc_error))
}
