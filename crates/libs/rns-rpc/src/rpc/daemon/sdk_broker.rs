use super::*;
use crate::broker::{AckStoredRequest, FetchRequest, ResumeRequest, CAPABILITY};
use crate::storage::broker::{BrokerCommand, BrokerError};

impl RpcDaemon {
    pub(super) fn pump_durable_dispatch(store: &MessagesStore, bridge: &Arc<dyn OutboundBridge>) {
        match store.broker_pending_dispatch() {
            Ok(pending) => {
                for (mut record, options) in pending {
                    let id = record.id.clone();
                    record.fields = match outbound_wire_fields(record.fields) {
                        Ok(fields) => fields,
                        Err(error) => {
                            log::error!(
                                "invalid persisted durable wire fields message_id={id}: {error}"
                            );
                            continue;
                        }
                    };
                    match store.broker_command(
                        crate::storage::broker::BrokerCommand::DispatchState {
                            message_id: id.clone(),
                            state: "scheduled".into(),
                        },
                    ) {
                        Ok(value)
                            if value.get("claimed").and_then(JsonValue::as_bool) == Some(true) => {}
                        Ok(_) => continue,
                        Err(error) => {
                            log::error!("durable dispatch claim failed: {error}");
                            continue;
                        }
                    }
                    if let Err(error) = bridge.deliver(&record, &options) {
                        log::warn!("durable dispatch deferred message_id={id}: {error}");
                        if let Err(error) = store.broker_command(
                            crate::storage::broker::BrokerCommand::DispatchState {
                                message_id: id,
                                state: "queued".into(),
                            },
                        ) {
                            log::error!("durable dispatch retry commit failed: {error}");
                        }
                    }
                }
            }
            Err(error) => log::error!("durable dispatch query failed: {error}"),
        }
    }

    pub(super) fn handle_sdk_broker(
        &self,
        request: RpcRequest,
    ) -> Result<RpcResponse, std::io::Error> {
        let id = request.id;
        let result = self.broker_dispatch(request);
        let response = match result {
            Ok(result) => RpcResponse { id, result: Some(result), error: None },
            Err(error) => RpcResponse { id, result: None, error: Some(error) },
        };
        Ok(response)
    }
    pub(super) fn broker_session_negotiated(&self) -> bool {
        super::RPC_ZMQ_PRINCIPAL.with(|context| context.borrow().is_some())
            && self
                .sdk_identity_sessions
                .lock()
                .expect("identity sessions poisoned")
                .get(&current_rpc_session_id())
                .is_some_and(|s| s.broker_negotiated)
    }
    #[allow(clippy::result_large_err)]
    fn broker_dispatch(&self, request: RpcRequest) -> Result<JsonValue, RpcError> {
        let principal =
            super::RPC_ZMQ_PRINCIPAL.with(|context| context.borrow().clone()).ok_or_else(|| {
                RpcError::new(
                    "SDK_BROKER_TRANSPORT_REQUIRED",
                    "durable custody is only available through authenticated ZeroMQ ingress",
                )
            })?;
        if !self.store.durable_broker_enabled().map_err(broker_store_error)? {
            return Err(RpcError::new(
                "SDK_BROKER_DISABLED",
                "operator must enable durable ZeroMQ storage before RCH cutover",
            ));
        }
        let session = current_rpc_session_id();
        let params = request.params.unwrap_or_else(|| json!({}));
        if request.method == "sdk_broker_negotiate_v1" {
            if params.get("ack_meaning").and_then(JsonValue::as_str) != Some("stored") {
                return Err(RpcError::new(
                    "SDK_BROKER_NEGOTIATION_REQUIRED",
                    "ack_stored acknowledges durable custody only",
                ));
            }
            if principal.len() > 256 || session.len() > 512 {
                return Err(RpcError::new(
                    "SDK_VALIDATION_BROKER_SESSION",
                    "session/principal exceeds durable protocol limits",
                ));
            }
            let mut sessions =
                self.sdk_identity_sessions.lock().expect("identity sessions poisoned");
            if !sessions.contains_key(&session) && sessions.len() >= 4096 {
                return Err(RpcError::new(
                    "SDK_STORAGE_WRITE_BUSY",
                    "negotiated session capacity exhausted",
                ));
            }
            sessions.entry(session).or_default().broker_negotiated = true;
            drop(sessions);
            return Ok(json!({"capability":CAPABILITY,"ack_meaning":"stored","writer_version":1,
                "runtime_id":self.identity_hash,"active_contract_version":2,
                "effective_capabilities":Self::sdk_supported_capabilities(),"effective_limits":Self::sdk_effective_limits_for_profile("desktop_local"),
                "contract_release":"v2.6","schema_namespace":"v2","sdk_version":SDK_VERSION,"python_reference":python_reference_meta(),"software_parity":software_parity_orientation()
            }));
        }
        if !self
            .sdk_identity_sessions
            .lock()
            .expect("identity sessions poisoned")
            .get(&session)
            .is_some_and(|s| s.broker_negotiated)
        {
            return Err(RpcError::new(
                "SDK_BROKER_NEGOTIATION_REQUIRED",
                "negotiate ack_stored before durable operations",
            ));
        }
        let identity = params.get("identity").and_then(JsonValue::as_str).ok_or_else(|| {
            RpcError::new("SDK_VALIDATION_IDENTITY", "explicit service identity required")
        })?;
        let identity = self.select_session_identity(Some(identity))?;
        // Even without a live bridge, never treat a supplied source as authority.
        let destination = self
            .sdk_identities
            .lock()
            .expect("identities poisoned")
            .get(&identity)
            .and_then(|bundle| bundle.delivery_destination.clone())
            .ok_or_else(|| {
                RpcError::new(
                    "SDK_RUNTIME_IDENTITY_NOT_FOUND",
                    "registered delivery identity is unavailable",
                )
            })?
            .to_ascii_lowercase();
        let parse_error = |error: serde_json::Error| {
            RpcError::new("SDK_VALIDATION_BROKER_REQUEST", error.to_string())
        };
        if request.method == "sdk_broker_announces_v1" {
            let records = self.store.broker_announce_projection().map_err(broker_store_error)?;
            let result = json!({"announces":records});
            validate_projection_size(&result)?;
            return Ok(result);
        }
        let command = match request.method.as_str() {
            "sdk_broker_admit_v1" => {
                let request: crate::broker::AdmitRequest =
                    serde_json::from_value(params).map_err(parse_error)?;
                let prior = self
                    .store
                    .broker_command(BrokerCommand::Reconcile {
                        owner: principal.clone(),
                        source: destination.clone(),
                        request: crate::broker::ReconcileRequest {
                            identity: request.identity.clone(),
                            operation_id: request.operation_id.clone(),
                        },
                    })
                    .map_err(broker_store_error)?;
                if !prior.is_null() {
                    return self
                        .store
                        .broker_command(BrokerCommand::Admit {
                            owner: principal,
                            source: destination,
                            request,
                        })
                        .map_err(broker_store_error);
                }
                if request
                    .options
                    .method
                    .as_deref()
                    .is_some_and(|method| method.trim().eq_ignore_ascii_case("paper"))
                {
                    return Err(RpcError::new(
                        "SDK_CAPABILITY_UNSUPPORTED",
                        "durable broker v1 does not support paper artifacts",
                    ));
                }
                let bridge = self.outbound_bridge.as_ref().ok_or_else(|| {
                    RpcError::new(
                        "SDK_RUNTIME_BRIDGE_UNAVAILABLE",
                        "durable dispatch requires a network bridge",
                    )
                })?;
                let record = MessageRecord {
                    id: request.operation_id.clone(),
                    source: destination.clone(),
                    destination: request.destination.clone(),
                    title: request.title.clone(),
                    content: request.content.clone(),
                    timestamp: now_i64(),
                    direction: "out".into(),
                    fields: request.fields.clone(),
                    receipt_status: None,
                };
                bridge
                    .validate_delivery(&record, &request.options)
                    .map_err(|e| RpcError::new("SDK_VALIDATION_DELIVERY", e.to_string()))?;
                BrokerCommand::Admit { owner: principal, source: destination, request }
            }
            "sdk_broker_reconcile_v1" => BrokerCommand::Reconcile {
                owner: principal,
                source: destination,
                request: serde_json::from_value(params).map_err(parse_error)?,
            },
            "sdk_broker_resume_v1" => BrokerCommand::Resume {
                owner: principal,
                destination,
                request: serde_json::from_value::<ResumeRequest>(params).map_err(parse_error)?,
            },
            "sdk_broker_fetch_v1" => BrokerCommand::Fetch {
                owner: principal,
                destination,
                request: serde_json::from_value::<FetchRequest>(params).map_err(parse_error)?,
            },
            "sdk_broker_ack_stored_v1" => BrokerCommand::Ack {
                owner: principal,
                destination,
                request: serde_json::from_value::<AckStoredRequest>(params).map_err(parse_error)?,
            },
            _ => {
                return Err(RpcError::new("SDK_CAPABILITY_UNSUPPORTED", "unknown broker operation"))
            }
        };
        self.store.broker_command(command).map_err(broker_store_error)
    }
}
#[allow(clippy::result_large_err)]
fn validate_projection_size(value: &JsonValue) -> Result<(), RpcError> {
    struct ByteLimit(usize);
    impl std::io::Write for ByteLimit {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0 = self
                .0
                .checked_add(bytes.len())
                .filter(|total| *total <= crate::broker::MAX_BATCH_BYTES - 4096)
                .ok_or_else(|| {
                    std::io::Error::other("encoded announce projection exceeds response byte limit")
                })?;
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    serde_json::to_writer(ByteLimit(0), value)
        .map_err(|error| RpcError::new("SDK_STORAGE_BROKER_EVENT_TOO_LARGE", error.to_string()))
}

fn broker_store_error(error: rusqlite::Error) -> RpcError {
    if let rusqlite::Error::ToSqlConversionFailure(source) = &error {
        if let Some(broker) = source.downcast_ref::<BrokerError>() {
            return RpcError::new(broker.code, broker.message.clone());
        }
    }
    match &error {
        rusqlite::Error::SqliteFailure(code, _)
            if matches!(
                code.code,
                rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked
            ) =>
        {
            RpcError::new("SDK_STORAGE_BUSY", error.to_string())
        }
        rusqlite::Error::SqliteFailure(code, _) if code.code == rusqlite::ErrorCode::DiskFull => {
            RpcError::new("SDK_STORAGE_FULL", error.to_string())
        }
        _ => {
            log::error!("durable broker storage failure: {error}");
            RpcError::new("SDK_STORAGE_FAILED", error.to_string())
        }
    }
}

impl RpcDaemon {
    pub fn shutdown_outbound_workers(&self) {
        self.outbound_delivery_stop.store(true, std::sync::atomic::Ordering::Release);
        for worker in self
            .outbound_delivery_workers
            .lock()
            .expect("outbound worker handles poisoned")
            .drain(..)
        {
            if let Err(error) = worker.join() {
                log::error!("outbound delivery worker panicked: {error:?}");
            }
        }
        if let Some(bridge) = self.outbound_bridge.as_ref() {
            bridge.shutdown_delivery();
        }
    }
    pub fn owns_durable_message(&self, id: &str) -> Result<bool, std::io::Error> {
        self.store.broker_owns_message(id).map_err(std::io::Error::other)
    }
    pub fn durable_prepared_payload(
        &self,
        id: &str,
    ) -> Result<Option<Option<JsonValue>>, std::io::Error> {
        if !self.store.durable_broker_enabled().map_err(std::io::Error::other)? {
            return Ok(None);
        }
        self.store.broker_prepared(id).map_err(std::io::Error::other)
    }
    pub fn persist_durable_prepared_payload(
        &self,
        id: &str,
        payload: JsonValue,
    ) -> Result<(), std::io::Error> {
        self.store
            .broker_command(BrokerCommand::Prepared { message_id: id.into(), payload })
            .map(|_| ())
            .map_err(std::io::Error::other)
    }
    pub fn defer_durable_dispatch(&self, id: &str) -> Result<(), std::io::Error> {
        self.store
            .broker_command(BrokerCommand::DispatchState {
                message_id: id.into(),
                state: "queued".into(),
            })
            .map(|_| ())
            .map_err(std::io::Error::other)
    }
}

impl RpcDaemon {
    #[allow(clippy::result_large_err)]
    pub(super) fn authorize_broker_message_mutation(&self, id: &str) -> Result<(), RpcError> {
        if !self.store.durable_broker_enabled().map_err(broker_store_error)? {
            return Ok(());
        }
        let binding = self.store.broker_message_owner(id).map_err(broker_store_error)?;
        if let Some((owner, source)) = binding {
            let principal = super::RPC_ZMQ_PRINCIPAL.with(|p| p.borrow().clone());
            if principal.as_deref() != Some(owner.as_str()) {
                return Err(RpcError::new(
                    "SDK_SECURITY_OPERATION_FORBIDDEN",
                    "durable message belongs to another authenticated principal",
                ));
            }
            self.validate_current_session_source(&source)?;
        }
        Ok(())
    }
}

impl RpcDaemon {
    /// Trusted network producer entry point. RPC callers remain subject to operation ownership.
    pub fn record_network_receipt(
        &self,
        message_id: &str,
        status: &str,
        mut metadata: JsonMap<String, JsonValue>,
    ) -> Result<JsonValue, std::io::Error> {
        metadata.insert("message_id".into(), json!(message_id));
        metadata.insert("status".into(), json!(status));
        let parsed: RecordReceiptParams = serde_json::from_value(JsonValue::Object(metadata))
            .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidInput, error))?;
        self.apply_receipt_params(parsed)
    }
    pub(super) fn apply_receipt_params(
        &self,
        parsed: RecordReceiptParams,
    ) -> Result<JsonValue, std::io::Error> {
        let RecordReceiptParams {
            message_id,
            status: requested_status,
            packet_hash,
            resource_hash,
            peer,
            method,
            delivery_kind,
            bytes,
            link_id,
            stage,
        } = parsed;
        let (status, updated, delivered_ticket_destination) = {
            let _status_guard =
                self.delivery_status_lock.lock().expect("delivery_status_lock mutex poisoned");
            let existing_message =
                self.store.get_message(&message_id).map_err(std::io::Error::other)?;
            let existing_status =
                existing_message.as_ref().and_then(|message| message.receipt_status.clone());
            if existing_message.is_none() {
                (requested_status, false, None)
            } else if existing_status.as_deref().is_some_and(|status| {
                Self::is_terminal_receipt_status(status)
                    || Self::is_receipt_status_regression(status, &requested_status)
            }) {
                (existing_status.unwrap_or(requested_status), false, None)
            } else {
                let delivered_ticket_destination = existing_message
                    .as_ref()
                    .filter(|message| {
                        requested_status.eq_ignore_ascii_case("delivered")
                            && Self::message_requested_ticket(message)
                    })
                    .map(|message| message.destination.clone());
                self.store
                    .update_receipt_status(&message_id, &requested_status)
                    .map_err(std::io::Error::other)?;
                (requested_status, true, delivered_ticket_destination)
            }
        };
        if updated {
            self.append_delivery_trace(&message_id, status.clone());
        }
        if let Some(destination) = delivered_ticket_destination {
            self.mark_ticket_delivered(destination.as_str());
        }
        let reason_code = delivery_reason_code(&status);
        let mut payload = JsonMap::new();
        payload.insert("message_id".into(), json!(message_id));
        payload.insert("status".into(), json!(status));
        payload.insert("updated".into(), json!(updated));
        payload.insert("reason_code".into(), json!(reason_code));
        if let Some(packet_hash) = packet_hash {
            payload.insert("packet_hash".into(), json!(packet_hash));
        }
        if let Some(resource_hash) = resource_hash {
            payload.insert("resource_hash".into(), json!(resource_hash));
        }
        if let Some(peer) = peer {
            payload.insert("peer".into(), json!(peer));
        }
        if let Some(method) = method {
            payload.insert("method".into(), json!(method));
        }
        if let Some(delivery_kind) = delivery_kind {
            payload.insert("delivery_kind".into(), json!(delivery_kind));
        }
        if let Some(bytes) = bytes {
            payload.insert("bytes".into(), json!(bytes));
        }
        if let Some(link_id) = link_id {
            payload.insert("link_id".into(), json!(link_id));
        }
        if let Some(stage) = stage {
            payload.insert("stage".into(), json!(stage));
        }
        let payload = JsonValue::Object(payload);
        let event = RpcEvent { event_type: "receipt".into(), payload: payload.clone() };
        self.publish_event(event);
        Ok(payload)
    }
}

#[cfg(test)]
mod tests;
