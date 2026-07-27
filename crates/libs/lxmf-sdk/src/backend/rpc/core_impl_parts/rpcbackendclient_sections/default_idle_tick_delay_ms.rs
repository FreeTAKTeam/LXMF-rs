impl RpcBackendClient {

    const DEFAULT_IDLE_TICK_DELAY_MS: u64 = 25;

    fn run_manual_tick_loop<F>(
        start_cursor: Option<EventCursor>,
        max_work_items: usize,
        max_poll_events: usize,
        mut poll_events: F,
    ) -> Result<(usize, Option<EventCursor>), SdkError>
    where
        F: FnMut(Option<EventCursor>, usize) -> Result<EventBatch, SdkError>,
    {
        let mut processed_items = 0usize;
        let mut cursor = start_cursor;
        while processed_items < max_work_items {
            let request_max = (max_work_items - processed_items).min(max_poll_events).max(1);
            let batch = poll_events(cursor.clone(), request_max)?;
            cursor = Some(batch.next_cursor);
            let batch_processed = batch.events.len();
            processed_items = processed_items.saturating_add(batch_processed);
            if batch_processed < request_max {
                break;
            }
        }
        Ok((processed_items, cursor))
    }

    fn recommended_tick_delay_ms(budget: &TickBudget, processed_items: usize, yielded: bool) -> u64 {
        if yielded || processed_items > 0 {
            0
        } else {
            budget.max_duration_ms.unwrap_or(Self::DEFAULT_IDLE_TICK_DELAY_MS)
        }
    }

    pub(super) fn negotiate_impl(
        &self,
        req: NegotiationRequest,
    ) -> Result<NegotiationResponse, SdkError> {
        let session_auth = self.session_auth_from_request(&req)?;
        let headers = self.headers_for_session_auth(&session_auth);
        let mtls_auth = Self::mtls_for_session_auth(&session_auth);
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
                    "client_cert_path": mtls.client_cert_path,
                    "client_key_path": mtls.client_key_path,
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
                    "extensions": req.extensions,
                }
            })),
            mtls_auth.as_ref(),
            headers,
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
        let sdk_version =
            Self::parse_optional_string_or_default(&result, "sdk_version", crate::SDK_VERSION)?;
        let python_reference = Self::parse_parity_reference(&result)?;
        let software_parity = Self::parse_software_parity(&result)?;
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
        {
            let mut guard =
                self.manual_tick_cursor.write().expect("manual_tick_cursor rwlock poisoned");
            *guard = None;
        }

        Ok(NegotiationResponse {
            runtime_id,
            active_contract_version,
            effective_capabilities,
            effective_limits,
            contract_release,
            schema_namespace,
            sdk_version,
            python_reference,
            software_parity,
        })
    }

    #[cfg(feature = "sdk-async")]
    pub(super) async fn negotiate_async_impl(
        &self,
        req: NegotiationRequest,
    ) -> Result<NegotiationResponse, SdkError> {
        let session_auth = self.session_auth_from_request(&req)?;
        let headers = self.headers_for_session_auth(&session_auth);
        let mtls_auth = Self::mtls_for_session_auth(&session_auth);
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
                    "client_cert_path": mtls.client_cert_path,
                    "client_key_path": mtls.client_key_path,
                })),
            })
        });
        let result = self
            .call_rpc_async_with_headers(
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
                        "extensions": req.extensions,
                    }
                })),
                mtls_auth.as_ref(),
                headers,
            )
            .await?;

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
        let sdk_version =
            Self::parse_optional_string_or_default(&result, "sdk_version", crate::SDK_VERSION)?;
        let python_reference = Self::parse_parity_reference(&result)?;
        let software_parity = Self::parse_software_parity(&result)?;
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
        {
            let mut guard =
                self.manual_tick_cursor.write().expect("manual_tick_cursor rwlock poisoned");
            *guard = None;
        }

        Ok(NegotiationResponse {
            runtime_id,
            active_contract_version,
            effective_capabilities,
            effective_limits,
            contract_release,
            schema_namespace,
            sdk_version,
            python_reference,
            software_parity,
        })
    }

    fn send_params(&self, req: SendRequest) -> JsonValue {
        let SendRequest {
            source,
            destination,
            payload,
            delivery_method,
            stamp_cost,
            include_ticket,
            try_propagation_on_fail,
            idempotency_key: _,
            ttl_ms: _,
            correlation_id: _,
            extensions: _,
        } = req;
        let rpc_message_id = self.next_message_id();
        let content = payload
            .get("content")
            .and_then(JsonValue::as_str)
            .map(str::to_owned)
            .unwrap_or_else(|| payload.to_string());
        let title =
            payload.get("title").and_then(JsonValue::as_str).map(str::to_owned).unwrap_or_default();
        let fields = lxmf_wire_fields_from_payload(payload);

        json!({
            "id": rpc_message_id,
            "source": source,
            "destination": destination,
            "title": title,
            "content": content,
            "fields": fields,
            "method": delivery_method,
            "stamp_cost": stamp_cost,
            "include_ticket": include_ticket,
            "try_propagation_on_fail": try_propagation_on_fail,
        })
    }

    pub(super) fn send_impl(&self, req: SendRequest) -> Result<MessageId, SdkError> {
        let params = Some(self.send_params(req));
        let result = self.call_rpc("sdk_send_v2", params)?;
        let message_id = Self::parse_required_string(&result, "message_id")?;
        Ok(MessageId(message_id))
    }

    #[cfg(feature = "sdk-async")]
    pub(super) async fn send_async_impl(&self, req: SendRequest) -> Result<MessageId, SdkError> {
        let params = Some(self.send_params(req));
        let result = self.call_rpc_async("sdk_send_v2", params).await?;
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
}
