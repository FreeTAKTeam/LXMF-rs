impl RpcDaemon {

    fn operation_spec(&self, id_or_alias: &str) -> Option<ResolvedSdkOperationSpec> {
        if let Some(spec) = SDK_OPERATION_SPECS.iter().find(|spec| {
            spec.id == id_or_alias || spec.aliases.iter().any(|alias| alias == &id_or_alias)
        }) {
            return Some(ResolvedSdkOperationSpec {
                id: spec.id.to_owned(),
                kind: spec.kind.to_owned(),
                rpc_method: spec.rpc_method,
            });
        }

        self.sdk_custom_operations
            .lock()
            .expect("sdk_custom_operations mutex poisoned")
            .iter()
            .find(|spec| {
                (spec.id == id_or_alias || spec.aliases.iter().any(|alias| alias == id_or_alias))
                    && spec
                        .required_capabilities
                        .iter()
                        .all(|capability| self.sdk_has_capability(capability))
            })
            .map(|spec| ResolvedSdkOperationSpec {
                id: spec.id.clone(),
                kind: spec.kind.clone(),
                rpc_method: "sdk_command_invoke_v2",
            })
    }

    pub(super) fn operation_registry_json(&self) -> JsonValue {
        let mut entries = SDK_OPERATION_SPECS
            .iter()
            .filter(|spec| {
                spec.required_capabilities
                    .iter()
                    .all(|capability| self.sdk_has_capability(capability))
            })
            .map(|spec| {
                json!({
                    "id": spec.id,
                    "group": spec.group,
                    "kind": spec.kind,
                    "transport_variant": spec.transport_variant,
                    "description": spec.description,
                    "aliases": spec.aliases,
                    "required_capabilities": spec.required_capabilities,
                })
            })
            .collect::<Vec<_>>();
        entries.extend(
            self.sdk_custom_operations
                .lock()
                .expect("sdk_custom_operations mutex poisoned")
                .iter()
                .filter(|spec| {
                    spec.required_capabilities
                        .iter()
                        .all(|capability| self.sdk_has_capability(capability))
                })
                .map(|spec| {
                    json!({
                        "id": spec.id,
                        "group": spec.group,
                        "kind": spec.kind,
                        "transport_variant": spec.transport_variant,
                        "description": spec.description,
                        "aliases": spec.aliases,
                        "required_capabilities": spec.required_capabilities,
                    })
                }),
        );
        json!({ "entries": entries })
    }

    pub(super) fn envelope_invalid(
        &self,
        request_id: u64,
        message: impl AsRef<str>,
    ) -> RpcResponse {
        self.sdk_error_response(request_id, "SDK_VALIDATION_INVALID_ARGUMENT", message.as_ref())
    }

    pub(super) fn handle_sdk_operation_registry_v2(
        &self,
        request: RpcRequest,
    ) -> Result<RpcResponse, std::io::Error> {
        let params = request.params.unwrap_or_else(|| JsonValue::Object(JsonMap::new()));
        let parsed: SdkOperationRegistryV2Params = serde_json::from_value(params)
            .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidInput, err))?;
        let _ = parsed.extensions.len();
        Ok(RpcResponse {
            id: request.id,
            result: Some(json!({ "registry": self.operation_registry_json() })),
            error: None,
        })
    }

    pub(super) fn envelope_execute_delegated(
        &self,
        request_id: u64,
        method: &str,
        params: JsonValue,
    ) -> Result<RpcResponse, std::io::Error> {
        let delegated = match method {
            "sdk_send_v2" => self.handle_rpc_legacy_messages(RpcRequest {
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
            "list_messages" => self.handle_rpc_legacy_messages(RpcRequest {
                id: request_id,
                method: method.to_owned(),
                params: Some(params),
            })?,
            "status" => RpcResponse {
                id: request_id,
                result: Some(json!({
                    "identity_hash": self.identity_hash,
                    "delivery_destination_hash": self.local_delivery_hash(),
                    "running": true,
                })),
                error: None,
            },
            "sdk_command_invoke_v2" => self.handle_sdk_command_invoke_v2(RpcRequest {
                id: request_id,
                method: method.to_owned(),
                params: Some(params),
            })?,
            _ => {
                return Ok(self.sdk_error_response(
                    request_id,
                    "SDK_RUNTIME_NOT_SUPPORTED",
                    "operation is not implemented by the rpc daemon",
                ))
            }
        };

        if let Some(error) = delegated.error {
            return Ok(RpcResponse { id: request_id, result: None, error: Some(error) });
        }
        let raw = delegated.result.unwrap_or(JsonValue::Null);
        let payload = match method {
            "sdk_send_v2" => raw,
            "sdk_identity_list_v2" => raw.get("identities").cloned().unwrap_or(JsonValue::Null),
            "sdk_identity_presence_list_v2" => {
                raw.get("presence_list").cloned().unwrap_or(JsonValue::Null)
            }
            "sdk_identity_contact_list_v2" => {
                raw.get("contact_list").cloned().unwrap_or(JsonValue::Null)
            }
            "sdk_identity_contact_update_v2" | "sdk_identity_bootstrap_v2" => {
                raw.get("contact").cloned().unwrap_or(JsonValue::Null)
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
