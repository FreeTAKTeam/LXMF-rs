impl RpcDaemon {
    fn handle_sdk_identity_list_v2(
        &self,
        request: RpcRequest,
    ) -> Result<RpcResponse, std::io::Error> {
        if !self.sdk_has_capability("sdk.capability.identity_multi") {
            return Ok(self.sdk_capability_disabled_response(
                request.id,
                "sdk_identity_list_v2",
                "sdk.capability.identity_multi",
            ));
        }
        let params = request.params.unwrap_or_else(|| JsonValue::Object(JsonMap::new()));
        let parsed: SdkIdentityListV2Params = serde_json::from_value(params)
            .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidInput, err))?;
        let _ = parsed.extensions.len();
        let mut identities = self
            .sdk_identities
            .lock()
            .expect("sdk_identities mutex poisoned")
            .values()
            .cloned()
            .collect::<Vec<_>>();
        identities.sort_by(|left, right| left.identity.cmp(&right.identity));
        Ok(RpcResponse {
            id: request.id,
            result: Some(json!({ "identities": identities })),
            error: None,
        })
    }

    fn handle_sdk_identity_activate_v2(
        &self,
        request: RpcRequest,
    ) -> Result<RpcResponse, std::io::Error> {
        if !self.sdk_has_capability("sdk.capability.identity_multi") {
            return Ok(self.sdk_capability_disabled_response(
                request.id,
                "sdk_identity_activate_v2",
                "sdk.capability.identity_multi",
            ));
        }
        let params = request.params.ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "missing params")
        })?;
        let parsed: SdkIdentityActivateV2Params = serde_json::from_value(params)
            .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidInput, err))?;
        let _ = parsed.extensions.len();
        let identity = match Self::normalize_non_empty(parsed.identity.as_str()) {
            Some(value) => value,
            None => {
                return Ok(self.sdk_error_response(
                    request.id,
                    "SDK_VALIDATION_INVALID_ARGUMENT",
                    "identity must not be empty",
                ))
            }
        };
        if !self
            .sdk_identities
            .lock()
            .expect("sdk_identities mutex poisoned")
            .contains_key(identity.as_str())
        {
            return Ok(self.sdk_error_response(
                request.id,
                "SDK_RUNTIME_NOT_FOUND",
                "identity not found",
            ));
        }
        *self.sdk_active_identity.lock().expect("sdk_active_identity mutex poisoned") =
            Some(identity.clone());
        Ok(RpcResponse {
            id: request.id,
            result: Some(json!({ "accepted": true, "identity": identity })),
            error: None,
        })
    }

    fn handle_sdk_identity_import_v2(
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
        let params = request.params.ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "missing params")
        })?;
        let parsed: SdkIdentityImportV2Params = serde_json::from_value(params)
            .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidInput, err))?;
        let _ = parsed.passphrase.as_deref();
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

        let parsed_bundle = serde_json::from_slice::<SdkIdentityBundle>(decoded.as_slice()).ok();
        let mut hasher = Sha256::new();
        hasher.update(decoded.as_slice());
        let generated_identity = format!("id-{}", &encode_hex(hasher.finalize())[..16]);
        let mut bundle = parsed_bundle.unwrap_or(SdkIdentityBundle {
            identity: generated_identity.clone(),
            public_key: format!("{generated_identity}-pub"),
            display_name: None,
            capabilities: Vec::new(),
            extensions: JsonMap::new(),
        });
        if Self::normalize_non_empty(bundle.identity.as_str()).is_none() {
            bundle.identity = generated_identity;
        }
        if Self::normalize_non_empty(bundle.public_key.as_str()).is_none() {
            bundle.public_key = format!("{}-pub", bundle.identity);
        }
        self.sdk_identities
            .lock()
            .expect("sdk_identities mutex poisoned")
            .insert(bundle.identity.clone(), bundle.clone());
        Ok(RpcResponse { id: request.id, result: Some(json!({ "identity": bundle })), error: None })
    }

    fn handle_sdk_identity_export_v2(
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
        let params = request.params.ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "missing params")
        })?;
        let parsed: SdkIdentityExportV2Params = serde_json::from_value(params)
            .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidInput, err))?;
        let _ = parsed.extensions.len();
        let identity = match Self::normalize_non_empty(parsed.identity.as_str()) {
            Some(value) => value,
            None => {
                return Ok(self.sdk_error_response(
                    request.id,
                    "SDK_VALIDATION_INVALID_ARGUMENT",
                    "identity must not be empty",
                ))
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
        let raw = serde_json::to_vec(&bundle).map_err(std::io::Error::other)?;
        let bundle_base64 = BASE64_STANDARD.encode(raw);
        Ok(RpcResponse {
            id: request.id,
            result: Some(json!({
                "bundle": {
                    "bundle_base64": bundle_base64,
                    "passphrase": JsonValue::Null,
                    "extensions": JsonMap::<String, JsonValue>::new(),
                }
            })),
            error: None,
        })
    }

    fn handle_sdk_identity_resolve_v2(
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
        let identities_guard = self.sdk_identities.lock().expect("sdk_identities mutex poisoned");
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
