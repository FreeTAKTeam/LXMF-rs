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
        Self::read_http_response_to_end(&mut stream)
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
        Self::read_http_response_to_end(&mut stream)
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
        Self::read_http_response_to_end(&mut tls)
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
        Self::read_http_response_to_end_async(&mut stream).await
    }

    #[cfg(all(feature = "sdk-async", unix))]
    async fn send_unix_request_async(path: &str, request: &[u8]) -> Result<Vec<u8>, SdkError> {
        let mut stream = tokio::net::UnixStream::connect(path).await.map_err(|err| {
            SdkError::new(code::INTERNAL, ErrorCategory::Transport, err.to_string())
        })?;
        stream.write_all(request).await.map_err(|err| {
            SdkError::new(code::INTERNAL, ErrorCategory::Transport, err.to_string())
        })?;
        Self::read_http_response_to_end_async(&mut stream).await
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
        Self::read_http_response_to_end_async(&mut stream).await
    }

    fn read_http_response_to_end<R>(reader: &mut R) -> Result<Vec<u8>, SdkError>
    where
        R: Read,
    {
        let mut response = Vec::new();
        let mut limited = reader.take((RPC_HTTP_RESPONSE_MAX_BYTES + 1) as u64);
        limited.read_to_end(&mut response).map_err(|err| {
            SdkError::new(code::INTERNAL, ErrorCategory::Transport, err.to_string())
        })?;
        if response.len() > RPC_HTTP_RESPONSE_MAX_BYTES {
            return Err(SdkError::new(
                code::INTERNAL,
                ErrorCategory::Transport,
                format!("rpc response exceeded {} bytes", RPC_HTTP_RESPONSE_MAX_BYTES),
            ));
        }
        Ok(response)
    }

    #[cfg(feature = "sdk-async")]
    async fn read_http_response_to_end_async<R>(reader: &mut R) -> Result<Vec<u8>, SdkError>
    where
        R: AsyncRead + Unpin,
    {
        let mut response = Vec::new();
        let mut chunk = [0_u8; 8192];
        loop {
            let read = reader.read(&mut chunk).await.map_err(|err| {
                SdkError::new(code::INTERNAL, ErrorCategory::Transport, err.to_string())
            })?;
            if read == 0 {
                break;
            }
            response.extend_from_slice(&chunk[..read]);
            if response.len() > RPC_HTTP_RESPONSE_MAX_BYTES {
                return Err(SdkError::new(
                    code::INTERNAL,
                    ErrorCategory::Transport,
                    format!("rpc response exceeded {} bytes", RPC_HTTP_RESPONSE_MAX_BYTES),
                ));
            }
        }
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
}
