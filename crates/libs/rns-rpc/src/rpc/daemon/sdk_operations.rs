use super::*;

#[derive(Clone, Copy)]
struct SdkOperationSpec {
    id: &'static str,
    group: &'static str,
    kind: &'static str,
    transport_variant: &'static str,
    description: &'static str,
    aliases: &'static [&'static str],
    required_capabilities: &'static [&'static str],
    rpc_method: &'static str,
}

#[derive(Debug, Clone)]
struct ResolvedSdkOperationSpec {
    id: String,
    kind: String,
    rpc_method: &'static str,
}

const SDK_OPERATION_SPECS: &[SdkOperationSpec] = &[
    SdkOperationSpec {
        id: "app.runtime.status",
        group: "runtime",
        kind: "query",
        transport_variant: "rpc",
        description: "Return runtime status and queue counters.",
        aliases: &["sdk_snapshot_v2"],
        required_capabilities: &[],
        rpc_method: "sdk_snapshot_v2",
    },
    SdkOperationSpec {
        id: "app.runtime.cursor_hint",
        group: "runtime",
        kind: "query",
        transport_variant: "rpc",
        description:
            "Return the latest remembered pagination cursor for one method or all methods.",
        aliases: &["sdk_cursor_hint_v2"],
        required_capabilities: &[],
        rpc_method: "sdk_cursor_hint_v2",
    },
    SdkOperationSpec {
        id: "app.delivery.send",
        group: "delivery",
        kind: "command",
        transport_variant: "rpc",
        description: "Queue one outbound message for delivery.",
        aliases: &["sdk_send_v2"],
        required_capabilities: &[],
        rpc_method: "sdk_send_v2",
    },
    SdkOperationSpec {
        id: "app.delivery.status",
        group: "delivery",
        kind: "query",
        transport_variant: "rpc",
        description: "Return delivery state for a specific message id.",
        aliases: &["sdk_status_v2"],
        required_capabilities: &[],
        rpc_method: "sdk_status_v2",
    },
    SdkOperationSpec {
        id: "app.event.poll",
        group: "events",
        kind: "query",
        transport_variant: "rpc",
        description: "Poll batches of runtime events.",
        aliases: &["sdk_poll_events_v2"],
        required_capabilities: &[],
        rpc_method: "sdk_poll_events_v2",
    },
    SdkOperationSpec {
        id: "app.identity.list",
        group: "identity",
        kind: "query",
        transport_variant: "rpc",
        description: "List identities visible to the runtime.",
        aliases: &["sdk_identity_list_v2"],
        required_capabilities: &["sdk.capability.identity_multi"],
        rpc_method: "sdk_identity_list_v2",
    },
    SdkOperationSpec {
        id: "app.identity.announce",
        group: "identity",
        kind: "command",
        transport_variant: "rpc",
        description: "Trigger an announce for the active identity.",
        aliases: &["sdk_identity_announce_now_v2"],
        required_capabilities: &["sdk.capability.identity_discovery"],
        rpc_method: "sdk_identity_announce_now_v2",
    },
    SdkOperationSpec {
        id: "app.identity.presence.list",
        group: "identity",
        kind: "query",
        transport_variant: "rpc",
        description: "List recently seen peers and announce-derived presence state.",
        aliases: &["sdk_identity_presence_list_v2"],
        required_capabilities: &["sdk.capability.identity_discovery"],
        rpc_method: "sdk_identity_presence_list_v2",
    },
    SdkOperationSpec {
        id: "app.contact.list",
        group: "identity",
        kind: "query",
        transport_variant: "rpc",
        description: "List contacts for a selected identity.",
        aliases: &["sdk_identity_contact_list_v2"],
        required_capabilities: &["sdk.capability.contact_management"],
        rpc_method: "sdk_identity_contact_list_v2",
    },
    SdkOperationSpec {
        id: "app.contact.update",
        group: "identity",
        kind: "command",
        transport_variant: "rpc",
        description: "Create or update a contact record for an identity.",
        aliases: &["sdk_identity_contact_update_v2"],
        required_capabilities: &["sdk.capability.contact_management"],
        rpc_method: "sdk_identity_contact_update_v2",
    },
    SdkOperationSpec {
        id: "app.identity.bootstrap",
        group: "identity",
        kind: "command",
        transport_variant: "rpc",
        description: "Bootstrap trust and optional sync state for an identity.",
        aliases: &["sdk_identity_bootstrap_v2"],
        required_capabilities: &["sdk.capability.contact_management"],
        rpc_method: "sdk_identity_bootstrap_v2",
    },
    SdkOperationSpec {
        id: "app.workflow.peer_ready",
        group: "workflow",
        kind: "command",
        transport_variant: "rpc",
        description: "Ensure a peer contact exists and optionally announce before use.",
        aliases: &["sdk_workflow_peer_ready_v2"],
        required_capabilities: &[
            "sdk.capability.contact_management",
            "sdk.capability.identity_discovery",
        ],
        rpc_method: "sdk_workflow_peer_ready_v2",
    },
    SdkOperationSpec {
        id: "app.workflow.topic_sync",
        group: "workflow",
        kind: "command",
        transport_variant: "rpc",
        description: "Ensure a topic exists, subscribe to it, and fetch a telemetry snapshot.",
        aliases: &["sdk_workflow_topic_sync_v2"],
        required_capabilities: &[
            "sdk.capability.topics",
            "sdk.capability.topic_subscriptions",
            "sdk.capability.telemetry_query",
        ],
        rpc_method: "sdk_workflow_topic_sync_v2",
    },
    SdkOperationSpec {
        id: "app.workflow.attachment_report_publish",
        group: "workflow",
        kind: "command",
        transport_variant: "rpc",
        description: "Ensure a topic, store an attachment, and publish a summary report.",
        aliases: &["sdk_workflow_attachment_report_publish_v2"],
        required_capabilities: &[
            "sdk.capability.topics",
            "sdk.capability.attachments",
            "sdk.capability.topic_fanout",
        ],
        rpc_method: "sdk_workflow_attachment_report_publish_v2",
    },
    SdkOperationSpec {
        id: "app.workflow.mission_update_send",
        group: "workflow",
        kind: "command",
        transport_variant: "rpc",
        description:
            "Ensure peer and optional topic state, store attachments, and send a mission update.",
        aliases: &["sdk_workflow_mission_update_send_v2"],
        required_capabilities: &[
            "sdk.capability.contact_management",
            "sdk.capability.identity_discovery",
            "sdk.capability.topics",
            "sdk.capability.attachments",
        ],
        rpc_method: "sdk_workflow_mission_update_send_v2",
    },
    SdkOperationSpec {
        id: "app.topic.create",
        group: "topics",
        kind: "command",
        transport_variant: "rpc",
        description: "Create a topic record for collaborative app flows.",
        aliases: &["sdk_topic_create_v2"],
        required_capabilities: &["sdk.capability.topics"],
        rpc_method: "sdk_topic_create_v2",
    },
    SdkOperationSpec {
        id: "app.topic.get",
        group: "topics",
        kind: "query",
        transport_variant: "rpc",
        description: "Fetch one topic record by id.",
        aliases: &["sdk_topic_get_v2"],
        required_capabilities: &["sdk.capability.topics"],
        rpc_method: "sdk_topic_get_v2",
    },
    SdkOperationSpec {
        id: "app.topic.list",
        group: "topics",
        kind: "query",
        transport_variant: "rpc",
        description: "List known topics with cursor pagination.",
        aliases: &["sdk_topic_list_v2"],
        required_capabilities: &["sdk.capability.topics"],
        rpc_method: "sdk_topic_list_v2",
    },
    SdkOperationSpec {
        id: "app.topic.subscribe",
        group: "topics",
        kind: "command",
        transport_variant: "rpc",
        description: "Subscribe the runtime to topic updates.",
        aliases: &["sdk_topic_subscribe_v2"],
        required_capabilities: &["sdk.capability.topic_subscriptions"],
        rpc_method: "sdk_topic_subscribe_v2",
    },
    SdkOperationSpec {
        id: "app.topic.unsubscribe",
        group: "topics",
        kind: "command",
        transport_variant: "rpc",
        description: "Remove a topic subscription from the runtime.",
        aliases: &["sdk_topic_unsubscribe_v2"],
        required_capabilities: &["sdk.capability.topic_subscriptions"],
        rpc_method: "sdk_topic_unsubscribe_v2",
    },
    SdkOperationSpec {
        id: "app.topic.publish",
        group: "topics",
        kind: "command",
        transport_variant: "rpc",
        description: "Publish one payload fanout to a topic.",
        aliases: &["sdk_topic_publish_v2"],
        required_capabilities: &["sdk.capability.topic_fanout"],
        rpc_method: "sdk_topic_publish_v2",
    },
    SdkOperationSpec {
        id: "app.telemetry.query",
        group: "telemetry",
        kind: "query",
        transport_variant: "rpc",
        description: "Query telemetry points filtered by peer, topic, and time bounds.",
        aliases: &["sdk_telemetry_query_v2"],
        required_capabilities: &["sdk.capability.telemetry_query"],
        rpc_method: "sdk_telemetry_query_v2",
    },
    SdkOperationSpec {
        id: "app.telemetry.subscribe",
        group: "telemetry",
        kind: "command",
        transport_variant: "rpc",
        description: "Subscribe the runtime to telemetry stream updates.",
        aliases: &["sdk_telemetry_subscribe_v2"],
        required_capabilities: &["sdk.capability.telemetry_stream"],
        rpc_method: "sdk_telemetry_subscribe_v2",
    },
    SdkOperationSpec {
        id: "app.attachment.store",
        group: "attachments",
        kind: "command",
        transport_variant: "rpc",
        description: "Store one attachment payload with optional topic associations.",
        aliases: &["sdk_attachment_store_v2"],
        required_capabilities: &["sdk.capability.attachments"],
        rpc_method: "sdk_attachment_store_v2",
    },
    SdkOperationSpec {
        id: "app.attachment.get",
        group: "attachments",
        kind: "query",
        transport_variant: "rpc",
        description: "Fetch one attachment metadata record by id.",
        aliases: &["sdk_attachment_get_v2"],
        required_capabilities: &["sdk.capability.attachments"],
        rpc_method: "sdk_attachment_get_v2",
    },
    SdkOperationSpec {
        id: "app.attachment.list",
        group: "attachments",
        kind: "query",
        transport_variant: "rpc",
        description: "List stored attachments with topic filtering and cursor pagination.",
        aliases: &["sdk_attachment_list_v2"],
        required_capabilities: &["sdk.capability.attachments"],
        rpc_method: "sdk_attachment_list_v2",
    },
    SdkOperationSpec {
        id: "app.attachment.delete",
        group: "attachments",
        kind: "command",
        transport_variant: "rpc",
        description: "Delete one stored attachment by id.",
        aliases: &["sdk_attachment_delete_v2"],
        required_capabilities: &["sdk.capability.attachment_delete"],
        rpc_method: "sdk_attachment_delete_v2",
    },
    SdkOperationSpec {
        id: "app.attachment.associate_topic",
        group: "attachments",
        kind: "command",
        transport_variant: "rpc",
        description: "Associate an existing attachment with an additional topic.",
        aliases: &["sdk_attachment_associate_topic_v2"],
        required_capabilities: &["sdk.capability.attachments"],
        rpc_method: "sdk_attachment_associate_topic_v2",
    },
    SdkOperationSpec {
        id: "app.attachment.upload_start",
        group: "attachments",
        kind: "command",
        transport_variant: "rpc",
        description: "Open a chunked attachment upload session.",
        aliases: &["sdk_attachment_upload_start_v2"],
        required_capabilities: &["sdk.capability.attachment_streaming"],
        rpc_method: "sdk_attachment_upload_start_v2",
    },
    SdkOperationSpec {
        id: "app.attachment.upload_chunk",
        group: "attachments",
        kind: "command",
        transport_variant: "rpc",
        description: "Append one chunk to an attachment upload session.",
        aliases: &["sdk_attachment_upload_chunk_v2"],
        required_capabilities: &["sdk.capability.attachment_streaming"],
        rpc_method: "sdk_attachment_upload_chunk_v2",
    },
    SdkOperationSpec {
        id: "app.attachment.upload_commit",
        group: "attachments",
        kind: "command",
        transport_variant: "rpc",
        description: "Commit a completed attachment upload session.",
        aliases: &["sdk_attachment_upload_commit_v2"],
        required_capabilities: &["sdk.capability.attachment_streaming"],
        rpc_method: "sdk_attachment_upload_commit_v2",
    },
    SdkOperationSpec {
        id: "app.attachment.download_chunk",
        group: "attachments",
        kind: "query",
        transport_variant: "rpc",
        description: "Read one chunk from a stored attachment payload.",
        aliases: &["sdk_attachment_download_chunk_v2"],
        required_capabilities: &["sdk.capability.attachment_streaming"],
        rpc_method: "sdk_attachment_download_chunk_v2",
    },
    SdkOperationSpec {
        id: "app.marker.create",
        group: "markers",
        kind: "command",
        transport_variant: "rpc",
        description: "Create a shared marker anchored to an optional topic.",
        aliases: &["sdk_marker_create_v2"],
        required_capabilities: &["sdk.capability.markers"],
        rpc_method: "sdk_marker_create_v2",
    },
    SdkOperationSpec {
        id: "app.marker.list",
        group: "markers",
        kind: "query",
        transport_variant: "rpc",
        description: "List markers with topic filtering and cursor pagination.",
        aliases: &["sdk_marker_list_v2"],
        required_capabilities: &["sdk.capability.markers"],
        rpc_method: "sdk_marker_list_v2",
    },
    SdkOperationSpec {
        id: "app.marker.update_position",
        group: "markers",
        kind: "command",
        transport_variant: "rpc",
        description: "Move an existing marker while enforcing revision checks.",
        aliases: &["sdk_marker_update_position_v2"],
        required_capabilities: &["sdk.capability.markers"],
        rpc_method: "sdk_marker_update_position_v2",
    },
    SdkOperationSpec {
        id: "app.marker.delete",
        group: "markers",
        kind: "command",
        transport_variant: "rpc",
        description: "Delete an existing marker while enforcing revision checks.",
        aliases: &["sdk_marker_delete_v2"],
        required_capabilities: &["sdk.capability.markers"],
        rpc_method: "sdk_marker_delete_v2",
    },
    SdkOperationSpec {
        id: "app.voice.session.open",
        group: "voice",
        kind: "command",
        transport_variant: "rpc",
        description: "Open a voice signaling session for a peer.",
        aliases: &["sdk_voice_session_open_v2"],
        required_capabilities: &["sdk.capability.voice_signaling"],
        rpc_method: "sdk_voice_session_open_v2",
    },
    SdkOperationSpec {
        id: "app.voice.session.update",
        group: "voice",
        kind: "command",
        transport_variant: "rpc",
        description: "Advance the state of a voice signaling session.",
        aliases: &["sdk_voice_session_update_v2"],
        required_capabilities: &["sdk.capability.voice_signaling"],
        rpc_method: "sdk_voice_session_update_v2",
    },
    SdkOperationSpec {
        id: "app.voice.session.close",
        group: "voice",
        kind: "command",
        transport_variant: "rpc",
        description: "Close a voice signaling session.",
        aliases: &["sdk_voice_session_close_v2"],
        required_capabilities: &["sdk.capability.voice_signaling"],
        rpc_method: "sdk_voice_session_close_v2",
    },
    SdkOperationSpec {
        id: "app.message.history.list",
        group: "messaging",
        kind: "query",
        transport_variant: "legacy_rpc",
        description: "List message history records for app chat flows.",
        aliases: &["list_messages"],
        required_capabilities: &[],
        rpc_method: "list_messages",
    },
    SdkOperationSpec {
        id: "app.delivery.destination_hash",
        group: "identity",
        kind: "query",
        transport_variant: "legacy_rpc",
        description: "Resolve the runtime delivery destination hash.",
        aliases: &["status"],
        required_capabilities: &[],
        rpc_method: "status",
    },
];

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

    pub(super) fn handle_sdk_envelope_execute_v2(
        &self,
        request: RpcRequest,
    ) -> Result<RpcResponse, std::io::Error> {
        let params = request.params.ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "missing params")
        })?;
        let parsed: SdkEnvelopeExecuteV2Params = serde_json::from_value(params)
            .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidInput, err))?;
        let operation_id = match Self::normalize_non_empty(parsed.operation_id.as_str()) {
            Some(value) => value,
            None => return Ok(self.envelope_invalid(request.id, "operation_id must not be empty")),
        };
        let kind = parsed.kind.trim().to_ascii_lowercase();
        if !matches!(kind.as_str(), "query" | "command") {
            return Ok(self.envelope_invalid(request.id, "kind must be query or command"));
        }

        let spec = self.operation_spec(operation_id.as_str());
        let (canonical_id, rpc_method) = if let Some(spec) = spec {
            if spec.kind != kind {
                return Ok(self.envelope_invalid(
                    request.id,
                    "envelope kind does not match registered operation kind",
                ));
            }
            (spec.id, spec.rpc_method)
        } else if kind == "command" {
            (operation_id, "sdk_command_invoke_v2")
        } else {
            return Ok(self.envelope_invalid(request.id, "unknown operation id"));
        };

        let delegated_params = match rpc_method {
            "sdk_send_v2" => parsed.payload,
            "sdk_snapshot_v2" => json!({}),
            "sdk_cursor_hint_v2" => parsed.payload,
            "sdk_status_v2" => json!({
                "message_id": parsed.payload.get("message_id").and_then(JsonValue::as_str),
            }),
            "sdk_poll_events_v2" => json!({
                "cursor": parsed.payload.get("cursor").cloned().unwrap_or(JsonValue::Null),
                "max": parsed.payload.get("max").cloned().unwrap_or(JsonValue::from(32_u64)),
            }),
            "sdk_identity_list_v2" => json!({}),
            "sdk_identity_announce_now_v2" => json!({}),
            "sdk_identity_presence_list_v2" => parsed.payload,
            "sdk_identity_contact_list_v2" => parsed.payload,
            "sdk_identity_contact_update_v2" => parsed.payload,
            "sdk_identity_bootstrap_v2" => parsed.payload,
            "sdk_workflow_peer_ready_v2" => parsed.payload,
            "sdk_workflow_topic_sync_v2" => parsed.payload,
            "sdk_workflow_attachment_report_publish_v2" => parsed.payload,
            "sdk_workflow_mission_update_send_v2" => parsed.payload,
            "sdk_topic_create_v2" => parsed.payload,
            "sdk_topic_get_v2" => json!({
                "topic_id": parsed.payload,
            }),
            "sdk_topic_list_v2" => parsed.payload,
            "sdk_topic_subscribe_v2" => parsed.payload,
            "sdk_topic_unsubscribe_v2" => json!({
                "topic_id": parsed.payload,
            }),
            "sdk_topic_publish_v2" => parsed.payload,
            "sdk_telemetry_query_v2" => parsed.payload,
            "sdk_telemetry_subscribe_v2" => parsed.payload,
            "sdk_attachment_store_v2" => parsed.payload,
            "sdk_attachment_get_v2" => json!({
                "attachment_id": parsed.payload,
            }),
            "sdk_attachment_list_v2" => parsed.payload,
            "sdk_attachment_delete_v2" => json!({
                "attachment_id": parsed.payload,
            }),
            "sdk_attachment_associate_topic_v2" => parsed.payload,
            "sdk_attachment_upload_start_v2" => parsed.payload,
            "sdk_attachment_upload_chunk_v2" => parsed.payload,
            "sdk_attachment_upload_commit_v2" => parsed.payload,
            "sdk_attachment_download_chunk_v2" => parsed.payload,
            "sdk_marker_create_v2" => parsed.payload,
            "sdk_marker_list_v2" => parsed.payload,
            "sdk_marker_update_position_v2" => parsed.payload,
            "sdk_marker_delete_v2" => parsed.payload,
            "sdk_voice_session_open_v2" => parsed.payload,
            "sdk_voice_session_update_v2" => parsed.payload,
            "sdk_voice_session_close_v2" => parsed.payload,
            "list_messages" => json!({
                "limit": parsed.payload.get("limit").cloned().unwrap_or(JsonValue::from(100_u64)),
                "offset": parsed.payload.get("offset").cloned().unwrap_or(JsonValue::from(0_u64)),
            }),
            "status" => json!({}),
            "sdk_command_invoke_v2" => json!({
                "command": canonical_id,
                "target": parsed.target,
                "payload": parsed.payload,
                "timeout_ms": parsed.timeout_ms,
                "extensions": parsed.extensions,
            }),
            _ => JsonValue::Null,
        };

        let delegated =
            self.envelope_execute_delegated(request.id, rpc_method, delegated_params)?;
        if let Some(error) = delegated.error {
            return Ok(RpcResponse { id: request.id, result: None, error: Some(error) });
        }
        let delegated_payload = delegated
            .result
            .and_then(|value| value.get("response").cloned())
            .unwrap_or(JsonValue::Null);
        let accepted =
            delegated_payload.get("accepted").and_then(JsonValue::as_bool).unwrap_or(true);
        let response_correlation_id = parsed.correlation_id;
        let extensions = delegated_payload
            .get("extensions")
            .and_then(JsonValue::as_object)
            .cloned()
            .unwrap_or_default();
        let payload = delegated_payload.get("payload").cloned().unwrap_or(delegated_payload);
        Ok(RpcResponse {
            id: request.id,
            result: Some(json!({
                "response": {
                    "operation_id": canonical_id,
                    "kind": "result",
                    "accepted": accepted,
                    "correlation_id": response_correlation_id,
                    "payload": payload,
                    "extensions": extensions,
                }
            })),
            error: None,
        })
    }

    pub(super) fn handle_sdk_workflow_peer_ready_v2(
        &self,
        request: RpcRequest,
    ) -> Result<RpcResponse, std::io::Error> {
        let params = request.params.unwrap_or_else(|| json!({}));
        let Some(identity) = params.get("identity").and_then(JsonValue::as_str) else {
            return Ok(self.sdk_error_response(
                request.id,
                "SDK_VALIDATION_INVALID_ARGUMENT",
                "workflow peer ready requires identity",
            ));
        };
        let announce = params.get("announce").and_then(JsonValue::as_bool).unwrap_or(true);
        let bootstrap = params.get("bootstrap").and_then(JsonValue::as_bool).unwrap_or(true);

        let mut existing_contact = None;
        let mut cursor = None;
        loop {
            let listed = self.handle_sdk_identity_contact_list_v2(RpcRequest {
                id: request.id,
                method: "sdk_identity_contact_list_v2".to_owned(),
                params: Some(json!({
                    "cursor": cursor,
                    "limit": 100,
                })),
            })?;
            if listed.error.is_some() {
                return Ok(listed);
            }
            let result = listed.result.unwrap_or(JsonValue::Null);
            let contact_list = result.get("contact_list").cloned().unwrap_or(JsonValue::Null);
            if let Some(found) = contact_list
                .get("contacts")
                .and_then(JsonValue::as_array)
                .and_then(|contacts| {
                    contacts.iter().find(|contact| {
                        contact.get("identity").and_then(JsonValue::as_str) == Some(identity)
                    })
                })
                .cloned()
            {
                existing_contact = Some(found);
                break;
            }
            match contact_list.get("next_cursor").and_then(JsonValue::as_str) {
                Some(next) if cursor.as_deref() != Some(next) => cursor = Some(next.to_owned()),
                _ => break,
            }
        }

        let announced = if announce {
            let announce_response = self.handle_sdk_identity_announce_now_v2(RpcRequest {
                id: request.id,
                method: "sdk_identity_announce_now_v2".to_owned(),
                params: Some(json!({})),
            })?;
            if announce_response.error.is_some() {
                return Ok(announce_response);
            }
            true
        } else {
            false
        };

        let contact = if let Some(contact) = existing_contact {
            (contact, false)
        } else {
            let created = if bootstrap {
                self.handle_sdk_identity_bootstrap_v2(RpcRequest {
                    id: request.id,
                    method: "sdk_identity_bootstrap_v2".to_owned(),
                    params: Some(json!({
                        "identity": identity,
                        "auto_sync": true,
                        "extensions": params.get("extensions").cloned().unwrap_or_else(|| json!({})),
                    })),
                })?
            } else {
                self.handle_sdk_identity_contact_update_v2(RpcRequest {
                    id: request.id,
                    method: "sdk_identity_contact_update_v2".to_owned(),
                    params: Some(json!({
                        "identity": identity,
                        "display_name": params.get("display_name").cloned().unwrap_or(JsonValue::Null),
                        "trust_level": params.get("trust_level").cloned().unwrap_or(JsonValue::Null),
                        "bootstrap": false,
                        "metadata": params.get("metadata").cloned().unwrap_or_else(|| json!({})),
                        "extensions": params.get("extensions").cloned().unwrap_or_else(|| json!({})),
                    })),
                })?
            };
            if created.error.is_some() {
                return Ok(created);
            }
            (
                created
                    .result
                    .unwrap_or(JsonValue::Null)
                    .get("contact")
                    .cloned()
                    .unwrap_or(JsonValue::Null),
                true,
            )
        };

        Ok(RpcResponse {
            id: request.id,
            result: Some(json!({
                "workflow": {
                    "identity": identity,
                    "contact": contact.0,
                    "was_created": contact.1,
                    "announced": announced,
                }
            })),
            error: None,
        })
    }

    pub(super) fn handle_sdk_workflow_topic_sync_v2(
        &self,
        request: RpcRequest,
    ) -> Result<RpcResponse, std::io::Error> {
        let params = request.params.unwrap_or_else(|| json!({}));
        let Some(topic_path) = params.get("topic_path").and_then(JsonValue::as_str) else {
            return Ok(self.sdk_error_response(
                request.id,
                "SDK_VALIDATION_INVALID_ARGUMENT",
                "workflow topic sync requires topic_path",
            ));
        };

        let mut topic = None;
        let mut cursor = None;
        loop {
            let listed = self.handle_sdk_topic_list_v2(RpcRequest {
                id: request.id,
                method: "sdk_topic_list_v2".to_owned(),
                params: Some(json!({
                    "cursor": cursor,
                    "limit": 100,
                })),
            })?;
            if listed.error.is_some() {
                return Ok(listed);
            }
            let result = listed.result.unwrap_or(JsonValue::Null);
            if let Some(found) = result
                .get("topics")
                .and_then(JsonValue::as_array)
                .and_then(|topics| {
                    topics.iter().find(|topic| {
                        topic.get("topic_path").and_then(JsonValue::as_str) == Some(topic_path)
                    })
                })
                .cloned()
            {
                topic = Some((found, false));
                break;
            }
            match result.get("next_cursor").and_then(JsonValue::as_str) {
                Some(next) if cursor.as_deref() != Some(next) => cursor = Some(next.to_owned()),
                _ => break,
            }
        }

        let (topic, was_created) = if let Some(topic) = topic {
            topic
        } else {
            let created = self.handle_sdk_topic_create_v2(RpcRequest {
                id: request.id,
                method: "sdk_topic_create_v2".to_owned(),
                params: Some(json!({
                    "topic_path": topic_path,
                    "metadata": params.get("metadata").cloned().unwrap_or_else(|| json!({})),
                    "extensions": params.get("extensions").cloned().unwrap_or_else(|| json!({})),
                })),
            })?;
            if created.error.is_some() {
                return Ok(created);
            }
            (
                created
                    .result
                    .unwrap_or(JsonValue::Null)
                    .get("topic")
                    .cloned()
                    .unwrap_or(JsonValue::Null),
                true,
            )
        };

        let topic_id =
            topic.get("topic_id").and_then(JsonValue::as_str).unwrap_or_default().to_owned();

        let subscribed = self.handle_sdk_topic_subscribe_v2(RpcRequest {
            id: request.id,
            method: "sdk_topic_subscribe_v2".to_owned(),
            params: Some(json!({
                "topic_id": topic_id,
            })),
        })?;
        if subscribed.error.is_some() {
            return Ok(subscribed);
        }

        let telemetry = self.handle_sdk_telemetry_query_v2(RpcRequest {
            id: request.id,
            method: "sdk_telemetry_query_v2".to_owned(),
            params: Some(json!({
                "topic_id": topic_id,
                "limit": params.get("telemetry_limit").cloned().unwrap_or(JsonValue::from(100_u64)),
            })),
        })?;
        if telemetry.error.is_some() {
            return Ok(telemetry);
        }

        Ok(RpcResponse {
            id: request.id,
            result: Some(json!({
                "workflow": {
                    "topic": topic,
                    "was_created": was_created,
                    "subscribed": subscribed.result.unwrap_or(JsonValue::Null).get("accepted").and_then(JsonValue::as_bool).unwrap_or(false),
                    "telemetry": telemetry.result.unwrap_or(JsonValue::Null).get("points").cloned().unwrap_or_else(|| json!([])),
                }
            })),
            error: None,
        })
    }

    pub(super) fn handle_sdk_workflow_attachment_report_publish_v2(
        &self,
        request: RpcRequest,
    ) -> Result<RpcResponse, std::io::Error> {
        let params = request.params.unwrap_or_else(|| json!({}));
        let topic_sync = self.handle_sdk_workflow_topic_sync_v2(RpcRequest {
            id: request.id,
            method: "sdk_workflow_topic_sync_v2".to_owned(),
            params: Some(json!({
                "topic_path": params.get("topic_path").cloned().unwrap_or(JsonValue::Null),
                "metadata": params.get("topic_metadata").cloned().unwrap_or_else(|| json!({})),
                "telemetry_limit": 0,
                "extensions": params.get("extensions").cloned().unwrap_or_else(|| json!({})),
            })),
        })?;
        if topic_sync.error.is_some() {
            return Ok(topic_sync);
        }
        let topic = topic_sync
            .result
            .unwrap_or(JsonValue::Null)
            .get("workflow")
            .and_then(|workflow| workflow.get("topic"))
            .cloned()
            .unwrap_or(JsonValue::Null);
        let topic_id =
            topic.get("topic_id").and_then(JsonValue::as_str).unwrap_or_default().to_owned();

        let Some(attachment) = params.get("attachment") else {
            return Ok(self.sdk_error_response(
                request.id,
                "SDK_VALIDATION_INVALID_ARGUMENT",
                "workflow attachment report requires attachment",
            ));
        };
        let stored = self.handle_sdk_attachment_store_v2(RpcRequest {
            id: request.id,
            method: "sdk_attachment_store_v2".to_owned(),
            params: Some(json!({
                "name": attachment.get("name").cloned().unwrap_or(JsonValue::Null),
                "content_type": attachment.get("content_type").cloned().unwrap_or(JsonValue::Null),
                "bytes_base64": attachment.get("bytes_base64").cloned().unwrap_or(JsonValue::Null),
                "topic_ids": [topic_id],
                "extensions": params.get("extensions").cloned().unwrap_or_else(|| json!({})),
            })),
        })?;
        if stored.error.is_some() {
            return Ok(stored);
        }
        let attachment_meta = stored
            .result
            .unwrap_or(JsonValue::Null)
            .get("attachment")
            .cloned()
            .unwrap_or(JsonValue::Null);

        let published = self.handle_sdk_topic_publish_v2(RpcRequest {
            id: request.id,
            method: "sdk_topic_publish_v2".to_owned(),
            params: Some(json!({
                "topic_id": topic.get("topic_id").cloned().unwrap_or(JsonValue::Null),
                "correlation_id": params.get("correlation_id").cloned().unwrap_or(JsonValue::Null),
                "payload": {
                    "summary": params.get("summary_payload").cloned().unwrap_or(JsonValue::Null),
                    "attachment_id": attachment_meta.get("attachment_id").cloned().unwrap_or(JsonValue::Null),
                    "attachment_name": attachment_meta.get("name").cloned().unwrap_or(JsonValue::Null),
                    "content_type": attachment_meta.get("content_type").cloned().unwrap_or(JsonValue::Null),
                },
                "extensions": params.get("extensions").cloned().unwrap_or_else(|| json!({})),
            })),
        })?;
        if published.error.is_some() {
            return Ok(published);
        }

        Ok(RpcResponse {
            id: request.id,
            result: Some(json!({
                "workflow": {
                    "topic": topic,
                    "attachment": attachment_meta,
                    "published": published.result.unwrap_or(JsonValue::Null),
                }
            })),
            error: None,
        })
    }

    pub(super) fn handle_sdk_workflow_mission_update_send_v2(
        &self,
        request: RpcRequest,
    ) -> Result<RpcResponse, std::io::Error> {
        let params = request.params.unwrap_or_else(|| json!({}));
        let metadata =
            params.get("metadata").and_then(JsonValue::as_object).cloned().unwrap_or_default();
        for key in ["content", "topic_id", "group_id", "file_attachments"] {
            if metadata.contains_key(key) {
                return Ok(self.sdk_error_response(
                    request.id,
                    "SDK_VALIDATION_INVALID_ARGUMENT",
                    "mission metadata cannot override reserved fields",
                ));
            }
        }

        let peer = self.handle_sdk_workflow_peer_ready_v2(RpcRequest {
            id: request.id,
            method: "sdk_workflow_peer_ready_v2".to_owned(),
            params: Some(json!({
                "identity": params.get("peer_identity").cloned().unwrap_or(JsonValue::Null),
                "display_name": params.get("display_name").cloned().unwrap_or(JsonValue::Null),
                "trust_level": params.get("trust_level").cloned().unwrap_or(JsonValue::Null),
                "bootstrap": params.get("bootstrap").cloned().unwrap_or(JsonValue::Bool(true)),
                "announce": params.get("announce").cloned().unwrap_or(JsonValue::Bool(true)),
                "metadata": JsonValue::Object(metadata.clone()),
                "extensions": params.get("extensions").cloned().unwrap_or_else(|| json!({})),
            })),
        })?;
        if peer.error.is_some() {
            return Ok(peer);
        }
        let peer_payload = peer
            .result
            .unwrap_or(JsonValue::Null)
            .get("workflow")
            .cloned()
            .unwrap_or(JsonValue::Null);

        let topic = if params.get("topic_path").and_then(JsonValue::as_str).is_some() {
            let ensured = self.handle_sdk_workflow_topic_sync_v2(RpcRequest {
                id: request.id,
                method: "sdk_workflow_topic_sync_v2".to_owned(),
                params: Some(json!({
                    "topic_path": params.get("topic_path").cloned().unwrap_or(JsonValue::Null),
                    "telemetry_limit": 0,
                    "metadata": {},
                    "extensions": params.get("extensions").cloned().unwrap_or_else(|| json!({})),
                })),
            })?;
            if ensured.error.is_some() {
                return Ok(ensured);
            }
            ensured
                .result
                .unwrap_or(JsonValue::Null)
                .get("workflow")
                .and_then(|workflow| workflow.get("topic"))
                .cloned()
        } else {
            None
        };

        let mut attachment_rows = Vec::new();
        if let Some(attachments) = params.get("attachments").and_then(JsonValue::as_array) {
            for attachment in attachments {
                let stored = self.handle_sdk_attachment_store_v2(RpcRequest {
                    id: request.id,
                    method: "sdk_attachment_store_v2".to_owned(),
                    params: Some(json!({
                        "name": attachment.get("name").cloned().unwrap_or(JsonValue::Null),
                        "content_type": attachment.get("content_type").cloned().unwrap_or(JsonValue::Null),
                        "bytes_base64": attachment.get("bytes_base64").cloned().unwrap_or(JsonValue::Null),
                        "topic_ids": topic
                            .as_ref()
                            .and_then(|topic| topic.get("topic_id").cloned())
                            .map(|topic_id| json!([topic_id]))
                            .unwrap_or_else(|| json!([])),
                        "extensions": params.get("extensions").cloned().unwrap_or_else(|| json!({})),
                    })),
                })?;
                if stored.error.is_some() {
                    return Ok(stored);
                }
                let attachment_meta = stored
                    .result
                    .unwrap_or(JsonValue::Null)
                    .get("attachment")
                    .cloned()
                    .unwrap_or(JsonValue::Null);
                attachment_rows.push(attachment_meta);
            }
        }

        let mut fields = metadata;
        if let Some(topic) = topic.as_ref() {
            if let Some(topic_id) = topic.get("topic_id").cloned() {
                fields.insert("topic_id".to_owned(), topic_id.clone());
                fields.insert("group_id".to_owned(), topic_id);
            }
        }
        if !attachment_rows.is_empty() {
            fields.insert(
                "file_attachments".to_owned(),
                JsonValue::Array(
                    attachment_rows
                        .iter()
                        .map(|attachment| {
                            json!({
                                "attachment_id": attachment.get("attachment_id").cloned().unwrap_or(JsonValue::Null),
                                "name": attachment.get("name").cloned().unwrap_or(JsonValue::Null),
                                "content_type": attachment.get("content_type").cloned().unwrap_or(JsonValue::Null),
                                "byte_len": attachment.get("byte_len").cloned().unwrap_or(JsonValue::Null),
                            })
                        })
                        .collect(),
                ),
            );
        }

        let sent = self.handle_rpc_legacy_messages(RpcRequest {
            id: request.id,
            method: "sdk_send_v2".to_owned(),
            params: Some(json!({
                "id": params
                    .get("idempotency_key")
                    .and_then(JsonValue::as_str)
                    .map(str::to_owned)
                    .unwrap_or_else(|| self.next_sdk_domain_id("workflow-mission")),
                "source": self.local_delivery_hash(),
                "destination": params.get("peer_identity").cloned().unwrap_or(JsonValue::Null),
                "title": "",
                "content": params.get("content").cloned().unwrap_or(JsonValue::Null),
                "fields": JsonValue::Object(fields),
            })),
        })?;
        if sent.error.is_some() {
            return Ok(sent);
        }

        Ok(RpcResponse {
            id: request.id,
            result: Some(json!({
                "workflow": {
                    "peer": peer_payload,
                    "message_id": sent.result.unwrap_or(JsonValue::Null).get("message_id").cloned().unwrap_or(JsonValue::Null),
                    "topic": topic,
                    "attachments": attachment_rows,
                }
            })),
            error: None,
        })
    }
}
