impl RpcDaemon {
pub(super) fn handle_sdk_identity_import_v2(
    &self,
    request: RpcRequest,
) -> Result<RpcResponse, std::io::Error> {
    if !self.sdk_has_capability("sdk.capability.identity_import_export") {
        return Ok(self.sdk_capability_disabled_response(
            request.id,
            "sdk_identity_import_v2",
            "sdk.capability.identity_import_export",
        ));
    }
    let _domain_state_guard = self.lock_and_restore_sdk_domain_snapshot()?;
    let params = request.params.ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, "missing params")
    })?;
    let parsed: SdkIdentityImportV2Params = serde_json::from_value(params)
        .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidInput, err))?;
    if parsed
        .passphrase
        .as_deref()
        .is_some_and(|passphrase| !passphrase.is_empty())
    {
        return Ok(self.sdk_error_response(
            request.id,
            "SDK_VALIDATION_INVALID_ARGUMENT",
            "passphrase-protected identity bundles are not supported",
        ));
    }
    let _ = parsed.extensions.len();
    let bundle_base64 = match Self::normalize_non_empty(parsed.bundle_base64.as_str()) {
        Some(value) => value,
        None => {
            return Ok(self.sdk_error_response(
                request.id,
                "SDK_VALIDATION_INVALID_ARGUMENT",
                "bundle_base64 must not be empty",
            ))
        }
    };
    let decoded = BASE64_STANDARD.decode(bundle_base64.as_bytes()).map_err(|_| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, "bundle_base64 is invalid")
    })?;

    let versioned = serde_json::from_slice::<SdkPrivateIdentityBundleV1>(decoded.as_slice()).ok();
    let legacy_metadata = serde_json::from_slice::<SdkIdentityBundle>(decoded.as_slice()).ok();
    let (private_key, bundle_display_name, bundle_capabilities, bundle_metadata) =
        if let Some(versioned) = versioned {
            if versioned.version != 1 {
                return Ok(self.sdk_error_response(
                    request.id,
                    "SDK_VALIDATION_INVALID_ARGUMENT",
                    "unsupported identity bundle version",
                ));
            }
            let private_key = match BASE64_STANDARD.decode(versioned.private_key_base64.as_bytes()) {
                Ok(private_key) => private_key,
                Err(_) => {
                    return Ok(self.sdk_error_response(
                        request.id,
                        "SDK_VALIDATION_INVALID_ARGUMENT",
                        "identity bundle private key is not valid base64",
                    ))
                }
            };
            (
                private_key,
                versioned.display_name,
                versioned.capabilities,
                versioned.metadata,
            )
        } else {
            (decoded, None, Vec::new(), JsonMap::new())
        };
    let spec = ServiceIdentitySpec {
        display_name: parsed.display_name.or(bundle_display_name),
        capabilities: if parsed.capabilities.is_empty() {
            bundle_capabilities
        } else {
            parsed.capabilities
        },
        metadata: if parsed.metadata.is_empty() {
            bundle_metadata
        } else {
            parsed.metadata
        },
    };
    let bundle = if let Some(bridge) = self.service_identity_bridge() {
        match bridge.import_service_identity(private_key.as_slice(), spec) {
            Ok(record) => Self::sdk_bundle_from_service(record),
            Err(error) => return Ok(self.identity_bridge_error_response(request.id, error)),
        }
    } else if let Some(mut legacy) = legacy_metadata {
        if spec.display_name.is_some() {
            legacy.display_name = spec.display_name;
        }
        if !spec.capabilities.is_empty() {
            legacy.capabilities = spec.capabilities;
        }
        if !spec.metadata.is_empty() {
            legacy.metadata = spec.metadata;
        }
        legacy.delivery_destination = None;
        legacy
    } else {
        let mut hasher = Sha256::new();
        hasher.update(private_key.as_slice());
        let generated_identity = format!("id-{}", &encode_hex(hasher.finalize())[..16]);
        SdkIdentityBundle {
            identity: generated_identity.clone(),
            delivery_destination: None,
            public_key: format!("{generated_identity}-pub"),
            display_name: spec.display_name,
            capabilities: spec.capabilities,
            metadata: spec.metadata,
            extensions: JsonMap::new(),
        }
    };
    self.authorize_session_identity(bundle.identity.as_str());
    self.sdk_identities
        .lock()
        .expect("sdk_identities mutex poisoned")
        .insert(bundle.identity.clone(), bundle.clone());
    self.persist_sdk_domain_snapshot()?;
    Ok(RpcResponse {
        id: request.id,
        result: Some(json!({ "identity": bundle })),
        error: None,
    })
}

pub(super) fn handle_sdk_identity_export_v2(
    &self,
    request: RpcRequest,
) -> Result<RpcResponse, std::io::Error> {
    if !self.sdk_has_capability("sdk.capability.identity_import_export") {
        return Ok(self.sdk_capability_disabled_response(
            request.id,
            "sdk_identity_export_v2",
            "sdk.capability.identity_import_export",
        ));
    }
    let _domain_state_guard = self.lock_and_restore_sdk_domain_snapshot()?;
    let params = request.params.ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, "missing params")
    })?;
    let parsed: SdkIdentityExportV2Params = serde_json::from_value(params)
        .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidInput, err))?;
    let _ = parsed.extensions.len();
    let identity = match self.select_session_identity(Some(parsed.identity.as_str())) {
        Ok(identity) => identity,
        Err(error) => {
            return Ok(RpcResponse {
                id: request.id,
                result: None,
                error: Some(error),
            })
        }
    };
    let bundle = self
        .sdk_identities
        .lock()
        .expect("sdk_identities mutex poisoned")
        .get(identity.as_str())
        .cloned();
    let Some(bundle) = bundle else {
        return Ok(self.sdk_error_response(
            request.id,
            "SDK_RUNTIME_NOT_FOUND",
            "identity not found",
        ));
    };
    let raw_private_key = if let Some(bridge) = self.service_identity_bridge() {
        match bridge.export_service_identity(identity.as_str()) {
            Ok(private_key) => private_key,
            Err(error) => return Ok(self.identity_bridge_error_response(request.id, error)),
        }
    } else {
        serde_json::to_vec(&bundle).map_err(std::io::Error::other)?
    };
    let export_bundle = SdkPrivateIdentityBundleV1 {
        version: 1,
        private_key_base64: BASE64_STANDARD.encode(raw_private_key),
        display_name: bundle.display_name.clone(),
        capabilities: bundle.capabilities.clone(),
        metadata: bundle.metadata.clone(),
    };
    let raw = serde_json::to_vec(&export_bundle).map_err(std::io::Error::other)?;
    let bundle_base64 = BASE64_STANDARD.encode(raw);
    Ok(RpcResponse {
        id: request.id,
        result: Some(json!({
            "bundle": {
                "bundle_base64": bundle_base64,
                "passphrase": JsonValue::Null,
                "display_name": bundle.display_name,
                "capabilities": bundle.capabilities,
                "metadata": bundle.metadata,
                "extensions": JsonMap::<String, JsonValue>::new(),
            }
        })),
        error: None,
    })
}

pub(super) fn handle_sdk_identity_resolve_v2(
    &self,
    request: RpcRequest,
) -> Result<RpcResponse, std::io::Error> {
    if !self.sdk_has_capability("sdk.capability.identity_hash_resolution") {
        return Ok(self.sdk_capability_disabled_response(
            request.id,
            "sdk_identity_resolve_v2",
            "sdk.capability.identity_hash_resolution",
        ));
    }
    let _domain_state_guard = self.lock_and_restore_sdk_domain_snapshot()?;
    let params = request.params.ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, "missing params")
    })?;
    let parsed: SdkIdentityResolveV2Params = serde_json::from_value(params)
        .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidInput, err))?;
    let _ = parsed.extensions.len();
    let query = match Self::normalize_non_empty(parsed.hash.as_str()) {
        Some(value) => value.to_ascii_lowercase(),
        None => {
            return Ok(self.sdk_error_response(
                request.id,
                "SDK_VALIDATION_INVALID_ARGUMENT",
                "hash must not be empty",
            ))
        }
    };
    let identities_guard = self
        .sdk_identities
        .lock()
        .expect("sdk_identities mutex poisoned");
    let identity = identities_guard.values().find_map(|bundle| {
        if bundle.identity.eq_ignore_ascii_case(query.as_str()) {
            return Some(bundle.identity.clone());
        }
        if bundle.public_key.to_ascii_lowercase().contains(query.as_str()) {
            return Some(bundle.identity.clone());
        }
        None
    });
    Ok(RpcResponse {
        id: request.id,
        result: Some(json!({ "identity": identity })),
        error: None,
    })
}
}
