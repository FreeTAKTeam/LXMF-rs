impl RpcDaemon {
    pub(super) fn envelope_execute_delegated(
        &self,
        request_id: u64,
        method: &str,
        params: JsonValue,
    ) -> Result<RpcResponse, std::io::Error> {
        let delegated = match method {
            "sdk_send_v2" | "sdk_send_batch_v2" | "peer_sync" => self.handle_rpc_legacy_messages(RpcRequest {
                id: request_id,
                method: method.to_owned(),
                params: Some(params),
            })?,
            "propagation_remote_status"
            | "propagation_acknowledge_sync_completion"
            | "get_outbound_propagation_cost"
            | "get_outbound_propagation_node"
            | "set_outbound_propagation_node"
            | "list_propagation_nodes"
            | "propagation_status"
            | "propagation_enable"
            | "get_delivery_policy"
            | "set_delivery_policy"
            | "allow_control"
            | "disallow_control"
            | "propagation_peer_maintenance"
            | "propagation_ingest"
            | "propagation_fetch" => {
                self.handle_rpc_legacy_propagation_request(request_id, method, params)?
            }
            method if propagation_policy_method(method) => {
                self.handle_rpc_legacy_propagation_request(request_id, method, params)?
            }
            "stamp_policy_get" | "stamp_policy_set" => self.handle_rpc_legacy_misc(RpcRequest {
                id: request_id,
                method: method.to_owned(),
                params: Some(params),
            })?,
            "propagation_remote_fetch"
            | "propagation_remote_download"
            | "propagation_remote_sync"
            | "propagation_remote_unpeer" => {
                match self.handle_rpc_legacy_propagation(RpcRequest {
                    id: request_id,
                    method: method.to_owned(),
                    params: Some(params.clone()),
                }) {
                    Ok(response) => response,
                    Err(err) if err.kind() != std::io::ErrorKind::InvalidInput => {
                        self.propagation_remote_failure_response(request_id, method, &params, &err)
                    }
                    Err(err) => return Err(err),
                }
            }
            "get_outbound_stamp_cost" => self.handle_rpc_legacy_misc(RpcRequest {
                id: request_id,
                method: method.to_owned(),
                params: Some(params),
            })?,
            "sdk_snapshot_v2" => self.handle_sdk_snapshot_v2(RpcRequest {
                id: request_id,
                method: method.to_owned(),
                params: Some(params),
            })?,
            "sdk_cursor_hint_v2" => self.handle_sdk_cursor_hint_v2(RpcRequest {
                id: request_id,
                method: method.to_owned(),
                params: Some(params),
            })?,
            "sdk_status_v2" => self.handle_sdk_status_v2(RpcRequest {
                id: request_id,
                method: method.to_owned(),
                params: Some(params),
            })?,
            "sdk_cancel_message_v2" => self.handle_rpc(RpcRequest {
                id: request_id,
                method: method.to_owned(),
                params: Some(params),
            })?,
            "sdk_poll_events_v2" => self.handle_sdk_poll_events_v2(RpcRequest {
                id: request_id,
                method: method.to_owned(),
                params: Some(params),
            })?,
            "sdk_identity_list_v2" => self.handle_sdk_identity_list_v2(RpcRequest {
                id: request_id,
                method: method.to_owned(),
                params: Some(params),
            })?,
            "sdk_identity_create_v2" => self.handle_sdk_identity_create_v2(RpcRequest {
                id: request_id,
                method: method.to_owned(),
                params: Some(params),
            })?,
            "sdk_identity_announce_now_v2" => {
                self.handle_sdk_identity_announce_now_v2(RpcRequest {
                    id: request_id,
                    method: method.to_owned(),
                    params: Some(params),
                })?
            }
            "sdk_identity_presence_list_v2" => {
                self.handle_sdk_identity_presence_list_v2(RpcRequest {
                    id: request_id,
                    method: method.to_owned(),
                    params: Some(params),
                })?
            }
            "sdk_identity_contact_list_v2" => {
                self.handle_sdk_identity_contact_list_v2(RpcRequest {
                    id: request_id,
                    method: method.to_owned(),
                    params: Some(params),
                })?
            }
            "sdk_identity_contact_update_v2" => {
                self.handle_sdk_identity_contact_update_v2(RpcRequest {
                    id: request_id,
                    method: method.to_owned(),
                    params: Some(params),
                })?
            }
            "sdk_identity_bootstrap_v2" => self.handle_sdk_identity_bootstrap_v2(RpcRequest {
                id: request_id,
                method: method.to_owned(),
                params: Some(params),
            })?,
            "sdk_peer_connect_v2" => self.handle_sdk_peer_connect_v2(RpcRequest {
                id: request_id,
                method: method.to_owned(),
                params: Some(params),
            })?,
            "sdk_peer_disconnect_v2" => self.handle_sdk_peer_disconnect_v2(RpcRequest {
                id: request_id,
                method: method.to_owned(),
                params: Some(params),
            })?,
            "sdk_peer_reconnect_v2" => self.handle_sdk_peer_reconnect_v2(RpcRequest {
                id: request_id,
                method: method.to_owned(),
                params: Some(params),
            })?,
            "sdk_workflow_peer_ready_v2" => self.handle_sdk_workflow_peer_ready_v2(RpcRequest {
                id: request_id,
                method: method.to_owned(),
                params: Some(params),
            })?,
            "sdk_workflow_topic_sync_v2" => self.handle_sdk_workflow_topic_sync_v2(RpcRequest {
                id: request_id,
                method: method.to_owned(),
                params: Some(params),
            })?,
            "sdk_workflow_attachment_report_publish_v2" => self
                .handle_sdk_workflow_attachment_report_publish_v2(RpcRequest {
                    id: request_id,
                    method: method.to_owned(),
                    params: Some(params),
                })?,
            "sdk_workflow_mission_update_send_v2" => self
                .handle_sdk_workflow_mission_update_send_v2(RpcRequest {
                    id: request_id,
                    method: method.to_owned(),
                    params: Some(params),
                })?,
            "sdk_topic_create_v2" => self.handle_sdk_topic_create_v2(RpcRequest {
                id: request_id,
                method: method.to_owned(),
                params: Some(params),
            })?,
            "sdk_topic_get_v2" => self.handle_sdk_topic_get_v2(RpcRequest {
                id: request_id,
                method: method.to_owned(),
                params: Some(params),
            })?,
            "sdk_topic_list_v2" => self.handle_sdk_topic_list_v2(RpcRequest {
                id: request_id,
                method: method.to_owned(),
                params: Some(params),
            })?,
            "sdk_topic_subscribe_v2" => self.handle_sdk_topic_subscribe_v2(RpcRequest {
                id: request_id,
                method: method.to_owned(),
                params: Some(params),
            })?,
            "sdk_topic_unsubscribe_v2" => self.handle_sdk_topic_unsubscribe_v2(RpcRequest {
                id: request_id,
                method: method.to_owned(),
                params: Some(params),
            })?,
            "sdk_topic_publish_v2" => self.handle_sdk_topic_publish_v2(RpcRequest {
                id: request_id,
                method: method.to_owned(),
                params: Some(params),
            })?,
            "sdk_telemetry_query_v2" => self.handle_sdk_telemetry_query_v2(RpcRequest {
                id: request_id,
                method: method.to_owned(),
                params: Some(params),
            })?,
            "sdk_telemetry_subscribe_v2" => self.handle_sdk_telemetry_subscribe_v2(RpcRequest {
                id: request_id,
                method: method.to_owned(),
                params: Some(params),
            })?,
            "sdk_attachment_store_v2" => self.handle_sdk_attachment_store_v2(RpcRequest {
                id: request_id,
                method: method.to_owned(),
                params: Some(params),
            })?,
            "sdk_attachment_get_v2" => self.handle_sdk_attachment_get_v2(RpcRequest {
                id: request_id,
                method: method.to_owned(),
                params: Some(params),
            })?,
            "sdk_attachment_list_v2" => self.handle_sdk_attachment_list_v2(RpcRequest {
                id: request_id,
                method: method.to_owned(),
                params: Some(params),
            })?,
            "sdk_attachment_delete_v2" => self.handle_sdk_attachment_delete_v2(RpcRequest {
                id: request_id,
                method: method.to_owned(),
                params: Some(params),
            })?,
            "sdk_attachment_associate_topic_v2" => {
                self.handle_sdk_attachment_associate_topic_v2(RpcRequest {
                    id: request_id,
                    method: method.to_owned(),
                    params: Some(params),
                })?
            }
            "sdk_attachment_upload_start_v2" => {
                self.handle_sdk_attachment_upload_start_v2(RpcRequest {
                    id: request_id,
                    method: method.to_owned(),
                    params: Some(params),
                })?
            }
            "sdk_attachment_upload_chunk_v2" => {
                self.handle_sdk_attachment_upload_chunk_v2(RpcRequest {
                    id: request_id,
                    method: method.to_owned(),
                    params: Some(params),
                })?
            }
            "sdk_attachment_upload_commit_v2" => {
                self.handle_sdk_attachment_upload_commit_v2(RpcRequest {
                    id: request_id,
                    method: method.to_owned(),
                    params: Some(params),
                })?
            }
            "sdk_attachment_download_chunk_v2" => {
                self.handle_sdk_attachment_download_chunk_v2(RpcRequest {
                    id: request_id,
                    method: method.to_owned(),
                    params: Some(params),
                })?
            }
            "sdk_marker_create_v2" => self.handle_sdk_marker_create_v2(RpcRequest {
                id: request_id,
                method: method.to_owned(),
                params: Some(params),
            })?,
            "sdk_marker_list_v2" => self.handle_sdk_marker_list_v2(RpcRequest {
                id: request_id,
                method: method.to_owned(),
                params: Some(params),
            })?,
            "sdk_marker_update_position_v2" => {
                self.handle_sdk_marker_update_position_v2(RpcRequest {
                    id: request_id,
                    method: method.to_owned(),
                    params: Some(params),
                })?
            }
            "sdk_marker_delete_v2" => self.handle_sdk_marker_delete_v2(RpcRequest {
                id: request_id,
                method: method.to_owned(),
                params: Some(params),
            })?,
            "sdk_paper_encode_v2" => self.handle_sdk_paper_encode_v2(RpcRequest {
                id: request_id,
                method: method.to_owned(),
                params: Some(params),
            })?,
            "sdk_paper_decode_v2" => self.handle_sdk_paper_decode_v2(RpcRequest {
                id: request_id,
                method: method.to_owned(),
                params: Some(params),
            })?,
            "sdk_voice_session_open_v2" => self.handle_sdk_voice_session_open_v2(RpcRequest {
                id: request_id,
                method: method.to_owned(),
                params: Some(params),
            })?,
            "sdk_voice_session_update_v2" => {
                self.handle_sdk_voice_session_update_v2(RpcRequest {
                    id: request_id,
                    method: method.to_owned(),
                    params: Some(params),
                })?
            }
            "sdk_voice_session_close_v2" => self.handle_sdk_voice_session_close_v2(RpcRequest {
                id: request_id,
                method: method.to_owned(),
                params: Some(params),
            })?,
            "list_conversations" | "list_messages" | "message_delivery_trace" => {
                self.handle_rpc_legacy_messages(RpcRequest {
                id: request_id,
                method: method.to_owned(),
                params: Some(params),
                })?
            }
            "ticket_generate" => self.handle_rpc_legacy_misc(RpcRequest {
                id: request_id,
                method: method.to_owned(),
                params: Some(params),
            })?,
            "status" => RpcResponse {
                id: request_id,
                result: Some(self.daemon_status_result(false)?),
                error: None,
            },
            "sdk_command_invoke_v2" => self.handle_sdk_command_invoke_v2(RpcRequest {
                id: request_id,
                method: method.to_owned(),
                params: Some(params),
            })?,
            _ => self.handle_rpc(RpcRequest {
                id: request_id,
                method: method.to_owned(),
                params: Some(params),
            })?,
        };

        if let Some(error) = delegated.error {
            return Ok(RpcResponse { id: request_id, result: None, error: Some(error) });
        }
        let raw = delegated.result.unwrap_or(JsonValue::Null);
        let payload = match method {
            "sdk_send_v2" | "sdk_send_batch_v2" => raw,
            "sdk_identity_list_v2" => raw.get("identities").cloned().unwrap_or(JsonValue::Null),
            "sdk_identity_create_v2" => {
                raw.get("identity").cloned().unwrap_or(JsonValue::Null)
            }
            "sdk_identity_presence_list_v2" => {
                raw.get("presence_list").cloned().unwrap_or(JsonValue::Null)
            }
            "sdk_identity_contact_list_v2" => {
                raw.get("contact_list").cloned().unwrap_or(JsonValue::Null)
            }
            "sdk_identity_contact_update_v2" | "sdk_identity_bootstrap_v2" => {
                raw.get("contact").cloned().unwrap_or(JsonValue::Null)
            }
            "sdk_peer_connect_v2" | "sdk_peer_disconnect_v2" | "sdk_peer_reconnect_v2" => {
                raw.get("peer").cloned().unwrap_or(JsonValue::Null)
            }
            "sdk_workflow_peer_ready_v2"
            | "sdk_workflow_topic_sync_v2"
            | "sdk_workflow_attachment_report_publish_v2"
            | "sdk_workflow_mission_update_send_v2" => {
                raw.get("workflow").cloned().unwrap_or(JsonValue::Null)
            }
            "sdk_topic_create_v2" => raw.get("topic").cloned().unwrap_or(JsonValue::Null),
            "sdk_topic_get_v2" => raw.get("topic").cloned().unwrap_or(JsonValue::Null),
            "sdk_topic_list_v2" => raw,
            "sdk_cursor_hint_v2" => raw,
            "sdk_topic_subscribe_v2" => raw,
            "sdk_topic_unsubscribe_v2" => raw,
            "sdk_topic_publish_v2" => raw,
            "sdk_telemetry_query_v2" => raw.get("points").cloned().unwrap_or(JsonValue::Null),
            "sdk_telemetry_subscribe_v2" => raw,
            "sdk_attachment_store_v2" => raw.get("attachment").cloned().unwrap_or(JsonValue::Null),
            "sdk_attachment_get_v2" => raw.get("attachment").cloned().unwrap_or(JsonValue::Null),
            "sdk_attachment_list_v2" => raw,
            "sdk_attachment_delete_v2" => raw,
            "sdk_attachment_associate_topic_v2" => raw,
            "sdk_attachment_upload_start_v2" => {
                raw.get("upload").cloned().unwrap_or(JsonValue::Null)
            }
            "sdk_attachment_upload_chunk_v2" => {
                raw.get("upload_chunk").cloned().unwrap_or(JsonValue::Null)
            }
            "sdk_attachment_upload_commit_v2" => {
                raw.get("attachment").cloned().unwrap_or(JsonValue::Null)
            }
            "sdk_attachment_download_chunk_v2" => {
                raw.get("download_chunk").cloned().unwrap_or(JsonValue::Null)
            }
            "sdk_marker_create_v2" => raw.get("marker").cloned().unwrap_or(JsonValue::Null),
            "sdk_marker_list_v2" => raw,
            "sdk_marker_update_position_v2" => {
                raw.get("marker").cloned().unwrap_or(JsonValue::Null)
            }
            "sdk_marker_delete_v2" => raw,
            "sdk_paper_encode_v2" => raw.get("envelope").cloned().unwrap_or(JsonValue::Null),
            "sdk_paper_decode_v2" => raw,
            "sdk_voice_session_open_v2" => {
                raw.get("session_id").cloned().unwrap_or(JsonValue::Null)
            }
            "sdk_voice_session_update_v2" => raw.get("state").cloned().unwrap_or(JsonValue::Null),
            "sdk_voice_session_close_v2" => raw,
            "sdk_command_invoke_v2" => raw.get("response").cloned().unwrap_or(raw),
            _ => raw,
        };
        Ok(RpcResponse {
            id: request_id,
            result: Some(json!({
                "response": payload
            })),
            error: None,
        })
    }

}
