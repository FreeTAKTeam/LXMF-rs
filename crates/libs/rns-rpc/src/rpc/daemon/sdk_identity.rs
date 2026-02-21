impl RpcDaemon {
    fn normalize_trust_level(value: &str) -> Option<String> {
        match value.trim().to_ascii_lowercase().as_str() {
            "unknown" => Some("unknown".to_string()),
            "untrusted" => Some("untrusted".to_string()),
            "trusted" => Some("trusted".to_string()),
            "blocked" => Some("blocked".to_string()),
            _ => None,
        }
    }

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
        let _domain_state_guard = self.lock_and_restore_sdk_domain_snapshot()?;
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

    fn handle_sdk_identity_announce_now_v2(
        &self,
        request: RpcRequest,
    ) -> Result<RpcResponse, std::io::Error> {
        if !self.sdk_has_capability("sdk.capability.identity_discovery") {
            return Ok(self.sdk_capability_disabled_response(
                request.id,
                "sdk_identity_announce_now_v2",
                "sdk.capability.identity_discovery",
            ));
        }
        let _domain_state_guard = self.lock_and_restore_sdk_domain_snapshot()?;
        let params = request.params.unwrap_or_else(|| JsonValue::Object(JsonMap::new()));
        let parsed: SdkIdentityAnnounceNowV2Params = serde_json::from_value(params)
            .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidInput, err))?;
        let _ = parsed.extensions.len();
        if let Some(bridge) = &self.announce_bridge {
            let _ = bridge.announce_now();
        }
        let timestamp = now_millis_u64() as i64;
        let event = RpcEvent {
            event_type: "announce_sent".into(),
            payload: json!({
                "timestamp": timestamp,
                "announce_id": request.id,
            }),
        };
        self.publish_event(event);
        Ok(RpcResponse {
            id: request.id,
            result: Some(json!({ "accepted": true, "announce_id": request.id })),
            error: None,
        })
    }

    fn handle_sdk_identity_presence_list_v2(
        &self,
        request: RpcRequest,
    ) -> Result<RpcResponse, std::io::Error> {
        if !self.sdk_has_capability("sdk.capability.identity_discovery") {
            return Ok(self.sdk_capability_disabled_response(
                request.id,
                "sdk_identity_presence_list_v2",
                "sdk.capability.identity_discovery",
            ));
        }
        let _domain_state_guard = self.lock_and_restore_sdk_domain_snapshot()?;
        let params = request.params.unwrap_or_else(|| JsonValue::Object(JsonMap::new()));
        let parsed: SdkIdentityPresenceListV2Params = serde_json::from_value(params)
            .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidInput, err))?;
        let _ = parsed.extensions.len();
        let start_index = match self.collection_cursor_index(parsed.cursor.as_deref(), "presence:")
        {
            Ok(index) => index,
            Err(error) => {
                return Ok(self.sdk_error_response(
                    request.id,
                    error.code.as_str(),
                    error.message.as_str(),
                ))
            }
        };
        let limit = parsed.limit.unwrap_or(100).clamp(1, 500);
        let mut peer_rows = self
            .peers
            .lock()
            .expect("peers mutex poisoned")
            .values()
            .cloned()
            .collect::<Vec<_>>();
        peer_rows.sort_by(|left, right| {
            right.last_seen.cmp(&left.last_seen).then_with(|| left.peer.cmp(&right.peer))
        });
        if start_index > peer_rows.len() {
            return Ok(self.sdk_error_response(
                request.id,
                "SDK_RUNTIME_INVALID_CURSOR",
                "presence cursor is out of range",
            ));
        }
        let contacts = self
            .sdk_contacts
            .lock()
            .expect("sdk_contacts mutex poisoned")
            .clone();
        let mut next_index = start_index;
        let mut peers = Vec::new();
        for peer in peer_rows.iter().skip(start_index) {
            next_index = next_index.saturating_add(1);
            let (trust_level, bootstrap) = contacts
                .get(peer.peer.as_str())
                .map(|contact| (Some(contact.trust_level.clone()), Some(contact.bootstrap)))
                .unwrap_or((None, None));
            peers.push(SdkPresenceRecord {
                peer_id: peer.peer.clone(),
                last_seen_ts_ms: peer.last_seen,
                first_seen_ts_ms: peer.first_seen,
                seen_count: peer.seen_count,
                name: peer.name.clone(),
                name_source: peer.name_source.clone(),
                trust_level,
                bootstrap,
                extensions: JsonMap::new(),
            });
            if peers.len() >= limit {
                break;
            }
        }
        let next_cursor = Self::collection_next_cursor("presence:", next_index, peer_rows.len());
        Ok(RpcResponse {
            id: request.id,
            result: Some(json!({
                "presence_list": {
                    "peers": peers,
                    "next_cursor": next_cursor,
                }
            })),
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
        let _domain_state_guard = self.lock_and_restore_sdk_domain_snapshot()?;
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
        self.persist_sdk_domain_snapshot()?;
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
        let _domain_state_guard = self.lock_and_restore_sdk_domain_snapshot()?;
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
        self.persist_sdk_domain_snapshot()?;
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
        let _domain_state_guard = self.lock_and_restore_sdk_domain_snapshot()?;
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

    fn handle_sdk_identity_contact_update_v2(
        &self,
        request: RpcRequest,
    ) -> Result<RpcResponse, std::io::Error> {
        if !self.sdk_has_capability("sdk.capability.contact_management") {
            return Ok(self.sdk_capability_disabled_response(
                request.id,
                "sdk_identity_contact_update_v2",
                "sdk.capability.contact_management",
            ));
        }
        let _domain_state_guard = self.lock_and_restore_sdk_domain_snapshot()?;
        let params = request.params.ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "missing params")
        })?;
        let parsed: SdkIdentityContactUpdateV2Params = serde_json::from_value(params)
            .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidInput, err))?;
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
        let display_name = parsed
            .display_name
            .as_deref()
            .and_then(Self::normalize_non_empty)
            .map(ToString::to_string);
        let trust_level = if let Some(level) = parsed.trust_level.as_deref() {
            match Self::normalize_trust_level(level) {
                Some(value) => Some(value),
                None => {
                    return Ok(self.sdk_error_response(
                        request.id,
                        "SDK_VALIDATION_INVALID_ARGUMENT",
                        "trust_level must be unknown, untrusted, trusted, or blocked",
                    ))
                }
            }
        } else {
            None
        };
        let now = now_millis_u64();
        let contact = {
            let mut contacts = self
                .sdk_contacts
                .lock()
                .expect("sdk_contacts mutex poisoned");
            let existing = contacts.get(identity).cloned();
            let record = SdkContactRecord {
                identity: identity.to_string(),
                display_name: display_name.or_else(|| {
                    existing.as_ref().and_then(|current| current.display_name.clone())
                }),
                trust_level: trust_level.unwrap_or_else(|| {
                    existing
                        .as_ref()
                        .map(|current| current.trust_level.clone())
                        .unwrap_or_else(|| "unknown".to_string())
                }),
                bootstrap: parsed.bootstrap.unwrap_or_else(|| {
                    existing.as_ref().is_some_and(|current| current.bootstrap)
                }),
                updated_ts_ms: now,
                metadata: if parsed.metadata.is_empty() {
                    existing
                        .as_ref()
                        .map(|current| current.metadata.clone())
                        .unwrap_or_default()
                } else {
                    parsed.metadata
                },
                extensions: if parsed.extensions.is_empty() {
                    existing
                        .as_ref()
                        .map(|current| current.extensions.clone())
                        .unwrap_or_default()
                } else {
                    parsed.extensions
                },
            };
            contacts.insert(identity.to_string(), record.clone());
            record
        };
        {
            let mut order = self
                .sdk_contact_order
                .lock()
                .expect("sdk_contact_order mutex poisoned");
            if !order.iter().any(|current| current == identity) {
                order.push(identity.to_string());
            }
        }
        if let Some(name) = contact.display_name.clone() {
            if let Some(bundle) = self
                .sdk_identities
                .lock()
                .expect("sdk_identities mutex poisoned")
                .get_mut(identity)
            {
                bundle.display_name = Some(name);
            }
        }
        self.persist_sdk_domain_snapshot()?;
        Ok(RpcResponse {
            id: request.id,
            result: Some(json!({ "contact": contact })),
            error: None,
        })
    }

    fn handle_sdk_identity_contact_list_v2(
        &self,
        request: RpcRequest,
    ) -> Result<RpcResponse, std::io::Error> {
        if !self.sdk_has_capability("sdk.capability.contact_management") {
            return Ok(self.sdk_capability_disabled_response(
                request.id,
                "sdk_identity_contact_list_v2",
                "sdk.capability.contact_management",
            ));
        }
        let _domain_state_guard = self.lock_and_restore_sdk_domain_snapshot()?;
        let params = request.params.unwrap_or_else(|| JsonValue::Object(JsonMap::new()));
        let parsed: SdkIdentityContactListV2Params = serde_json::from_value(params)
            .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidInput, err))?;
        let _ = parsed.extensions.len();
        let start_index = match self.collection_cursor_index(parsed.cursor.as_deref(), "contact:") {
            Ok(index) => index,
            Err(error) => {
                return Ok(self.sdk_error_response(
                    request.id,
                    error.code.as_str(),
                    error.message.as_str(),
                ))
            }
        };
        let limit = parsed.limit.unwrap_or(100).clamp(1, 500);
        let order_guard = self
            .sdk_contact_order
            .lock()
            .expect("sdk_contact_order mutex poisoned");
        if start_index > order_guard.len() {
            return Ok(self.sdk_error_response(
                request.id,
                "SDK_RUNTIME_INVALID_CURSOR",
                "contact cursor is out of range",
            ));
        }
        let contacts_guard = self
            .sdk_contacts
            .lock()
            .expect("sdk_contacts mutex poisoned");
        let mut contacts = Vec::new();
        let mut next_index = start_index;
        for identity in order_guard.iter().skip(start_index) {
            next_index = next_index.saturating_add(1);
            let Some(record) = contacts_guard.get(identity).cloned() else {
                continue;
            };
            contacts.push(record);
            if contacts.len() >= limit {
                break;
            }
        }
        let next_cursor = Self::collection_next_cursor("contact:", next_index, order_guard.len());
        Ok(RpcResponse {
            id: request.id,
            result: Some(json!({
                "contact_list": {
                    "contacts": contacts,
                    "next_cursor": next_cursor,
                }
            })),
            error: None,
        })
    }

    fn handle_sdk_identity_bootstrap_v2(
        &self,
        request: RpcRequest,
    ) -> Result<RpcResponse, std::io::Error> {
        if !self.sdk_has_capability("sdk.capability.contact_management") {
            return Ok(self.sdk_capability_disabled_response(
                request.id,
                "sdk_identity_bootstrap_v2",
                "sdk.capability.contact_management",
            ));
        }
        let _domain_state_guard = self.lock_and_restore_sdk_domain_snapshot()?;
        let params = request.params.ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::InvalidInput, "missing params")
        })?;
        let parsed: SdkIdentityBootstrapV2Params = serde_json::from_value(params)
            .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidInput, err))?;
        let _ = parsed.extensions.len();
        let identity = match Self::normalize_non_empty(parsed.identity.as_str()) {
            Some(value) => value.to_string(),
            None => {
                return Ok(self.sdk_error_response(
                    request.id,
                    "SDK_VALIDATION_INVALID_ARGUMENT",
                    "identity must not be empty",
                ))
            }
        };
        let now = now_millis_u64();
        let contact = {
            let mut contacts = self
                .sdk_contacts
                .lock()
                .expect("sdk_contacts mutex poisoned");
            let existing = contacts.get(identity.as_str()).cloned();
            let record = SdkContactRecord {
                identity: identity.clone(),
                display_name: existing.as_ref().and_then(|current| current.display_name.clone()),
                trust_level: "trusted".to_string(),
                bootstrap: true,
                updated_ts_ms: now,
                metadata: existing
                    .as_ref()
                    .map(|current| current.metadata.clone())
                    .unwrap_or_default(),
                extensions: existing
                    .as_ref()
                    .map(|current| current.extensions.clone())
                    .unwrap_or_default(),
            };
            contacts.insert(identity.clone(), record.clone());
            record
        };
        {
            let mut order = self
                .sdk_contact_order
                .lock()
                .expect("sdk_contact_order mutex poisoned");
            if !order.iter().any(|current| current == identity.as_str()) {
                order.push(identity.clone());
            }
        }
        if parsed.auto_sync {
            let timestamp = now as i64;
            let _ = self.upsert_peer(identity.clone(), timestamp, contact.display_name.clone(), Some("bootstrap".to_string()));
        }
        self.persist_sdk_domain_snapshot()?;
        Ok(RpcResponse {
            id: request.id,
            result: Some(json!({
                "contact": contact,
                "synced": parsed.auto_sync,
            })),
            error: None,
        })
    }

}
