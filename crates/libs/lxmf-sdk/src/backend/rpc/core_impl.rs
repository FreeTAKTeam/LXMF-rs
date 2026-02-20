use super::*;
use serde_json::json;

impl RpcBackendClient {
    pub(super) fn negotiate_impl(
        &self,
        req: NegotiationRequest,
    ) -> Result<NegotiationResponse, SdkError> {
        let session_auth = self.session_auth_from_request(&req)?;
        let headers = self.headers_for_session_auth(&session_auth);
        let rpc_backend = req.rpc_backend.as_ref().map(|config| {
            json!({
                "listen_addr": config.listen_addr,
                "read_timeout_ms": config.read_timeout_ms,
                "write_timeout_ms": config.write_timeout_ms,
                "max_header_bytes": config.max_header_bytes,
                "max_body_bytes": config.max_body_bytes,
                "token_auth": config.token_auth.as_ref().map(|token| json!({
                    "issuer": token.issuer,
                    "audience": token.audience,
                    "jti_cache_ttl_ms": token.jti_cache_ttl_ms,
                    "clock_skew_ms": token.clock_skew_ms,
                    "shared_secret": token.shared_secret,
                })),
                "mtls_auth": config.mtls_auth.as_ref().map(|mtls| json!({
                    "ca_bundle_path": mtls.ca_bundle_path,
                    "require_client_cert": mtls.require_client_cert,
                    "allowed_san": mtls.allowed_san,
                })),
            })
        });
        let result = self.call_rpc_with_headers(
            "sdk_negotiate_v2",
            Some(json!({
                "supported_contract_versions": req.supported_contract_versions,
                "requested_capabilities": req.requested_capabilities,
                "config": {
                    "profile": Self::profile_to_wire(req.profile),
                    "bind_mode": Self::bind_mode_to_wire(req.bind_mode),
                    "auth_mode": Self::auth_mode_to_wire(req.auth_mode),
                    "overflow_policy": Self::overflow_policy_to_wire(req.overflow_policy),
                    "block_timeout_ms": req.block_timeout_ms,
                    "rpc_backend": rpc_backend,
                }
            })),
            &headers,
        )?;

        let runtime_id = Self::parse_required_string(&result, "runtime_id")?;
        let active_contract_version = Self::parse_required_u16(&result, "active_contract_version")?;
        let effective_capabilities = result
            .get("effective_capabilities")
            .and_then(JsonValue::as_array)
            .map(|values| {
                values.iter().filter_map(JsonValue::as_str).map(str::to_owned).collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let effective_limits =
            Self::parse_effective_limits(result.get("effective_limits").ok_or_else(|| {
                SdkError::new(
                    code::INTERNAL,
                    ErrorCategory::Internal,
                    "rpc response missing effective_limits",
                )
            })?)?;
        let contract_release = Self::parse_required_string(&result, "contract_release")?;
        let schema_namespace = Self::parse_required_string(&result, "schema_namespace")?;
        {
            let mut guard = self
                .negotiated_capabilities
                .write()
                .expect("negotiated_capabilities rwlock poisoned");
            *guard = effective_capabilities.clone();
        }
        {
            let mut guard =
                self.negotiated_limits.write().expect("negotiated_limits rwlock poisoned");
            *guard = Some(effective_limits.clone());
        }
        {
            let mut guard = self.session_auth.write().expect("session_auth rwlock poisoned");
            *guard = session_auth;
        }

        Ok(NegotiationResponse {
            runtime_id,
            active_contract_version,
            effective_capabilities,
            effective_limits,
            contract_release,
            schema_namespace,
        })
    }

    pub(super) fn send_impl(&self, req: SendRequest) -> Result<MessageId, SdkError> {
        let SendRequest {
            source,
            destination,
            payload,
            idempotency_key,
            ttl_ms,
            correlation_id,
            extensions,
        } = req;
        let rpc_message_id = format!("sdk-{}", self.next_request_id());
        let content = payload
            .get("content")
            .and_then(JsonValue::as_str)
            .map(str::to_owned)
            .unwrap_or_else(|| payload.to_string());
        let title =
            payload.get("title").and_then(JsonValue::as_str).map(str::to_owned).unwrap_or_default();

        let mut fields = match payload {
            JsonValue::Object(map) => JsonValue::Object(map),
            other => json!({ "payload": other }),
        };
        if let JsonValue::Object(map) = &mut fields {
            let mut sdk_meta = JsonMap::new();
            if let Some(idempotency_key) = idempotency_key {
                sdk_meta.insert("idempotency_key".to_string(), JsonValue::String(idempotency_key));
            }
            if let Some(ttl_ms) = ttl_ms {
                sdk_meta.insert("ttl_ms".to_string(), JsonValue::from(ttl_ms));
            }
            if let Some(correlation_id) = correlation_id {
                sdk_meta.insert("correlation_id".to_string(), JsonValue::String(correlation_id));
            }
            if !extensions.is_empty() {
                let extension_map = extensions.into_iter().collect::<JsonMap<String, JsonValue>>();
                sdk_meta.insert("extensions".to_string(), JsonValue::Object(extension_map));
            }
            if !sdk_meta.is_empty() {
                map.insert("_sdk".to_string(), JsonValue::Object(sdk_meta));
            }
        }

        let params = Some(json!({
            "id": rpc_message_id,
            "source": source,
            "destination": destination,
            "title": title,
            "content": content,
            "fields": fields,
        }));
        let result = if self.legacy_send_fallback_enabled() {
            self.call_rpc_with_fallback("sdk_send_v2", "send_message_v2", params)?
        } else {
            self.call_rpc("sdk_send_v2", params)?
        };
        let message_id = Self::parse_required_string(&result, "message_id")?;
        Ok(MessageId(message_id))
    }

    pub(super) fn cancel_impl(&self, id: MessageId) -> Result<CancelResult, SdkError> {
        let result = self.call_rpc(
            "sdk_cancel_message_v2",
            Some(json!({
                "message_id": id.0,
            })),
        )?;
        let value = Self::parse_required_string(&result, "result")?;
        Self::parse_cancel_result(value.as_str())
    }

    fn parse_cancel_result(value: &str) -> Result<CancelResult, SdkError> {
        match value {
            "Accepted" => Ok(CancelResult::Accepted),
            "AlreadyTerminal" => Ok(CancelResult::AlreadyTerminal),
            "NotFound" => Ok(CancelResult::NotFound),
            "TooLateToCancel" => Ok(CancelResult::TooLateToCancel),
            _ => Err(SdkError::new(
                code::INTERNAL,
                ErrorCategory::Internal,
                "rpc returned unknown cancel result variant",
            )
            .with_detail("cancel_result", JsonValue::String(value.to_owned()))),
        }
    }

    pub(super) fn status_impl(&self, id: MessageId) -> Result<Option<DeliverySnapshot>, SdkError> {
        let message_id = id.0.clone();
        let result = self.call_rpc(
            "sdk_status_v2",
            Some(json!({
                "message_id": message_id,
            })),
        )?;
        let Some(record) = result.get("message") else {
            return Ok(None);
        };
        if record.is_null() {
            return Ok(None);
        }

        let receipt_status = record.get("receipt_status").and_then(JsonValue::as_str);
        let state = Self::parse_delivery_state(receipt_status);
        let has_receipt_terminality = self.has_capability("sdk.capability.receipt_terminality");
        let terminal = match state {
            DeliveryState::Sent => !has_receipt_terminality,
            DeliveryState::Delivered
            | DeliveryState::Failed
            | DeliveryState::Cancelled
            | DeliveryState::Expired
            | DeliveryState::Rejected => true,
            DeliveryState::Queued
            | DeliveryState::Dispatching
            | DeliveryState::InFlight
            | DeliveryState::Unknown => false,
        };
        let timestamp = record.get("timestamp").and_then(JsonValue::as_i64).unwrap_or(0_i64);
        let last_updated_ms = u64::try_from(timestamp.max(0)).unwrap_or(0).saturating_mul(1000);

        Ok(Some(DeliverySnapshot {
            message_id: id,
            state,
            terminal,
            last_updated_ms,
            attempts: 0,
            reason_code: None,
        }))
    }

    pub(super) fn configure_impl(
        &self,
        expected_revision: u64,
        patch: ConfigPatch,
    ) -> Result<Ack, SdkError> {
        let patch = serde_json::to_value(patch).map_err(|err| {
            SdkError::new(code::INTERNAL, ErrorCategory::Internal, err.to_string())
        })?;
        let result = self.call_rpc(
            "sdk_configure_v2",
            Some(json!({
                "expected_revision": expected_revision,
                "patch": patch,
            })),
        )?;
        Ok(Ack {
            accepted: result.get("accepted").and_then(JsonValue::as_bool).unwrap_or(false),
            revision: result.get("revision").and_then(JsonValue::as_u64),
        })
    }

    pub(super) fn poll_events_impl(
        &self,
        cursor: Option<EventCursor>,
        max: usize,
    ) -> Result<EventBatch, SdkError> {
        let result = self.call_rpc(
            "sdk_poll_events_v2",
            Some(json!({
                "cursor": cursor.map(|cursor| cursor.0),
                "max": max,
            })),
        )?;

        let mut events = Vec::new();
        if let Some(rows) = result.get("events").and_then(JsonValue::as_array) {
            for row in rows {
                let event_id = Self::parse_required_string(row, "event_id")?;
                let runtime_id = Self::parse_required_string(row, "runtime_id")?;
                let stream_id = Self::parse_required_string(row, "stream_id")?;
                let seq_no = Self::parse_required_u64(row, "seq_no")?;
                let contract_version = Self::parse_required_u16(row, "contract_version")?;
                let ts_ms = Self::parse_required_u64(row, "ts_ms")?;
                let event_type = Self::parse_required_string(row, "event_type")?;
                let severity = row
                    .get("severity")
                    .and_then(JsonValue::as_str)
                    .map(Self::parse_severity)
                    .unwrap_or(Severity::Info);
                let source_component = row
                    .get("source_component")
                    .and_then(JsonValue::as_str)
                    .unwrap_or("rns-rpc")
                    .to_owned();
                let payload =
                    row.get("payload").cloned().unwrap_or(JsonValue::Object(JsonMap::new()));

                events.push(SdkEvent {
                    event_id,
                    runtime_id,
                    stream_id,
                    seq_no,
                    contract_version,
                    ts_ms,
                    event_type,
                    severity,
                    source_component,
                    operation_id: row
                        .get("operation_id")
                        .and_then(JsonValue::as_str)
                        .map(str::to_owned),
                    message_id: row
                        .get("message_id")
                        .and_then(JsonValue::as_str)
                        .map(str::to_owned),
                    peer_id: row.get("peer_id").and_then(JsonValue::as_str).map(str::to_owned),
                    correlation_id: row
                        .get("correlation_id")
                        .and_then(JsonValue::as_str)
                        .map(str::to_owned),
                    trace_id: row.get("trace_id").and_then(JsonValue::as_str).map(str::to_owned),
                    payload,
                    extensions: BTreeMap::new(),
                });
            }
        }

        let next_cursor = EventCursor(Self::parse_required_string(&result, "next_cursor")?);
        let dropped_count = result.get("dropped_count").and_then(JsonValue::as_u64).unwrap_or(0);
        let snapshot_high_watermark_seq_no =
            result.get("snapshot_high_watermark_seq_no").and_then(JsonValue::as_u64);

        Ok(EventBatch {
            events,
            next_cursor,
            dropped_count,
            snapshot_high_watermark_seq_no,
            extensions: BTreeMap::new(),
        })
    }

    pub(super) fn snapshot_impl(&self) -> Result<RuntimeSnapshot, SdkError> {
        let result = self.call_rpc("sdk_snapshot_v2", Some(json!({ "include_counts": true })))?;
        Ok(RuntimeSnapshot {
            runtime_id: Self::parse_required_string(&result, "runtime_id")?,
            state: result
                .get("state")
                .and_then(JsonValue::as_str)
                .map(Self::parse_runtime_state)
                .unwrap_or(RuntimeState::Running),
            active_contract_version: Self::parse_required_u16(&result, "active_contract_version")?,
            event_stream_position: Self::parse_required_u64(&result, "event_stream_position")?,
            config_revision: Self::parse_required_u64(&result, "config_revision")?,
            queued_messages: result.get("queued_messages").and_then(JsonValue::as_u64).unwrap_or(0),
            in_flight_messages: result
                .get("in_flight_messages")
                .and_then(JsonValue::as_u64)
                .unwrap_or(0),
        })
    }

    pub(super) fn shutdown_impl(&self, mode: ShutdownMode) -> Result<Ack, SdkError> {
        let mode = match mode {
            ShutdownMode::Graceful => "graceful",
            ShutdownMode::Immediate => "immediate",
        };
        let result = self.call_rpc(
            "sdk_shutdown_v2",
            Some(json!({
                "mode": mode,
            })),
        )?;
        Ok(Ack {
            accepted: result.get("accepted").and_then(JsonValue::as_bool).unwrap_or(false),
            revision: None,
        })
    }

    pub(super) fn tick_impl(&self, _budget: TickBudget) -> Result<TickResult, SdkError> {
        Err(SdkError::capability_disabled("sdk.capability.manual_tick"))
    }

    #[cfg(feature = "sdk-async")]
    fn fast_forward_tail_cursor(
        &self,
        target_seq_no: u64,
    ) -> Result<Option<EventCursor>, SdkError> {
        if target_seq_no == 0 {
            return Ok(None);
        }

        let poll_max = self.negotiated_max_poll_events();
        let mut cursor: Option<EventCursor> = None;

        // Prevent unbounded loops if the backend keeps returning the same cursor.
        for _ in 0..1024 {
            let batch = self.poll_events_impl(cursor.clone(), poll_max)?;
            let next_cursor = batch.next_cursor.clone();
            let reached_target =
                batch.events.last().map(|event| event.seq_no >= target_seq_no).unwrap_or(true);
            cursor = Some(next_cursor);
            if reached_target {
                return Ok(cursor);
            }
        }

        Err(SdkError::new(
            code::INTERNAL,
            ErrorCategory::Internal,
            "unable to fast-forward event cursor to tail within bounded attempts",
        ))
    }

    #[cfg(feature = "sdk-async")]
    pub(super) fn subscribe_events_impl(
        &self,
        start: SubscriptionStart,
    ) -> Result<EventSubscription, SdkError> {
        if !self.has_capability("sdk.capability.async_events") {
            return Err(SdkError::capability_disabled("sdk.capability.async_events"));
        }

        let cursor = match start {
            SubscriptionStart::Head | SubscriptionStart::Snapshot => None,
            SubscriptionStart::Tail => {
                let snapshot = self.snapshot_impl()?;
                self.fast_forward_tail_cursor(snapshot.event_stream_position)?
            }
        };

        Ok(EventSubscription { start, cursor })
    }
}

#[cfg(test)]
mod tests {
    use super::RpcBackendClient;

    #[test]
    fn parse_cancel_result_accepts_contract_variants() {
        assert!(matches!(
            RpcBackendClient::parse_cancel_result("Accepted"),
            Ok(crate::types::CancelResult::Accepted)
        ));
        assert!(matches!(
            RpcBackendClient::parse_cancel_result("AlreadyTerminal"),
            Ok(crate::types::CancelResult::AlreadyTerminal)
        ));
        assert!(matches!(
            RpcBackendClient::parse_cancel_result("NotFound"),
            Ok(crate::types::CancelResult::NotFound)
        ));
        assert!(matches!(
            RpcBackendClient::parse_cancel_result("TooLateToCancel"),
            Ok(crate::types::CancelResult::TooLateToCancel)
        ));
    }

    #[test]
    fn parse_cancel_result_rejects_unknown_variant() {
        let err = RpcBackendClient::parse_cancel_result("LegacyUnsupported")
            .expect_err("unknown cancel result must fail");
        assert_eq!(err.machine_code, crate::error::code::INTERNAL);
        assert_eq!(
            err.details.get("cancel_result"),
            Some(&serde_json::Value::String("LegacyUnsupported".to_owned()))
        );
    }
}
