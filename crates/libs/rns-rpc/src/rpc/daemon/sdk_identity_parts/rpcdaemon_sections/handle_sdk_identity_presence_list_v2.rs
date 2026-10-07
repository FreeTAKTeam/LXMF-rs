impl RpcDaemon {
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
        let (mut peers, total) = {
            let records = self.peers.lock().expect("peers mutex poisoned");
            // Sort borrowed records, then copy only metadata for this page.
            // Cloning PeerRecord would copy every peer's propagation inventory.
            let mut rows = records
                .values()
                .filter(|peer| {
                    parsed.min_last_seen_ts_ms.is_none_or(|minimum| peer.last_seen >= minimum)
                })
                .collect::<Vec<_>>();
            rows.sort_by(|left, right| {
                right.last_seen.cmp(&left.last_seen).then_with(|| left.peer.cmp(&right.peer))
            });
            let total = rows.len();
            if start_index > total {
                return Ok(self.sdk_error_response(
                    request.id,
                    "SDK_RUNTIME_INVALID_CURSOR",
                    "presence cursor is out of range",
                ));
            }
            let page = rows
                .into_iter()
                .skip(start_index)
                .take(limit)
                .map(|peer| SdkPresenceRecord {
                    peer_id: peer.peer.clone(),
                    last_seen_ts_ms: peer.last_seen,
                    first_seen_ts_ms: peer.first_seen,
                    seen_count: peer.seen_count,
                    name: peer.name.clone(),
                    name_source: peer.name_source.clone(),
                    trust_level: None,
                    bootstrap: None,
                    extensions: JsonMap::new(),
                })
                .collect::<Vec<_>>();
            (page, total)
        };
        {
            let contacts = self.sdk_contacts.lock().expect("sdk_contacts mutex poisoned");
            for peer in &mut peers {
                if let Some(contact) = contacts.get(peer.peer_id.as_str()) {
                    peer.trust_level = Some(contact.trust_level.clone());
                    peer.bootstrap = Some(contact.bootstrap);
                }
            }
        }
        let next_cursor = Self::collection_next_cursor(
            "presence:",
            start_index.saturating_add(peers.len()),
            total,
        );
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
}
