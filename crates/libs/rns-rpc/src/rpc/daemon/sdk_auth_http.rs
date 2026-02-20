impl RpcDaemon {
    fn response_meta(&self) -> JsonValue {
        let profile = self.sdk_profile.lock().expect("sdk_profile mutex poisoned").clone();
        json!({
            "contract_version": format!("v{}", self.active_contract_version()),
            "profile": profile,
            "rpc_endpoint": JsonValue::Null,
        })
    }

    pub fn authorize_http_request(
        &self,
        headers: &[(String, String)],
        peer_ip: Option<&str>,
    ) -> Result<(), RpcError> {
        let config =
            self.sdk_runtime_config.lock().expect("sdk_runtime_config mutex poisoned").clone();
        let trust_forwarded = config
            .get("extensions")
            .and_then(|value| value.get("trusted_proxy"))
            .and_then(JsonValue::as_bool)
            .unwrap_or(false);
        let trusted_proxy_ips = config
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
        let peer_ip = peer_ip.map(str::trim).filter(|value| !value.is_empty()).map(str::to_string);
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
            peer_ip.clone()
        }
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "unknown".to_string());

        let bind_mode =
            config.get("bind_mode").and_then(JsonValue::as_str).unwrap_or("local_only").to_string();
        if bind_mode == "local_only" && !Self::is_loopback_source(source_ip.as_str()) {
            return Err(RpcError {
                code: "SDK_SECURITY_REMOTE_BIND_DISALLOWED".to_string(),
                message: "remote source is not allowed in local_only bind mode".to_string(),
            });
        }

        let auth_mode = config
            .get("auth_mode")
            .and_then(JsonValue::as_str)
            .unwrap_or("local_trusted")
            .to_string();
        let mut principal = "local".to_string();
        match auth_mode.as_str() {
            "local_trusted" => {}
            "token" => {
                let auth_header =
                    Self::header_value(headers, "authorization").ok_or_else(|| RpcError {
                        code: "SDK_SECURITY_AUTH_REQUIRED".to_string(),
                        message: "authorization header is required".to_string(),
                    })?;
                let token = auth_header
                    .strip_prefix("Bearer ")
                    .or_else(|| auth_header.strip_prefix("bearer "))
                    .ok_or_else(|| RpcError {
                        code: "SDK_SECURITY_TOKEN_INVALID".to_string(),
                        message: "authorization header must use Bearer token format".to_string(),
                    })?;
                let claims = Self::parse_token_claims(token).ok_or_else(|| RpcError {
                    code: "SDK_SECURITY_TOKEN_INVALID".to_string(),
                    message: "token claims are malformed".to_string(),
                })?;
                let (
                    expected_issuer,
                    expected_audience,
                    jti_ttl_ms,
                    clock_skew_secs,
                    shared_secret,
                ) = self.sdk_token_auth_config().ok_or_else(|| RpcError {
                    code: "SDK_SECURITY_AUTH_REQUIRED".to_string(),
                    message: "token auth mode requires token auth configuration".to_string(),
                })?;
                let issuer = claims.get("iss").map(String::as_str).ok_or_else(|| RpcError {
                    code: "SDK_SECURITY_TOKEN_INVALID".to_string(),
                    message: "token issuer claim is missing".to_string(),
                })?;
                let audience = claims.get("aud").map(String::as_str).ok_or_else(|| RpcError {
                    code: "SDK_SECURITY_TOKEN_INVALID".to_string(),
                    message: "token audience claim is missing".to_string(),
                })?;
                let jti = claims.get("jti").cloned().ok_or_else(|| RpcError {
                    code: "SDK_SECURITY_TOKEN_INVALID".to_string(),
                    message: "token jti claim is missing".to_string(),
                })?;
                let subject =
                    claims.get("sub").cloned().unwrap_or_else(|| "sdk-client".to_string());
                let iat = claims
                    .get("iat")
                    .and_then(|value| value.parse::<u64>().ok())
                    .ok_or_else(|| RpcError {
                        code: "SDK_SECURITY_TOKEN_INVALID".to_string(),
                        message: "token iat claim is missing or invalid".to_string(),
                    })?;
                let exp = claims
                    .get("exp")
                    .and_then(|value| value.parse::<u64>().ok())
                    .ok_or_else(|| RpcError {
                        code: "SDK_SECURITY_TOKEN_INVALID".to_string(),
                        message: "token exp claim is missing or invalid".to_string(),
                    })?;
                let signature = claims.get("sig").map(String::as_str).ok_or_else(|| RpcError {
                    code: "SDK_SECURITY_TOKEN_INVALID".to_string(),
                    message: "token signature is missing".to_string(),
                })?;
                let signed_payload = format!(
                    "iss={issuer};aud={audience};jti={jti};sub={subject};iat={iat};exp={exp}"
                );
                let expected_signature =
                    Self::token_signature(shared_secret.as_str(), signed_payload.as_str())
                        .ok_or_else(|| RpcError {
                            code: "SDK_SECURITY_TOKEN_INVALID".to_string(),
                            message: "token signature verification failed".to_string(),
                        })?;
                if signature != expected_signature {
                    return Err(RpcError {
                        code: "SDK_SECURITY_TOKEN_INVALID".to_string(),
                        message: "token signature does not match runtime policy".to_string(),
                    });
                }
                if issuer != expected_issuer || audience != expected_audience {
                    return Err(RpcError {
                        code: "SDK_SECURITY_TOKEN_INVALID".to_string(),
                        message: "token issuer/audience does not match runtime policy".to_string(),
                    });
                }
                let now_seconds = now_seconds_u64();
                if iat > now_seconds.saturating_add(clock_skew_secs) {
                    return Err(RpcError {
                        code: "SDK_SECURITY_TOKEN_INVALID".to_string(),
                        message: "token iat is outside accepted clock skew".to_string(),
                    });
                }
                if exp.saturating_add(clock_skew_secs) < now_seconds {
                    return Err(RpcError {
                        code: "SDK_SECURITY_TOKEN_INVALID".to_string(),
                        message: "token has expired".to_string(),
                    });
                }
                principal = subject;
                let now = now_millis_u64();
                let mut replay_cache =
                    self.sdk_seen_jti.lock().expect("sdk_seen_jti mutex poisoned");
                replay_cache.retain(|_, expires_at| *expires_at > now);
                if replay_cache.contains_key(jti.as_str()) {
                    return Err(RpcError {
                        code: "SDK_SECURITY_TOKEN_REPLAYED".to_string(),
                        message: "token jti has already been used".to_string(),
                    });
                }
                replay_cache.insert(jti, now.saturating_add(jti_ttl_ms.max(1)));
            }
            "mtls" => {
                let (require_client_cert, allowed_san) =
                    self.sdk_mtls_auth_config().ok_or_else(|| RpcError {
                        code: "SDK_SECURITY_AUTH_REQUIRED".to_string(),
                        message: "mtls auth mode requires mtls auth configuration".to_string(),
                    })?;
                let cert_present = Self::header_value(headers, "x-client-cert-present")
                    .map(|value| {
                        value.eq_ignore_ascii_case("1") || value.eq_ignore_ascii_case("true")
                    })
                    .unwrap_or(false);
                if require_client_cert && !cert_present {
                    return Err(RpcError {
                        code: "SDK_SECURITY_AUTH_REQUIRED".to_string(),
                        message: "client certificate is required for mtls auth mode".to_string(),
                    });
                }
                if let Some(expected_san) = allowed_san {
                    let observed_san = Self::header_value(headers, "x-client-san")
                        .map(str::trim)
                        .filter(|value| !value.is_empty())
                        .ok_or_else(|| RpcError {
                            code: "SDK_SECURITY_AUTHZ_DENIED".to_string(),
                            message: "client SAN header is required for configured mtls policy"
                                .to_string(),
                        })?;
                    if observed_san != expected_san {
                        return Err(RpcError {
                            code: "SDK_SECURITY_AUTHZ_DENIED".to_string(),
                            message: "client SAN is not authorized by mtls policy".to_string(),
                        });
                    }
                }
                principal = Self::header_value(headers, "x-client-subject")
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                    .unwrap_or("mtls-client")
                    .to_string();
            }
            _ => {
                return Err(RpcError {
                    code: "SDK_SECURITY_AUTH_REQUIRED".to_string(),
                    message: "unknown auth mode".to_string(),
                })
            }
        }

        self.enforce_rate_limits(source_ip.as_str(), principal.as_str())
    }

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
                return Err(RpcError {
                    code: "SDK_SECURITY_RATE_LIMITED".to_string(),
                    message: "per-ip request rate limit exceeded".to_string(),
                });
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
                return Err(RpcError {
                    code: "SDK_SECURITY_RATE_LIMITED".to_string(),
                    message: "per-principal request rate limit exceeded".to_string(),
                });
            }
        }

        Ok(())
    }

}
