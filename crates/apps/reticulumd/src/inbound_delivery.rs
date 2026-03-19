use std::borrow::Cow;

use lxmf::inbound_decode::{decode_inbound_message, InboundPayloadMode};
use lxmf::WireMessage;
use rns_rpc::{MessageRecord, RpcDaemon};

use crate::lxmf_bridge::rmpv_to_json;
use crate::lxmf_stamps::validate_stamp;

pub fn decode_inbound_payload(
    destination: [u8; 16],
    payload: &[u8],
    mode: InboundPayloadMode,
) -> Option<MessageRecord> {
    decode_inbound_payload_with_diagnostics(destination, payload, mode).0
}

#[derive(Debug, Clone)]
pub struct DecodeAttempt {
    pub candidate: &'static str,
    pub len: usize,
    pub error: String,
}

#[derive(Debug, Clone, Default)]
pub struct InboundDecodeDiagnostics {
    pub attempts: Vec<DecodeAttempt>,
}

impl InboundDecodeDiagnostics {
    pub fn summary(&self) -> String {
        if self.attempts.is_empty() {
            return "no decode attempts".to_string();
        }
        self.attempts
            .iter()
            .map(|attempt| format!("{}(len={}):{}", attempt.candidate, attempt.len, attempt.error))
            .collect::<Vec<_>>()
            .join(" | ")
    }
}

pub fn decode_inbound_payload_with_diagnostics(
    destination: [u8; 16],
    payload: &[u8],
    mode: InboundPayloadMode,
) -> (Option<MessageRecord>, InboundDecodeDiagnostics) {
    let mut diagnostics = InboundDecodeDiagnostics::default();
    match decode_inbound_payload_mode(destination, payload, mode) {
        Ok(record) => (Some(record), diagnostics),
        Err(error) => {
            diagnostics.attempts.push(DecodeAttempt {
                candidate: inbound_mode_label(mode),
                len: payload.len(),
                error: error.to_string(),
            });
            (None, diagnostics)
        }
    }
}

fn decode_inbound_payload_mode(
    destination: [u8; 16],
    payload: &[u8],
    mode: InboundPayloadMode,
) -> Result<MessageRecord, lxmf::LxmfError> {
    let message = decode_inbound_message(destination, payload, mode)?;
    Ok(MessageRecord {
        id: message.id,
        source: hex::encode(message.source),
        destination: hex::encode(message.destination),
        title: message.title,
        content: message.content,
        timestamp: message.timestamp,
        direction: "in".into(),
        fields: message.fields.as_ref().and_then(rmpv_to_json),
        receipt_status: None,
    })
}

pub fn inbound_stamp_policy_allows_payload(
    daemon: &RpcDaemon,
    fallback_destination: [u8; 16],
    payload: &[u8],
    mode: InboundPayloadMode,
) -> Result<(), String> {
    let policy = daemon.current_stamp_policy();
    if policy.target_cost == 0 {
        return Ok(());
    }

    let wire = match mode {
        InboundPayloadMode::FullWire => Cow::Borrowed(payload),
        InboundPayloadMode::DestinationStripped => {
            let mut with_destination_prefix = Vec::with_capacity(16 + payload.len());
            with_destination_prefix.extend_from_slice(&fallback_destination);
            with_destination_prefix.extend_from_slice(payload);
            Cow::Owned(with_destination_prefix)
        }
    };
    let message = WireMessage::unpack(wire.as_ref())
        .map_err(|error| format!("stamp validation decode failed: {error}"))?;
    let source_hex = hex::encode(message.source);
    let tickets = daemon.valid_issued_tickets_for(source_hex.as_str());
    let stamp = message.payload.stamp.as_deref().map(|value| value.as_ref());
    validate_stamp(stamp, &message.message_id(), policy.target_cost, &tickets)
        .map(|_| ())
        .ok_or_else(|| {
            format!(
                "invalid LXMF stamp for source {} and target cost {}",
                source_hex, policy.target_cost
            )
        })
}

fn inbound_mode_label(mode: InboundPayloadMode) -> &'static str {
    match mode {
        InboundPayloadMode::FullWire => "full_wire",
        InboundPayloadMode::DestinationStripped => "destination_stripped",
    }
}

#[cfg(test)]
mod tests {
    use super::{decode_inbound_payload_with_diagnostics, inbound_stamp_policy_allows_payload};
    use lxmf::inbound_decode::InboundPayloadMode;
    use rns_core::identity::PrivateIdentity;
    use rns_rpc::{RpcDaemon, RpcRequest};

    use crate::lxmf_bridge::build_wire_message_with_options;

    #[test]
    fn decode_inbound_payload_accepts_integer_timestamp_wire() {
        let destination = [0x11; 16];
        let source = [0x22; 16];
        let signature = [0x33; 64];
        let payload = rmp_serde::to_vec(&rmpv::Value::Array(vec![
            rmpv::Value::from(1_770_000_000_i64),
            rmpv::Value::from("title"),
            rmpv::Value::from("hello from python-like payload"),
            rmpv::Value::Nil,
        ]))
        .expect("payload encoding");
        let mut wire = Vec::new();
        wire.extend_from_slice(&destination);
        wire.extend_from_slice(&source);
        wire.extend_from_slice(&signature);
        wire.extend_from_slice(&payload);

        let (record, _) = decode_inbound_payload_with_diagnostics(
            destination,
            &wire,
            InboundPayloadMode::FullWire,
        );
        let record = record.expect("decoded record");
        assert_eq!(record.source, hex::encode(source));
        assert_eq!(record.destination, hex::encode(destination));
        assert_eq!(record.title, "title");
        assert_eq!(record.content, "hello from python-like payload");
        assert_eq!(record.timestamp, 1_770_000_000_i64);
        assert_eq!(record.direction, "in");
    }

    #[test]
    fn inbound_stamp_policy_rejects_missing_stamp_when_required() {
        let daemon = RpcDaemon::test_instance();
        daemon
            .handle_rpc(RpcRequest {
                id: 1,
                method: "stamp_policy_set".to_string(),
                params: Some(serde_json::json!({"target_cost": 4, "flexibility": 0})),
            })
            .expect("set stamp policy");
        let identity = PrivateIdentity::new_from_name("inbound-missing-stamp");
        let mut source = [0u8; 16];
        source.copy_from_slice(identity.address_hash().as_slice());
        let destination = [0x71u8; 16];
        let wire = build_wire_message_with_options(
            source,
            destination,
            "title",
            "content",
            None,
            &identity,
            None,
            None,
            None,
        )
        .expect("wire");

        let err = inbound_stamp_policy_allows_payload(
            &daemon,
            destination,
            &wire,
            InboundPayloadMode::FullWire,
        )
        .expect_err("missing stamp must be rejected");
        assert!(err.contains("invalid LXMF stamp"));
    }

    #[test]
    fn inbound_stamp_policy_accepts_generated_pow_stamp() {
        let daemon = RpcDaemon::test_instance();
        daemon
            .handle_rpc(RpcRequest {
                id: 2,
                method: "stamp_policy_set".to_string(),
                params: Some(serde_json::json!({"target_cost": 1, "flexibility": 0})),
            })
            .expect("set stamp policy");
        let identity = PrivateIdentity::new_from_name("inbound-pow-stamp");
        let mut source = [0u8; 16];
        source.copy_from_slice(identity.address_hash().as_slice());
        let destination = [0x72u8; 16];
        let wire = build_wire_message_with_options(
            source,
            destination,
            "title",
            "content",
            None,
            &identity,
            Some(1),
            None,
            None,
        )
        .expect("wire");

        inbound_stamp_policy_allows_payload(
            &daemon,
            destination,
            &wire,
            InboundPayloadMode::FullWire,
        )
        .expect("valid pow stamp should pass");
    }

    #[test]
    fn inbound_stamp_policy_accepts_issued_ticket_stamp() {
        let daemon = RpcDaemon::test_instance();
        daemon
            .handle_rpc(RpcRequest {
                id: 3,
                method: "stamp_policy_set".to_string(),
                params: Some(serde_json::json!({"target_cost": 16, "flexibility": 0})),
            })
            .expect("set stamp policy");
        let identity = PrivateIdentity::new_from_name("inbound-ticket-stamp");
        let mut source = [0u8; 16];
        source.copy_from_slice(identity.address_hash().as_slice());
        let source_hex = hex::encode(source);
        let ticket = daemon.ensure_ticket(source_hex.as_str(), None).expect("issue ticket");
        let destination = [0x73u8; 16];
        let wire = build_wire_message_with_options(
            source,
            destination,
            "title",
            "content",
            None,
            &identity,
            None,
            Some(ticket.ticket.as_str()),
            None,
        )
        .expect("wire");

        inbound_stamp_policy_allows_payload(
            &daemon,
            destination,
            &wire,
            InboundPayloadMode::FullWire,
        )
        .expect("ticket-validated stamp should pass");
    }

    #[test]
    fn inbound_stamp_policy_accepts_destination_stripped_pow_stamp() {
        let daemon = RpcDaemon::test_instance();
        daemon
            .handle_rpc(RpcRequest {
                id: 4,
                method: "stamp_policy_set".to_string(),
                params: Some(serde_json::json!({"target_cost": 1, "flexibility": 0})),
            })
            .expect("set stamp policy");
        let identity = PrivateIdentity::new_from_name("inbound-destination-stripped-stamp");
        let mut source = [0u8; 16];
        source.copy_from_slice(identity.address_hash().as_slice());
        let destination = [0x74u8; 16];
        let wire = build_wire_message_with_options(
            source,
            destination,
            "title",
            "content",
            None,
            &identity,
            Some(1),
            None,
            None,
        )
        .expect("wire");
        let stripped = &wire[16..];

        inbound_stamp_policy_allows_payload(
            &daemon,
            destination,
            stripped,
            InboundPayloadMode::DestinationStripped,
        )
        .expect("valid destination-stripped stamp should pass");
    }
}
