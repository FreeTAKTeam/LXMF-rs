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
                self.push_event(event.clone());
                let _ = self.events.send(event);
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
                self.push_event(event.clone());
                let _ = self.events.send(event);
                return Err(RpcError {
                    code: "SDK_SECURITY_RATE_LIMITED".to_string(),
                    message: "per-principal request rate limit exceeded".to_string(),
                });
            }
        }

        Ok(())
    }

    fn sdk_rate_limits(&self) -> (u32, u32) {
        let config =
            self.sdk_runtime_config.lock().expect("sdk_runtime_config mutex poisoned").clone();
        let per_ip = config
            .get("extensions")
            .and_then(|value| value.get("rate_limits"))
            .and_then(|value| value.get("per_ip_per_minute"))
            .and_then(JsonValue::as_u64)
            .and_then(|value| u32::try_from(value).ok())
            .unwrap_or(120);
        let per_principal = config
            .get("extensions")
            .and_then(|value| value.get("rate_limits"))
            .and_then(|value| value.get("per_principal_per_minute"))
            .and_then(JsonValue::as_u64)
            .and_then(|value| u32::try_from(value).ok())
            .unwrap_or(120);
        (per_ip, per_principal)
    }

    fn sdk_token_auth_config(&self) -> Option<(String, String, u64, u64, String)> {
        let config =
            self.sdk_runtime_config.lock().expect("sdk_runtime_config mutex poisoned").clone();
        let token_auth = config.get("rpc_backend")?.get("token_auth")?;
        let issuer = token_auth.get("issuer")?.as_str()?.to_string();
        let audience = token_auth.get("audience")?.as_str()?.to_string();
        let jti_ttl_ms = token_auth.get("jti_cache_ttl_ms")?.as_u64()?;
        let clock_skew_secs =
            token_auth.get("clock_skew_ms").and_then(JsonValue::as_u64).unwrap_or(0) / 1000;
        let shared_secret = token_auth.get("shared_secret")?.as_str()?.to_string();
        Some((issuer, audience, jti_ttl_ms, clock_skew_secs, shared_secret))
    }

    fn sdk_mtls_auth_config(&self) -> Option<(bool, Option<String>)> {
        let config =
            self.sdk_runtime_config.lock().expect("sdk_runtime_config mutex poisoned").clone();
        let mtls_auth = config.get("rpc_backend")?.get("mtls_auth")?;
        let require_client_cert =
            mtls_auth.get("require_client_cert").and_then(JsonValue::as_bool).unwrap_or(true);
        let allowed_san = mtls_auth
            .get("allowed_san")
            .and_then(JsonValue::as_str)
            .map(str::trim)
            .and_then(|value| if value.is_empty() { None } else { Some(value.to_string()) });
        Some((require_client_cert, allowed_san))
    }

    fn header_value<'a>(headers: &'a [(String, String)], key: &str) -> Option<&'a str> {
        headers
            .iter()
            .find(|(name, _)| name.eq_ignore_ascii_case(key))
            .map(|(_, value)| value.as_str())
    }

    fn parse_token_claims(token: &str) -> Option<HashMap<String, String>> {
        let mut claims = HashMap::new();
        for part in token.split(';') {
            let (key, value) = part.split_once('=')?;
            let key = key.trim();
            let value = value.trim();
            if key.is_empty() || value.is_empty() {
                return None;
            }
            claims.insert(key.to_string(), value.to_string());
        }
        if claims.is_empty() {
            return None;
        }
        Some(claims)
    }

    fn token_signature(secret: &str, payload: &str) -> Option<String> {
        let mut mac = hmac::Hmac::<sha2::Sha256>::new_from_slice(secret.as_bytes()).ok()?;
        mac.update(payload.as_bytes());
        Some(hex::encode(mac.finalize().into_bytes()))
    }

    fn is_loopback_source(source: &str) -> bool {
        let normalized = source.trim().to_ascii_lowercase();
        normalized == "127.0.0.1"
            || normalized == "::1"
            || normalized == "[::1]"
            || normalized == "localhost"
            || normalized.starts_with("127.")
    }

    fn is_terminal_receipt_status(status: &str) -> bool {
        let normalized = status.trim().to_ascii_lowercase();
        normalized.starts_with("failed")
            || matches!(normalized.as_str(), "cancelled" | "delivered" | "expired" | "rejected")
    }

    fn active_contract_version(&self) -> u16 {
        *self
            .sdk_active_contract_version
            .lock()
            .expect("sdk_active_contract_version mutex poisoned")
    }

    fn sdk_supported_capabilities() -> Vec<String> {
        vec![
            "sdk.capability.cursor_replay".to_string(),
            "sdk.capability.async_events".to_string(),
            "sdk.capability.token_auth".to_string(),
            "sdk.capability.mtls_auth".to_string(),
            "sdk.capability.receipt_terminality".to_string(),
            "sdk.capability.config_revision_cas".to_string(),
            "sdk.capability.idempotency_ttl".to_string(),
            "sdk.capability.topics".to_string(),
            "sdk.capability.topic_subscriptions".to_string(),
            "sdk.capability.topic_fanout".to_string(),
            "sdk.capability.telemetry_query".to_string(),
            "sdk.capability.telemetry_stream".to_string(),
            "sdk.capability.attachments".to_string(),
            "sdk.capability.attachment_delete".to_string(),
            "sdk.capability.markers".to_string(),
            "sdk.capability.identity_multi".to_string(),
            "sdk.capability.identity_import_export".to_string(),
            "sdk.capability.identity_hash_resolution".to_string(),
            "sdk.capability.paper_messages".to_string(),
            "sdk.capability.remote_commands".to_string(),
            "sdk.capability.voice_signaling".to_string(),
            "sdk.capability.shared_instance_rpc_auth".to_string(),
        ]
    }

    fn sdk_supported_capabilities_for_profile(profile: &str) -> Vec<String> {
        let mut caps = Self::sdk_supported_capabilities();
        if profile == "embedded-alloc" {
            caps.retain(|capability| capability != "sdk.capability.async_events");
        }
        caps
    }

    fn sdk_required_capabilities_for_profile(profile: &str) -> Vec<String> {
        match profile {
            "desktop-local-runtime" => vec![
                "sdk.capability.cursor_replay".to_string(),
                "sdk.capability.receipt_terminality".to_string(),
                "sdk.capability.config_revision_cas".to_string(),
                "sdk.capability.idempotency_ttl".to_string(),
            ],
            _ => vec![
                "sdk.capability.cursor_replay".to_string(),
                "sdk.capability.async_events".to_string(),
                "sdk.capability.receipt_terminality".to_string(),
                "sdk.capability.config_revision_cas".to_string(),
                "sdk.capability.idempotency_ttl".to_string(),
            ],
        }
    }

    fn sdk_effective_limits_for_profile(profile: &str) -> JsonValue {
        match profile {
            "desktop-local-runtime" => json!({
                "max_poll_events": 64,
                "max_event_bytes": 32_768,
                "max_batch_bytes": 1_048_576,
                "max_extension_keys": 32,
                "idempotency_ttl_ms": 43_200_000_u64,
            }),
            "embedded-alloc" => json!({
                "max_poll_events": 32,
                "max_event_bytes": 8_192,
                "max_batch_bytes": 262_144,
                "max_extension_keys": 32,
                "idempotency_ttl_ms": 7_200_000_u64,
            }),
            _ => json!({
                "max_poll_events": 256,
                "max_event_bytes": 65_536,
                "max_batch_bytes": 1_048_576,
                "max_extension_keys": 32,
                "idempotency_ttl_ms": 86_400_000_u64,
            }),
        }
    }

    fn sdk_max_poll_events(&self) -> usize {
        if let Some(value) = self
            .sdk_runtime_config
            .lock()
            .expect("sdk_runtime_config mutex poisoned")
            .get("event_stream")
            .and_then(|value| value.get("max_poll_events"))
            .and_then(JsonValue::as_u64)
            .and_then(|value| usize::try_from(value).ok())
        {
            return value;
        }
        match self.sdk_profile.lock().expect("sdk_profile mutex poisoned").as_str() {
            "desktop-local-runtime" => 64,
            "embedded-alloc" => 32,
            _ => 256,
        }
    }

    fn sdk_error_response(&self, id: u64, code: &str, message: &str) -> RpcResponse {
        RpcResponse {
            id,
            result: None,
            error: Some(RpcError { code: code.to_string(), message: message.to_string() }),
        }
    }

    fn sdk_capability_disabled_response(
        &self,
        id: u64,
        method: &str,
        capability: &str,
    ) -> RpcResponse {
        self.sdk_error_response(
            id,
            "SDK_CAPABILITY_DISABLED",
            &format!("method '{method}' requires capability '{capability}'"),
        )
    }

    fn sdk_encode_cursor(&self, seq_no: u64) -> String {
        format!("v2:{}:{}:{}", self.identity_hash, SDK_STREAM_ID, seq_no)
    }

    fn sdk_decode_cursor(&self, cursor: Option<&str>) -> Result<Option<u64>, SdkCursorError> {
        let Some(cursor) = cursor else {
            return Ok(None);
        };
        let cursor = cursor.trim();
        if cursor.is_empty() {
            return Err(SdkCursorError {
                code: "SDK_RUNTIME_INVALID_CURSOR".to_string(),
                message: "cursor must not be empty".to_string(),
            });
        }

        let mut parts = cursor.split(':');
        let version = parts.next();
        let runtime_id = parts.next();
        let stream_id = parts.next();
        let seq = parts.next();
        let has_extra = parts.next().is_some();
        if version != Some("v2")
            || runtime_id != Some(self.identity_hash.as_str())
            || stream_id != Some(SDK_STREAM_ID)
            || has_extra
        {
            return Err(SdkCursorError {
                code: "SDK_RUNTIME_INVALID_CURSOR".to_string(),
                message: "cursor scope does not match runtime".to_string(),
            });
        }

        let seq =
            seq.and_then(|value| value.parse::<u64>().ok()).ok_or_else(|| SdkCursorError {
                code: "SDK_RUNTIME_INVALID_CURSOR".to_string(),
                message: "cursor sequence is invalid".to_string(),
            })?;
        Ok(Some(seq))
    }

    fn event_severity(event_type: &str) -> &'static str {
        if event_type.eq_ignore_ascii_case("StreamGap") {
            return "warn";
        }
        if event_type.eq_ignore_ascii_case("error")
            || event_type.eq_ignore_ascii_case("delivery_failed")
        {
            return "error";
        }
        "info"
    }

    #[allow(clippy::too_many_arguments)]
    fn store_outbound(
        &self,
        request_id: u64,
        id: String,
        source: String,
        destination: String,
        title: String,
        content: String,
        fields: Option<JsonValue>,
        method: Option<String>,
        stamp_cost: Option<u32>,
        options: OutboundDeliveryOptions,
        include_ticket: Option<bool>,
    ) -> Result<RpcResponse, std::io::Error> {
        let timestamp = now_i64();
        self.append_delivery_trace(&id, "queued".to_string());
        let mut record = MessageRecord {
            id: id.clone(),
            source,
            destination,
            title,
            content,
            timestamp,
            direction: "out".into(),
            fields: merge_fields_with_options(fields, method.clone(), stamp_cost, include_ticket),
            receipt_status: None,
        };

        self.store.insert_message(&record).map_err(std::io::Error::other)?;
        self.append_delivery_trace(&id, "sending".to_string());
        let deliver_result = if let Some(bridge) = &self.outbound_bridge {
            bridge.deliver(&record, &options)
        } else {
            let _delivered = crate::transport::test_bridge::deliver_outbound(&record);
            Ok(())
        };
        if let Err(err) = deliver_result {
            let status = format!("failed: {err}");
            let resolved_status = {
                let _status_guard =
                    self.delivery_status_lock.lock().expect("delivery_status_lock mutex poisoned");
                let existing_status = self
                    .store
                    .get_message(&id)
                    .map_err(std::io::Error::other)?
                    .and_then(|message| message.receipt_status);
                if existing_status.as_deref().is_some_and(Self::is_terminal_receipt_status) {
                    existing_status.unwrap_or(status.clone())
                } else {
                    self.store
                        .update_receipt_status(&id, &status)
                        .map_err(std::io::Error::other)?;
                    self.append_delivery_trace(&id, status.clone());
                    status.clone()
                }
            };
            record.receipt_status = Some(resolved_status.clone());
            let reason_code = delivery_reason_code(&resolved_status);
            let event = RpcEvent {
                event_type: "outbound".into(),
                payload: json!({
                    "message": record,
                    "method": method,
                    "error": err.to_string(),
                    "reason_code": reason_code,
                }),
            };
            self.push_event(event.clone());
            let _ = self.events.send(event);
            return Ok(RpcResponse {
                id: request_id,
                result: None,
                error: Some(RpcError { code: "DELIVERY_FAILED".into(), message: err.to_string() }),
            });
        }
        let sent_status = format!("sent: {}", method.as_deref().unwrap_or("direct"));
        let resolved_status = {
            let _status_guard =
                self.delivery_status_lock.lock().expect("delivery_status_lock mutex poisoned");
            let existing_status = self
                .store
                .get_message(&id)
                .map_err(std::io::Error::other)?
                .and_then(|message| message.receipt_status);
            if existing_status.as_deref().is_some_and(Self::is_terminal_receipt_status) {
                existing_status.unwrap_or(sent_status.clone())
            } else {
                self.store
                    .update_receipt_status(&id, &sent_status)
                    .map_err(std::io::Error::other)?;
                self.append_delivery_trace(&id, sent_status.clone());
                sent_status.clone()
            }
        };
        record.receipt_status = Some(resolved_status.clone());
        let event = RpcEvent {
            event_type: "outbound".into(),
            payload: json!({
                "message": record,
                "method": method,
                "reason_code": delivery_reason_code(&resolved_status),
            }),
        };
        self.push_event(event.clone());
        let _ = self.events.send(event);

        Ok(RpcResponse { id: request_id, result: Some(json!({ "message_id": id })), error: None })
    }

    fn local_delivery_hash(&self) -> String {
        self.delivery_destination_hash
            .lock()
            .expect("delivery_destination_hash mutex poisoned")
            .clone()
            .unwrap_or_else(|| self.identity_hash.clone())
    }

    fn capabilities() -> Vec<&'static str> {
        vec![
            "status",
            "daemon_status_ex",
            "list_messages",
            "list_announces",
            "list_peers",
            "send_message",
            "send_message_v2",
            "sdk_send_v2",
            "sdk_negotiate_v2",
            "sdk_status_v2",
            "sdk_configure_v2",
            "sdk_poll_events_v2",
            "sdk_cancel_message_v2",
            "sdk_snapshot_v2",
            "sdk_shutdown_v2",
            "sdk_topic_create_v2",
            "sdk_topic_get_v2",
            "sdk_topic_list_v2",
            "sdk_topic_subscribe_v2",
            "sdk_topic_unsubscribe_v2",
            "sdk_topic_publish_v2",
            "sdk_telemetry_query_v2",
            "sdk_telemetry_subscribe_v2",
            "sdk_attachment_store_v2",
            "sdk_attachment_get_v2",
            "sdk_attachment_list_v2",
            "sdk_attachment_delete_v2",
            "sdk_attachment_download_v2",
            "sdk_attachment_associate_topic_v2",
            "sdk_marker_create_v2",
            "sdk_marker_list_v2",
            "sdk_marker_update_position_v2",
            "sdk_marker_delete_v2",
            "sdk_identity_list_v2",
            "sdk_identity_activate_v2",
            "sdk_identity_import_v2",
            "sdk_identity_export_v2",
            "sdk_identity_resolve_v2",
            "sdk_paper_encode_v2",
            "sdk_paper_decode_v2",
            "sdk_command_invoke_v2",
            "sdk_command_reply_v2",
            "sdk_voice_session_open_v2",
            "sdk_voice_session_update_v2",
            "sdk_voice_session_close_v2",
            "announce_now",
            "list_interfaces",
            "set_interfaces",
            "reload_config",
            "peer_sync",
            "peer_unpeer",
            "set_delivery_policy",
            "get_delivery_policy",
            "propagation_status",
            "propagation_enable",
            "propagation_ingest",
            "propagation_fetch",
            "get_outbound_propagation_node",
            "set_outbound_propagation_node",
            "list_propagation_nodes",
            "paper_ingest_uri",
            "stamp_policy_get",
            "stamp_policy_set",
            "ticket_generate",
            "message_delivery_trace",
        ]
    }

}
