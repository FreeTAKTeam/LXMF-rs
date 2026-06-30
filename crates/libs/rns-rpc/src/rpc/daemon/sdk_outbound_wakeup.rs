use super::*;

impl RpcDaemon {
    pub(super) fn wake_lxmf_delivery_outbound_for_announce(
        &self,
        destination: &str,
    ) -> Result<usize, std::io::Error> {
        if self.outbound_bridge.is_none() {
            return Ok(0);
        }

        let candidates = self
            .store
            .list_retryable_outbound_for_destination(destination)
            .map_err(std::io::Error::other)?;
        let mut scheduled = 0_usize;
        for record in candidates {
            let Some(method) = Self::lxmf_wakeable_delivery_method(&record) else {
                continue;
            };
            let options = Self::outbound_delivery_options_from_record(&record, method);
            let message_id = record.id.clone();
            let _status_guard =
                self.delivery_status_lock.lock().expect("delivery_status_lock mutex poisoned");
            let Some(current) =
                self.store.get_message(message_id.as_str()).map_err(std::io::Error::other)?
            else {
                continue;
            };
            if current
                .receipt_status
                .as_deref()
                .is_some_and(Self::receipt_status_blocks_delivery_wakeup)
            {
                continue;
            }
            let previous_status = current.receipt_status.clone();
            let resolved_status = self
                .store
                .resolve_receipt_status(message_id.as_str(), "sending")
                .map_err(std::io::Error::other)?
                .unwrap_or_else(|| "sending".to_string());
            if resolved_status != "sending" {
                continue;
            }
            if let Err(err) = self.schedule_bridge_delivery(current, options) {
                log::warn!(
                    "[daemon] failed to wake pending outbound message {message_id} after delivery announce: {err}"
                );
                self.store
                    .update_receipt_status(
                        message_id.as_str(),
                        previous_status.as_deref().unwrap_or("queued"),
                    )
                    .map_err(std::io::Error::other)?;
                continue;
            }
            self.append_delivery_trace(&message_id, "sending".to_string());
            scheduled = scheduled.saturating_add(1);
        }
        Ok(scheduled)
    }

    fn receipt_status_blocks_delivery_wakeup(status: &str) -> bool {
        let normalized = status.trim().to_ascii_lowercase();
        normalized.starts_with("sent")
            || normalized.starts_with("sending")
            || normalized.starts_with("failed")
            || Self::is_terminal_receipt_status(status)
    }

    fn lxmf_wakeable_delivery_method(record: &MessageRecord) -> Option<Option<String>> {
        let method = record
            .fields
            .as_ref()
            .and_then(|fields| fields.get("_lxmf"))
            .and_then(JsonValue::as_object)
            .and_then(|lxmf| lxmf.get("method"))
            .and_then(JsonValue::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_ascii_lowercase);
        match method.as_deref().unwrap_or("direct") {
            "direct" | "opportunistic" => Some(method),
            _ => None,
        }
    }

    fn outbound_delivery_options_from_record(
        record: &MessageRecord,
        method: Option<String>,
    ) -> OutboundDeliveryOptions {
        let lxmf = record
            .fields
            .as_ref()
            .and_then(|fields| fields.get("_lxmf"))
            .and_then(JsonValue::as_object);
        OutboundDeliveryOptions {
            method,
            stamp_cost: lxmf
                .and_then(|fields| fields.get("stamp_cost"))
                .and_then(JsonValue::as_u64)
                .and_then(|value| u32::try_from(value).ok()),
            include_ticket: lxmf
                .and_then(|fields| fields.get("include_ticket"))
                .and_then(JsonValue::as_bool)
                .unwrap_or_default(),
            try_propagation_on_fail: lxmf
                .and_then(|fields| fields.get("try_propagation_on_fail"))
                .and_then(JsonValue::as_bool)
                .unwrap_or_default(),
            ticket: lxmf
                .and_then(|fields| fields.get("ticket"))
                .and_then(JsonValue::as_str)
                .map(ToOwned::to_owned),
            source_private_key: lxmf
                .and_then(|fields| fields.get("source_private_key"))
                .and_then(JsonValue::as_str)
                .map(ToOwned::to_owned),
        }
    }
}
