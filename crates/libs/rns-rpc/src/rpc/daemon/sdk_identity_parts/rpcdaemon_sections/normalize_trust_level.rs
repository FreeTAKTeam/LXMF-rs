impl RpcDaemon {

    pub(super) fn normalize_trust_level(value: &str) -> Result<Option<String>, &'static str> {
        match value.trim().to_ascii_lowercase().as_str() {
            "" => Ok(None),
            "unknown" => Ok(Some("unknown".to_string())),
            "untrusted" => Ok(Some("untrusted".to_string())),
            "trusted" => Ok(Some("trusted".to_string())),
            "blocked" => Ok(Some("blocked".to_string())),
            _ => Err("unknown trust level"),
        }
    }

    pub(crate) fn service_identity_bridge(&self) -> Option<Arc<dyn ServiceIdentityBridge>> {
        self.service_identity_bridge
            .lock()
            .expect("service_identity_bridge mutex poisoned")
            .clone()
    }

    fn sdk_bundle_from_service(record: ServiceIdentityRecord) -> SdkIdentityBundle {
        SdkIdentityBundle {
            identity: record.identity,
            delivery_destination: Some(record.delivery_destination),
            public_key: record.public_key,
            display_name: record.display_name,
            capabilities: record.capabilities,
            metadata: record.metadata,
            extensions: JsonMap::new(),
        }
    }

    fn authorize_session_identity(&self, identity: &str) {
        let session_id = current_rpc_session_id();
        let mut sessions = self
            .sdk_identity_sessions
            .lock()
            .expect("sdk_identity_sessions mutex poisoned");
        let session = sessions.entry(session_id).or_default();
        session.authorized_identities.insert(identity.to_owned());
        if session.active_identity.is_none() {
            session.active_identity = Some(identity.to_owned());
        }
    }

    #[allow(clippy::result_large_err)]
    fn select_session_identity(&self, requested: Option<&str>) -> Result<String, RpcError> {
        let session_id = current_rpc_session_id();
        let sessions = self
            .sdk_identity_sessions
            .lock()
            .expect("sdk_identity_sessions mutex poisoned");
        let session = sessions.get(session_id.as_str()).cloned().unwrap_or_default();
        if let Some(identity) = requested.and_then(Self::normalize_non_empty) {
            return if session.authorized_identities.contains(identity.as_str()) {
                Ok(identity)
            } else {
                Err(RpcError::new(
                    "SDK_SECURITY_IDENTITY_FORBIDDEN",
                    "identity is not authorized for this SDK session",
                ))
            };
        }
        if let Some(active) = session
            .active_identity
            .filter(|identity| session.authorized_identities.contains(identity))
        {
            return Ok(active);
        }
        match session.authorized_identities.len() {
            0 => Err(RpcError::new(
                "SDK_RUNTIME_NOT_FOUND",
                "no service identity is registered for this SDK session",
            )),
            1 => Ok(session
                .authorized_identities
                .iter()
                .next()
                .expect("one authorized identity")
                .clone()),
            _ => Err(RpcError::new(
                "SDK_VALIDATION_AMBIGUOUS_IDENTITY",
                "identity must be specified when the SDK session owns multiple identities",
            )),
        }
    }

    fn identity_bridge_error_response(&self, request_id: u64, error: std::io::Error) -> RpcResponse {
        let code = match error.kind() {
            std::io::ErrorKind::NotFound => "SDK_RUNTIME_NOT_FOUND",
            std::io::ErrorKind::InvalidInput | std::io::ErrorKind::InvalidData => {
                "SDK_VALIDATION_INVALID_ARGUMENT"
            }
            std::io::ErrorKind::PermissionDenied => "SDK_SECURITY_IDENTITY_FORBIDDEN",
            _ => "SDK_RUNTIME_IDENTITY_FAILURE",
        };
        self.sdk_error_response(request_id, code, error.to_string().as_str())
    }

    pub(super) fn current_session_delivery_destinations(&self) -> HashSet<String> {
        let session_id = current_rpc_session_id();
        let authorized = self
            .sdk_identity_sessions
            .lock()
            .expect("sdk_identity_sessions mutex poisoned")
            .get(session_id.as_str())
            .map(|session| session.authorized_identities.clone())
            .unwrap_or_default();
        if let Some(bridge) = self.service_identity_bridge() {
            return bridge
                .list_service_identities()
                .unwrap_or_default()
                .into_iter()
                .filter(|record| authorized.contains(record.identity.as_str()))
                .map(|record| record.delivery_destination.to_ascii_lowercase())
                .collect();
        }
        self.sdk_identities
            .lock()
            .expect("sdk_identities mutex poisoned")
            .values()
            .filter(|bundle| authorized.contains(bundle.identity.as_str()))
            .filter_map(|bundle| bundle.delivery_destination.clone())
            .map(|destination| destination.to_ascii_lowercase())
            .collect()
    }

    #[allow(clippy::result_large_err)]
    pub(super) fn validate_current_session_source(&self, source: &str) -> Result<(), RpcError> {
        if self.service_identity_bridge().is_none() {
            return Ok(());
        }
        let authorized = self.current_session_delivery_destinations();
        if authorized.contains(source.to_ascii_lowercase().as_str()) {
            Ok(())
        } else {
            Err(RpcError::new(
                "SDK_SECURITY_IDENTITY_FORBIDDEN",
                "source delivery destination is not authorized for this SDK session",
            ))
        }
    }

    pub(super) fn event_visible_to_current_identity_session(
        &self,
        event: &RpcEvent,
        delivery_destinations: &HashSet<String>,
    ) -> bool {
        let Some(message) = event.payload.get("message") else {
            return true;
        };
        let direction = message.get("direction").and_then(JsonValue::as_str);
        let scoped_destination = match direction {
            Some("in") => message.get("destination"),
            Some("out") => message.get("source"),
            _ => message
                .get("local_destination")
                .or_else(|| message.get("destination")),
        }
        .and_then(JsonValue::as_str);
        scoped_destination.is_none_or(|destination| {
            delivery_destinations.contains(destination.to_ascii_lowercase().as_str())
        })
    }

    pub(super) fn handle_sdk_identity_list_v2(
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
        let session_id = current_rpc_session_id();
        let authorized = self
            .sdk_identity_sessions
            .lock()
            .expect("sdk_identity_sessions mutex poisoned")
            .get(session_id.as_str())
            .map(|session| session.authorized_identities.clone())
            .unwrap_or_default();
        let mut identities = if let Some(bridge) = self.service_identity_bridge() {
            match bridge.list_service_identities() {
                Ok(records) => records
                    .into_iter()
                    .filter(|record| authorized.contains(record.identity.as_str()))
                    .map(Self::sdk_bundle_from_service)
                    .collect::<Vec<_>>(),
                Err(error) => return Ok(self.identity_bridge_error_response(request.id, error)),
            }
        } else {
            self.sdk_identities
                .lock()
                .expect("sdk_identities mutex poisoned")
                .values()
                .filter(|bundle| authorized.contains(bundle.identity.as_str()))
                .cloned()
                .collect::<Vec<_>>()
        };
        identities.sort_by(|left, right| left.identity.cmp(&right.identity));
        Ok(RpcResponse {
            id: request.id,
            result: Some(json!({ "identities": identities })),
            error: None,
        })
    }

    pub(super) fn handle_sdk_identity_create_v2(
        &self,
        request: RpcRequest,
    ) -> Result<RpcResponse, std::io::Error> {
        if !self.sdk_has_capability("sdk.capability.identity_multi")
            || !self.sdk_has_capability("sdk.capability.identity_import_export")
        {
            return Ok(self.sdk_capability_disabled_response(
                request.id,
                "sdk_identity_create_v2",
                "sdk.capability.identity_multi",
            ));
        }
        let _domain_state_guard = self.lock_and_restore_sdk_domain_snapshot()?;
        let parsed: SdkIdentityCreateV2Params = serde_json::from_value(
            request.params.unwrap_or_else(|| JsonValue::Object(JsonMap::new())),
        )
        .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidInput, error))?;
        let _ = parsed.extensions.len();
        let Some(bridge) = self.service_identity_bridge() else {
            return Ok(self.sdk_error_response(
                request.id,
                "SDK_RUNTIME_IDENTITY_UNAVAILABLE",
                "real service identity bridge is not configured",
            ));
        };
        let record = match bridge.create_service_identity(ServiceIdentitySpec {
            display_name: parsed.display_name,
            capabilities: parsed.capabilities,
            metadata: parsed.metadata,
        }) {
            Ok(record) => record,
            Err(error) => return Ok(self.identity_bridge_error_response(request.id, error)),
        };
        let bundle = Self::sdk_bundle_from_service(record);
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

    pub(super) fn handle_sdk_identity_announce_now_v2(
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
        let identity = match self.select_session_identity(parsed.identity.as_deref()) {
            Ok(identity) => identity,
            Err(error) => {
                return Ok(RpcResponse {
                    id: request.id,
                    result: None,
                    error: Some(error),
                })
            }
        };
        let existing = self
            .sdk_identities
            .lock()
            .expect("sdk_identities mutex poisoned")
            .get(identity.as_str())
            .cloned();
        let spec = ServiceIdentitySpec {
            display_name: parsed
                .display_name
                .or_else(|| existing.as_ref().and_then(|bundle| bundle.display_name.clone())),
            capabilities: if parsed.capabilities.is_empty() {
                existing
                    .as_ref()
                    .map(|bundle| bundle.capabilities.clone())
                    .unwrap_or_default()
            } else {
                parsed.capabilities
            },
            metadata: if parsed.metadata.is_empty() {
                existing
                    .as_ref()
                    .map(|bundle| bundle.metadata.clone())
                    .unwrap_or_default()
            } else {
                parsed.metadata
            },
        };
        let bundle = if let Some(bridge) = self.service_identity_bridge() {
            match bridge.announce_service_identity(identity.as_str(), spec) {
                Ok(record) => Self::sdk_bundle_from_service(record),
                Err(error) => return Ok(self.identity_bridge_error_response(request.id, error)),
            }
        } else {
            if let Some(bridge) = &self.announce_bridge {
                if let Some(destination) =
                    existing.as_ref().and_then(|bundle| bundle.delivery_destination.as_deref())
                {
                    bridge.announce_delivery(destination)?;
                } else {
                    bridge.announce_now()?;
                }
            }
            existing.unwrap_or_else(|| Self::default_sdk_identity(identity.as_str()))
        };
        self.sdk_identities
            .lock()
            .expect("sdk_identities mutex poisoned")
            .insert(identity, bundle.clone());
        self.persist_sdk_domain_snapshot()?;
        let timestamp = now_millis_u64() as i64;
        let event = RpcEvent {
            event_type: "announce_sent".into(),
            payload: json!({
                "timestamp": timestamp,
                "announce_id": request.id,
                "identity": bundle.identity,
                "delivery_destination": bundle.delivery_destination,
                "display_name": bundle.display_name,
            }),
        };
        self.publish_event(event);
        Ok(RpcResponse {
            id: request.id,
            result: Some(json!({
                "accepted": true,
                "announce_id": request.id,
                "identity": bundle.identity,
                "delivery_destination": bundle.delivery_destination,
                "display_name": bundle.display_name,
                "capabilities": bundle.capabilities,
                "metadata": bundle.metadata,
                "extensions": JsonMap::<String, JsonValue>::new(),
            })),
            error: None,
        })
    }

    pub(super) fn handle_sdk_identity_presence_list_v2(
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
        let mut peer_rows =
            self.peers.lock().expect("peers mutex poisoned").values().cloned().collect::<Vec<_>>();
        if let Some(min_last_seen_ts_ms) = parsed.min_last_seen_ts_ms {
            peer_rows.retain(|peer| peer.last_seen >= min_last_seen_ts_ms);
        }
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
        let contacts = self.sdk_contacts.lock().expect("sdk_contacts mutex poisoned").clone();
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

    pub(super) fn handle_sdk_identity_activate_v2(
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
        let session_id = current_rpc_session_id();
        let mut sessions = self
            .sdk_identity_sessions
            .lock()
            .expect("sdk_identity_sessions mutex poisoned");
        let session = sessions.entry(session_id).or_default();
        if !session.authorized_identities.contains(identity.as_str()) {
            return Ok(self.sdk_error_response(
                request.id,
                "SDK_SECURITY_IDENTITY_FORBIDDEN",
                "identity is not authorized for this SDK session",
            ));
        }
        session.active_identity = Some(identity.clone());
        Ok(RpcResponse {
            id: request.id,
            result: Some(json!({ "accepted": true, "identity": identity })),
            error: None,
        })
    }

}

include!("identity_import_export_resolve.rs");
