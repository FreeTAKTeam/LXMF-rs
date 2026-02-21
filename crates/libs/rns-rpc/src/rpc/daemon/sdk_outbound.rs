impl RpcDaemon {
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
                if let Some(existing_status) = existing_status {
                    if Self::is_terminal_receipt_status(&existing_status) {
                        existing_status
                    } else {
                        self.store
                            .update_receipt_status(&id, &status)
                            .map_err(std::io::Error::other)?;
                        self.append_delivery_trace(&id, status.clone());
                        status
                    }
                } else {
                    self.store
                        .update_receipt_status(&id, &status)
                        .map_err(std::io::Error::other)?;
                    self.append_delivery_trace(&id, status.clone());
                    status
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
            self.publish_event(event);
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
            let existing_status = self
                .store
                .get_message(&id)
                .map_err(std::io::Error::other)?
                .and_then(|message| message.receipt_status);
            if let Some(existing_status) = existing_status {
                if Self::is_terminal_receipt_status(&existing_status) {
                    existing_status
                } else {
                    self.store
                        .update_receipt_status(&id, &sent_status)
                        .map_err(std::io::Error::other)?;
                    self.append_delivery_trace(&id, sent_status.clone());
                    sent_status
                }
            } else {
                self.store
                    .update_receipt_status(&id, &sent_status)
                    .map_err(std::io::Error::other)?;
                self.append_delivery_trace(&id, sent_status.clone());
                sent_status
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
        self.publish_event(event);

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
            "sdk_identity_announce_now_v2",
            "sdk_identity_presence_list_v2",
            "sdk_identity_activate_v2",
            "sdk_identity_import_v2",
            "sdk_identity_export_v2",
            "sdk_identity_resolve_v2",
            "sdk_identity_contact_update_v2",
            "sdk_identity_contact_list_v2",
            "sdk_identity_bootstrap_v2",
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
