impl RpcDaemon {
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
            "sdk_topic_create_v2" => self.handle_sdk_topic_create_v2(request),
            "sdk_topic_get_v2" => self.handle_sdk_topic_get_v2(request),
            "sdk_topic_list_v2" => self.handle_sdk_topic_list_v2(request),
            "sdk_topic_subscribe_v2" => self.handle_sdk_topic_subscribe_v2(request),
            "sdk_topic_unsubscribe_v2" => self.handle_sdk_topic_unsubscribe_v2(request),
            "sdk_topic_publish_v2" => self.handle_sdk_topic_publish_v2(request),
            "sdk_telemetry_query_v2" => self.handle_sdk_telemetry_query_v2(request),
            "sdk_telemetry_subscribe_v2" => self.handle_sdk_telemetry_subscribe_v2(request),
            "sdk_attachment_store_v2" => self.handle_sdk_attachment_store_v2(request),
            "sdk_attachment_get_v2" => self.handle_sdk_attachment_get_v2(request),
            "sdk_attachment_list_v2" => self.handle_sdk_attachment_list_v2(request),
            "sdk_attachment_delete_v2" => self.handle_sdk_attachment_delete_v2(request),
            "sdk_attachment_download_v2" => self.handle_sdk_attachment_download_v2(request),
            "sdk_attachment_associate_topic_v2" => {
                self.handle_sdk_attachment_associate_topic_v2(request)
            }
            "sdk_marker_create_v2" => self.handle_sdk_marker_create_v2(request),
            "sdk_marker_list_v2" => self.handle_sdk_marker_list_v2(request),
            "sdk_marker_update_position_v2" => self.handle_sdk_marker_update_position_v2(request),
            "sdk_marker_delete_v2" => self.handle_sdk_marker_delete_v2(request),
            "sdk_identity_list_v2" => self.handle_sdk_identity_list_v2(request),
            "sdk_identity_activate_v2" => self.handle_sdk_identity_activate_v2(request),
            "sdk_identity_import_v2" => self.handle_sdk_identity_import_v2(request),
            "sdk_identity_export_v2" => self.handle_sdk_identity_export_v2(request),
            "sdk_identity_resolve_v2" => self.handle_sdk_identity_resolve_v2(request),
            "sdk_paper_encode_v2" => self.handle_sdk_paper_encode_v2(request),
            "sdk_paper_decode_v2" => self.handle_sdk_paper_decode_v2(request),
            "sdk_command_invoke_v2" => self.handle_sdk_command_invoke_v2(request),
            "sdk_command_reply_v2" => self.handle_sdk_command_reply_v2(request),
            "sdk_voice_session_open_v2" => self.handle_sdk_voice_session_open_v2(request),
            "sdk_voice_session_update_v2" => self.handle_sdk_voice_session_update_v2(request),
            "sdk_voice_session_close_v2" => self.handle_sdk_voice_session_close_v2(request),
            _ => self.handle_rpc_legacy(request),
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

}
