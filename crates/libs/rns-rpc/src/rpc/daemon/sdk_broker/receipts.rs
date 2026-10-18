use super::*;
use crate::broker::PropagationCustody;

enum Disposition {
    Ordinary,
    PropagationCustody(PropagationCustody),
}

impl RpcDaemon {
    /// Trusted network producer entry point. RPC callers remain subject to operation ownership.
    pub fn record_network_receipt(
        &self,
        message_id: &str,
        status: &str,
        mut metadata: JsonMap<String, JsonValue>,
    ) -> Result<JsonValue, std::io::Error> {
        metadata.insert("message_id".into(), json!(message_id));
        metadata.insert("status".into(), json!(status));
        let parsed: RecordReceiptParams = serde_json::from_value(JsonValue::Object(metadata))
            .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidInput, error))?;
        self.apply_receipt_params(parsed)
    }
    pub(in crate::rpc::daemon) fn apply_receipt_params(
        &self,
        parsed: RecordReceiptParams,
    ) -> Result<JsonValue, std::io::Error> {
        self.apply_receipt_disposition(parsed, Disposition::Ordinary)
    }

    /// Trusted native acknowledgement. No RPC field can select this disposition.
    pub fn record_propagation_custody(
        &self,
        message_id: &str,
        representation: PropagationCustody,
        mut metadata: JsonMap<String, JsonValue>,
    ) -> Result<JsonValue, std::io::Error> {
        metadata.insert("message_id".into(), json!(message_id));
        metadata.insert("status".into(), json!(representation.status()));
        let parsed: RecordReceiptParams = serde_json::from_value(JsonValue::Object(metadata))
            .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidInput, error))?;
        self.apply_receipt_disposition(parsed, Disposition::PropagationCustody(representation))
    }

    fn apply_receipt_disposition(
        &self,
        parsed: RecordReceiptParams,
        disposition: Disposition,
    ) -> Result<JsonValue, std::io::Error> {
        let RecordReceiptParams {
            message_id,
            status: requested_status,
            packet_hash,
            resource_hash,
            peer,
            method,
            delivery_kind,
            bytes,
            link_id,
            stage,
        } = parsed;
        let (status, updated, delivered_ticket_destination) = {
            let _status_guard =
                self.delivery_status_lock.lock().expect("delivery_status_lock mutex poisoned");
            let existing_message =
                self.store.get_message(&message_id).map_err(std::io::Error::other)?;
            let existing_status =
                existing_message.as_ref().and_then(|message| message.receipt_status.clone());
            if existing_message.is_none() {
                if matches!(disposition, Disposition::PropagationCustody(_)) {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::NotFound,
                        format!("propagation custody message not found: {message_id}"),
                    ));
                }
                (requested_status, false, None)
            } else if existing_status.as_deref().is_some_and(|status| {
                Self::is_terminal_receipt_status(status)
                    || Self::is_receipt_status_regression(status, &requested_status)
            }) {
                (existing_status.unwrap_or(requested_status), false, None)
            } else {
                let delivered_ticket_destination = existing_message
                    .as_ref()
                    .filter(|message| {
                        requested_status.eq_ignore_ascii_case("delivered")
                            && Self::message_requested_ticket(message)
                    })
                    .map(|message| message.destination.clone());
                if let Disposition::PropagationCustody(representation) = disposition {
                    if self.store.broker_owns_message(&message_id).map_err(std::io::Error::other)? {
                        let result = self
                            .store
                            .complete_propagation_custody(&message_id, representation)
                            .map_err(std::io::Error::other)?;
                        let updated =
                            result.get("committed").and_then(JsonValue::as_bool).ok_or_else(
                                || std::io::Error::other("custody result lacks committed state"),
                            )?;
                        let status = result
                            .get("status")
                            .and_then(JsonValue::as_str)
                            .ok_or_else(|| {
                                std::io::Error::other("custody result lacks receipt status")
                            })?
                            .to_owned();
                        (status, updated, None)
                    } else {
                        self.store
                            .update_receipt_status(&message_id, &requested_status)
                            .map_err(std::io::Error::other)?;
                        (requested_status, true, None)
                    }
                } else {
                    self.store
                        .update_receipt_status(&message_id, &requested_status)
                        .map_err(std::io::Error::other)?;
                    (requested_status, true, delivered_ticket_destination)
                }
            }
        };
        if updated {
            self.append_delivery_trace(&message_id, status.clone());
        }
        if let Some(destination) = delivered_ticket_destination {
            self.mark_ticket_delivered(destination.as_str());
        }
        let reason_code = delivery_reason_code(&status);
        let mut payload = JsonMap::new();
        payload.insert("message_id".into(), json!(message_id));
        payload.insert("status".into(), json!(status));
        payload.insert("updated".into(), json!(updated));
        payload.insert("reason_code".into(), json!(reason_code));
        if let Some(packet_hash) = packet_hash {
            payload.insert("packet_hash".into(), json!(packet_hash));
        }
        if let Some(resource_hash) = resource_hash {
            payload.insert("resource_hash".into(), json!(resource_hash));
        }
        if let Some(peer) = peer {
            payload.insert("peer".into(), json!(peer));
        }
        if let Some(method) = method {
            payload.insert("method".into(), json!(method));
        }
        if let Some(delivery_kind) = delivery_kind {
            payload.insert("delivery_kind".into(), json!(delivery_kind));
        }
        if let Some(bytes) = bytes {
            payload.insert("bytes".into(), json!(bytes));
        }
        if let Some(link_id) = link_id {
            payload.insert("link_id".into(), json!(link_id));
        }
        if let Some(stage) = stage {
            payload.insert("stage".into(), json!(stage));
        }
        let payload = JsonValue::Object(payload);
        let event = RpcEvent { event_type: "receipt".into(), payload: payload.clone() };
        self.publish_event(event);
        Ok(payload)
    }
}
