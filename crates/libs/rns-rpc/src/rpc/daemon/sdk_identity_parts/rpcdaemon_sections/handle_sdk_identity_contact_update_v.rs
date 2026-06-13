impl RpcDaemon {

    pub(super) fn handle_sdk_identity_contact_update_v2(
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
        let display_name = parsed.display_name.as_deref().and_then(Self::normalize_non_empty);
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
            let mut contacts = self.sdk_contacts.lock().expect("sdk_contacts mutex poisoned");
            let existing = contacts.get(&identity).cloned();
            let record = SdkContactRecord {
                identity: identity.to_string(),
                display_name: display_name
                    .or_else(|| existing.as_ref().and_then(|current| current.display_name.clone())),
                trust_level: trust_level.unwrap_or_else(|| {
                    existing
                        .as_ref()
                        .map(|current| current.trust_level.clone())
                        .unwrap_or_else(|| "unknown".to_string())
                }),
                bootstrap: parsed
                    .bootstrap
                    .unwrap_or_else(|| existing.as_ref().is_some_and(|current| current.bootstrap)),
                updated_ts_ms: now,
                metadata: if parsed.metadata.is_empty() {
                    existing.as_ref().map(|current| current.metadata.clone()).unwrap_or_default()
                } else {
                    parsed.metadata
                },
                extensions: if parsed.extensions.is_empty() {
                    existing.as_ref().map(|current| current.extensions.clone()).unwrap_or_default()
                } else {
                    parsed.extensions
                },
            };
            contacts.insert(identity.to_string(), record.clone());
            record
        };
        {
            let mut order =
                self.sdk_contact_order.lock().expect("sdk_contact_order mutex poisoned");
            if !order.iter().any(|current| current == &identity) {
                order.push(identity.to_string());
            }
        }
        if let Some(name) = contact.display_name.clone() {
            if let Some(bundle) = self
                .sdk_identities
                .lock()
                .expect("sdk_identities mutex poisoned")
                .get_mut(&identity)
            {
                bundle.display_name = Some(name);
            }
        }
        self.persist_sdk_domain_snapshot()?;
        self.publish_event(RpcEvent {
            event_type: "contact_updated".into(),
            payload: json!({
                "identity": contact.identity,
                "display_name": contact.display_name,
                "trust_level": contact.trust_level,
                "bootstrap": contact.bootstrap,
                "updated_ts_ms": contact.updated_ts_ms,
            }),
        });
        Ok(RpcResponse { id: request.id, result: Some(json!({ "contact": contact })), error: None })
    }

    pub(super) fn handle_sdk_identity_contact_list_v2(
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
        let order_guard = self.sdk_contact_order.lock().expect("sdk_contact_order mutex poisoned");
        if start_index > order_guard.len() {
            return Ok(self.sdk_error_response(
                request.id,
                "SDK_RUNTIME_INVALID_CURSOR",
                "contact cursor is out of range",
            ));
        }
        let contacts_guard = self.sdk_contacts.lock().expect("sdk_contacts mutex poisoned");
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

    pub(super) fn handle_sdk_identity_bootstrap_v2(
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
            Some(value) => value,
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
            let mut contacts = self.sdk_contacts.lock().expect("sdk_contacts mutex poisoned");
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
            let mut order =
                self.sdk_contact_order.lock().expect("sdk_contact_order mutex poisoned");
            if !order.iter().any(|current| current == identity.as_str()) {
                order.push(identity.clone());
            }
        }
        if parsed.auto_sync {
            let timestamp = now as i64;
            let _ = self.upsert_peer(
                identity,
                timestamp,
                Vec::new(),
                contact.display_name.clone(),
                Some("bootstrap".to_string()),
                Some("bootstrap".to_string()),
            );
        }
        self.persist_sdk_domain_snapshot()?;
        self.publish_event(RpcEvent {
            event_type: "contact_bootstrapped".into(),
            payload: json!({
                "identity": contact.identity,
                "display_name": contact.display_name,
                "trust_level": contact.trust_level,
                "bootstrap": contact.bootstrap,
                "updated_ts_ms": contact.updated_ts_ms,
                "synced": parsed.auto_sync,
            }),
        });
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
