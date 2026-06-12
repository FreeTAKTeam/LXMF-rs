impl RpcBackendClient {

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
        let auth = {
            let auth_guard = self.session_auth.read().expect("session_auth rwlock poisoned");
            EventStreamRequestAuth::from_session_auth(&auth_guard, self.next_request_id())
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
            run_rpc_http_event_stream(endpoint, auth, cursor, tx).await;
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
