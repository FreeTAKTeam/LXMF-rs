impl RpcBackendClient {

    #[cfg(feature = "sdk-async")]
    pub(super) async fn status_async_impl(
        &self,
        id: MessageId,
    ) -> Result<Option<DeliverySnapshot>, SdkError> {
        let message_id = id.0.clone();
        let result = self
            .call_rpc_async(
                "sdk_status_v2",
                Some(json!({
                    "message_id": message_id,
                })),
            )
            .await?;
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

    #[cfg(feature = "sdk-async")]
    pub(super) async fn snapshot_async_impl(&self) -> Result<RuntimeSnapshot, SdkError> {
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
        let ack = Ack {
            accepted: result.get("accepted").and_then(JsonValue::as_bool).unwrap_or(false),
            revision: None,
        };
        if ack.accepted {
            let mut guard =
                self.manual_tick_cursor.write().expect("manual_tick_cursor rwlock poisoned");
            *guard = None;
        }
        Ok(ack)
    }

    #[cfg(feature = "sdk-async")]
    pub(super) async fn shutdown_async_impl(&self, mode: ShutdownMode) -> Result<Ack, SdkError> {
        let mode = match mode {
            ShutdownMode::Graceful => "graceful",
            ShutdownMode::Immediate => "immediate",
        };
        let result = self
            .call_rpc_async(
                "sdk_shutdown_v2",
                Some(json!({
                    "mode": mode,
                })),
            )
            .await?;
        let ack = Ack {
            accepted: result.get("accepted").and_then(JsonValue::as_bool).unwrap_or(false),
            revision: None,
        };
        if ack.accepted {
            let mut guard =
                self.manual_tick_cursor.write().expect("manual_tick_cursor rwlock poisoned");
            *guard = None;
        }
        Ok(ack)
    }

    pub(super) fn tick_impl(&self, budget: TickBudget) -> Result<TickResult, SdkError> {
        if !self.has_capability("sdk.capability.manual_tick") {
            return Err(SdkError::capability_disabled("sdk.capability.manual_tick"));
        }
        if budget.max_work_items == 0 {
            return Err(SdkError::new(
                code::VALIDATION_INVALID_ARGUMENT,
                ErrorCategory::Validation,
                "tick budget max_work_items must be greater than zero",
            )
            .with_user_actionable(true));
        }

        let start_cursor =
            self.manual_tick_cursor.read().expect("manual_tick_cursor rwlock poisoned").clone();
        let (processed_items, next_cursor) = Self::run_manual_tick_loop(
            start_cursor,
            budget.max_work_items,
            self.negotiated_max_poll_events(),
            |cursor, max| self.poll_events_impl(cursor, max),
        )?;
        {
            let mut guard =
                self.manual_tick_cursor.write().expect("manual_tick_cursor rwlock poisoned");
            *guard = next_cursor;
        }

        let yielded = processed_items >= budget.max_work_items;
        let next_recommended_delay_ms =
            Self::recommended_tick_delay_ms(&budget, processed_items, yielded);
        Ok(TickResult { processed_items, yielded, next_recommended_delay_ms })
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
