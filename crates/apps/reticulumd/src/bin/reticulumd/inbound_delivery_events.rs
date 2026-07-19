use crate::bridge_helpers::payload_preview;
use lxmf::{inbound_decode::InboundPayloadMode, WireMessage};
use reticulum_daemon::inbound_delivery::{
    annotate_inbound_record_stamp_status, decode_inbound_payload_with_diagnostics,
    evaluate_inbound_stamp_policy, inbound_record_allowed_by_delivery_policy,
};
use rns_rpc::{MessageRecord, RpcDaemon};
use rns_transport::hash::AddressHash;
use rns_transport::identity_bridge::to_core_identity;
use rns_transport::transport::{ReceivedPayloadMode, Transport};
use serde_json::{Map, Value};
use std::borrow::Cow;

#[path = "inbound_delivery_events_parts/drop_events.rs"]
mod drop_events;
pub(crate) use drop_events::{
    emit_inbound_drop_event, emit_propagation_duplicate_drop_event,
    emit_propagation_predecode_drop_event, InboundDeliveryKind, InboundDropEvent,
};

pub(super) async fn accept_delivery_resource(
    daemon: &RpcDaemon,
    transport: &Transport,
    destination: [u8; 16],
    data: &[u8],
) {
    let raw_destination_hex = hex::encode(destination);
    if let Some(limit_bytes) = direct_delivery_resource_limit_exceeded(daemon, data) {
        emit_inbound_drop_event(
            daemon,
            InboundDropEvent {
                reason: "delivery_resource_too_large",
                delivery_kind: InboundDeliveryKind::Resource,
                raw_destination_hex: raw_destination_hex.as_str(),
                destination,
                payload_mode: InboundPayloadMode::FullWire,
                bytes_len: data.len(),
                detail: Some(format!(
                    "resource size {} exceeds delivery limit {} bytes",
                    data.len(),
                    limit_bytes
                )),
                record: None,
            },
        );
        return;
    }
    let (record, diagnostics) =
        decode_inbound_payload_with_diagnostics(destination, data, InboundPayloadMode::FullWire);
    let Some(mut record) = record else {
        let diagnostics_summary = diagnostics.summary();
        log::debug!(
            "[daemon-rx] resource decode-failed raw_dst={} attempts={}",
            raw_destination_hex,
            diagnostics_summary
        );
        emit_inbound_drop_event(
            daemon,
            InboundDropEvent {
                reason: "decode_failed",
                delivery_kind: InboundDeliveryKind::Resource,
                raw_destination_hex: raw_destination_hex.as_str(),
                destination,
                payload_mode: InboundPayloadMode::FullWire,
                bytes_len: data.len(),
                detail: Some(diagnostics_summary),
                record: None,
            },
        );
        return;
    };
    let stamp_status = match evaluate_inbound_stamp_policy(
        daemon,
        destination,
        data,
        InboundPayloadMode::FullWire,
    ) {
        Ok(status) => status,
        Err(error) => {
            log::warn!("[daemon-rx] dropping inbound resource due to stamp policy: {}", error);
            emit_inbound_drop_event(
                daemon,
                InboundDropEvent {
                    reason: "stamp_policy_rejected",
                    delivery_kind: InboundDeliveryKind::Resource,
                    raw_destination_hex: raw_destination_hex.as_str(),
                    destination,
                    payload_mode: InboundPayloadMode::FullWire,
                    bytes_len: data.len(),
                    detail: Some(error.to_string()),
                    record: Some(&record),
                },
            );
            return;
        }
    };
    annotate_inbound_record_stamp_status(&mut record, stamp_status);
    annotate_inbound_signature_status(
        Some(transport),
        &mut record,
        destination,
        data,
        InboundPayloadMode::FullWire,
    )
    .await;
    annotate_direct_delivery_transport_metadata(&mut record, 2);
    if !inbound_record_allowed_by_delivery_policy(daemon, &record) {
        emit_inbound_drop_event(
            daemon,
            InboundDropEvent {
                reason: "delivery_policy_rejected",
                delivery_kind: InboundDeliveryKind::Resource,
                raw_destination_hex: raw_destination_hex.as_str(),
                destination,
                payload_mode: InboundPayloadMode::FullWire,
                bytes_len: data.len(),
                detail: None,
                record: Some(&record),
            },
        );
        return;
    }
    persist_inbound_record(
        daemon,
        &record,
        data,
        InboundPersistenceContext {
            delivery_kind: InboundDeliveryKind::Resource,
            label: "resource",
            raw_destination_hex: raw_destination_hex.as_str(),
            destination,
            payload_mode: InboundPayloadMode::FullWire,
        },
    );
}

pub(super) async fn accept_delivery_packet(
    daemon: &RpcDaemon,
    transport: &Transport,
    raw_destination_hex: &str,
    destination: [u8; 16],
    data: &[u8],
    payload_mode: ReceivedPayloadMode,
) {
    let payload_mode = inbound_payload_mode(payload_mode);
    let (record, diagnostics) =
        decode_inbound_payload_with_diagnostics(destination, data, payload_mode);
    if let Some(ref decoded) = record {
        log::debug!(
            "[daemon-rx] decoded msg_id={} src={} dst={} title_len={} content_len={}",
            decoded.id,
            decoded.source,
            decoded.destination,
            decoded.title.len(),
            decoded.content.len()
        );
    } else {
        let diagnostics_summary = diagnostics.summary();
        log::debug!(
            "[daemon-rx] decode-failed raw_dst={} resolved_dst={} attempts={}",
            raw_destination_hex,
            hex::encode(destination),
            diagnostics_summary
        );
        emit_inbound_drop_event(
            daemon,
            InboundDropEvent {
                reason: "decode_failed",
                delivery_kind: InboundDeliveryKind::Packet,
                raw_destination_hex,
                destination,
                payload_mode,
                bytes_len: data.len(),
                detail: Some(diagnostics_summary),
                record: None,
            },
        );
        return;
    }
    let mut record = record.expect("decode success checked before policy evaluation");
    let stamp_status = match evaluate_inbound_stamp_policy(daemon, destination, data, payload_mode)
    {
        Ok(status) => status,
        Err(error) => {
            log::warn!(
                "[daemon-rx] dropping inbound payload due to stamp policy: raw_dst={} resolved_dst={}",
                raw_destination_hex,
                hex::encode(destination)
            );
            emit_inbound_drop_event(
                daemon,
                InboundDropEvent {
                    reason: "stamp_policy_rejected",
                    delivery_kind: InboundDeliveryKind::Packet,
                    raw_destination_hex,
                    destination,
                    payload_mode,
                    bytes_len: data.len(),
                    detail: Some(error.to_string()),
                    record: Some(&record),
                },
            );
            return;
        }
    };
    annotate_inbound_record_stamp_status(&mut record, stamp_status);
    annotate_inbound_signature_status(
        Some(transport),
        &mut record,
        destination,
        data,
        payload_mode,
    )
    .await;
    let method = match payload_mode {
        InboundPayloadMode::DestinationStripped => 1,
        InboundPayloadMode::FullWire => 2,
    };
    annotate_direct_delivery_transport_metadata(&mut record, method);
    if !inbound_record_allowed_by_delivery_policy(daemon, &record) {
        emit_inbound_drop_event(
            daemon,
            InboundDropEvent {
                reason: "delivery_policy_rejected",
                delivery_kind: InboundDeliveryKind::Packet,
                raw_destination_hex,
                destination,
                payload_mode,
                bytes_len: data.len(),
                detail: None,
                record: Some(&record),
            },
        );
        return;
    }
    if !persist_inbound_record(
        daemon,
        &record,
        data,
        InboundPersistenceContext {
            delivery_kind: InboundDeliveryKind::Packet,
            label: "packet",
            raw_destination_hex,
            destination,
            payload_mode,
        },
    ) {
        return;
    }
    daemon.record_inbound_peer_activity(&record.source, data.len());
}

#[derive(Clone, Copy)]
struct InboundPersistenceContext<'a> {
    delivery_kind: InboundDeliveryKind,
    label: &'static str,
    raw_destination_hex: &'a str,
    destination: [u8; 16],
    payload_mode: InboundPayloadMode,
}

fn persist_inbound_record(
    daemon: &RpcDaemon,
    record: &MessageRecord,
    data: &[u8],
    context: InboundPersistenceContext<'_>,
) -> bool {
    let (reason, detail) = match daemon.message_exists(record.id.as_str()) {
        Ok(true) => ("duplicate", "message already stored".to_string()),
        Ok(false) => match daemon.accept_inbound_with_raw(record.clone(), data) {
            Ok(()) => return true,
            Err(err) => {
                log::error!(
                    "[daemon-rx] inbound {} persistence failed id={}: {err}",
                    context.label,
                    record.id
                );
                ("inbound_persistence_failed", err.to_string())
            }
        },
        Err(err) => {
            log::error!(
                "[daemon-rx] inbound {} duplicate lookup failed id={}: {err}",
                context.label,
                record.id
            );
            ("message_lookup_failed", err.to_string())
        }
    };
    emit_inbound_drop_event(
        daemon,
        InboundDropEvent {
            reason,
            delivery_kind: context.delivery_kind,
            raw_destination_hex: context.raw_destination_hex,
            destination: context.destination,
            payload_mode: context.payload_mode,
            bytes_len: data.len(),
            detail: Some(detail),
            record: Some(record),
        },
    );
    false
}

pub(super) fn log_resolved_packet(
    raw_destination_hex: &str,
    resolved_destination: impl std::fmt::Debug,
    payload_mode: ReceivedPayloadMode,
    ratchet_used: bool,
    data: &[u8],
) {
    log::debug!(
        "[daemon-rx] dst={} resolved={:?} mode={:?} len={} ratchet_used={} data_prefix={}",
        raw_destination_hex,
        resolved_destination,
        payload_mode,
        data.len(),
        ratchet_used,
        payload_preview(data, 16)
    );
}

fn inbound_payload_mode(mode: ReceivedPayloadMode) -> InboundPayloadMode {
    match mode {
        ReceivedPayloadMode::FullWire => InboundPayloadMode::FullWire,
        ReceivedPayloadMode::DestinationStripped => InboundPayloadMode::DestinationStripped,
    }
}

fn direct_delivery_resource_limit_exceeded(daemon: &RpcDaemon, data: &[u8]) -> Option<u64> {
    let limit_bytes =
        u64::from(daemon.current_propagation_state().delivery_limit).saturating_mul(1000);
    let resource_size = data.len() as u64;
    (resource_size > limit_bytes).then_some(limit_bytes)
}

pub(crate) async fn annotate_inbound_signature_status(
    transport: Option<&Transport>,
    record: &mut MessageRecord,
    destination: [u8; 16],
    payload: &[u8],
    mode: InboundPayloadMode,
) {
    let wire = match mode {
        InboundPayloadMode::FullWire => Cow::Borrowed(payload),
        InboundPayloadMode::DestinationStripped => {
            let mut with_destination = Vec::with_capacity(16 + payload.len());
            with_destination.extend_from_slice(&destination);
            with_destination.extend_from_slice(payload);
            Cow::Owned(with_destination)
        }
    };

    let mut checked = false;
    let mut valid = false;
    let mut reason = "source_identity_unknown".to_string();

    match WireMessage::unpack(wire.as_ref()) {
        Ok(message) => {
            let source_hash = AddressHash::new(message.source);
            if let Some(identity) = match transport {
                Some(transport) => transport.destination_identity(&source_hash).await,
                None => None,
            } {
                checked = true;
                match message.verify(&to_core_identity(&identity)) {
                    Ok(true) => {
                        valid = true;
                        reason = "verified".to_string();
                    }
                    Ok(false) => {
                        reason = "signature_invalid".to_string();
                    }
                    Err(error) => {
                        reason = format!("verification_error: {error}");
                    }
                }
            }
        }
        Err(error) => {
            reason = format!("decode_error: {error}");
        }
    }

    annotate_lxmf_metadata(record, |lxmf| {
        lxmf.insert("signature_checked".to_string(), Value::Bool(checked));
        lxmf.insert("signature_valid".to_string(), Value::Bool(valid));
        lxmf.insert("signature_status".to_string(), Value::String(reason));
    });
}

fn annotate_direct_delivery_transport_metadata(record: &mut MessageRecord, method: u8) {
    annotate_lxmf_metadata(record, |lxmf| {
        lxmf.insert("method".to_string(), Value::from(method));
        lxmf.insert("transport_encrypted".to_string(), Value::Bool(true));
        lxmf.insert("transport_encryption".to_string(), Value::String("Curve25519".to_string()));
    });
}

fn annotate_lxmf_metadata(
    record: &mut MessageRecord,
    update: impl FnOnce(&mut Map<String, Value>),
) {
    let mut root = match record.fields.take() {
        Some(Value::Object(map)) => map,
        Some(other) => {
            let mut map = Map::new();
            map.insert("_fields_raw".to_string(), other);
            map
        }
        None => Map::new(),
    };
    let mut lxmf = match root.remove("_lxmf") {
        Some(Value::Object(map)) => map,
        _ => Map::new(),
    };
    update(&mut lxmf);
    root.insert("_lxmf".to_string(), Value::Object(lxmf));
    record.fields = Some(Value::Object(root));
}
