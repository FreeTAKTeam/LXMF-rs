use super::*;

const OUTBOUND_DELIVERY_QUEUE_CAPACITY: usize = 1024;
const OUTBOUND_DELIVERY_WORKER_LANES: usize = 16;

impl RpcDaemon {
    pub(super) fn spawn_outbound_delivery_worker(
        bridge: Option<Arc<dyn OutboundBridge>>,
        store: Arc<MessagesStore>,
        delivery_traces: Arc<Mutex<HashMap<String, Vec<DeliveryTraceEntry>>>>,
        delivery_status_lock: Arc<Mutex<()>>,
        outbound_delivery_handoffs: Arc<Mutex<HashSet<String>>>,
    ) -> Option<mpsc::SyncSender<OutboundDeliveryCommand>> {
        let bridge = bridge?;
        let (tx, rx) =
            mpsc::sync_channel::<OutboundDeliveryCommand>(OUTBOUND_DELIVERY_QUEUE_CAPACITY);
        let rx = Arc::new(Mutex::new(rx));
        for lane in 0..OUTBOUND_DELIVERY_WORKER_LANES {
            let bridge = Arc::clone(&bridge);
            let store = Arc::clone(&store);
            let delivery_traces = Arc::clone(&delivery_traces);
            let delivery_status_lock = Arc::clone(&delivery_status_lock);
            let outbound_delivery_handoffs = Arc::clone(&outbound_delivery_handoffs);
            let rx = Arc::clone(&rx);
            std::thread::Builder::new()
                .name(format!("rpc-outbound-delivery-worker-{lane}"))
                .spawn(move || loop {
                    let command = {
                        let guard = rx.lock().expect("outbound delivery receiver mutex poisoned");
                        guard.recv()
                    };
                    let Ok(command) = command else {
                        break;
                    };
                    Self::process_outbound_delivery_command(
                        &bridge,
                        &store,
                        &delivery_traces,
                        &delivery_status_lock,
                        &outbound_delivery_handoffs,
                        command,
                    );
                })
                .expect("spawn rpc outbound delivery worker");
        }
        Some(tx)
    }

    #[allow(clippy::too_many_arguments)]
    pub(super) fn store_outbound(
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
        if let Err(error) = self.validate_current_session_source(source.as_str()) {
            return Ok(RpcResponse { id: request_id, result: None, error: Some(error) });
        }
        let timestamp = now_i64();
        if self.enforce_store_forward_retention(timestamp)? {
            return Ok(self.sdk_error_response(
                request_id,
                "SDK_RUNTIME_STORE_FORWARD_CAPACITY_REACHED",
                "store-forward capacity reached and policy rejected new outbound message",
            ));
        }
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

        let store_started = std::time::Instant::now();
        self.store.insert_message(&record).map_err(std::io::Error::other)?;
        let store_elapsed_ns = store_started.elapsed().as_nanos().min(u128::from(u64::MAX)) as u64;
        self.metrics_record_sdk_send_store_write(store_elapsed_ns);
        self.append_delivery_trace(&id, "sending".to_string());
        if self.outbound_bridge.is_some() {
            let _status_guard =
                self.delivery_status_lock.lock().expect("delivery_status_lock mutex poisoned");
            let sending_status = "sending".to_string();
            let resolved_status = self
                .store
                .resolve_receipt_status(&id, &sending_status)
                .map_err(std::io::Error::other)?
                .unwrap_or(sending_status);
            if resolved_status == "sending" {
                self.append_delivery_trace(&id, "sending".to_string());
            }
            record.receipt_status = Some(resolved_status);
        }
        if let Some(bridge) = &self.outbound_bridge {
            if let Err(err) = bridge.validate_delivery(&record, &options) {
                let status = format!("failed: {err}");
                let resolved_status = {
                    let _status_guard = self
                        .delivery_status_lock
                        .lock()
                        .expect("delivery_status_lock mutex poisoned");
                    let resolved_status = self
                        .store
                        .resolve_receipt_status(&id, &status)
                        .map_err(std::io::Error::other)?
                        .unwrap_or_else(|| status.clone());
                    if resolved_status == status {
                        self.append_delivery_trace(&id, status);
                    }
                    resolved_status
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
                let publish_started = std::time::Instant::now();
                self.publish_event(event);
                let publish_elapsed_ns =
                    publish_started.elapsed().as_nanos().min(u128::from(u64::MAX)) as u64;
                self.metrics_record_sdk_send_event_publish(publish_elapsed_ns);
                return Ok(RpcResponse {
                    id: request_id,
                    result: None,
                    error: Some(RpcError::new("DELIVERY_FAILED", err.to_string())),
                });
            }
            let delivery_started = std::time::Instant::now();
            let schedule_result = self.schedule_bridge_delivery(record.clone(), options);
            let delivery_elapsed_ns =
                delivery_started.elapsed().as_nanos().min(u128::from(u64::MAX)) as u64;
            self.metrics_record_sdk_send_delivery_schedule(delivery_elapsed_ns);
            if let Err(err) = schedule_result {
                let status = format!("failed: {err}");
                let resolved_status = {
                    let _status_guard = self
                        .delivery_status_lock
                        .lock()
                        .expect("delivery_status_lock mutex poisoned");
                    let resolved_status = self
                        .store
                        .resolve_receipt_status(&id, &status)
                        .map_err(std::io::Error::other)?
                        .unwrap_or_else(|| status.clone());
                    if resolved_status == status {
                        self.append_delivery_trace(&id, status);
                    }
                    resolved_status
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
                let publish_started = std::time::Instant::now();
                self.publish_event(event);
                let publish_elapsed_ns =
                    publish_started.elapsed().as_nanos().min(u128::from(u64::MAX)) as u64;
                self.metrics_record_sdk_send_event_publish(publish_elapsed_ns);
                return Ok(RpcResponse {
                    id: request_id,
                    result: None,
                    error: Some(RpcError::new("DELIVERY_FAILED", err.to_string())),
                });
            }
            let reason_code = record.receipt_status.as_deref().and_then(delivery_reason_code);
            let event = RpcEvent {
                event_type: "outbound".into(),
                payload: json!({
                    "message": record,
                    "method": method,
                    "reason_code": reason_code,
                }),
            };
            self.publish_event(event);
            return Ok(RpcResponse {
                id: request_id,
                result: Some(json!({ "message_id": id })),
                error: None,
            });
        }

        let delivery_started = std::time::Instant::now();
        let deliver_result: Result<(), std::io::Error> = {
            let _delivered = crate::transport::test_bridge::deliver_outbound(&record);
            Ok(())
        };
        let delivery_elapsed_ns =
            delivery_started.elapsed().as_nanos().min(u128::from(u64::MAX)) as u64;
        self.metrics_record_sdk_send_delivery_schedule(delivery_elapsed_ns);
        if let Err(err) = deliver_result {
            let status = format!("failed: {err}");
            let resolved_status = {
                let _status_guard =
                    self.delivery_status_lock.lock().expect("delivery_status_lock mutex poisoned");
                let resolved_status = self
                    .store
                    .resolve_receipt_status(&id, &status)
                    .map_err(std::io::Error::other)?
                    .unwrap_or_else(|| status.clone());
                if resolved_status == status {
                    self.append_delivery_trace(&id, status);
                }
                resolved_status
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
            let publish_started = std::time::Instant::now();
            self.publish_event(event);
            let publish_elapsed_ns =
                publish_started.elapsed().as_nanos().min(u128::from(u64::MAX)) as u64;
            self.metrics_record_sdk_send_event_publish(publish_elapsed_ns);
            return Ok(RpcResponse {
                id: request_id,
                result: None,
                error: Some(RpcError::new("DELIVERY_FAILED", err.to_string())),
            });
        }
        let sent_status = format!("sent: {}", method.as_deref().unwrap_or("direct"));
        let resolved_status = {
            let _status_guard =
                self.delivery_status_lock.lock().expect("delivery_status_lock mutex poisoned");
            let resolved_status = self
                .store
                .resolve_receipt_status(&id, &sent_status)
                .map_err(std::io::Error::other)?
                .unwrap_or_else(|| sent_status.clone());
            if resolved_status == sent_status {
                self.append_delivery_trace(&id, sent_status);
            }
            resolved_status
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
        let publish_started = std::time::Instant::now();
        self.publish_event(event);
        let publish_elapsed_ns =
            publish_started.elapsed().as_nanos().min(u128::from(u64::MAX)) as u64;
        self.metrics_record_sdk_send_event_publish(publish_elapsed_ns);

        Ok(RpcResponse { id: request_id, result: Some(json!({ "message_id": id })), error: None })
    }

    pub(super) fn store_outbound_batch(
        &self,
        request_id: u64,
        parsed: NormalizedSendBatchRequest,
    ) -> Result<RpcResponse, std::io::Error> {
        let mut accepted_count = 0_u64;
        let mut rejected_count = 0_u64;
        let mut results = Vec::with_capacity(parsed.messages.len());
        for item in parsed.messages {
            let item_id = item.id.clone();
            match self.store_outbound(
                request_id,
                item.id,
                parsed.source.clone(),
                item.destination,
                item.title,
                item.content,
                item.fields,
                item.method,
                item.stamp_cost,
                item.options,
                item.include_ticket,
            ) {
                Ok(response) => {
                    if let Some(error) = response.error {
                        rejected_count = rejected_count.saturating_add(1);
                        results.push(json!({
                            "id": item_id,
                            "accepted": false,
                            "error": error,
                        }));
                    } else {
                        accepted_count = accepted_count.saturating_add(1);
                        let message_id = response
                            .result
                            .as_ref()
                            .and_then(|result| result.get("message_id"))
                            .and_then(JsonValue::as_str)
                            .unwrap_or(item_id.as_str());
                        results.push(json!({
                            "id": item_id,
                            "message_id": message_id,
                            "accepted": true,
                        }));
                    }
                }
                Err(err) => {
                    rejected_count = rejected_count.saturating_add(1);
                    results.push(json!({
                        "id": item_id,
                        "accepted": false,
                        "error": RpcError::new("SDK_INTERNAL", err.to_string()),
                    }));
                }
            }
        }

        Ok(RpcResponse {
            id: request_id,
            result: Some(json!({
                "batch_id": parsed.batch_id,
                "accepted_count": accepted_count,
                "rejected_count": rejected_count,
                "results": results,
            })),
            error: None,
        })
    }

    pub(super) fn schedule_bridge_delivery(
        &self,
        record: MessageRecord,
        options: OutboundDeliveryOptions,
    ) -> Result<(), std::io::Error> {
        let Some(tx) = &self.outbound_delivery_tx else {
            return Err(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                "outbound delivery worker unavailable",
            ));
        };
        tx.try_send(OutboundDeliveryCommand { record, options }).map_err(|err| {
            let message = match err {
                mpsc::TrySendError::Full(_) => "outbound delivery queue full",
                mpsc::TrySendError::Disconnected(_) => "outbound delivery worker disconnected",
            };
            std::io::Error::new(std::io::ErrorKind::WouldBlock, message)
        })
    }

    pub(super) fn local_delivery_hash(&self) -> String {
        self.delivery_destination_hash
            .lock()
            .expect("delivery_destination_hash mutex poisoned")
            .clone()
            .unwrap_or_else(|| self.identity_hash.clone())
    }

    pub(super) fn capabilities() -> Vec<&'static str> {
        vec![
            "status",
            "daemon_status_ex",
            "list_messages",
            "list_announces",
            "list_peers",
            "send_message",
            "send_message_v2",
            "sdk_send_v2",
            "sdk_send_batch_v2",
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
            "sdk_attachment_upload_start_v2",
            "sdk_attachment_upload_chunk_v2",
            "sdk_attachment_upload_commit_v2",
            "sdk_attachment_download_chunk_v2",
            "sdk_attachment_associate_topic_v2",
            "sdk_marker_create_v2",
            "sdk_marker_list_v2",
            "sdk_marker_update_position_v2",
            "sdk_marker_delete_v2",
            "sdk_identity_list_v2",
            "sdk_identity_create_v2",
            "sdk_identity_announce_now_v2",
            "sdk_identity_presence_list_v2",
            "sdk_identity_activate_v2",
            "sdk_identity_import_v2",
            "sdk_identity_export_v2",
            "sdk_identity_resolve_v2",
            "sdk_identity_contact_update_v2",
            "sdk_identity_contact_list_v2",
            "sdk_identity_bootstrap_v2",
            "sdk_workflow_peer_ready_v2",
            "sdk_workflow_topic_sync_v2",
            "sdk_workflow_attachment_report_publish_v2",
            "sdk_workflow_mission_update_send_v2",
            "sdk_paper_encode_v2",
            "sdk_paper_decode_v2",
            "sdk_command_invoke_v2",
            "sdk_command_reply_v2",
            "sdk_command_session_get_v2",
            "sdk_command_session_list_v2",
            "sdk_voice_session_open_v2",
            "sdk_voice_session_update_v2",
            "sdk_voice_session_close_v2",
            "announce_now",
            "announce_delivery",
            "list_interfaces",
            "set_interfaces",
            "reload_config",
            "peer_sync",
            "peer_unpeer",
            "set_delivery_policy",
            "get_delivery_policy",
            "allow_destination",
            "disallow_destination",
            "prioritise_destination",
            "propagation_status",
            "propagation_enable",
            "propagation_ingest",
            "propagation_fetch",
            "get_outbound_propagation_cost",
            "get_outbound_propagation_node",
            "set_outbound_propagation_node",
            "list_propagation_nodes",
            "propagation_remote_fetch",
            "propagation_remote_download",
            "paper_ingest_uri",
            "stamp_policy_get",
            "stamp_policy_set",
            "ticket_generate",
            "message_delivery_trace",
        ]
    }
}
