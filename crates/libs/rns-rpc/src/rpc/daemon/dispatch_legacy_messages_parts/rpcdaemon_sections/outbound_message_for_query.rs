impl RpcDaemon {

    fn outbound_message_for_query(
        &self,
        lookup: &str,
    ) -> Result<Option<MessageRecord>, std::io::Error> {
        if let Some(message) = self.store.get_message(lookup).map_err(std::io::Error::other)? {
            return Ok(Some(message));
        }

        let messages = self.store.list_messages(500, None).map_err(std::io::Error::other)?;
        Ok(messages.into_iter().find(|message| {
            message.id == lookup || Self::message_lxmf_field_matches(message, lookup)
        }))
    }

    fn message_lxmf_field_matches(message: &MessageRecord, lookup: &str) -> bool {
        Self::message_lxmf(message).is_some_and(|lxmf| {
            ["message_id", "lxm_hash", "hash", "transient_id", "propagation_transient_id"]
                .iter()
                .any(|key| lxmf.get(*key).and_then(JsonValue::as_str) == Some(lookup))
        })
    }

    fn outbound_progress_for_message(message: &MessageRecord) -> Option<f64> {
        if message.direction != "out" {
            return None;
        }
        if let Some(status) = message.receipt_status.as_deref() {
            let normalized = status.trim().to_ascii_lowercase();
            if normalized.starts_with("sent") || normalized == "delivered" {
                return Some(1.0);
            }
            if normalized.starts_with("failed")
                || matches!(normalized.as_str(), "cancelled" | "expired" | "rejected")
            {
                return None;
            }
        }
        let lxmf = Self::message_lxmf(message);
        let stamp_state = lxmf
            .and_then(|lxmf| lxmf.get("stamp_state"))
            .and_then(JsonValue::as_str)
            .map(|state| state.trim().to_ascii_lowercase());
        let propagation_stamp_state = lxmf
            .and_then(|lxmf| lxmf.get("propagation_stamp_state"))
            .and_then(JsonValue::as_str)
            .map(|state| state.trim().to_ascii_lowercase());
        let explicit_progress =
            lxmf.and_then(|lxmf| lxmf.get("progress")).and_then(JsonValue::as_f64);

        if matches!(stamp_state.as_deref(), Some("failed" | "cancelled"))
            || matches!(propagation_stamp_state.as_deref(), Some("failed" | "cancelled"))
        {
            return None;
        }
        if let Some(progress) = explicit_progress {
            return Some(progress.clamp(0.0, 1.0));
        }
        if matches!(stamp_state.as_deref(), Some("generating"))
            || matches!(propagation_stamp_state.as_deref(), Some("generating"))
        {
            return Some(0.0);
        }
        if message.receipt_status.as_deref().is_some_and(|status| {
            matches!(status.trim().to_ascii_lowercase().as_str(), "queued" | "sending")
        }) {
            return Some(0.01);
        }
        Some(0.0)
    }

    fn outbound_stamp_cost_for_message(message: &MessageRecord) -> Option<u32> {
        if message.direction != "out" {
            return None;
        }
        if message.receipt_status.as_deref().is_some_and(Self::outbound_query_terminal_status) {
            return None;
        }
        let lxmf = Self::message_lxmf(message)?;
        if Self::lxmf_state_is_terminal(lxmf, "stamp_state") {
            return None;
        }
        if Self::has_outbound_ticket_marker(lxmf.get("outbound_ticket"))
            || Self::has_outbound_ticket_marker(lxmf.get("stamp_ticket_source"))
            || lxmf.get("stamp_kind").and_then(JsonValue::as_str) == Some("ticket")
        {
            return None;
        }
        Self::json_u32(lxmf.get("stamp_cost")).ok().flatten()
            .or_else(|| Self::json_u32(lxmf.get("stamp_target_cost")).ok().flatten())
    }

    fn outbound_propagation_stamp_cost_for_message(message: &MessageRecord) -> Option<u32> {
        if message.direction != "out" {
            return None;
        }
        if message.receipt_status.as_deref().is_some_and(Self::outbound_query_terminal_status) {
            return None;
        }
        let lxmf = Self::message_lxmf(message)?;
        if Self::lxmf_state_is_terminal(lxmf, "propagation_stamp_state") {
            return None;
        }
        Self::json_u32(lxmf.get("propagation_target_cost")).ok().flatten()
            .or_else(|| Self::json_u32(lxmf.get("propagation_stamp_target_cost")).ok().flatten())
    }

    fn lxmf_state_is_terminal(lxmf: &serde_json::Map<String, JsonValue>, state_key: &str) -> bool {
        lxmf.get(state_key).and_then(JsonValue::as_str).is_some_and(|state| {
            matches!(state.trim().to_ascii_lowercase().as_str(), "failed" | "cancelled")
        })
    }

    fn has_outbound_ticket_marker(value: Option<&JsonValue>) -> bool {
        match value {
            Some(JsonValue::String(ticket)) => !ticket.trim().is_empty(),
            Some(JsonValue::Null) | None => false,
            Some(_) => true,
        }
    }

    fn outbound_query_terminal_status(status: &str) -> bool {
        let normalized = status.trim().to_ascii_lowercase();
        normalized.starts_with("sent")
            || normalized.starts_with("failed")
            || matches!(normalized.as_str(), "delivered" | "cancelled" | "expired" | "rejected")
    }

    fn message_lxmf(message: &MessageRecord) -> Option<&serde_json::Map<String, JsonValue>> {
        let JsonValue::Object(fields) = message.fields.as_ref()? else {
            return None;
        };
        let JsonValue::Object(lxmf) = fields.get("_lxmf")? else {
            return None;
        };
        Some(lxmf)
    }

    fn json_u32(value: Option<&JsonValue>) -> Result<Option<u32>, &'static str> {
        let Some(v) = value else { return Ok(None) };
        match v {
            JsonValue::Number(number) => {
                let parsed = number.as_u64().and_then(|n| u32::try_from(n).ok()).or_else(|| {
                    let f = number.as_f64()?;
                    (f.is_finite() && f.fract() == 0.0 && f >= 0.0 && f <= f64::from(u32::MAX))
                        .then_some(f as u32)
                });
                parsed.map(Some).ok_or("number is out of u32 range")
            }
            JsonValue::String(s) => {
                Self::string_u32(s).map(Some).ok_or("string is not a valid u32")
            }
            _ => Err("value is not a number or string"),
        }
    }

    fn string_u32(value: &str) -> Option<u32> {
        let value = value.trim();
        value.parse::<u32>().ok().or_else(|| {
            let value = value.parse::<f64>().ok()?;
            (value.is_finite()
                && value.fract() == 0.0
                && value >= 0.0
                && value <= f64::from(u32::MAX))
            .then_some(value as u32)
        })
    }

    fn message_requested_ticket(message: &MessageRecord) -> bool {
        Self::message_lxmf(message)
            .and_then(|lxmf| lxmf.get("include_ticket"))
            .and_then(JsonValue::as_bool)
            .unwrap_or(false)
    }

    fn clear_invalid_restored_peer_peering_key(&self, record: &PeerRecord) {
        let (Some(peering_cost), Some(peering_key_value)) =
            (record.peering_cost, record.peering_key_value)
        else {
            return;
        };
        if peering_key_value >= peering_cost {
            return;
        }
        let mut guard = self.peers.lock().expect("peers mutex poisoned");
        if let Some(existing) = guard.get_mut(&record.peer) {
            if existing.peering_cost == Some(peering_cost)
                && existing.peering_key_value == Some(peering_key_value)
            {
                existing.peering_key_stamp = None;
                existing.peering_key_value = None;
            }
        }
    }

    pub(super) fn restore_peer_record_queue_marks(
        &self,
        record: &PeerRecord,
    ) -> Result<(), std::io::Error> {
        fn push_unique(ids: &mut Vec<String>, transient_id: String) {
            if !ids.iter().any(|id| id.eq_ignore_ascii_case(transient_id.as_str())) {
                ids.push(transient_id);
            }
        }

        let mut restored_unhandled_ids = Vec::new();
        for transient_id in &record.restored_unhandled_ids {
            let transient_id = transient_id.trim().to_ascii_lowercase();
            if self
                .store
                .get_propagation_entry(transient_id.as_str())
                .map_err(std::io::Error::other)?
                .is_some()
            {
                self.store
                    .mark_peer_unhandled_propagation(record.peer.as_str(), transient_id.as_str())
                    .map_err(std::io::Error::other)?;
                push_unique(&mut restored_unhandled_ids, transient_id);
            }
        }
        for entry in self
            .store
            .list_peer_unhandled_propagation(record.peer.as_str())
            .map_err(std::io::Error::other)?
        {
            push_unique(
                &mut restored_unhandled_ids,
                entry.transient_id.trim().to_ascii_lowercase(),
            );
        }

        let mut restored_handled_ids = Vec::new();
        for transient_id in &record.restored_handled_ids {
            let transient_id = transient_id.trim().to_ascii_lowercase();
            if self
                .store
                .get_propagation_entry(transient_id.as_str())
                .map_err(std::io::Error::other)?
                .is_some()
            {
                self.store
                    .mark_peer_handled_propagation(record.peer.as_str(), transient_id.as_str())
                    .map_err(std::io::Error::other)?;
                push_unique(&mut restored_handled_ids, transient_id);
            }
        }
        for transient_id in self
            .store
            .list_peer_handled_propagation_ids(record.peer.as_str())
            .map_err(std::io::Error::other)?
        {
            let transient_id = transient_id.trim().to_ascii_lowercase();
            if self
                .store
                .get_propagation_entry(transient_id.as_str())
                .map_err(std::io::Error::other)?
                .is_some()
            {
                push_unique(&mut restored_handled_ids, transient_id);
            }
        }
        restored_unhandled_ids.retain(|transient_id| {
            !restored_handled_ids
                .iter()
                .any(|handled_id| handled_id.eq_ignore_ascii_case(transient_id))
        });

        let mut guard = self.peers.lock().expect("peers mutex poisoned");
        let existing_peer_key = guard
            .keys()
            .find(|existing| existing.eq_ignore_ascii_case(record.peer.as_str()))
            .cloned();
        if let Some(existing_peer_key) = existing_peer_key {
            if let Some(existing) = guard.get_mut(&existing_peer_key) {
                existing.restored_handled_ids = restored_handled_ids;
                existing.restored_unhandled_ids = restored_unhandled_ids;
            }
        }

        Ok(())
    }

    fn record_peer_queue_handled(&self, peer: &str, transient_id: &str) {
        self.record_peer_queue_handled_id(peer, transient_id);
    }

    pub(super) fn restart_required_response(
        id: u64,
        operation: &str,
        affected_interfaces: Vec<String>,
    ) -> RpcResponse {
        let mut error = RpcError::new(
            "CONFIG_RESTART_REQUIRED",
            "requested interface mutation requires daemon restart",
        );
        error.machine_code = Some("UNSUPPORTED_MUTATION_KIND_REQUIRES_RESTART".to_string());
        error.category = Some("Config".to_string());
        error.retryable = Some(false);
        error.is_user_actionable = Some(true);

        let mut details = serde_json::Map::new();
        details.insert("operation".to_string(), JsonValue::String(operation.to_string()));
        details.insert(
            "affected_interfaces".to_string(),
            JsonValue::Array(
                affected_interfaces
                    .iter()
                    .map(|item| JsonValue::String(item.clone()))
                    .collect::<Vec<_>>(),
            ),
        );
        details.insert(
            "legacy_hot_apply_supported_kinds".to_string(),
            json!(["tcp_client", "tcp_server"]),
        );
        error.details = Some(Box::new(details));

        RpcResponse { id, result: None, error: Some(error) }
    }

    pub(super) fn is_legacy_hot_apply_kind(kind: &str) -> bool {
        matches!(kind, "tcp_client" | "tcp_server")
    }

    pub(super) fn interface_identifier(iface: &InterfaceRecord, index: usize) -> String {
        iface
            .name
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| format!("{}[{index}]", iface.kind))
    }

    pub(super) fn is_reload_hot_apply_compatible(
        current: &[InterfaceRecord],
        next: &[InterfaceRecord],
    ) -> bool {
        if current.len() != next.len() {
            return false;
        }
        current.iter().zip(next.iter()).all(|(before, after)| {
            before.kind == after.kind && Self::is_legacy_hot_apply_kind(before.kind.as_str())
        })
    }

    pub(super) fn validate_legacy_hot_apply_uniqueness(
        interfaces: &[InterfaceRecord],
    ) -> Result<(), std::io::Error> {
        let mut seen = std::collections::HashSet::new();
        for (index, iface) in interfaces.iter().enumerate() {
            if iface.kind != "tcp_client" {
                continue;
            }
            let Some(key) = Self::legacy_tcp_interface_key(iface) else {
                continue;
            };
            if !seen.insert(key.clone()) {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    format!(
                        "duplicate legacy tcp interface key '{}' at {}",
                        key,
                        Self::interface_identifier(iface, index)
                    ),
                ));
            }
        }
        Ok(())
    }

    pub(super) fn legacy_tcp_interface_key(iface: &InterfaceRecord) -> Option<String> {
        if iface.kind != "tcp_client" {
            return None;
        }
        if let Some(name) = iface.name.as_deref().map(str::trim).filter(|value| !value.is_empty()) {
            return Some(name.to_string());
        }
        let host = iface.host.as_deref()?.trim();
        let port = iface.port?;
        Some(format!("{host}:{port}"))
    }
}
