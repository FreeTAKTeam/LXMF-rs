impl RpcDaemon {
    fn operation_spec(&self, id_or_alias: &str) -> Option<ResolvedSdkOperationSpec> {
        if let Some(spec) = SDK_OPERATION_SPECS
            .iter()
            .chain(IDENTITY_SDK_OPERATION_SPECS.iter())
            .chain(TICKET_SDK_OPERATION_SPECS.iter())
            .chain(CONVERSATION_SDK_OPERATION_SPECS.iter())
            .chain(DELIVERY_SDK_OPERATION_SPECS.iter())
            .chain(PROPAGATION_SDK_OPERATION_SPECS.iter())
            .chain(LEGACY_SDK_OPERATION_SPECS.iter())
            .chain(RNS_SDK_OPERATION_SPECS.iter())
            .find(|spec| spec.id == id_or_alias || spec.aliases.iter().any(|alias| alias == &id_or_alias))
        {
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
            .chain(IDENTITY_SDK_OPERATION_SPECS.iter())
            .chain(TICKET_SDK_OPERATION_SPECS.iter())
            .chain(CONVERSATION_SDK_OPERATION_SPECS.iter())
            .chain(DELIVERY_SDK_OPERATION_SPECS.iter())
            .chain(PROPAGATION_SDK_OPERATION_SPECS.iter())
            .chain(LEGACY_SDK_OPERATION_SPECS.iter())
            .chain(RNS_SDK_OPERATION_SPECS.iter())
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

}
