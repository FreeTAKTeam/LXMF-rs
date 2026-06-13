impl SdkConfig {
    pub fn desktop_local_default() -> Self {
        Self {
            profile: Profile::DesktopLocalRuntime,
            bind_mode: BindMode::LocalOnly,
            auth_mode: AuthMode::LocalTrusted,
            overflow_policy: OverflowPolicy::Reject,
            block_timeout_ms: None,
            store_forward: default_store_forward(&Profile::DesktopLocalRuntime),
            event_stream: default_event_stream(&Profile::DesktopLocalRuntime),
            event_sink: default_event_sink(&Profile::DesktopLocalRuntime),
            idempotency_ttl_ms: 43_200_000,
            redaction: default_redaction(),
            rpc_backend: Some(default_rpc_backend(DEFAULT_RPC_LISTEN_ADDR)),
            extensions: BTreeMap::new(),
        }
    }

    pub fn desktop_full_default() -> Self {
        Self {
            profile: Profile::DesktopFull,
            bind_mode: BindMode::LocalOnly,
            auth_mode: AuthMode::LocalTrusted,
            overflow_policy: OverflowPolicy::Reject,
            block_timeout_ms: None,
            store_forward: default_store_forward(&Profile::DesktopFull),
            event_stream: default_event_stream(&Profile::DesktopFull),
            event_sink: default_event_sink(&Profile::DesktopFull),
            idempotency_ttl_ms: 86_400_000,
            redaction: default_redaction(),
            rpc_backend: Some(default_rpc_backend(DEFAULT_RPC_LISTEN_ADDR)),
            extensions: BTreeMap::new(),
        }
    }

    pub fn embedded_alloc_default() -> Self {
        Self {
            profile: Profile::EmbeddedAlloc,
            bind_mode: BindMode::LocalOnly,
            auth_mode: AuthMode::LocalTrusted,
            overflow_policy: OverflowPolicy::Reject,
            block_timeout_ms: None,
            store_forward: default_store_forward(&Profile::EmbeddedAlloc),
            event_stream: default_event_stream(&Profile::EmbeddedAlloc),
            event_sink: default_event_sink(&Profile::EmbeddedAlloc),
            idempotency_ttl_ms: 7_200_000,
            redaction: default_redaction(),
            rpc_backend: Some(RpcBackendConfig {
                listen_addr: DEFAULT_RPC_LISTEN_ADDR.to_owned(),
                read_timeout_ms: 2_000,
                write_timeout_ms: 2_000,
                max_header_bytes: 8_192,
                max_body_bytes: 65_536,
                token_auth: None,
                mtls_auth: None,
            }),
            extensions: BTreeMap::new(),
        }
    }

    pub fn with_rpc_listen_addr(mut self, listen_addr: impl Into<String>) -> Self {
        let listen_addr = listen_addr.into();
        match self.rpc_backend.as_mut() {
            Some(backend) => backend.listen_addr = listen_addr,
            None => self.rpc_backend = Some(default_rpc_backend(listen_addr)),
        }
        self
    }

    pub fn with_token_auth(
        mut self,
        issuer: impl Into<String>,
        audience: impl Into<String>,
        shared_secret: impl Into<String>,
    ) -> Self {
        self.bind_mode = BindMode::Remote;
        self.auth_mode = AuthMode::Token;
        let backend =
            self.rpc_backend.get_or_insert_with(|| default_rpc_backend(DEFAULT_RPC_LISTEN_ADDR));
        backend.mtls_auth = None;
        backend.token_auth = Some(TokenAuthConfig {
            issuer: issuer.into(),
            audience: audience.into(),
            jti_cache_ttl_ms: 60_000,
            clock_skew_ms: 5_000,
            shared_secret: shared_secret.into(),
        });
        self
    }

    pub fn with_mtls_auth(mut self, ca_bundle_path: impl Into<String>) -> Self {
        self.bind_mode = BindMode::Remote;
        self.auth_mode = AuthMode::Mtls;
        let backend =
            self.rpc_backend.get_or_insert_with(|| default_rpc_backend(DEFAULT_RPC_LISTEN_ADDR));
        backend.token_auth = None;
        backend.mtls_auth = Some(MtlsAuthConfig {
            ca_bundle_path: ca_bundle_path.into(),
            require_client_cert: false,
            allowed_san: None,
            client_cert_path: None,
            client_key_path: None,
        });
        self
    }

    pub fn with_mtls_client_credentials(
        mut self,
        client_cert_path: impl Into<String>,
        client_key_path: impl Into<String>,
    ) -> Self {
        self.bind_mode = BindMode::Remote;
        self.auth_mode = AuthMode::Mtls;
        let mtls =
            self.rpc_backend.get_or_insert_with(|| default_rpc_backend(DEFAULT_RPC_LISTEN_ADDR));
        mtls.token_auth = None;
        if mtls.mtls_auth.is_none() {
            mtls.mtls_auth = Some(MtlsAuthConfig {
                ca_bundle_path: "ca.pem".to_owned(),
                require_client_cert: true,
                allowed_san: None,
                client_cert_path: None,
                client_key_path: None,
            });
        }
        let auth = mtls.mtls_auth.as_mut().expect("mtls auth");
        auth.require_client_cert = true;
        auth.client_cert_path = Some(client_cert_path.into());
        auth.client_key_path = Some(client_key_path.into());
        self
    }

    pub fn with_store_forward_limits(
        mut self,
        max_messages: usize,
        max_message_age_ms: u64,
    ) -> Self {
        self.store_forward.max_messages = max_messages;
        self.store_forward.max_message_age_ms = max_message_age_ms;
        self
    }

    pub fn with_store_forward_policy(
        mut self,
        capacity_policy: StoreForwardCapacityPolicy,
        eviction_priority: StoreForwardEvictionPriority,
    ) -> Self {
        self.store_forward.capacity_policy = capacity_policy;
        self.store_forward.eviction_priority = eviction_priority;
        self
    }

    pub fn with_event_sink(
        mut self,
        enabled: bool,
        max_event_bytes: usize,
        allow_kinds: Vec<EventSinkKind>,
    ) -> Self {
        self.event_sink.enabled = enabled;
        self.event_sink.max_event_bytes = max_event_bytes;
        self.event_sink.allow_kinds = allow_kinds;
        self
    }

    pub fn validate(&self) -> Result<(), SdkError> {
        if self.overflow_policy == OverflowPolicy::Block && self.block_timeout_ms.is_none() {
            return Err(SdkError::new(
                code::VALIDATION_INVALID_ARGUMENT,
                ErrorCategory::Validation,
                "overflow_policy=block requires block_timeout_ms",
            )
            .with_user_actionable(true)
            .with_detail("field", JsonValue::String("block_timeout_ms".to_owned())));
        }

        if self.event_stream.max_extension_keys > 32 {
            return Err(SdkError::new(
                code::VALIDATION_MAX_EXTENSION_KEYS_EXCEEDED,
                ErrorCategory::Validation,
                "event stream extension key limit exceeds contract maximum",
            )
            .with_user_actionable(true)
            .with_detail("limit_name", JsonValue::String("max_extension_keys".to_owned()))
            .with_detail("limit_value", JsonValue::from(self.event_stream.max_extension_keys)));
        }

        if !(256..=2_097_152).contains(&self.event_sink.max_event_bytes) {
            return Err(SdkError::new(
                code::VALIDATION_INVALID_ARGUMENT,
                ErrorCategory::Validation,
                "event_sink.max_event_bytes must be in the range 256..=2097152",
            )
            .with_user_actionable(true)
            .with_detail("field", JsonValue::String("event_sink.max_event_bytes".to_owned())));
        }

        if self.event_sink.allow_kinds.is_empty() {
            return Err(SdkError::new(
                code::VALIDATION_INVALID_ARGUMENT,
                ErrorCategory::Validation,
                "event_sink.allow_kinds must include at least one kind",
            )
            .with_user_actionable(true)
            .with_detail("field", JsonValue::String("event_sink.allow_kinds".to_owned())));
        }

        if self.event_sink.enabled && !self.redaction.enabled {
            return Err(SdkError::new(
                code::SECURITY_REDACTION_REQUIRED,
                ErrorCategory::Security,
                "event sink requires redaction.enabled=true",
            )
            .with_user_actionable(true)
            .with_detail("field", JsonValue::String("redaction.enabled".to_owned())));
        }

        if self.store_forward.max_messages == 0 {
            return Err(SdkError::new(
                code::VALIDATION_INVALID_ARGUMENT,
                ErrorCategory::Validation,
                "store_forward.max_messages must be greater than zero",
            )
            .with_user_actionable(true)
            .with_detail("field", JsonValue::String("store_forward.max_messages".to_owned())));
        }

        if self.store_forward.max_message_age_ms == 0 {
            return Err(SdkError::new(
                code::VALIDATION_INVALID_ARGUMENT,
                ErrorCategory::Validation,
                "store_forward.max_message_age_ms must be greater than zero",
            )
            .with_user_actionable(true)
            .with_detail(
                "field",
                JsonValue::String("store_forward.max_message_age_ms".to_owned()),
            ));
        }

        match self.bind_mode {
            BindMode::LocalOnly => {
                if self.auth_mode != AuthMode::LocalTrusted {
                    return Err(SdkError::new(
                        code::SECURITY_AUTH_REQUIRED,
                        ErrorCategory::Security,
                        "local_only bind mode requires local_trusted auth mode",
                    )
                    .with_user_actionable(true));
                }
            }
            BindMode::Remote => {
                if !matches!(self.auth_mode, AuthMode::Token | AuthMode::Mtls) {
                    return Err(SdkError::new(
                        code::SECURITY_REMOTE_BIND_DISALLOWED,
                        ErrorCategory::Security,
                        "remote bind mode requires token or mtls auth mode",
                    )
                    .with_user_actionable(true));
                }
            }
        }

        match self.auth_mode {
            AuthMode::LocalTrusted => {}
            AuthMode::Token => {
                let token_auth =
                    self.rpc_backend.as_ref().and_then(|backend| backend.token_auth.as_ref());
                let Some(token_auth) = token_auth else {
                    return Err(SdkError::new(
                        code::SECURITY_AUTH_REQUIRED,
                        ErrorCategory::Security,
                        "token auth mode requires rpc_backend.token_auth configuration",
                    )
                    .with_user_actionable(true));
                };
                if token_auth.issuer.trim().is_empty() || token_auth.audience.trim().is_empty() {
                    return Err(SdkError::new(
                        code::VALIDATION_INVALID_ARGUMENT,
                        ErrorCategory::Validation,
                        "token auth configuration requires issuer and audience",
                    )
                    .with_user_actionable(true));
                }
                if token_auth.jti_cache_ttl_ms == 0 {
                    return Err(SdkError::new(
                        code::VALIDATION_INVALID_ARGUMENT,
                        ErrorCategory::Validation,
                        "token auth jti_cache_ttl_ms must be greater than zero",
                    )
                    .with_user_actionable(true));
                }
                if token_auth.shared_secret.trim().is_empty() {
                    return Err(SdkError::new(
                        code::SECURITY_AUTH_REQUIRED,
                        ErrorCategory::Security,
                        "token auth shared_secret must be configured",
                    )
                    .with_user_actionable(true));
                }
            }
            AuthMode::Mtls => {
                if self.profile == Profile::EmbeddedAlloc {
                    return Err(SdkError::new(
                        code::VALIDATION_INVALID_ARGUMENT,
                        ErrorCategory::Validation,
                        "embedded-alloc profile does not support mtls auth mode",
                    )
                    .with_user_actionable(true));
                }
                let mtls_auth =
                    self.rpc_backend.as_ref().and_then(|backend| backend.mtls_auth.as_ref());
                let Some(mtls_auth) = mtls_auth else {
                    return Err(SdkError::new(
                        code::SECURITY_AUTH_REQUIRED,
                        ErrorCategory::Security,
                        "mtls auth mode requires rpc_backend.mtls_auth configuration",
                    )
                    .with_user_actionable(true));
                };
                if mtls_auth.ca_bundle_path.trim().is_empty() {
                    return Err(SdkError::new(
                        code::VALIDATION_INVALID_ARGUMENT,
                        ErrorCategory::Validation,
                        "mtls auth mode requires non-empty rpc_backend.mtls_auth.ca_bundle_path",
                    )
                    .with_user_actionable(true));
                }
                let client_cert_path = mtls_auth
                    .client_cert_path
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty());
                let client_key_path = mtls_auth
                    .client_key_path
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty());
                if client_cert_path.is_some() ^ client_key_path.is_some() {
                    return Err(SdkError::new(
                        code::VALIDATION_INVALID_ARGUMENT,
                        ErrorCategory::Validation,
                        "mtls client certificate and key paths must be configured together",
                    )
                    .with_user_actionable(true));
                }
                if mtls_auth.require_client_cert
                    && (client_cert_path.is_none() || client_key_path.is_none())
                {
                    return Err(SdkError::new(
                        code::SECURITY_AUTH_REQUIRED,
                        ErrorCategory::Security,
                        "mtls auth mode with require_client_cert=true requires client_cert_path and client_key_path",
                    )
                    .with_user_actionable(true));
                }
            }
        }

        Ok(())
    }
}
