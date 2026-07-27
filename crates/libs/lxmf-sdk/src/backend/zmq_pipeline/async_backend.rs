use super::{json, send, JsonValue, ZmqPipelineBackendClient};
use crate::backend::{SdkBackendAsyncEvents, SdkBackendAsyncOps, SdkEventStream};
use crate::capability::{NegotiationRequest, NegotiationResponse};
use crate::error::{code, ErrorCategory, SdkError};
use crate::event::{EventCursor, EventSubscription, SubscriptionStart};
use crate::types::{
    Ack, CancelResult, ConfigPatch, DeliverySnapshot, DeliveryState, MessageId, RuntimeSnapshot,
    RuntimeState, SendRequest, ShutdownMode,
};
use tokio_stream::wrappers::ReceiverStream;

impl ZmqPipelineBackendClient {
    async fn negotiate_async_impl(
        &self,
        req: NegotiationRequest,
    ) -> Result<NegotiationResponse, SdkError> {
        let (bind_mode, auth_mode, rpc_backend) = self.negotiation_security_config(&req);
        let result = self
            .call_rpc_async(
                "sdk_negotiate_v2",
                Some(json!({
                    "supported_contract_versions": req.supported_contract_versions,
                    "requested_capabilities": req.requested_capabilities,
                    "config": {
                        "profile": req.profile,
                        "bind_mode": bind_mode,
                        "auth_mode": auth_mode,
                        "overflow_policy": req.overflow_policy,
                        "block_timeout_ms": req.block_timeout_ms,
                        "rpc_backend": rpc_backend,
                        "extensions": req.extensions,
                    }
                })),
            )
            .await?;
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
        *self.negotiated_capabilities.write().expect("negotiated_capabilities rwlock poisoned") =
            effective_capabilities.clone();
        Ok(NegotiationResponse {
            runtime_id: Self::parse_required_string(&result, "runtime_id")?,
            active_contract_version: Self::parse_required_u16(&result, "active_contract_version")?,
            effective_capabilities,
            effective_limits,
            contract_release: Self::parse_required_string(&result, "contract_release")?,
            schema_namespace: Self::parse_required_string(&result, "schema_namespace")?,
            sdk_version: Self::parse_optional_string_or_default(
                &result,
                "sdk_version",
                crate::SDK_VERSION,
            )?,
            python_reference: Self::parse_parity_reference(&result)?,
            software_parity: Self::parse_software_parity(&result)?,
        })
    }

    async fn send_async_impl(&self, req: SendRequest) -> Result<MessageId, SdkError> {
        let value = self
            .call_rpc_async("sdk_send_v2", Some(send::send_params(req, self.next_message_id())))
            .await?;
        Ok(MessageId(Self::parse_required_string(&value, "message_id")?))
    }

    async fn status_async_impl(&self, id: MessageId) -> Result<Option<DeliverySnapshot>, SdkError> {
        let result =
            self.call_rpc_async("sdk_status_v2", Some(json!({ "message_id": id.0 }))).await?;
        let Some(record) = result.get("message") else {
            return Ok(None);
        };
        if record.is_null() {
            return Ok(None);
        }
        let state =
            Self::parse_delivery_state(record.get("receipt_status").and_then(JsonValue::as_str));
        let terminal = match state {
            DeliveryState::Sent => !self.has_capability("sdk.capability.receipt_terminality"),
            DeliveryState::Delivered
            | DeliveryState::Failed
            | DeliveryState::Cancelled
            | DeliveryState::Expired
            | DeliveryState::Rejected => true,
            _ => false,
        };
        let timestamp = record.get("timestamp").and_then(JsonValue::as_i64).unwrap_or(0);
        Ok(Some(DeliverySnapshot {
            message_id: id,
            state,
            terminal,
            last_updated_ms: u64::try_from(timestamp.max(0)).unwrap_or(0).saturating_mul(1000),
            attempts: Self::parse_optional_u32(record, "attempts")?.unwrap_or(0),
            reason_code: Self::parse_optional_string(record, "reason_code")?,
        }))
    }

    async fn cancel_async_impl(&self, id: MessageId) -> Result<CancelResult, SdkError> {
        let result = self
            .call_rpc_async("sdk_cancel_message_v2", Some(json!({ "message_id": id.0 })))
            .await?;
        Self::parse_cancel_result(Self::parse_required_string(&result, "result")?.as_str())
    }

    async fn configure_async_impl(
        &self,
        expected_revision: u64,
        patch: ConfigPatch,
    ) -> Result<Ack, SdkError> {
        let result = self
            .call_rpc_async(
                "sdk_configure_v2",
                Some(json!({ "expected_revision": expected_revision, "patch": patch })),
            )
            .await?;
        Ok(Ack {
            accepted: result.get("accepted").and_then(JsonValue::as_bool).unwrap_or(false),
            revision: result.get("revision").and_then(JsonValue::as_u64),
        })
    }

    async fn snapshot_async_impl(&self) -> Result<RuntimeSnapshot, SdkError> {
        let result =
            self.call_rpc_async("sdk_snapshot_v2", Some(json!({ "include_counts": true }))).await?;
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

    async fn shutdown_async_impl(&self, mode: ShutdownMode) -> Result<Ack, SdkError> {
        let mode = match mode {
            ShutdownMode::Graceful => "graceful",
            ShutdownMode::Immediate => "immediate",
        };
        let result = self.call_rpc_async("sdk_shutdown_v2", Some(json!({ "mode": mode }))).await?;
        Ok(Ack {
            accepted: result.get("accepted").and_then(JsonValue::as_bool).unwrap_or(false),
            revision: None,
        })
    }

    async fn poll_events_async_impl(
        &self,
        cursor: Option<EventCursor>,
        max: usize,
    ) -> Result<crate::event::EventBatch, SdkError> {
        let result = self
            .call_rpc_async(
                "sdk_poll_events_v2",
                Some(json!({ "cursor": cursor.map(|cursor| cursor.0), "max": max })),
            )
            .await?;
        Self::parse_event_batch(&result)
    }

    fn event_tail_cursor(&self) -> Result<Option<EventCursor>, SdkError> {
        let result =
            self.call_rpc("sdk_cursor_hint_v2", Some(json!({ "method": "sdk_poll_events_v2" })))?;
        Ok(result
            .get("hint")
            .and_then(JsonValue::as_str)
            .map(|value| EventCursor(value.to_owned())))
    }
}

impl SdkBackendAsyncOps for ZmqPipelineBackendClient {
    fn negotiate_async(
        &self,
        req: NegotiationRequest,
    ) -> crate::SdkBoxFuture<'_, NegotiationResponse> {
        Box::pin(async move { self.negotiate_async_impl(req).await })
    }

    fn send_async(&self, req: SendRequest) -> crate::SdkBoxFuture<'_, MessageId> {
        Box::pin(async move { self.send_async_impl(req).await })
    }

    fn cancel_async(&self, id: MessageId) -> crate::SdkBoxFuture<'_, CancelResult> {
        Box::pin(async move { self.cancel_async_impl(id).await })
    }

    fn status_async(&self, id: MessageId) -> crate::SdkBoxFuture<'_, Option<DeliverySnapshot>> {
        Box::pin(async move { self.status_async_impl(id).await })
    }

    fn configure_async(
        &self,
        expected_revision: u64,
        patch: ConfigPatch,
    ) -> crate::SdkBoxFuture<'_, Ack> {
        Box::pin(async move { self.configure_async_impl(expected_revision, patch).await })
    }

    fn poll_events_async(
        &self,
        cursor: Option<EventCursor>,
        max: usize,
    ) -> crate::SdkBoxFuture<'_, crate::event::EventBatch> {
        Box::pin(async move { self.poll_events_async_impl(cursor, max).await })
    }

    fn snapshot_async(&self) -> crate::SdkBoxFuture<'_, RuntimeSnapshot> {
        Box::pin(async move { self.snapshot_async_impl().await })
    }

    fn shutdown_async(&self, mode: ShutdownMode) -> crate::SdkBoxFuture<'_, Ack> {
        Box::pin(async move { self.shutdown_async_impl(mode).await })
    }
}

impl SdkBackendAsyncEvents for ZmqPipelineBackendClient {
    fn subscribe_events(&self, start: SubscriptionStart) -> Result<EventSubscription, SdkError> {
        if !self.has_capability("sdk.capability.async_events") {
            return Err(SdkError::capability_disabled("sdk.capability.async_events"));
        }
        let cursor = match start {
            SubscriptionStart::Tail => self.event_tail_cursor()?,
            SubscriptionStart::Head | SubscriptionStart::Snapshot => None,
        };
        Ok(EventSubscription { start, cursor })
    }

    fn open_event_stream(
        &self,
        subscription: &EventSubscription,
    ) -> Result<Option<SdkEventStream>, SdkError> {
        let client = ZmqPipelineBackendClient::new_async_only(self.config.clone())?;
        let mut cursor = subscription.cursor.clone();
        let (tx, rx) = tokio::sync::mpsc::channel(256);
        tokio::spawn(async move {
            let mut last_sequence_by_stream = std::collections::BTreeMap::<String, u64>::new();
            loop {
                if tx.is_closed() {
                    return;
                }
                match client.poll_events_async_impl(cursor.clone(), 64).await {
                    Ok(batch) => {
                        cursor = Some(batch.next_cursor);
                        if batch.events.is_empty() {
                            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                            continue;
                        }
                        for event in batch.events {
                            if last_sequence_by_stream
                                .get(&event.stream_id)
                                .is_some_and(|last| event.seq_no <= *last)
                            {
                                continue;
                            }
                            last_sequence_by_stream.insert(event.stream_id.clone(), event.seq_no);
                            if tx.send(Ok(event)).await.is_err() {
                                return;
                            }
                        }
                    }
                    Err(error) => {
                        if tx.send(Err(error)).await.is_err() {
                            return;
                        }
                        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                    }
                }
            }
        });
        Ok(Some(Box::pin(ReceiverStream::new(rx))))
    }
}
