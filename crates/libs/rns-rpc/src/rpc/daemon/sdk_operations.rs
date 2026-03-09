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
    fn operation_spec(id_or_alias: &str) -> Option<&'static SdkOperationSpec> {
        SDK_OPERATION_SPECS.iter().find(|spec| {
            spec.id == id_or_alias || spec.aliases.iter().any(|alias| alias == &id_or_alias)
        })
    }

    fn operation_registry_json(&self) -> JsonValue {
        let entries = SDK_OPERATION_SPECS
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
        json!({ "entries": entries })
    }

    fn envelope_invalid(&self, request_id: u64, message: impl AsRef<str>) -> RpcResponse {
        self.sdk_error_response(
            request_id,
            "SDK_VALIDATION_INVALID_ARGUMENT",
            message.as_ref(),
        )
    }

    fn handle_sdk_operation_registry_v2(
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

    fn envelope_execute_delegated(
        &self,
        request_id: u64,
        method: &str,
        params: JsonValue,
    ) -> Result<RpcResponse, std::io::Error> {
        let delegated = match method {
            "sdk_snapshot_v2" => self.handle_sdk_snapshot_v2(RpcRequest {
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
            "sdk_identity_contact_list_v2" => self.handle_sdk_identity_contact_list_v2(RpcRequest {
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
            "sdk_identity_list_v2" => raw.get("identities").cloned().unwrap_or(JsonValue::Null),
            "sdk_identity_contact_list_v2" => {
                raw.get("contact_list").cloned().unwrap_or(JsonValue::Null)
            }
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

    fn handle_sdk_envelope_execute_v2(
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
            return Ok(self.envelope_invalid(
                request.id,
                "kind must be query or command",
            ));
        }

        let spec = Self::operation_spec(operation_id.as_str());
        let (canonical_id, rpc_method) = if let Some(spec) = spec {
            if spec.kind != kind {
                return Ok(self.envelope_invalid(
                    request.id,
                    "envelope kind does not match registered operation kind",
                ));
            }
            (spec.id.to_owned(), spec.rpc_method)
        } else if kind == "command" {
            (operation_id.clone(), "sdk_command_invoke_v2")
        } else {
            return Ok(self.envelope_invalid(request.id, "unknown operation id"));
        };

        let delegated_params = match rpc_method {
            "sdk_snapshot_v2" => json!({}),
            "sdk_status_v2" => json!({
                "message_id": parsed.payload.get("message_id").and_then(JsonValue::as_str),
            }),
            "sdk_poll_events_v2" => json!({
                "cursor": parsed.payload.get("cursor").cloned().unwrap_or(JsonValue::Null),
                "max": parsed.payload.get("max").cloned().unwrap_or(JsonValue::from(32_u64)),
            }),
            "sdk_identity_list_v2" => json!({}),
            "sdk_identity_contact_list_v2" => parsed.payload,
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

        let delegated = self.envelope_execute_delegated(request.id, rpc_method, delegated_params)?;
        if let Some(error) = delegated.error {
            return Ok(RpcResponse { id: request.id, result: None, error: Some(error) });
        }
        let delegated_payload = delegated
            .result
            .and_then(|value| value.get("response").cloned())
            .unwrap_or(JsonValue::Null);
        let accepted = delegated_payload
            .get("accepted")
            .and_then(JsonValue::as_bool)
            .unwrap_or(true);
        let extensions = delegated_payload
            .get("extensions")
            .and_then(JsonValue::as_object)
            .cloned()
            .unwrap_or_default();
        let payload = delegated_payload
            .get("payload")
            .cloned()
            .unwrap_or(delegated_payload);
        Ok(RpcResponse {
            id: request.id,
            result: Some(json!({
                "response": {
                    "operation_id": canonical_id,
                    "kind": "result",
                    "accepted": accepted,
                    "correlation_id": parsed.correlation_id,
                    "payload": payload,
                    "extensions": extensions,
                }
            })),
            error: None,
        })
    }
}
