impl RpcDaemon {
    fn response_meta(&self) -> JsonValue {
        let profile = self.sdk_profile.lock().expect("sdk_profile mutex poisoned").clone();
        json!({
            "contract_version": format!("v{}", self.active_contract_version()),
            "profile": profile,
            "rpc_endpoint": JsonValue::Null,
        })
    }

    #[allow(clippy::result_large_err)]
    pub fn authorize_http_request(
        &self,
        headers: &[(String, String)],
        peer_ip: Option<&str>,
    ) -> Result<(), RpcError> {
        self.authorize_http_request_with_transport(headers, peer_ip, None)
    }

    #[allow(clippy::result_large_err)]
    pub fn authorize_http_request_with_transport(
        &self,
        headers: &[(String, String)],
        peer_ip: Option<&str>,
        transport_auth: Option<&crate::rpc::http::TransportAuthContext>,
    ) -> Result<(), RpcError> {
        let auth_started = std::time::Instant::now();
        let result: Result<(), RpcError> = (|| {
            let (trust_forwarded, trusted_proxy_ips, bind_mode, auth_mode) = {
                let config_guard =
                    self.sdk_runtime_config.lock().expect("sdk_runtime_config mutex poisoned");
                let trust_forwarded = config_guard
                    .get("extensions")
                    .and_then(|value| value.get("trusted_proxy"))
                    .and_then(JsonValue::as_bool)
                    .unwrap_or(false);
                let trusted_proxy_ips = config_guard
                    .get("extensions")
                    .and_then(|value| value.get("trusted_proxy_ips"))
                    .and_then(JsonValue::as_array)
                    .map(|entries| {
                        entries
                            .iter()
                            .filter_map(JsonValue::as_str)
                            .map(str::trim)
                            .filter(|entry| !entry.is_empty())
                            .map(str::to_string)
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default();
                let bind_mode = config_guard
                    .get("bind_mode")
                    .and_then(JsonValue::as_str)
                    .unwrap_or("local_only")
                    .to_string();
                let auth_mode = config_guard
                    .get("auth_mode")
                    .and_then(JsonValue::as_str)
                    .unwrap_or("local_trusted")
                    .to_string();
                (trust_forwarded, trusted_proxy_ips, bind_mode, auth_mode)
            };
            let peer_ip =
                peer_ip.map(str::trim).filter(|value| !value.is_empty()).map(str::to_string);
            let peer_is_trusted_proxy = peer_ip
                .as_deref()
                .is_some_and(|ip| trusted_proxy_ips.iter().any(|trusted| trusted == ip));
            let allow_forwarded = trust_forwarded && peer_is_trusted_proxy;

            let source_ip = if allow_forwarded {
                Self::header_value(headers, "x-forwarded-for")
                    .or_else(|| Self::header_value(headers, "x-real-ip"))
                .or(peer_ip.as_deref())
                .map(|value| value.split(',').next().unwrap_or(value).trim().to_string())
            } else {
                peer_ip
            }
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| "unknown".to_string());

            if bind_mode == "local_only" && !Self::is_loopback_source(source_ip.as_str()) {
                return Err(RpcError::new(
                    "SDK_SECURITY_REMOTE_BIND_DISALLOWED".to_string(),
                    "remote source is not allowed in local_only bind mode".to_string(),
                ));
            }

            let mut principal = "local".to_string();
            match auth_mode.as_str() {
                "local_trusted" => {}
                "token" => {
                    let auth_header = Self::header_value(headers, "authorization").ok_or_else(
                        || {
                            RpcError::new(
                                "SDK_SECURITY_AUTH_REQUIRED".to_string(),
                                "authorization header is required".to_string(),
                            )
                        },
                    )?;
                    let token = auth_header
                        .strip_prefix("Bearer ")
                        .or_else(|| auth_header.strip_prefix("bearer "))
                        .ok_or_else(|| {
                            RpcError::new(
                                "SDK_SECURITY_TOKEN_INVALID".to_string(),
                                "authorization header must use Bearer token format".to_string(),
                            )
                        })?;
                    let claims = Self::parse_token_claims(token).ok_or_else(|| {
                        RpcError::new(
                            "SDK_SECURITY_TOKEN_INVALID".to_string(),
                            "token claims are malformed".to_string(),
                        )
                    })?;
                    let (
                        expected_issuer,
                        expected_audience,
                        jti_ttl_ms,
                        clock_skew_secs,
                        shared_secret,
                    ) = self.sdk_token_auth_config().ok_or_else(|| {
                        RpcError::new(
                            "SDK_SECURITY_AUTH_REQUIRED".to_string(),
                            "token auth mode requires token auth configuration".to_string(),
                        )
                    })?;
                    let issuer = claims.get("iss").map(String::as_str).ok_or_else(|| {
                        RpcError::new(
                            "SDK_SECURITY_TOKEN_INVALID".to_string(),
                            "token issuer claim is missing".to_string(),
                        )
                    })?;
                    let audience = claims.get("aud").map(String::as_str).ok_or_else(|| {
                        RpcError::new(
                            "SDK_SECURITY_TOKEN_INVALID".to_string(),
                            "token audience claim is missing".to_string(),
                        )
                    })?;
                    let jti = claims.get("jti").cloned().ok_or_else(|| {
                        RpcError::new(
                            "SDK_SECURITY_TOKEN_INVALID".to_string(),
                            "token jti claim is missing".to_string(),
                        )
                    })?;
                    let subject =
                        claims.get("sub").cloned().unwrap_or_else(|| "sdk-client".to_string());
                    let iat = claims
                        .get("iat")
                        .and_then(|value| value.parse::<u64>().ok())
                        .ok_or_else(|| {
                            RpcError::new(
                                "SDK_SECURITY_TOKEN_INVALID".to_string(),
                                "token iat claim is missing or invalid".to_string(),
                            )
                        })?;
                    let exp = claims
                        .get("exp")
                        .and_then(|value| value.parse::<u64>().ok())
                        .ok_or_else(|| {
                            RpcError::new(
                                "SDK_SECURITY_TOKEN_INVALID".to_string(),
                                "token exp claim is missing or invalid".to_string(),
                            )
                        })?;
                    let signature = claims.get("sig").map(String::as_str).ok_or_else(|| {
                        RpcError::new(
                            "SDK_SECURITY_TOKEN_INVALID".to_string(),
                            "token signature is missing".to_string(),
                        )
                    })?;
                    let signed_payload = zeroize::Zeroizing::new(format!(
                        "iss={issuer};aud={audience};jti={jti};sub={subject};iat={iat};exp={exp}"
                    ));
                    let expected_signature = zeroize::Zeroizing::new(
                        Self::token_signature(shared_secret.as_str(), signed_payload.as_str())
                            .ok_or_else(|| {
                                RpcError::new(
                                    "SDK_SECURITY_TOKEN_INVALID".to_string(),
                                    "token signature verification failed".to_string(),
                                )
                            })?,
                    );
                    if signature != expected_signature.as_str() {
                        return Err(RpcError::new(
                            "SDK_SECURITY_TOKEN_INVALID".to_string(),
                            "token signature does not match runtime policy".to_string(),
                        ));
                    }
                    if issuer != expected_issuer || audience != expected_audience {
                        return Err(RpcError::new(
                            "SDK_SECURITY_TOKEN_INVALID".to_string(),
                            "token issuer/audience does not match runtime policy".to_string(),
                        ));
                    }
                    let now_seconds = now_seconds_u64();
                    if iat > now_seconds.saturating_add(clock_skew_secs) {
                        return Err(RpcError::new(
                            "SDK_SECURITY_TOKEN_INVALID".to_string(),
                            "token iat is outside accepted clock skew".to_string(),
                        ));
                    }
                    if exp.saturating_add(clock_skew_secs) < now_seconds {
                        return Err(RpcError::new(
                            "SDK_SECURITY_TOKEN_INVALID".to_string(),
                            "token has expired".to_string(),
                        ));
                    }
                    principal = subject;
                    let now = now_millis_u64();
                    let mut replay_cache =
                        self.sdk_seen_jti.lock().expect("sdk_seen_jti mutex poisoned");
                    replay_cache.retain(|_, expires_at| *expires_at > now);
                    if replay_cache.contains_key(jti.as_str()) {
                        return Err(RpcError::new(
                            "SDK_SECURITY_TOKEN_REPLAYED".to_string(),
                            "token jti has already been used".to_string(),
                        ));
                    }
                    replay_cache.insert(jti, now.saturating_add(jti_ttl_ms.max(1)));
                }
                "mtls" => {
                    let transport_auth = transport_auth.ok_or_else(|| {
                        RpcError::new(
                            "SDK_SECURITY_AUTH_REQUIRED".to_string(),
                            "mtls auth mode requires tls transport context".to_string(),
                        )
                    })?;
                    let (require_client_cert, allowed_san) =
                        self.sdk_mtls_auth_config().ok_or_else(|| {
                            RpcError::new(
                                "SDK_SECURITY_AUTH_REQUIRED".to_string(),
                                "mtls auth mode requires mtls auth configuration".to_string(),
                            )
                        })?;
                    let cert_present = transport_auth.client_cert_present;
                    if require_client_cert && !cert_present {
                        return Err(RpcError::new(
                            "SDK_SECURITY_AUTH_REQUIRED".to_string(),
                            "client certificate is required for mtls auth mode".to_string(),
                        ));
                    }
                    if let Some(expected_san) = allowed_san {
                        let san_matches = transport_auth
                            .client_sans
                            .iter()
                            .map(|san| san.trim())
                            .any(|san| !san.is_empty() && san == expected_san);
                        if !san_matches {
                            return Err(RpcError::new(
                                "SDK_SECURITY_AUTHZ_DENIED".to_string(),
                                "client SAN is not authorized by mtls policy".to_string(),
                            ));
                        }
                    }
                    principal = transport_auth
                        .client_subject
                        .as_deref()
                        .map(str::trim)
                        .filter(|value| !value.is_empty())
                        .unwrap_or("mtls-client")
                        .to_string();
                }
                _ => {
                    return Err(RpcError::new(
                        "SDK_SECURITY_AUTH_REQUIRED".to_string(),
                        "unknown auth mode".to_string(),
                    ));
                }
            }

            self.enforce_rate_limits(source_ip.as_str(), principal.as_str())
        })();

        let elapsed_ms = auth_started.elapsed().as_millis() as u64;
        self.metrics_record_auth_result(elapsed_ms, result.is_ok());
        result
    }

    #[allow(clippy::result_large_err)]
    fn enforce_rate_limits(&self, source_ip: &str, principal: &str) -> Result<(), RpcError> {
        let (per_ip_limit, per_principal_limit) = self.sdk_rate_limits();
        if per_ip_limit == 0 && per_principal_limit == 0 {
            return Ok(());
        }

        let now = now_millis_u64();
        {
            let mut window_started = self
                .sdk_rate_window_started_ms
                .lock()
                .expect("sdk_rate_window_started_ms mutex poisoned");
            if *window_started == 0 || now.saturating_sub(*window_started) >= 60_000 {
                *window_started = now;
                self.sdk_rate_ip_counts.lock().expect("sdk_rate_ip_counts mutex poisoned").clear();
                self.sdk_rate_principal_counts
                    .lock()
                    .expect("sdk_rate_principal_counts mutex poisoned")
                    .clear();
            }
        }

        if per_ip_limit > 0 {
            let mut counts =
                self.sdk_rate_ip_counts.lock().expect("sdk_rate_ip_counts mutex poisoned");
            let count = counts.entry(source_ip.to_string()).or_insert(0);
            *count = count.saturating_add(1);
            if *count > per_ip_limit {
                let event = RpcEvent {
                    event_type: "sdk_security_rate_limited".to_string(),
                    payload: json!({
                        "scope": "ip",
                        "source_ip": source_ip,
                        "principal": principal,
                        "limit": per_ip_limit,
                        "count": *count,
                    }),
                };
                self.publish_event(event);
                return Err(RpcError::new("SDK_SECURITY_RATE_LIMITED".to_string(), "per-ip request rate limit exceeded".to_string()));
            }
        }

        if per_principal_limit > 0 {
            let mut counts = self
                .sdk_rate_principal_counts
                .lock()
                .expect("sdk_rate_principal_counts mutex poisoned");
            let count = counts.entry(principal.to_string()).or_insert(0);
            *count = count.saturating_add(1);
            if *count > per_principal_limit {
                let event = RpcEvent {
                    event_type: "sdk_security_rate_limited".to_string(),
                    payload: json!({
                        "scope": "principal",
                        "source_ip": source_ip,
                        "principal": principal,
                        "limit": per_principal_limit,
                        "count": *count,
                    }),
                };
                self.publish_event(event);
                return Err(RpcError::new("SDK_SECURITY_RATE_LIMITED".to_string(), "per-principal request rate limit exceeded".to_string()));
            }
        }

        Ok(())
    }

}
