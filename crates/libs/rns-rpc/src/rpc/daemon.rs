use super::*;
use hmac::Mac;

impl RpcDaemon {
    pub fn with_store(store: MessagesStore, identity_hash: String) -> Self {
        let (events, _rx) = broadcast::channel(64);
        Self {
            store,
            identity_hash,
            delivery_destination_hash: Mutex::new(None),
            events,
            event_queue: Mutex::new(VecDeque::new()),
            sdk_event_log: Mutex::new(VecDeque::new()),
            sdk_next_event_seq: Mutex::new(0),
            sdk_dropped_event_count: Mutex::new(0),
            sdk_active_contract_version: Mutex::new(2),
            sdk_profile: Mutex::new("desktop-full".to_string()),
            sdk_config_revision: Mutex::new(0),
            sdk_runtime_config: Mutex::new(JsonValue::Object(JsonMap::new())),
            sdk_config_apply_lock: Mutex::new(()),
            sdk_effective_capabilities: Mutex::new(Self::sdk_supported_capabilities()),
            sdk_stream_degraded: Mutex::new(false),
            sdk_seen_jti: Mutex::new(HashMap::new()),
            sdk_rate_window_started_ms: Mutex::new(0),
            sdk_rate_ip_counts: Mutex::new(HashMap::new()),
            sdk_rate_principal_counts: Mutex::new(HashMap::new()),
            peers: Mutex::new(HashMap::new()),
            interfaces: Mutex::new(Vec::new()),
            delivery_policy: Mutex::new(DeliveryPolicy::default()),
            propagation_state: Mutex::new(PropagationState::default()),
            propagation_payloads: Mutex::new(HashMap::new()),
            outbound_propagation_node: Mutex::new(None),
            paper_ingest_seen: Mutex::new(HashSet::new()),
            stamp_policy: Mutex::new(StampPolicy::default()),
            ticket_cache: Mutex::new(HashMap::new()),
            delivery_traces: Mutex::new(HashMap::new()),
            delivery_status_lock: Mutex::new(()),
            outbound_bridge: None,
            announce_bridge: None,
        }
    }

    pub fn with_store_and_bridge(
        store: MessagesStore,
        identity_hash: String,
        outbound_bridge: Arc<dyn OutboundBridge>,
    ) -> Self {
        let (events, _rx) = broadcast::channel(64);
        Self {
            store,
            identity_hash,
            delivery_destination_hash: Mutex::new(None),
            events,
            event_queue: Mutex::new(VecDeque::new()),
            sdk_event_log: Mutex::new(VecDeque::new()),
            sdk_next_event_seq: Mutex::new(0),
            sdk_dropped_event_count: Mutex::new(0),
            sdk_active_contract_version: Mutex::new(2),
            sdk_profile: Mutex::new("desktop-full".to_string()),
            sdk_config_revision: Mutex::new(0),
            sdk_runtime_config: Mutex::new(JsonValue::Object(JsonMap::new())),
            sdk_config_apply_lock: Mutex::new(()),
            sdk_effective_capabilities: Mutex::new(Self::sdk_supported_capabilities()),
            sdk_stream_degraded: Mutex::new(false),
            sdk_seen_jti: Mutex::new(HashMap::new()),
            sdk_rate_window_started_ms: Mutex::new(0),
            sdk_rate_ip_counts: Mutex::new(HashMap::new()),
            sdk_rate_principal_counts: Mutex::new(HashMap::new()),
            peers: Mutex::new(HashMap::new()),
            interfaces: Mutex::new(Vec::new()),
            delivery_policy: Mutex::new(DeliveryPolicy::default()),
            propagation_state: Mutex::new(PropagationState::default()),
            propagation_payloads: Mutex::new(HashMap::new()),
            outbound_propagation_node: Mutex::new(None),
            paper_ingest_seen: Mutex::new(HashSet::new()),
            stamp_policy: Mutex::new(StampPolicy::default()),
            ticket_cache: Mutex::new(HashMap::new()),
            delivery_traces: Mutex::new(HashMap::new()),
            delivery_status_lock: Mutex::new(()),
            outbound_bridge: Some(outbound_bridge),
            announce_bridge: None,
        }
    }

    pub fn with_store_and_bridges(
        store: MessagesStore,
        identity_hash: String,
        outbound_bridge: Option<Arc<dyn OutboundBridge>>,
        announce_bridge: Option<Arc<dyn AnnounceBridge>>,
    ) -> Self {
        let (events, _rx) = broadcast::channel(64);
        Self {
            store,
            identity_hash,
            delivery_destination_hash: Mutex::new(None),
            events,
            event_queue: Mutex::new(VecDeque::new()),
            sdk_event_log: Mutex::new(VecDeque::new()),
            sdk_next_event_seq: Mutex::new(0),
            sdk_dropped_event_count: Mutex::new(0),
            sdk_active_contract_version: Mutex::new(2),
            sdk_profile: Mutex::new("desktop-full".to_string()),
            sdk_config_revision: Mutex::new(0),
            sdk_runtime_config: Mutex::new(JsonValue::Object(JsonMap::new())),
            sdk_config_apply_lock: Mutex::new(()),
            sdk_effective_capabilities: Mutex::new(Self::sdk_supported_capabilities()),
            sdk_stream_degraded: Mutex::new(false),
            sdk_seen_jti: Mutex::new(HashMap::new()),
            sdk_rate_window_started_ms: Mutex::new(0),
            sdk_rate_ip_counts: Mutex::new(HashMap::new()),
            sdk_rate_principal_counts: Mutex::new(HashMap::new()),
            peers: Mutex::new(HashMap::new()),
            interfaces: Mutex::new(Vec::new()),
            delivery_policy: Mutex::new(DeliveryPolicy::default()),
            propagation_state: Mutex::new(PropagationState::default()),
            propagation_payloads: Mutex::new(HashMap::new()),
            outbound_propagation_node: Mutex::new(None),
            paper_ingest_seen: Mutex::new(HashSet::new()),
            stamp_policy: Mutex::new(StampPolicy::default()),
            ticket_cache: Mutex::new(HashMap::new()),
            delivery_traces: Mutex::new(HashMap::new()),
            delivery_status_lock: Mutex::new(()),
            outbound_bridge,
            announce_bridge,
        }
    }

    pub fn test_instance() -> Self {
        let store = MessagesStore::in_memory().expect("in-memory store");
        Self::with_store(store, "test-identity".into())
    }

    #[cfg_attr(not(test), allow(dead_code))]
    pub fn test_instance_with_identity(identity: impl Into<String>) -> Self {
        let store = MessagesStore::in_memory().expect("in-memory store");
        Self::with_store(store, identity.into())
    }

    pub fn set_delivery_destination_hash(&self, hash: Option<String>) {
        let mut guard = self
            .delivery_destination_hash
            .lock()
            .expect("delivery_destination_hash mutex poisoned");
        *guard = hash.and_then(|value| {
            let trimmed = value.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        });
    }

    pub fn replace_interfaces(&self, interfaces: Vec<InterfaceRecord>) {
        let mut guard = self.interfaces.lock().expect("interfaces mutex poisoned");
        *guard = interfaces;
    }

    pub fn set_propagation_state(
        &self,
        enabled: bool,
        store_root: Option<String>,
        target_cost: u32,
    ) {
        let mut guard = self.propagation_state.lock().expect("propagation mutex poisoned");
        guard.enabled = enabled;
        guard.store_root = store_root;
        guard.target_cost = target_cost;
    }

    pub fn update_propagation_sync_state<F>(&self, update: F)
    where
        F: FnOnce(&mut PropagationState),
    {
        let mut guard = self.propagation_state.lock().expect("propagation mutex poisoned");
        update(&mut guard);
    }

    fn store_inbound_record(&self, record: MessageRecord) -> Result<(), std::io::Error> {
        self.store.insert_message(&record).map_err(std::io::Error::other)?;
        let event =
            RpcEvent { event_type: "inbound".into(), payload: json!({ "message": record }) };
        self.push_event(event.clone());
        let _ = self.events.send(event);
        Ok(())
    }

    pub fn accept_inbound(&self, record: MessageRecord) -> Result<(), std::io::Error> {
        self.store_inbound_record(record)
    }

    pub fn accept_announce(&self, peer: String, timestamp: i64) -> Result<(), std::io::Error> {
        self.accept_announce_with_metadata(
            peer, timestamp, None, None, None, None, None, None, None, None, None, None, None,
            None, None, None, None, None,
        )
    }

    pub fn accept_announce_with_details(
        &self,
        peer: String,
        timestamp: i64,
        name: Option<String>,
        name_source: Option<String>,
    ) -> Result<(), std::io::Error> {
        self.accept_announce_with_metadata(
            peer,
            timestamp,
            name,
            name_source,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn accept_announce_with_metadata(
        &self,
        peer: String,
        timestamp: i64,
        name: Option<String>,
        name_source: Option<String>,
        app_data_hex: Option<String>,
        capabilities: Option<Vec<String>>,
        rssi: Option<f64>,
        snr: Option<f64>,
        q: Option<f64>,
        stamp_cost: Option<u32>,
        stamp_cost_flexibility: Option<Option<u32>>,
        peering_cost: Option<Option<u32>>,
        aspect: Option<String>,
        hops: Option<u32>,
        interface: Option<String>,
        source_private_key: Option<String>,
        source_identity: Option<String>,
        source_node: Option<String>,
    ) -> Result<(), std::io::Error> {
        let _ = stamp_cost;
        let stamp_cost_flexibility = stamp_cost_flexibility.flatten();
        let peering_cost = peering_cost.flatten();
        let record = self.upsert_peer(peer, timestamp, name, name_source);
        let capability_list = if let Some(caps) = capabilities {
            normalize_capabilities(caps)
        } else {
            parse_capabilities_from_app_data_hex(app_data_hex.as_deref())
        };

        let announce_record = AnnounceRecord {
            id: format!("announce-{}-{}-{}", record.last_seen, record.peer, record.seen_count),
            peer: record.peer.clone(),
            timestamp: record.last_seen,
            name: record.name.clone(),
            name_source: record.name_source.clone(),
            first_seen: record.first_seen,
            seen_count: record.seen_count,
            app_data_hex: clean_optional_text(app_data_hex),
            capabilities: capability_list.clone(),
            rssi,
            snr,
            q,
            stamp_cost_flexibility,
            peering_cost,
        };
        self.store.insert_announce(&announce_record).map_err(std::io::Error::other)?;

        let event = RpcEvent {
            event_type: "announce_received".into(),
            payload: json!({
                "id": announce_record.id,
                "peer": record.peer,
                "timestamp": record.last_seen,
                "name": record.name,
                "name_source": record.name_source,
                "first_seen": record.first_seen,
                "seen_count": record.seen_count,
                "app_data_hex": announce_record.app_data_hex,
                "capabilities": capability_list,
                "rssi": rssi,
                "snr": snr,
                "q": q,
                "stamp_cost_flexibility": stamp_cost_flexibility,
                "peering_cost": peering_cost,
                "aspect": aspect,
                "hops": hops,
                "interface": interface,
                "source_private_key": source_private_key,
                "source_identity": source_identity,
                "source_node": source_node,
            }),
        };
        self.push_event(event.clone());
        let _ = self.events.send(event);
        Ok(())
    }

    fn upsert_peer(
        &self,
        peer: String,
        timestamp: i64,
        name: Option<String>,
        name_source: Option<String>,
    ) -> PeerRecord {
        let cleaned_name = clean_optional_text(name);
        let cleaned_name_source = clean_optional_text(name_source);

        let mut guard = self.peers.lock().expect("peers mutex poisoned");
        if let Some(existing) = guard.get_mut(&peer) {
            existing.last_seen = timestamp;
            existing.seen_count = existing.seen_count.saturating_add(1);
            if let Some(name) = cleaned_name {
                existing.name = Some(name);
                existing.name_source = cleaned_name_source;
            }
            return existing.clone();
        }

        let record = PeerRecord {
            peer: peer.clone(),
            last_seen: timestamp,
            name: cleaned_name,
            name_source: cleaned_name_source,
            first_seen: timestamp,
            seen_count: 1,
        };
        guard.insert(peer, record.clone());
        record
    }

    #[allow(dead_code)]
    pub(crate) fn accept_inbound_for_test(
        &self,
        record: MessageRecord,
    ) -> Result<(), std::io::Error> {
        self.store_inbound_record(record)
    }

    fn handle_sdk_negotiate_v2(&self, request: RpcRequest) -> Result<RpcResponse, std::io::Error> {
        let params = request.params.ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "missing params")
        })?;
        let parsed: SdkNegotiateV2Params = serde_json::from_value(params)
            .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidInput, err))?;

        let active_contract_version = parsed
            .supported_contract_versions
            .iter()
            .copied()
            .filter(|version| *version == 2)
            .max();

        let Some(active_contract_version) = active_contract_version else {
            return Ok(self.sdk_error_response(
                request.id,
                "SDK_CAPABILITY_CONTRACT_INCOMPATIBLE",
                "no compatible contract version",
            ));
        };

        let profile = parsed.config.profile.trim().to_ascii_lowercase();
        if !matches!(profile.as_str(), "desktop-full" | "desktop-local-runtime") {
            return Ok(self.sdk_error_response(
                request.id,
                "SDK_CAPABILITY_CONTRACT_INCOMPATIBLE",
                "profile is not supported by the rpc backend",
            ));
        }

        let bind_mode =
            parsed.config.bind_mode.as_deref().unwrap_or("local_only").trim().to_ascii_lowercase();
        if !matches!(bind_mode.as_str(), "local_only" | "remote") {
            return Ok(self.sdk_error_response(
                request.id,
                "SDK_VALIDATION_INVALID_ARGUMENT",
                "bind_mode must be local_only or remote",
            ));
        }

        let auth_mode = parsed
            .config
            .auth_mode
            .as_deref()
            .unwrap_or("local_trusted")
            .trim()
            .to_ascii_lowercase();
        if !matches!(auth_mode.as_str(), "local_trusted" | "token" | "mtls") {
            return Ok(self.sdk_error_response(
                request.id,
                "SDK_VALIDATION_INVALID_ARGUMENT",
                "auth_mode must be local_trusted, token, or mtls",
            ));
        }
        if bind_mode == "remote" && !matches!(auth_mode.as_str(), "token" | "mtls") {
            return Ok(self.sdk_error_response(
                request.id,
                "SDK_SECURITY_REMOTE_BIND_DISALLOWED",
                "remote bind mode requires token or mtls auth mode",
            ));
        }
        if bind_mode == "local_only" && auth_mode != "local_trusted" {
            return Ok(self.sdk_error_response(
                request.id,
                "SDK_SECURITY_AUTH_REQUIRED",
                "local_only bind mode requires local_trusted auth mode",
            ));
        }

        let overflow_policy = parsed
            .config
            .overflow_policy
            .as_deref()
            .unwrap_or("reject")
            .trim()
            .to_ascii_lowercase();
        if !matches!(overflow_policy.as_str(), "reject" | "drop_oldest" | "block") {
            return Ok(self.sdk_error_response(
                request.id,
                "SDK_VALIDATION_INVALID_ARGUMENT",
                "overflow_policy must be reject, drop_oldest, or block",
            ));
        }
        if overflow_policy == "block" && parsed.config.block_timeout_ms.is_none() {
            return Ok(self.sdk_error_response(
                request.id,
                "SDK_VALIDATION_INVALID_ARGUMENT",
                "overflow_policy=block requires block_timeout_ms",
            ));
        }

        match auth_mode.as_str() {
            "token" => {
                let Some(token_auth) = parsed
                    .config
                    .rpc_backend
                    .as_ref()
                    .and_then(|backend| backend.token_auth.as_ref())
                else {
                    return Ok(self.sdk_error_response(
                        request.id,
                        "SDK_SECURITY_AUTH_REQUIRED",
                        "token auth mode requires rpc_backend.token_auth configuration",
                    ));
                };
                if token_auth.issuer.trim().is_empty() || token_auth.audience.trim().is_empty() {
                    return Ok(self.sdk_error_response(
                        request.id,
                        "SDK_VALIDATION_INVALID_ARGUMENT",
                        "token auth configuration requires issuer and audience",
                    ));
                }
                if token_auth.jti_cache_ttl_ms == 0 {
                    return Ok(self.sdk_error_response(
                        request.id,
                        "SDK_VALIDATION_INVALID_ARGUMENT",
                        "token auth jti_cache_ttl_ms must be greater than zero",
                    ));
                }
                if token_auth.shared_secret.trim().is_empty() {
                    return Ok(self.sdk_error_response(
                        request.id,
                        "SDK_SECURITY_AUTH_REQUIRED",
                        "token auth shared_secret must be configured",
                    ));
                }
                let _clock_skew_ms = token_auth.clock_skew_ms.unwrap_or(0);
            }
            "mtls" => {
                return Ok(self.sdk_error_response(
                    request.id,
                    "SDK_CAPABILITY_DISABLED",
                    "mtls auth mode is not available until transport-bound certificate verification is implemented",
                ));
            }
            _ => {}
        }

        let supported_capabilities = Self::sdk_supported_capabilities_for_profile(profile.as_str());
        let mut effective_capabilities = Vec::new();
        if parsed.requested_capabilities.is_empty() {
            effective_capabilities = supported_capabilities.clone();
        } else {
            for requested in parsed.requested_capabilities {
                let normalized = requested.trim().to_ascii_lowercase();
                if normalized.is_empty() {
                    continue;
                }
                if supported_capabilities.contains(&normalized)
                    && !effective_capabilities.contains(&normalized)
                {
                    effective_capabilities.push(normalized);
                }
            }
            if effective_capabilities.is_empty() {
                return Ok(self.sdk_error_response(
                    request.id,
                    "SDK_CAPABILITY_CONTRACT_INCOMPATIBLE",
                    "no overlap between requested and supported capabilities",
                ));
            }
        }

        let limits = Self::sdk_effective_limits_for_profile(profile.as_str());

        {
            let mut guard = self
                .sdk_active_contract_version
                .lock()
                .expect("sdk_active_contract_version mutex poisoned");
            *guard = active_contract_version;
        }
        {
            let mut guard = self.sdk_profile.lock().expect("sdk_profile mutex poisoned");
            *guard = profile.clone();
        }
        {
            let mut guard = self
                .sdk_effective_capabilities
                .lock()
                .expect("sdk_effective_capabilities mutex poisoned");
            *guard = effective_capabilities.clone();
        }
        {
            let mut guard =
                self.sdk_runtime_config.lock().expect("sdk_runtime_config mutex poisoned");
            let rpc_backend =
                parsed.config.rpc_backend.as_ref().map_or(JsonValue::Null, |backend| {
                    json!({
                        "listen_addr": backend.listen_addr,
                        "read_timeout_ms": backend.read_timeout_ms,
                        "write_timeout_ms": backend.write_timeout_ms,
                        "max_header_bytes": backend.max_header_bytes,
                        "max_body_bytes": backend.max_body_bytes,
                        "token_auth": backend.token_auth.as_ref().map(|token| json!({
                            "issuer": token.issuer,
                            "audience": token.audience,
                            "jti_cache_ttl_ms": token.jti_cache_ttl_ms,
                            "clock_skew_ms": token.clock_skew_ms.unwrap_or(0),
                            "shared_secret": token.shared_secret,
                        })),
                        "mtls_auth": backend.mtls_auth.as_ref().map(|mtls| json!({
                            "ca_bundle_path": mtls.ca_bundle_path,
                            "require_client_cert": mtls.require_client_cert,
                            "allowed_san": mtls.allowed_san,
                        })),
                    })
                });
            *guard = json!({
                "profile": profile,
                "bind_mode": bind_mode,
                "auth_mode": auth_mode,
                "overflow_policy": overflow_policy,
                "block_timeout_ms": parsed.config.block_timeout_ms,
                "rpc_backend": rpc_backend,
                "event_stream": {
                    "max_poll_events": limits.get("max_poll_events").and_then(JsonValue::as_u64).unwrap_or(256),
                    "max_event_bytes": limits.get("max_event_bytes").and_then(JsonValue::as_u64).unwrap_or(65_536),
                    "max_batch_bytes": limits.get("max_batch_bytes").and_then(JsonValue::as_u64).unwrap_or(1_048_576),
                    "max_extension_keys": limits.get("max_extension_keys").and_then(JsonValue::as_u64).unwrap_or(32),
                },
                "idempotency_ttl_ms": limits.get("idempotency_ttl_ms").and_then(JsonValue::as_u64).unwrap_or(86_400_000_u64),
                "extensions": {
                    "rate_limits": {
                        "per_ip_per_minute": 120,
                        "per_principal_per_minute": 120,
                    }
                }
            });
        }
        {
            let mut guard =
                self.sdk_stream_degraded.lock().expect("sdk_stream_degraded mutex poisoned");
            *guard = false;
        }
        {
            self.sdk_seen_jti.lock().expect("sdk_seen_jti mutex poisoned").clear();
            *self
                .sdk_rate_window_started_ms
                .lock()
                .expect("sdk_rate_window_started_ms mutex poisoned") = 0;
            self.sdk_rate_ip_counts.lock().expect("sdk_rate_ip_counts mutex poisoned").clear();
            self.sdk_rate_principal_counts
                .lock()
                .expect("sdk_rate_principal_counts mutex poisoned")
                .clear();
        }

        Ok(RpcResponse {
            id: request.id,
            result: Some(json!({
                "runtime_id": self.identity_hash,
                "active_contract_version": active_contract_version,
                "effective_capabilities": effective_capabilities,
                "effective_limits": limits,
                "contract_release": "v2.5",
                "schema_namespace": "v2",
                "meta": self.response_meta(),
            })),
            error: None,
        })
    }

    fn handle_sdk_poll_events_v2(
        &self,
        request: RpcRequest,
    ) -> Result<RpcResponse, std::io::Error> {
        let params = request.params.ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "missing params")
        })?;
        let parsed: SdkPollEventsV2Params = serde_json::from_value(params)
            .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidInput, err))?;

        let clear_degraded_on_success = {
            let degraded =
                self.sdk_stream_degraded.lock().expect("sdk_stream_degraded mutex poisoned");
            if *degraded && parsed.cursor.is_some() {
                return Ok(self.sdk_error_response(
                    request.id,
                    "SDK_RUNTIME_STREAM_DEGRADED",
                    "stream is degraded; reset cursor to recover",
                ));
            }
            *degraded && parsed.cursor.is_none()
        };

        if parsed.max == 0 {
            return Ok(self.sdk_error_response(
                request.id,
                "SDK_VALIDATION_INVALID_ARGUMENT",
                "poll max must be greater than zero",
            ));
        }

        let max_poll_events = self.sdk_max_poll_events();
        if parsed.max > max_poll_events {
            return Ok(self.sdk_error_response(
                request.id,
                "SDK_VALIDATION_MAX_POLL_EVENTS_EXCEEDED",
                "poll max exceeds supported limit",
            ));
        }

        let cursor_seq = match self.sdk_decode_cursor(parsed.cursor.as_deref()) {
            Ok(value) => value,
            Err(error) => {
                return Ok(self.sdk_error_response(request.id, &error.code, &error.message))
            }
        };

        let log_guard = self.sdk_event_log.lock().expect("sdk_event_log mutex poisoned");
        let dropped_count =
            *self.sdk_dropped_event_count.lock().expect("sdk_dropped_event_count mutex poisoned");
        let oldest_seq = log_guard.front().map(|entry| entry.seq_no);
        let latest_seq = log_guard.back().map(|entry| entry.seq_no);

        if let (Some(cursor_seq), Some(oldest_seq)) = (cursor_seq, oldest_seq) {
            if cursor_seq.saturating_add(1) < oldest_seq {
                let mut degraded =
                    self.sdk_stream_degraded.lock().expect("sdk_stream_degraded mutex poisoned");
                *degraded = true;
                return Ok(self.sdk_error_response(
                    request.id,
                    "SDK_RUNTIME_CURSOR_EXPIRED",
                    "cursor is outside retained event window",
                ));
            }
        }

        let start_seq = cursor_seq.map(|value| value.saturating_add(1)).or(oldest_seq).unwrap_or(0);
        let mut event_rows = Vec::new();

        if parsed.cursor.is_none() && dropped_count > 0 && event_rows.len() < parsed.max {
            let observed_seq_no = oldest_seq.unwrap_or(0);
            let expected_seq_no = observed_seq_no.saturating_sub(dropped_count);
            let gap_seq_no = observed_seq_no.saturating_sub(1);
            event_rows.push(json!({
                "event_id": format!("gap-{}", gap_seq_no),
                "runtime_id": self.identity_hash,
                "stream_id": SDK_STREAM_ID,
                "seq_no": gap_seq_no,
                "contract_version": self.active_contract_version(),
                "ts_ms": (now_i64().max(0) as u64) * 1000,
                "event_type": "StreamGap",
                "severity": "warn",
                "source_component": "rns-rpc",
                "payload": {
                    "expected_seq_no": expected_seq_no,
                    "observed_seq_no": observed_seq_no,
                    "dropped_count": dropped_count,
                },
            }));
        }

        let remaining_slots = parsed.max.saturating_sub(event_rows.len());
        for entry in
            log_guard.iter().filter(|entry| entry.seq_no >= start_seq).take(remaining_slots)
        {
            event_rows.push(json!({
                "event_id": format!("evt-{}", entry.seq_no),
                "runtime_id": self.identity_hash,
                "stream_id": SDK_STREAM_ID,
                "seq_no": entry.seq_no,
                "contract_version": self.active_contract_version(),
                "ts_ms": (now_i64().max(0) as u64) * 1000,
                "event_type": entry.event.event_type.clone(),
                "severity": Self::event_severity(entry.event.event_type.as_str()),
                "source_component": "rns-rpc",
                "payload": entry.event.payload.clone(),
            }));
        }

        let next_seq = event_rows
            .iter()
            .rev()
            .find_map(|event| event.get("seq_no").and_then(JsonValue::as_u64))
            .or(cursor_seq)
            .or(latest_seq)
            .unwrap_or(0);
        let next_cursor = self.sdk_encode_cursor(next_seq);

        if clear_degraded_on_success {
            let mut degraded =
                self.sdk_stream_degraded.lock().expect("sdk_stream_degraded mutex poisoned");
            *degraded = false;
        }

        Ok(RpcResponse {
            id: request.id,
            result: Some(json!({
                "runtime_id": self.identity_hash,
                "stream_id": SDK_STREAM_ID,
                "events": event_rows,
                "next_cursor": next_cursor,
                "dropped_count": if parsed.cursor.is_none() { dropped_count } else { 0 },
                "meta": self.response_meta(),
            })),
            error: None,
        })
    }

    fn handle_sdk_cancel_message_v2(
        &self,
        request: RpcRequest,
    ) -> Result<RpcResponse, std::io::Error> {
        let params = request.params.ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "missing params")
        })?;
        let parsed: SdkCancelMessageV2Params = serde_json::from_value(params)
            .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidInput, err))?;
        let message_id = parsed.message_id.trim();
        if message_id.is_empty() {
            return Ok(self.sdk_error_response(
                request.id,
                "SDK_VALIDATION_INVALID_ARGUMENT",
                "message_id must not be empty",
            ));
        }

        let _status_guard =
            self.delivery_status_lock.lock().expect("delivery_status_lock mutex poisoned");
        let message = self.store.get_message(message_id).map_err(std::io::Error::other)?;
        if message.is_none() {
            return Ok(RpcResponse {
                id: request.id,
                result: Some(json!({
                    "message_id": message_id,
                    "result": "NotFound",
                })),
                error: None,
            });
        }

        let message_status = message.and_then(|record| record.receipt_status);

        let transitions = self
            .delivery_traces
            .lock()
            .expect("delivery traces mutex poisoned")
            .get(message_id)
            .cloned()
            .unwrap_or_default();

        let mut cancel_result = "Accepted";
        if let Some(status) = &message_status {
            let normalized = status.trim().to_ascii_lowercase();
            if normalized.starts_with("sent") {
                cancel_result = "TooLateToCancel";
            } else if matches!(
                normalized.as_str(),
                "cancelled" | "delivered" | "failed" | "expired" | "rejected"
            ) {
                cancel_result = "AlreadyTerminal";
            }
        }

        for transition in &transitions {
            if cancel_result != "Accepted" {
                break;
            }
            let normalized = transition.status.trim().to_ascii_lowercase();
            if normalized.starts_with("sent") {
                cancel_result = "TooLateToCancel";
                break;
            }
            if matches!(
                normalized.as_str(),
                "cancelled" | "delivered" | "failed" | "expired" | "rejected"
            ) {
                cancel_result = "AlreadyTerminal";
                break;
            }
        }

        if cancel_result == "Accepted" {
            self.store
                .update_receipt_status(message_id, "cancelled")
                .map_err(std::io::Error::other)?;
            self.append_delivery_trace(message_id, "cancelled".to_string());
            let event = RpcEvent {
                event_type: "delivery_cancelled".into(),
                payload: json!({ "message_id": message_id, "result": "Accepted" }),
            };
            self.push_event(event.clone());
            let _ = self.events.send(event);
        }

        Ok(RpcResponse {
            id: request.id,
            result: Some(json!({
                "message_id": message_id,
                "result": cancel_result,
            })),
            error: None,
        })
    }

    fn handle_sdk_status_v2(&self, request: RpcRequest) -> Result<RpcResponse, std::io::Error> {
        let params = request.params.ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "missing params")
        })?;
        let parsed: SdkStatusV2Params = serde_json::from_value(params)
            .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidInput, err))?;
        let message_id = parsed.message_id.trim();
        if message_id.is_empty() {
            return Ok(self.sdk_error_response(
                request.id,
                "SDK_VALIDATION_INVALID_ARGUMENT",
                "message_id must not be empty",
            ));
        }
        let message = self.store.get_message(message_id).map_err(std::io::Error::other)?;
        Ok(RpcResponse {
            id: request.id,
            result: Some(json!({
                "message": message,
                "meta": self.response_meta(),
            })),
            error: None,
        })
    }

    fn handle_sdk_configure_v2(&self, request: RpcRequest) -> Result<RpcResponse, std::io::Error> {
        let params = request.params.ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "missing params")
        })?;
        let parsed: SdkConfigureV2Params = serde_json::from_value(params)
            .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidInput, err))?;

        let patch_map = parsed.patch.as_object().ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "patch must be an object")
        })?;
        const ALLOWED_KEYS: &[&str] = &[
            "overflow_policy",
            "block_timeout_ms",
            "event_stream",
            "idempotency_ttl_ms",
            "redaction",
            "rpc_backend",
            "extensions",
        ];
        if let Some(key) = patch_map.keys().find(|key| !ALLOWED_KEYS.contains(&key.as_str())) {
            return Ok(self.sdk_error_response(
                request.id,
                "SDK_CONFIG_UNKNOWN_KEY",
                &format!("unknown config key '{key}'"),
            ));
        }

        let _apply_guard =
            self.sdk_config_apply_lock.lock().expect("sdk_config_apply_lock mutex poisoned");
        let mut revision_guard =
            self.sdk_config_revision.lock().expect("sdk_config_revision mutex poisoned");
        if parsed.expected_revision != *revision_guard {
            return Ok(self.sdk_error_response(
                request.id,
                "SDK_CONFIG_CONFLICT",
                "config revision mismatch",
            ));
        }
        *revision_guard = revision_guard.saturating_add(1);
        let revision = *revision_guard;

        {
            let mut config_guard =
                self.sdk_runtime_config.lock().expect("sdk_runtime_config mutex poisoned");
            merge_json_patch(&mut config_guard, &parsed.patch);
        }
        drop(revision_guard);

        let event = RpcEvent {
            event_type: "config_updated".into(),
            payload: json!({
                "revision": revision,
                "patch": parsed.patch,
            }),
        };
        self.push_event(event.clone());
        let _ = self.events.send(event);

        Ok(RpcResponse {
            id: request.id,
            result: Some(json!({
                "accepted": true,
                "revision": revision,
            })),
            error: None,
        })
    }

    fn handle_sdk_shutdown_v2(&self, request: RpcRequest) -> Result<RpcResponse, std::io::Error> {
        let params = request.params.ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "missing params")
        })?;
        let parsed: SdkShutdownV2Params = serde_json::from_value(params)
            .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidInput, err))?;
        let mode = parsed.mode.trim().to_ascii_lowercase();
        if mode != "graceful" && mode != "immediate" {
            return Ok(self.sdk_error_response(
                request.id,
                "SDK_VALIDATION_INVALID_ARGUMENT",
                "shutdown mode must be 'graceful' or 'immediate'",
            ));
        }

        let event = RpcEvent {
            event_type: "runtime_shutdown_requested".into(),
            payload: json!({
                "mode": mode,
                "flush_timeout_ms": parsed.flush_timeout_ms,
            }),
        };
        self.push_event(event.clone());
        let _ = self.events.send(event);

        Ok(RpcResponse {
            id: request.id,
            result: Some(json!({
                "accepted": true,
                "mode": mode,
            })),
            error: None,
        })
    }

    fn handle_sdk_snapshot_v2(&self, request: RpcRequest) -> Result<RpcResponse, std::io::Error> {
        let params = request
            .params
            .map(serde_json::from_value::<SdkSnapshotV2Params>)
            .transpose()
            .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidInput, err))?
            .unwrap_or_default();
        let active_contract_version = self.active_contract_version();
        let event_stream_position = self
            .sdk_event_log
            .lock()
            .expect("sdk_event_log mutex poisoned")
            .back()
            .map(|entry| entry.seq_no)
            .unwrap_or(0);
        let config_revision =
            *self.sdk_config_revision.lock().expect("sdk_config_revision mutex poisoned");
        let profile = self.sdk_profile.lock().expect("sdk_profile mutex poisoned").clone();
        let effective_capabilities = self
            .sdk_effective_capabilities
            .lock()
            .expect("sdk_effective_capabilities mutex poisoned")
            .clone();

        let (queued_messages, in_flight_messages) =
            self.store.count_message_buckets().map_err(std::io::Error::other)?;

        Ok(RpcResponse {
            id: request.id,
            result: Some(json!({
                "runtime_id": self.identity_hash,
                "state": "running",
                "active_contract_version": active_contract_version,
                "event_stream_position": event_stream_position,
                "config_revision": config_revision,
                "profile": profile,
                "effective_capabilities": effective_capabilities,
                "queued_messages": queued_messages,
                "in_flight_messages": in_flight_messages,
                "counts_included": params.include_counts,
                "meta": self.response_meta(),
            })),
            error: None,
        })
    }

    pub fn handle_rpc(&self, request: RpcRequest) -> Result<RpcResponse, std::io::Error> {
        match request.method.as_str() {
            "status" => Ok(RpcResponse {
                id: request.id,
                result: Some(json!({
                    "identity_hash": self.identity_hash,
                    "delivery_destination_hash": self.local_delivery_hash(),
                    "running": true
                })),
                error: None,
            }),
            "sdk_negotiate_v2" => self.handle_sdk_negotiate_v2(request),
            "daemon_status_ex" => {
                let peer_count = self.peers.lock().expect("peers mutex poisoned").len();
                let interfaces = self.interfaces.lock().expect("interfaces mutex poisoned").clone();
                let message_count =
                    self.store.list_messages(10_000, None).map_err(std::io::Error::other)?.len();
                let delivery_policy =
                    self.delivery_policy.lock().expect("policy mutex poisoned").clone();
                let propagation =
                    self.propagation_state.lock().expect("propagation mutex poisoned").clone();
                let stamp_policy = self.stamp_policy.lock().expect("stamp mutex poisoned").clone();

                Ok(RpcResponse {
                    id: request.id,
                    result: Some(json!({
                        "identity_hash": self.identity_hash,
                        "delivery_destination_hash": self.local_delivery_hash(),
                        "running": true,
                        "peer_count": peer_count,
                        "message_count": message_count,
                        "interface_count": interfaces.len(),
                        "interfaces": interfaces,
                        "delivery_policy": delivery_policy,
                        "propagation": propagation,
                        "stamp_policy": stamp_policy,
                        "capabilities": Self::capabilities(),
                    })),
                    error: None,
                })
            }
            "sdk_snapshot_v2" => self.handle_sdk_snapshot_v2(request),
            "sdk_status_v2" => self.handle_sdk_status_v2(request),
            "sdk_configure_v2" => self.handle_sdk_configure_v2(request),
            "sdk_shutdown_v2" => self.handle_sdk_shutdown_v2(request),
            "list_messages" => {
                let items = self.store.list_messages(100, None).map_err(std::io::Error::other)?;
                Ok(RpcResponse {
                    id: request.id,
                    result: Some(json!({
                        "messages": items,
                        "meta": self.response_meta(),
                    })),
                    error: None,
                })
            }
            "sdk_poll_events_v2" => self.handle_sdk_poll_events_v2(request),
            "list_announces" => {
                let parsed = request
                    .params
                    .map(serde_json::from_value::<ListAnnouncesParams>)
                    .transpose()
                    .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidInput, err))?
                    .unwrap_or_default();
                let limit = parsed.limit.unwrap_or(200).clamp(1, 5000);
                let (before_ts, before_id) = match parsed.before_ts {
                    Some(timestamp) => (Some(timestamp), None),
                    None => parse_announce_cursor(parsed.cursor.as_deref()).unwrap_or((None, None)),
                };
                let items = self
                    .store
                    .list_announces(limit, before_ts, before_id.as_deref())
                    .map_err(std::io::Error::other)?;
                let next_cursor = if items.len() >= limit {
                    items.last().map(|record| format!("{}:{}", record.timestamp, record.id))
                } else {
                    None
                };
                Ok(RpcResponse {
                    id: request.id,
                    result: Some(json!({
                        "announces": items,
                        "next_cursor": next_cursor,
                        "meta": self.response_meta(),
                    })),
                    error: None,
                })
            }
            "list_peers" => {
                let mut peers = self
                    .peers
                    .lock()
                    .expect("peers mutex poisoned")
                    .values()
                    .cloned()
                    .collect::<Vec<_>>();
                peers.sort_by(|a, b| {
                    b.last_seen.cmp(&a.last_seen).then_with(|| a.peer.cmp(&b.peer))
                });
                Ok(RpcResponse {
                    id: request.id,
                    result: Some(json!({
                        "peers": peers,
                        "meta": self.response_meta(),
                    })),
                    error: None,
                })
            }
            "list_interfaces" => {
                let interfaces = self.interfaces.lock().expect("interfaces mutex poisoned").clone();
                Ok(RpcResponse {
                    id: request.id,
                    result: Some(json!({
                        "interfaces": interfaces,
                        "meta": self.response_meta(),
                    })),
                    error: None,
                })
            }
            "set_interfaces" => {
                let params = request.params.ok_or_else(|| {
                    std::io::Error::new(std::io::ErrorKind::InvalidInput, "missing params")
                })?;
                let parsed: SetInterfacesParams = serde_json::from_value(params)
                    .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidInput, err))?;

                for iface in &parsed.interfaces {
                    if iface.kind.trim().is_empty() {
                        return Err(std::io::Error::new(
                            std::io::ErrorKind::InvalidInput,
                            "interface type is required",
                        ));
                    }
                    if iface.kind == "tcp_client" && (iface.host.is_none() || iface.port.is_none())
                    {
                        return Err(std::io::Error::new(
                            std::io::ErrorKind::InvalidInput,
                            "tcp_client requires host and port",
                        ));
                    }
                    if iface.kind == "tcp_server" && iface.port.is_none() {
                        return Err(std::io::Error::new(
                            std::io::ErrorKind::InvalidInput,
                            "tcp_server requires port",
                        ));
                    }
                }

                {
                    let mut guard = self.interfaces.lock().expect("interfaces mutex poisoned");
                    *guard = parsed.interfaces.clone();
                }

                let event = RpcEvent {
                    event_type: "interfaces_updated".into(),
                    payload: json!({ "interfaces": parsed.interfaces }),
                };
                self.push_event(event.clone());
                let _ = self.events.send(event);

                Ok(RpcResponse {
                    id: request.id,
                    result: Some(json!({ "updated": true })),
                    error: None,
                })
            }
            "reload_config" => {
                let timestamp = now_i64();
                let event = RpcEvent {
                    event_type: "config_reloaded".into(),
                    payload: json!({ "timestamp": timestamp }),
                };
                self.push_event(event.clone());
                let _ = self.events.send(event);
                Ok(RpcResponse {
                    id: request.id,
                    result: Some(json!({ "reloaded": true, "timestamp": timestamp })),
                    error: None,
                })
            }
            "peer_sync" => {
                let params = request.params.ok_or_else(|| {
                    std::io::Error::new(std::io::ErrorKind::InvalidInput, "missing params")
                })?;
                let parsed: PeerOpParams = serde_json::from_value(params)
                    .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidInput, err))?;

                let timestamp = now_i64();
                let record = self.upsert_peer(parsed.peer, timestamp, None, None);
                let event = RpcEvent {
                    event_type: "peer_sync".into(),
                    payload: json!({
                        "peer": record.peer.clone(),
                        "timestamp": timestamp,
                        "name": record.name.clone(),
                        "name_source": record.name_source.clone(),
                        "first_seen": record.first_seen,
                        "seen_count": record.seen_count,
                    }),
                };
                self.push_event(event.clone());
                let _ = self.events.send(event);

                Ok(RpcResponse {
                    id: request.id,
                    result: Some(json!({ "peer": record.peer, "synced": true })),
                    error: None,
                })
            }
            "peer_unpeer" => {
                let params = request.params.ok_or_else(|| {
                    std::io::Error::new(std::io::ErrorKind::InvalidInput, "missing params")
                })?;
                let parsed: PeerOpParams = serde_json::from_value(params)
                    .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidInput, err))?;

                let removed = {
                    let mut guard = self.peers.lock().expect("peers mutex poisoned");
                    guard.remove(&parsed.peer).is_some()
                };
                let event = RpcEvent {
                    event_type: "peer_unpeer".into(),
                    payload: json!({ "peer": parsed.peer, "removed": removed }),
                };
                self.push_event(event.clone());
                let _ = self.events.send(event);
                Ok(RpcResponse {
                    id: request.id,
                    result: Some(json!({ "removed": removed })),
                    error: None,
                })
            }
            "send_message" | "send_message_v2" => {
                let params = request.params.ok_or_else(|| {
                    std::io::Error::new(std::io::ErrorKind::InvalidInput, "missing params")
                })?;
                let parsed = parse_outbound_send_request(request.method.as_str(), params)?;

                self.store_outbound(
                    request.id,
                    parsed.id,
                    parsed.source,
                    parsed.destination,
                    parsed.title,
                    parsed.content,
                    parsed.fields,
                    parsed.method,
                    parsed.stamp_cost,
                    parsed.options,
                    parsed.include_ticket,
                )
            }
            "receive_message" => {
                let params = request.params.ok_or_else(|| {
                    std::io::Error::new(std::io::ErrorKind::InvalidInput, "missing params")
                })?;
                let parsed: ReceiveMessageParams = serde_json::from_value(params)
                    .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidInput, err))?;
                let timestamp = now_i64();
                let record = MessageRecord {
                    id: parsed.id.clone(),
                    source: parsed.source,
                    destination: parsed.destination,
                    title: parsed.title,
                    content: parsed.content,
                    timestamp,
                    direction: "in".into(),
                    fields: parsed.fields,
                    receipt_status: None,
                };
                self.store_inbound_record(record)?;
                Ok(RpcResponse {
                    id: request.id,
                    result: Some(json!({ "message_id": parsed.id })),
                    error: None,
                })
            }
            "record_receipt" => {
                let params = request.params.ok_or_else(|| {
                    std::io::Error::new(std::io::ErrorKind::InvalidInput, "missing params")
                })?;
                let parsed: RecordReceiptParams = serde_json::from_value(params)
                    .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidInput, err))?;
                let message_id = parsed.message_id;
                let requested_status = parsed.status;
                let (status, updated) = {
                    let _status_guard = self
                        .delivery_status_lock
                        .lock()
                        .expect("delivery_status_lock mutex poisoned");
                    let existing_message =
                        self.store.get_message(&message_id).map_err(std::io::Error::other)?;
                    let existing_status = existing_message
                        .as_ref()
                        .and_then(|message| message.receipt_status.clone());
                    if existing_message.is_none() {
                        (requested_status.clone(), false)
                    } else if existing_status
                        .as_deref()
                        .is_some_and(Self::is_terminal_receipt_status)
                    {
                        (existing_status.unwrap_or(requested_status.clone()), false)
                    } else {
                        self.store
                            .update_receipt_status(&message_id, &requested_status)
                            .map_err(std::io::Error::other)?;
                        (requested_status, true)
                    }
                };
                if updated {
                    self.append_delivery_trace(&message_id, status.clone());
                }
                let reason_code = delivery_reason_code(&status);
                let event = RpcEvent {
                    event_type: "receipt".into(),
                    payload: json!({
                        "message_id": message_id,
                        "status": status,
                        "updated": updated,
                        "reason_code": reason_code,
                    }),
                };
                self.push_event(event.clone());
                let _ = self.events.send(event);
                Ok(RpcResponse {
                    id: request.id,
                    result: Some(json!({
                        "message_id": message_id,
                        "status": status,
                        "updated": updated,
                        "reason_code": reason_code,
                    })),
                    error: None,
                })
            }
            "sdk_cancel_message_v2" => self.handle_sdk_cancel_message_v2(request),
            "message_delivery_trace" => {
                let params = request.params.ok_or_else(|| {
                    std::io::Error::new(std::io::ErrorKind::InvalidInput, "missing params")
                })?;
                let parsed: MessageDeliveryTraceParams = serde_json::from_value(params)
                    .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidInput, err))?;
                let traces = self
                    .delivery_traces
                    .lock()
                    .expect("delivery traces mutex poisoned")
                    .get(parsed.message_id.as_str())
                    .cloned()
                    .unwrap_or_default();
                Ok(RpcResponse {
                    id: request.id,
                    result: Some(json!({
                        "message_id": parsed.message_id,
                        "transitions": traces,
                        "meta": self.response_meta(),
                    })),
                    error: None,
                })
            }
            "get_delivery_policy" => {
                let policy = self.delivery_policy.lock().expect("policy mutex poisoned").clone();
                Ok(RpcResponse {
                    id: request.id,
                    result: Some(json!({ "policy": policy })),
                    error: None,
                })
            }
            "set_delivery_policy" => {
                let params = request.params.ok_or_else(|| {
                    std::io::Error::new(std::io::ErrorKind::InvalidInput, "missing params")
                })?;
                let parsed: DeliveryPolicyParams = serde_json::from_value(params)
                    .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidInput, err))?;

                let policy = {
                    let mut guard = self.delivery_policy.lock().expect("policy mutex poisoned");
                    if let Some(value) = parsed.auth_required {
                        guard.auth_required = value;
                    }
                    if let Some(value) = parsed.allowed_destinations {
                        guard.allowed_destinations = value;
                    }
                    if let Some(value) = parsed.denied_destinations {
                        guard.denied_destinations = value;
                    }
                    if let Some(value) = parsed.ignored_destinations {
                        guard.ignored_destinations = value;
                    }
                    if let Some(value) = parsed.prioritised_destinations {
                        guard.prioritised_destinations = value;
                    }
                    guard.clone()
                };

                Ok(RpcResponse {
                    id: request.id,
                    result: Some(json!({ "policy": policy })),
                    error: None,
                })
            }
            "propagation_status" => {
                let state =
                    self.propagation_state.lock().expect("propagation mutex poisoned").clone();
                Ok(RpcResponse {
                    id: request.id,
                    result: Some(json!({ "propagation": state })),
                    error: None,
                })
            }
            "propagation_enable" => {
                let params = request.params.ok_or_else(|| {
                    std::io::Error::new(std::io::ErrorKind::InvalidInput, "missing params")
                })?;
                let parsed: PropagationEnableParams = serde_json::from_value(params)
                    .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidInput, err))?;

                let state = {
                    let mut guard =
                        self.propagation_state.lock().expect("propagation mutex poisoned");
                    guard.enabled = parsed.enabled;
                    if parsed.store_root.is_some() {
                        guard.store_root = parsed.store_root;
                    }
                    if let Some(cost) = parsed.target_cost {
                        guard.target_cost = cost;
                    }
                    guard.clone()
                };
                Ok(RpcResponse {
                    id: request.id,
                    result: Some(json!({ "propagation": state })),
                    error: None,
                })
            }
            "propagation_ingest" => {
                let params = request.params.ok_or_else(|| {
                    std::io::Error::new(std::io::ErrorKind::InvalidInput, "missing params")
                })?;
                let parsed: PropagationIngestParams = serde_json::from_value(params)
                    .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidInput, err))?;

                let payload_hex = parsed.payload_hex.unwrap_or_default();
                let transient_id = parsed.transient_id.unwrap_or_else(|| {
                    let mut hasher = Sha256::new();
                    hasher.update(payload_hex.as_bytes());
                    encode_hex(hasher.finalize())
                });

                if !payload_hex.is_empty() {
                    self.propagation_payloads
                        .lock()
                        .expect("propagation payload mutex poisoned")
                        .insert(transient_id.clone(), payload_hex);
                }

                let state = {
                    let mut guard =
                        self.propagation_state.lock().expect("propagation mutex poisoned");
                    let ingested_count = usize::from(!transient_id.is_empty());
                    guard.last_ingest_count = ingested_count;
                    guard.total_ingested += ingested_count;
                    guard.clone()
                };

                Ok(RpcResponse {
                    id: request.id,
                    result: Some(json!({
                        "ingested_count": state.last_ingest_count,
                        "transient_id": transient_id,
                    })),
                    error: None,
                })
            }
            "propagation_fetch" => {
                let params = request.params.ok_or_else(|| {
                    std::io::Error::new(std::io::ErrorKind::InvalidInput, "missing params")
                })?;
                let parsed: PropagationFetchParams = serde_json::from_value(params)
                    .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidInput, err))?;

                let payload = self
                    .propagation_payloads
                    .lock()
                    .expect("propagation payload mutex poisoned")
                    .get(&parsed.transient_id)
                    .cloned()
                    .ok_or_else(|| {
                        std::io::Error::new(std::io::ErrorKind::NotFound, "transient_id not found")
                    })?;

                Ok(RpcResponse {
                    id: request.id,
                    result: Some(json!({
                        "transient_id": parsed.transient_id,
                        "payload_hex": payload,
                    })),
                    error: None,
                })
            }
            "get_outbound_propagation_node" => {
                let selected = self
                    .outbound_propagation_node
                    .lock()
                    .expect("propagation node mutex poisoned")
                    .clone();
                Ok(RpcResponse {
                    id: request.id,
                    result: Some(json!({
                        "peer": selected,
                        "meta": self.response_meta(),
                    })),
                    error: None,
                })
            }
            "set_outbound_propagation_node" => {
                let parsed = request
                    .params
                    .map(serde_json::from_value::<SetOutboundPropagationNodeParams>)
                    .transpose()
                    .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidInput, err))?;
                let peer = parsed
                    .and_then(|value| value.peer)
                    .map(|value| value.trim().to_string())
                    .filter(|value| !value.is_empty());
                {
                    let mut guard = self
                        .outbound_propagation_node
                        .lock()
                        .expect("propagation node mutex poisoned");
                    *guard = peer.clone();
                }
                let event = RpcEvent {
                    event_type: "propagation_node_selected".into(),
                    payload: json!({ "peer": peer }),
                };
                self.push_event(event.clone());
                let _ = self.events.send(event);
                Ok(RpcResponse {
                    id: request.id,
                    result: Some(json!({
                        "peer": peer,
                        "meta": self.response_meta(),
                    })),
                    error: None,
                })
            }
            "list_propagation_nodes" => {
                let selected = self
                    .outbound_propagation_node
                    .lock()
                    .expect("propagation node mutex poisoned")
                    .clone();
                let announces =
                    self.store.list_announces(500, None, None).map_err(std::io::Error::other)?;
                let mut by_peer: HashMap<String, PropagationNodeRecord> = HashMap::new();
                for announce in announces {
                    if !announce.capabilities.iter().any(|cap| cap == "propagation") {
                        continue;
                    }

                    let key = announce.peer.clone();
                    let entry =
                        by_peer.entry(key.clone()).or_insert_with(|| PropagationNodeRecord {
                            peer: key.clone(),
                            name: announce.name.clone(),
                            last_seen: announce.timestamp,
                            capabilities: announce.capabilities.clone(),
                            selected: selected.as_deref() == Some(key.as_str()),
                        });
                    if announce.timestamp > entry.last_seen {
                        entry.last_seen = announce.timestamp;
                        entry.name = announce.name.clone();
                        entry.capabilities = announce.capabilities.clone();
                    }
                    if selected.as_deref() == Some(key.as_str()) {
                        entry.selected = true;
                    }
                }

                let mut nodes = by_peer.into_values().collect::<Vec<_>>();
                nodes.sort_by(|a, b| {
                    b.last_seen.cmp(&a.last_seen).then_with(|| a.peer.cmp(&b.peer))
                });
                Ok(RpcResponse {
                    id: request.id,
                    result: Some(json!({
                        "nodes": nodes,
                        "meta": self.response_meta(),
                    })),
                    error: None,
                })
            }
            "paper_ingest_uri" => {
                let params = request.params.ok_or_else(|| {
                    std::io::Error::new(std::io::ErrorKind::InvalidInput, "missing params")
                })?;
                let parsed: PaperIngestUriParams = serde_json::from_value(params)
                    .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidInput, err))?;

                if !parsed.uri.starts_with("lxm://") {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        "paper URI must start with lxm://",
                    ));
                }

                let transient_id = {
                    let mut hasher = Sha256::new();
                    hasher.update(parsed.uri.as_bytes());
                    encode_hex(hasher.finalize())
                };

                let duplicate = {
                    let mut guard =
                        self.paper_ingest_seen.lock().expect("paper ingest mutex poisoned");
                    if guard.contains(&transient_id) {
                        true
                    } else {
                        guard.insert(transient_id.clone());
                        false
                    }
                };

                let body = parsed.uri.trim_start_matches("lxm://");
                let destination = first_n_chars(body, 32).unwrap_or_default();

                Ok(RpcResponse {
                    id: request.id,
                    result: Some(json!({
                        "destination": destination,
                        "transient_id": transient_id,
                        "duplicate": duplicate,
                        "bytes_len": parsed.uri.len(),
                    })),
                    error: None,
                })
            }
            "stamp_policy_get" => {
                let policy = self.stamp_policy.lock().expect("stamp mutex poisoned").clone();
                Ok(RpcResponse {
                    id: request.id,
                    result: Some(json!({ "stamp_policy": policy })),
                    error: None,
                })
            }
            "stamp_policy_set" => {
                let params = request.params.ok_or_else(|| {
                    std::io::Error::new(std::io::ErrorKind::InvalidInput, "missing params")
                })?;
                let parsed: StampPolicySetParams = serde_json::from_value(params)
                    .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidInput, err))?;

                let policy = {
                    let mut guard = self.stamp_policy.lock().expect("stamp mutex poisoned");
                    if let Some(value) = parsed.target_cost {
                        guard.target_cost = value;
                    }
                    if let Some(value) = parsed.flexibility {
                        guard.flexibility = value;
                    }
                    guard.clone()
                };

                Ok(RpcResponse {
                    id: request.id,
                    result: Some(json!({ "stamp_policy": policy })),
                    error: None,
                })
            }
            "ticket_generate" => {
                let params = request.params.ok_or_else(|| {
                    std::io::Error::new(std::io::ErrorKind::InvalidInput, "missing params")
                })?;
                let parsed: TicketGenerateParams = serde_json::from_value(params)
                    .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidInput, err))?;

                let ttl_secs = parsed.ttl_secs.unwrap_or(3600);
                let ttl = i64::try_from(ttl_secs).map_err(|_| {
                    std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        format!("ttl_secs exceeds supported range: {ttl_secs}"),
                    )
                })?;
                let now = now_i64();
                let expires_at = now.checked_add(ttl).ok_or_else(|| {
                    std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        format!("ttl_secs causes timestamp overflow: {ttl_secs}"),
                    )
                })?;
                let mut hasher = Sha256::new();
                hasher.update(parsed.destination.as_bytes());
                hasher.update(now.to_be_bytes());
                let ticket = encode_hex(hasher.finalize());
                let record = TicketRecord {
                    destination: parsed.destination.clone(),
                    ticket: ticket.clone(),
                    expires_at,
                };

                self.ticket_cache
                    .lock()
                    .expect("ticket mutex poisoned")
                    .insert(parsed.destination, record.clone());

                Ok(RpcResponse {
                    id: request.id,
                    result: Some(json!({
                        "ticket": record.ticket,
                        "destination": record.destination,
                        "expires_at": record.expires_at,
                        "ttl_secs": ttl_secs,
                    })),
                    error: None,
                })
            }
            "announce_now" => {
                let timestamp = now_i64();
                if let Some(bridge) = &self.announce_bridge {
                    let _ = bridge.announce_now();
                }
                let event = RpcEvent {
                    event_type: "announce_sent".into(),
                    payload: json!({ "timestamp": timestamp }),
                };
                self.push_event(event.clone());
                let _ = self.events.send(event);
                Ok(RpcResponse {
                    id: request.id,
                    result: Some(json!({ "announce_id": request.id })),
                    error: None,
                })
            }
            "announce_received" => {
                let params = request.params.ok_or_else(|| {
                    std::io::Error::new(std::io::ErrorKind::InvalidInput, "missing params")
                })?;
                let parsed: AnnounceReceivedParams = serde_json::from_value(params)
                    .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidInput, err))?;
                let timestamp = parsed.timestamp.unwrap_or_else(now_i64);
                let peer = parsed.peer.clone();
                let (parsed_stamp_cost_flexibility, parsed_peering_cost) =
                    parse_announce_costs_from_app_data_hex(parsed.app_data_hex.as_deref());
                let stamp_cost_flexibility =
                    parsed.stamp_cost_flexibility.or(parsed_stamp_cost_flexibility);
                let peering_cost = parsed.peering_cost.or(parsed_peering_cost);
                self.accept_announce_with_metadata(
                    parsed.peer,
                    timestamp,
                    parsed.name,
                    parsed.name_source,
                    parsed.app_data_hex,
                    parsed.capabilities,
                    parsed.rssi,
                    parsed.snr,
                    parsed.q,
                    None,
                    Some(stamp_cost_flexibility),
                    Some(peering_cost),
                    None,
                    None,
                    None,
                    None,
                    None,
                    None,
                )?;
                let record =
                    self.peers.lock().expect("peers mutex poisoned").get(peer.as_str()).cloned();
                Ok(RpcResponse {
                    id: request.id,
                    result: Some(json!({ "peer": record })),
                    error: None,
                })
            }
            "clear_messages" => {
                self.store.clear_messages().map_err(std::io::Error::other)?;
                Ok(RpcResponse {
                    id: request.id,
                    result: Some(json!({ "cleared": "messages" })),
                    error: None,
                })
            }
            "clear_resources" => Ok(RpcResponse {
                id: request.id,
                result: Some(json!({ "cleared": "resources" })),
                error: None,
            }),
            "clear_peers" => {
                {
                    let mut guard = self.peers.lock().expect("peers mutex poisoned");
                    guard.clear();
                }
                self.store.clear_announces().map_err(std::io::Error::other)?;
                Ok(RpcResponse {
                    id: request.id,
                    result: Some(json!({ "cleared": "peers" })),
                    error: None,
                })
            }
            "clear_all" => {
                self.store.clear_messages().map_err(std::io::Error::other)?;
                self.store.clear_announces().map_err(std::io::Error::other)?;
                {
                    let mut guard = self.peers.lock().expect("peers mutex poisoned");
                    guard.clear();
                }
                {
                    let mut guard =
                        self.delivery_traces.lock().expect("delivery traces mutex poisoned");
                    guard.clear();
                }
                Ok(RpcResponse {
                    id: request.id,
                    result: Some(json!({ "cleared": "all" })),
                    error: None,
                })
            }
            _ => Ok(RpcResponse {
                id: request.id,
                result: None,
                error: Some(RpcError {
                    code: "NOT_IMPLEMENTED".into(),
                    message: "method not implemented".into(),
                }),
            }),
        }
    }

    fn append_delivery_trace(&self, message_id: &str, status: String) {
        const MAX_DELIVERY_TRACE_ENTRIES: usize = 32;
        const MAX_TRACKED_MESSAGE_TRACES: usize = 2048;

        let timestamp = now_i64();
        let reason_code = delivery_reason_code(&status).map(ToOwned::to_owned);
        let mut guard = self.delivery_traces.lock().expect("delivery traces mutex poisoned");
        let entry = guard.entry(message_id.to_string()).or_default();
        entry.push(DeliveryTraceEntry { status, timestamp, reason_code });
        if entry.len() > MAX_DELIVERY_TRACE_ENTRIES {
            let drain_count = entry.len().saturating_sub(MAX_DELIVERY_TRACE_ENTRIES);
            entry.drain(0..drain_count);
        }

        if guard.len() > MAX_TRACKED_MESSAGE_TRACES {
            let overflow = guard.len() - MAX_TRACKED_MESSAGE_TRACES;
            let mut evicted_ids = Vec::with_capacity(overflow);
            for key in guard.keys() {
                if key != message_id {
                    evicted_ids.push(key.clone());
                    if evicted_ids.len() == overflow {
                        break;
                    }
                }
            }
            for id in evicted_ids {
                guard.remove(&id);
            }

            if guard.len() > MAX_TRACKED_MESSAGE_TRACES {
                let still_over = guard.len() - MAX_TRACKED_MESSAGE_TRACES;
                let mut fallback = Vec::with_capacity(still_over);
                for key in guard.keys().take(still_over).cloned() {
                    fallback.push(key);
                }
                for id in fallback {
                    guard.remove(&id);
                }
            }
        }
    }

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
                return Err(RpcError {
                    code: "SDK_CAPABILITY_DISABLED".to_string(),
                    message:
                        "mtls auth mode is not available until transport-bound certificate verification is implemented"
                            .to_string(),
                });
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
            "sdk.capability.receipt_terminality".to_string(),
            "sdk.capability.config_revision_cas".to_string(),
            "sdk.capability.idempotency_ttl".to_string(),
        ]
    }

    fn sdk_supported_capabilities_for_profile(profile: &str) -> Vec<String> {
        let mut caps = Self::sdk_supported_capabilities();
        if profile == "embedded-alloc" {
            caps.retain(|capability| capability != "sdk.capability.async_events");
        }
        caps
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
            "sdk_negotiate_v2",
            "sdk_status_v2",
            "sdk_configure_v2",
            "sdk_poll_events_v2",
            "sdk_cancel_message_v2",
            "sdk_snapshot_v2",
            "sdk_shutdown_v2",
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

    pub fn handle_framed_request(&self, bytes: &[u8]) -> Result<Vec<u8>, std::io::Error> {
        let request: RpcRequest = codec::decode_frame(bytes)
            .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidInput, err))?;
        let response = self.handle_rpc(request)?;
        codec::encode_frame(&response).map_err(std::io::Error::other)
    }

    pub fn subscribe_events(&self) -> broadcast::Receiver<RpcEvent> {
        self.events.subscribe()
    }

    pub fn take_event(&self) -> Option<RpcEvent> {
        let mut guard = self.event_queue.lock().expect("event_queue mutex poisoned");
        guard.pop_front()
    }

    pub fn push_event(&self, event: RpcEvent) {
        {
            let mut guard = self.event_queue.lock().expect("event_queue mutex poisoned");
            if guard.len() >= 32 {
                guard.pop_front();
            }
            guard.push_back(event.clone());
        }

        let seq_no = {
            let mut seq_guard =
                self.sdk_next_event_seq.lock().expect("sdk_next_event_seq mutex poisoned");
            *seq_guard = seq_guard.saturating_add(1);
            *seq_guard
        };
        let mut log_guard = self.sdk_event_log.lock().expect("sdk_event_log mutex poisoned");
        if log_guard.len() >= SDK_EVENT_LOG_CAPACITY {
            log_guard.pop_front();
            let mut dropped = self
                .sdk_dropped_event_count
                .lock()
                .expect("sdk_dropped_event_count mutex poisoned");
            *dropped = dropped.saturating_add(1);
        }
        log_guard.push_back(SequencedRpcEvent { seq_no, event });
    }

    pub fn emit_event(&self, event: RpcEvent) {
        self.push_event(event.clone());
        let _ = self.events.send(event);
    }

    pub fn schedule_announce_for_test(&self, id: u64) {
        let timestamp = now_i64();
        let event = RpcEvent {
            event_type: "announce_sent".into(),
            payload: json!({ "timestamp": timestamp, "announce_id": id }),
        };
        self.push_event(event.clone());
        let _ = self.events.send(event);
    }

    pub fn start_announce_scheduler(
        self: std::rc::Rc<Self>,
        interval_secs: u64,
    ) -> tokio::task::JoinHandle<()> {
        tokio::task::spawn_local(async move {
            if interval_secs == 0 {
                return;
            }

            let mut interval = tokio::time::interval(Duration::from_secs(interval_secs));
            loop {
                // First tick is immediate, so we announce once at scheduler start.
                interval.tick().await;
                let id = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|value| value.as_secs())
                    .unwrap_or(0);

                if let Some(bridge) = &self.announce_bridge {
                    let _ = bridge.announce_now();
                }

                let timestamp = now_i64();
                let event = RpcEvent {
                    event_type: "announce_sent".into(),
                    payload: json!({ "timestamp": timestamp, "announce_id": id }),
                };
                self.push_event(event.clone());
                let _ = self.events.send(event);
            }
        })
    }

    pub fn inject_inbound_test_message(&self, content: &str) {
        let timestamp = now_i64();
        let record = crate::storage::messages::MessageRecord {
            id: format!("test-{}", timestamp),
            source: "test-peer".into(),
            destination: "local".into(),
            title: "".into(),
            content: content.into(),
            timestamp,
            direction: "in".into(),
            fields: None,
            receipt_status: None,
        };
        let _ = self.store.insert_message(&record);
        let event =
            RpcEvent { event_type: "inbound".into(), payload: json!({ "message": record }) };
        self.push_event(event.clone());
        let _ = self.events.send(event);
    }

    pub fn emit_link_event_for_test(&self) {
        let event = RpcEvent {
            event_type: "link_activated".into(),
            payload: json!({ "link_id": "test-link" }),
        };
        self.push_event(event.clone());
        let _ = self.events.send(event);
    }
}

#[derive(Debug)]
struct SdkCursorError {
    code: String,
    message: String,
}

fn parse_announce_cursor(cursor: Option<&str>) -> Option<(Option<i64>, Option<String>)> {
    let raw = cursor?.trim();
    if raw.is_empty() {
        return None;
    }
    if let Some((timestamp_raw, id)) = raw.split_once(':') {
        let timestamp = timestamp_raw.parse::<i64>().ok()?;
        let before_id = if id.is_empty() { None } else { Some(id.to_string()) };
        return Some((Some(timestamp), before_id));
    }
    raw.parse::<i64>().ok().map(|timestamp| (Some(timestamp), None))
}

fn delivery_reason_code(status: &str) -> Option<&'static str> {
    let normalized = status.trim().to_ascii_lowercase();
    if normalized.is_empty() {
        return None;
    }
    if normalized.contains("receipt timeout") {
        return Some("receipt_timeout");
    }
    if normalized.contains("timeout") {
        return Some("timeout");
    }
    if normalized.contains("no route")
        || normalized.contains("no path")
        || normalized.contains("no known path")
    {
        return Some("no_path");
    }
    if normalized.contains("no propagation relay selected") {
        return Some("relay_unset");
    }
    if normalized.contains("retry budget exhausted") {
        return Some("retry_budget_exhausted");
    }
    None
}

fn merge_json_patch(target: &mut JsonValue, patch: &JsonValue) {
    let JsonValue::Object(patch_map) = patch else {
        *target = patch.clone();
        return;
    };

    if !target.is_object() {
        *target = JsonValue::Object(JsonMap::new());
    }
    let target_map = target.as_object_mut().expect("target must be object after initialization");
    for (key, value) in patch_map {
        if value.is_null() {
            target_map.remove(key);
            continue;
        }
        match target_map.get_mut(key) {
            Some(existing) if existing.is_object() && value.is_object() => {
                merge_json_patch(existing, value);
            }
            _ => {
                target_map.insert(key.clone(), value.clone());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rpc_request(id: u64, method: &str, params: JsonValue) -> RpcRequest {
        RpcRequest { id, method: method.to_string(), params: Some(params) }
    }

    #[test]
    fn sdk_negotiate_v2_selects_contract_and_profile_limits() {
        let daemon = RpcDaemon::test_instance();
        let response = daemon
            .handle_rpc(rpc_request(
                1,
                "sdk_negotiate_v2",
                json!({
                    "supported_contract_versions": [1, 2],
                    "requested_capabilities": [
                        "sdk.capability.cursor_replay",
                        "sdk.capability.async_events"
                    ],
                    "config": {
                        "profile": "desktop-local-runtime"
                    }
                }),
            ))
            .expect("negotiate should succeed");
        assert!(response.error.is_none());
        let result = response.result.expect("result");
        assert_eq!(result["active_contract_version"], json!(2));
        assert_eq!(result["contract_release"], json!("v2.5"));
        assert_eq!(result["effective_limits"]["max_poll_events"], json!(64));
    }

    #[test]
    fn sdk_negotiate_v2_fails_on_capability_overlap_miss() {
        let daemon = RpcDaemon::test_instance();
        let response = daemon
            .handle_rpc(rpc_request(
                2,
                "sdk_negotiate_v2",
                json!({
                    "supported_contract_versions": [2],
                    "requested_capabilities": ["sdk.capability.not-real"],
                    "config": { "profile": "desktop-full" }
                }),
            ))
            .expect("rpc call");
        let error = response.error.expect("must fail");
        assert_eq!(error.code, "SDK_CAPABILITY_CONTRACT_INCOMPATIBLE");
    }

    #[test]
    fn sdk_negotiate_v2_rejects_embedded_alloc_profile() {
        let daemon = RpcDaemon::test_instance();
        let response = daemon
            .handle_rpc(rpc_request(
                20,
                "sdk_negotiate_v2",
                json!({
                    "supported_contract_versions": [2],
                    "requested_capabilities": [],
                    "config": { "profile": "embedded-alloc" }
                }),
            ))
            .expect("rpc call");
        let error = response.error.expect("must fail");
        assert_eq!(error.code, "SDK_CAPABILITY_CONTRACT_INCOMPATIBLE");
    }

    #[test]
    fn sdk_security_authorize_http_request_blocks_remote_source_in_local_only_mode() {
        let daemon = RpcDaemon::test_instance();
        let _ = daemon.handle_rpc(rpc_request(
            21,
            "sdk_negotiate_v2",
            json!({
                "supported_contract_versions": [2],
                "requested_capabilities": [],
                "config": {
                    "profile": "desktop-full",
                    "bind_mode": "local_only",
                    "auth_mode": "local_trusted"
                }
            }),
        ));

        let err = daemon
            .authorize_http_request(&[], Some("10.1.2.3"))
            .expect_err("remote source should be rejected in local_only mode");
        assert_eq!(err.code, "SDK_SECURITY_REMOTE_BIND_DISALLOWED");
    }

    #[test]
    fn sdk_security_forwarded_headers_require_trusted_proxy_allowlist() {
        let daemon = RpcDaemon::test_instance();
        let _ = daemon.handle_rpc(rpc_request(
            21,
            "sdk_negotiate_v2",
            json!({
                "supported_contract_versions": [2],
                "requested_capabilities": [],
                "config": {
                    "profile": "desktop-full",
                    "bind_mode": "local_only",
                    "auth_mode": "local_trusted"
                }
            }),
        ));
        let _ = daemon.handle_rpc(rpc_request(
            22,
            "sdk_configure_v2",
            json!({
                "expected_revision": 0,
                "patch": {
                    "extensions": {
                        "trusted_proxy": true,
                        "trusted_proxy_ips": ["127.0.0.1"]
                    }
                }
            }),
        ));

        let forwarded = vec![("x-forwarded-for".to_string(), "127.0.0.1".to_string())];
        let err = daemon
            .authorize_http_request(&forwarded, Some("10.9.8.7"))
            .expect_err("untrusted proxy peer must not be able to spoof forwarded headers");
        assert_eq!(err.code, "SDK_SECURITY_REMOTE_BIND_DISALLOWED");

        daemon
            .authorize_http_request(&forwarded, Some("127.0.0.1"))
            .expect("allowlisted proxy may forward loopback source");
    }

    #[test]
    fn sdk_security_authorize_http_request_rejects_replayed_token_jti() {
        let daemon = RpcDaemon::test_instance();
        let response = daemon
            .handle_rpc(rpc_request(
                22,
                "sdk_negotiate_v2",
                json!({
                    "supported_contract_versions": [2],
                    "requested_capabilities": [],
                    "config": {
                        "profile": "desktop-full",
                        "bind_mode": "remote",
                        "auth_mode": "token",
                        "rpc_backend": {
                            "token_auth": {
                                "issuer": "test-issuer",
                                "audience": "test-audience",
                                "jti_cache_ttl_ms": 30_000,
                                "clock_skew_ms": 0,
                                "shared_secret": "test-secret"
                            }
                        }
                    }
                }),
            ))
            .expect("negotiate");
        assert!(response.error.is_none());

        let iat = now_seconds_u64();
        let exp = iat.saturating_add(60);
        let payload =
            format!("iss=test-issuer;aud=test-audience;jti=token-1;sub=cli;iat={iat};exp={exp}");
        let signature =
            RpcDaemon::token_signature("test-secret", payload.as_str()).expect("token signature");
        let token = format!("{payload};sig={signature}");
        let headers = vec![("authorization".to_string(), format!("Bearer {token}"))];
        daemon.authorize_http_request(&headers, Some("10.5.6.7")).expect("first token should pass");
        let replay = daemon
            .authorize_http_request(&headers, Some("10.5.6.7"))
            .expect_err("replayed token jti should be rejected");
        assert_eq!(replay.code, "SDK_SECURITY_TOKEN_REPLAYED");
    }

    #[test]
    fn sdk_security_authorize_http_request_rejects_invalid_token_signature_and_expiry() {
        let daemon = RpcDaemon::test_instance();
        let response = daemon
            .handle_rpc(rpc_request(
                23,
                "sdk_negotiate_v2",
                json!({
                    "supported_contract_versions": [2],
                    "requested_capabilities": [],
                    "config": {
                        "profile": "desktop-full",
                        "bind_mode": "remote",
                        "auth_mode": "token",
                        "rpc_backend": {
                            "token_auth": {
                                "issuer": "test-issuer",
                                "audience": "test-audience",
                                "jti_cache_ttl_ms": 30_000,
                                "clock_skew_ms": 0,
                                "shared_secret": "test-secret"
                            }
                        }
                    }
                }),
            ))
            .expect("negotiate");
        assert!(response.error.is_none());

        let now = now_seconds_u64();
        let expired_payload = format!(
            "iss=test-issuer;aud=test-audience;jti=expired-1;sub=cli;iat={};exp={}",
            now.saturating_sub(120),
            now.saturating_sub(60)
        );
        let expired_sig = RpcDaemon::token_signature("test-secret", expired_payload.as_str())
            .expect("token signature");
        let expired_headers = vec![(
            "authorization".to_string(),
            format!("Bearer {expired_payload};sig={expired_sig}"),
        )];
        let expired = daemon
            .authorize_http_request(&expired_headers, Some("10.5.6.7"))
            .expect_err("expired token should be rejected");
        assert_eq!(expired.code, "SDK_SECURITY_TOKEN_INVALID");

        let valid_payload = format!(
            "iss=test-issuer;aud=test-audience;jti=tampered-1;sub=cli;iat={now};exp={}",
            now.saturating_add(60)
        );
        let tampered_headers =
            vec![("authorization".to_string(), format!("Bearer {valid_payload};sig=deadbeef"))];
        let tampered = daemon
            .authorize_http_request(&tampered_headers, Some("10.5.6.7"))
            .expect_err("tampered signature should be rejected");
        assert_eq!(tampered.code, "SDK_SECURITY_TOKEN_INVALID");
    }

    #[test]
    fn sdk_security_authorize_http_request_enforces_rate_limits_and_emits_event() {
        let daemon = RpcDaemon::test_instance();
        let _ = daemon.handle_rpc(rpc_request(
            23,
            "sdk_negotiate_v2",
            json!({
                "supported_contract_versions": [2],
                "requested_capabilities": [],
                "config": {
                    "profile": "desktop-full",
                    "bind_mode": "local_only",
                    "auth_mode": "local_trusted"
                }
            }),
        ));
        let _ = daemon.handle_rpc(rpc_request(
            24,
            "sdk_configure_v2",
            json!({
                "expected_revision": 0,
                "patch": {
                    "extensions": {
                        "rate_limits": {
                            "per_ip_per_minute": 1,
                            "per_principal_per_minute": 1
                        }
                    }
                }
            }),
        ));

        daemon.authorize_http_request(&[], Some("127.0.0.1")).expect("first request should pass");
        let limited = daemon
            .authorize_http_request(&[], Some("127.0.0.1"))
            .expect_err("second request should be rate limited");
        assert_eq!(limited.code, "SDK_SECURITY_RATE_LIMITED");

        let mut found_security_event = false;
        for _ in 0..8 {
            let Some(event) = daemon.take_event() else {
                break;
            };
            if event.event_type == "sdk_security_rate_limited" {
                found_security_event = true;
                break;
            }
        }
        assert!(found_security_event, "rate-limit violations should emit security event");
    }

    #[test]
    fn sdk_poll_events_v2_validates_cursor_and_expires_stale_tokens() {
        let daemon = RpcDaemon::test_instance();
        daemon.emit_event(RpcEvent {
            event_type: "inbound".to_string(),
            payload: json!({ "message_id": "m-1" }),
        });
        let first = daemon
            .handle_rpc(rpc_request(
                3,
                "sdk_poll_events_v2",
                json!({
                    "cursor": null,
                    "max": 4
                }),
            ))
            .expect("poll");
        let first_result = first.result.expect("result");
        let cursor = first_result["next_cursor"].as_str().expect("cursor").to_string();
        assert!(first_result["events"].as_array().is_some_and(|events| !events.is_empty()));

        let invalid = daemon
            .handle_rpc(rpc_request(
                4,
                "sdk_poll_events_v2",
                json!({
                    "cursor": "bad-cursor",
                    "max": 4
                }),
            ))
            .expect("invalid poll should still return response");
        assert_eq!(invalid.error.expect("error").code, "SDK_RUNTIME_INVALID_CURSOR");

        for idx in 0..(SDK_EVENT_LOG_CAPACITY + 8) {
            daemon.emit_event(RpcEvent {
                event_type: "inbound".to_string(),
                payload: json!({ "message_id": format!("overflow-{idx}") }),
            });
        }

        let expired = daemon
            .handle_rpc(rpc_request(
                5,
                "sdk_poll_events_v2",
                json!({
                    "cursor": cursor,
                    "max": 2
                }),
            ))
            .expect("expired poll should return response");
        assert_eq!(expired.error.expect("error").code, "SDK_RUNTIME_CURSOR_EXPIRED");
    }

    #[test]
    fn sdk_poll_events_v2_requires_successful_reset_after_degraded_state() {
        let daemon = RpcDaemon::test_instance();
        daemon.emit_event(RpcEvent { event_type: "inbound".to_string(), payload: json!({}) });
        let first = daemon
            .handle_rpc(rpc_request(
                30,
                "sdk_poll_events_v2",
                json!({
                    "cursor": null,
                    "max": 1
                }),
            ))
            .expect("initial poll");
        let cursor =
            first.result.expect("result")["next_cursor"].as_str().expect("cursor").to_string();

        for idx in 0..(SDK_EVENT_LOG_CAPACITY + 4) {
            daemon.emit_event(RpcEvent {
                event_type: "inbound".to_string(),
                payload: json!({ "idx": idx }),
            });
        }

        let expired = daemon
            .handle_rpc(rpc_request(
                31,
                "sdk_poll_events_v2",
                json!({
                    "cursor": cursor,
                    "max": 1
                }),
            ))
            .expect("expired");
        assert_eq!(expired.error.expect("error").code, "SDK_RUNTIME_CURSOR_EXPIRED");

        let invalid_reset = daemon
            .handle_rpc(rpc_request(
                32,
                "sdk_poll_events_v2",
                json!({
                    "cursor": null,
                    "max": 0
                }),
            ))
            .expect("invalid reset");
        assert_eq!(invalid_reset.error.expect("error").code, "SDK_VALIDATION_INVALID_ARGUMENT");

        let still_degraded = daemon
            .handle_rpc(rpc_request(
                33,
                "sdk_poll_events_v2",
                json!({
                    "cursor": "v2:test-identity:sdk-events:999999",
                    "max": 1
                }),
            ))
            .expect("still degraded");
        assert_eq!(still_degraded.error.expect("error").code, "SDK_RUNTIME_STREAM_DEGRADED");

        let reset_ok = daemon
            .handle_rpc(rpc_request(
                34,
                "sdk_poll_events_v2",
                json!({
                    "cursor": null,
                    "max": 1
                }),
            ))
            .expect("reset");
        assert!(reset_ok.error.is_none());
    }

    #[test]
    fn sdk_cancel_message_v2_distinguishes_not_found_and_too_late() {
        let daemon = RpcDaemon::test_instance();

        let not_found = daemon
            .handle_rpc(rpc_request(6, "sdk_cancel_message_v2", json!({ "message_id": "missing" })))
            .expect("cancel missing");
        assert_eq!(not_found.result.expect("result")["result"], json!("NotFound"));

        let send = daemon
            .handle_rpc(rpc_request(
                7,
                "send_message_v2",
                json!({
                    "id": "outbound-1",
                    "source": "src",
                    "destination": "dst",
                    "title": "",
                    "content": "hello"
                }),
            ))
            .expect("send");
        assert!(send.error.is_none());

        let too_late = daemon
            .handle_rpc(rpc_request(
                8,
                "sdk_cancel_message_v2",
                json!({ "message_id": "outbound-1" }),
            ))
            .expect("cancel");
        assert_eq!(too_late.result.expect("result")["result"], json!("TooLateToCancel"));
    }

    #[test]
    fn sdk_status_v2_returns_message_record() {
        let daemon = RpcDaemon::test_instance();
        let _ = daemon
            .handle_rpc(rpc_request(
                40,
                "send_message_v2",
                json!({
                    "id": "status-1",
                    "source": "src",
                    "destination": "dst",
                    "title": "",
                    "content": "hello"
                }),
            ))
            .expect("send");
        let response = daemon
            .handle_rpc(rpc_request(
                41,
                "sdk_status_v2",
                json!({
                    "message_id": "status-1"
                }),
            ))
            .expect("status");
        assert_eq!(response.result.expect("result")["message"]["id"], json!("status-1"));
    }

    #[test]
    fn sdk_property_terminal_receipt_status_is_sticky() {
        let daemon = RpcDaemon::test_instance();
        let _ = daemon
            .handle_rpc(rpc_request(
                45,
                "send_message_v2",
                json!({
                    "id": "property-1",
                    "source": "src",
                    "destination": "dst",
                    "title": "",
                    "content": "hello"
                }),
            ))
            .expect("send");

        let delivered = daemon
            .handle_rpc(rpc_request(
                46,
                "record_receipt",
                json!({
                    "message_id": "property-1",
                    "status": "delivered"
                }),
            ))
            .expect("record delivered");
        assert_eq!(delivered.result.expect("result")["updated"], json!(true));
        let trace_before = daemon
            .handle_rpc(rpc_request(
                460,
                "message_delivery_trace",
                json!({
                    "message_id": "property-1"
                }),
            ))
            .expect("trace before ignored update");
        let trace_before_len = trace_before.result.expect("result")["transitions"]
            .as_array()
            .expect("trace entries")
            .len();

        let ignored = daemon
            .handle_rpc(rpc_request(
                47,
                "record_receipt",
                json!({
                    "message_id": "property-1",
                    "status": "sent: direct"
                }),
            ))
            .expect("record after terminal");
        let ignored_result = ignored.result.expect("result");
        assert_eq!(ignored_result["updated"], json!(false));
        assert_eq!(ignored_result["status"], json!("delivered"));
        let trace_after = daemon
            .handle_rpc(rpc_request(
                470,
                "message_delivery_trace",
                json!({
                    "message_id": "property-1"
                }),
            ))
            .expect("trace after ignored update");
        let trace_after_len = trace_after.result.expect("result")["transitions"]
            .as_array()
            .expect("trace entries")
            .len();
        assert_eq!(
            trace_after_len, trace_before_len,
            "ignored terminal updates must not append delivery trace entries"
        );

        let status = daemon
            .handle_rpc(rpc_request(
                48,
                "sdk_status_v2",
                json!({
                    "message_id": "property-1"
                }),
            ))
            .expect("status");
        assert_eq!(status.result.expect("result")["message"]["receipt_status"], json!("delivered"));
    }

    #[test]
    fn sdk_property_event_sequence_is_monotonic() {
        let daemon = RpcDaemon::test_instance();
        daemon.emit_event(RpcEvent {
            event_type: "property".to_string(),
            payload: json!({ "idx": 1 }),
        });
        daemon.emit_event(RpcEvent {
            event_type: "property".to_string(),
            payload: json!({ "idx": 2 }),
        });

        let response = daemon
            .handle_rpc(rpc_request(
                49,
                "sdk_poll_events_v2",
                json!({
                    "cursor": null,
                    "max": 2
                }),
            ))
            .expect("poll");
        let events =
            response.result.expect("result")["events"].as_array().expect("events array").to_vec();
        assert_eq!(events.len(), 2);
        let first = events[0]["seq_no"].as_u64().expect("first seq");
        let second = events[1]["seq_no"].as_u64().expect("second seq");
        assert!(second > first, "event sequence must be strictly increasing");
    }

    #[test]
    fn sdk_configure_v2_applies_revision_cas() {
        let daemon = RpcDaemon::test_instance();
        let first = daemon
            .handle_rpc(rpc_request(
                42,
                "sdk_configure_v2",
                json!({
                    "expected_revision": 0,
                    "patch": { "event_stream": { "max_poll_events": 64 } }
                }),
            ))
            .expect("configure");
        assert_eq!(first.result.expect("result")["revision"], json!(1));

        let conflict = daemon
            .handle_rpc(rpc_request(
                43,
                "sdk_configure_v2",
                json!({
                    "expected_revision": 0,
                    "patch": { "event_stream": { "max_poll_events": 32 } }
                }),
            ))
            .expect("configure conflict");
        assert_eq!(conflict.error.expect("error").code, "SDK_CONFIG_CONFLICT");
    }

    #[test]
    fn sdk_shutdown_v2_accepts_graceful_mode() {
        let daemon = RpcDaemon::test_instance();
        let response = daemon
            .handle_rpc(rpc_request(
                44,
                "sdk_shutdown_v2",
                json!({
                    "mode": "graceful"
                }),
            ))
            .expect("shutdown");
        assert!(response.error.is_none());
        assert_eq!(response.result.expect("result")["accepted"], json!(true));
    }

    #[test]
    fn sdk_snapshot_v2_returns_runtime_summary() {
        let daemon = RpcDaemon::test_instance();
        let _ = daemon.handle_rpc(rpc_request(
            9,
            "sdk_negotiate_v2",
            json!({
                "supported_contract_versions": [2],
                "requested_capabilities": [],
                "config": { "profile": "desktop-full" }
            }),
        ));

        let snapshot = daemon
            .handle_rpc(rpc_request(10, "sdk_snapshot_v2", json!({ "include_counts": true })))
            .expect("snapshot");
        assert!(snapshot.error.is_none());
        let result = snapshot.result.expect("result");
        assert_eq!(result["runtime_id"], json!("test-identity"));
        assert_eq!(result["state"], json!("running"));
        assert!(result.get("event_stream_position").is_some());
    }
}
