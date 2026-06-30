use crate::bridge_helpers::payload_preview;
use lxmf::{inbound_decode::InboundPayloadMode, WireMessage};
use reticulum_daemon::inbound_delivery::{
    annotate_inbound_record_stamp_status, decode_inbound_payload_with_diagnostics,
    evaluate_inbound_stamp_policy, inbound_record_allowed_by_delivery_policy,
};
use rns_rpc::{MessageRecord, RpcDaemon, RpcEvent};
use rns_transport::hash::AddressHash;
use rns_transport::identity_bridge::to_core_identity;
use rns_transport::transport::{ReceivedPayloadMode, Transport};
use serde_json::{json, Map, Value};
use std::borrow::Cow;

pub(super) async fn accept_delivery_resource(
    daemon: &RpcDaemon,
    transport: &Transport,
    destination: [u8; 16],
    data: &[u8],
) {
    let raw_destination_hex = hex::encode(destination);
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
    let _ = daemon.accept_inbound_with_raw(record, data);
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
    if matches!(daemon.message_exists(record.id.as_str()), Ok(true)) {
        return;
    }
    daemon.record_inbound_peer_activity(&record.source, data.len());
    let _ = daemon.accept_inbound_with_raw(record, data);
}

pub(crate) struct InboundDropEvent<'a> {
    pub(crate) reason: &'a str,
    pub(crate) delivery_kind: InboundDeliveryKind,
    pub(crate) raw_destination_hex: &'a str,
    pub(crate) destination: [u8; 16],
    pub(crate) payload_mode: InboundPayloadMode,
    pub(crate) bytes_len: usize,
    pub(crate) detail: Option<String>,
    pub(crate) record: Option<&'a MessageRecord>,
}

#[derive(Clone, Copy)]
pub(crate) enum InboundDeliveryKind {
    Packet,
    Propagation,
    Resource,
}

fn inbound_drop_event_payload(event: InboundDropEvent<'_>) -> Value {
    let mut payload = json!({
        "reason": event.reason,
        "delivery_kind": inbound_delivery_kind_name(event.delivery_kind),
        "raw_destination_hash": event.raw_destination_hex,
        "resolved_destination_hash": hex::encode(event.destination),
        "payload_mode": inbound_payload_mode_name(event.payload_mode),
        "bytes_len": event.bytes_len,
    });
    if let Some(detail) = inbound_drop_detail(event.reason, event.detail) {
        payload["detail"] = Value::String(detail);
    }
    if let Some(record) = event.record {
        payload["dropped_message_id"] = Value::String(record.id.clone());
        payload["source_hash"] = Value::String(record.source.clone());
        payload["destination_hash"] = Value::String(record.destination.clone());
    }
    payload
}

pub(crate) fn emit_inbound_drop_event(daemon: &RpcDaemon, event: InboundDropEvent<'_>) {
    let payload = inbound_drop_event_payload(event);
    daemon.publish_event(RpcEvent { event_type: "inbound_dropped".to_string(), payload });
}

pub(crate) fn emit_propagation_predecode_drop_event(
    daemon: &RpcDaemon,
    destination: [u8; 16],
    transient_payload: &[u8],
    reason: &'static str,
    detail: impl Into<String>,
) {
    let raw_destination_hex =
        propagated_transient_raw_destination_hex(destination, transient_payload);
    emit_inbound_drop_event(
        daemon,
        InboundDropEvent {
            reason,
            delivery_kind: InboundDeliveryKind::Propagation,
            raw_destination_hex: raw_destination_hex.as_str(),
            destination,
            payload_mode: InboundPayloadMode::FullWire,
            bytes_len: transient_payload.len(),
            detail: Some(detail.into()),
            record: None,
        },
    );
}

pub(crate) fn emit_propagation_duplicate_drop_event(
    daemon: &RpcDaemon,
    destination: [u8; 16],
    transient_payload: &[u8],
    transient_id: &str,
    detail: &'static str,
) {
    let raw_destination_hex =
        propagated_transient_raw_destination_hex(destination, transient_payload);
    let mut payload = inbound_drop_event_payload(InboundDropEvent {
        reason: "duplicate",
        delivery_kind: InboundDeliveryKind::Propagation,
        raw_destination_hex: raw_destination_hex.as_str(),
        destination,
        payload_mode: InboundPayloadMode::FullWire,
        bytes_len: transient_payload.len(),
        detail: Some(detail.to_string()),
        record: None,
    });
    // Propagation transient IDs are protocol-visible content identifiers used by
    // queue and duplicate-accounting APIs; keep them structured, not in detail.
    payload["transient_id"] = Value::String(transient_id.to_string());
    daemon.publish_event(RpcEvent { event_type: "inbound_dropped".to_string(), payload });
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

fn propagated_transient_raw_destination_hex(
    destination: [u8; 16],
    transient_payload: &[u8],
) -> String {
    if transient_payload.len() >= 16 {
        hex::encode(&transient_payload[..16])
    } else {
        hex::encode(destination)
    }
}

fn inbound_payload_mode_name(mode: InboundPayloadMode) -> &'static str {
    match mode {
        InboundPayloadMode::FullWire => "full_wire",
        InboundPayloadMode::DestinationStripped => "destination_stripped",
    }
}

fn inbound_delivery_kind_name(kind: InboundDeliveryKind) -> &'static str {
    match kind {
        InboundDeliveryKind::Packet => "packet",
        InboundDeliveryKind::Propagation => "propagation",
        InboundDeliveryKind::Resource => "resource",
    }
}

fn inbound_drop_detail(reason: &str, detail: Option<String>) -> Option<String> {
    let detail = detail.filter(|value| !value.is_empty())?;
    if reason == "stamp_policy_rejected" {
        return Some("invalid LXMF stamp".to_string());
    }
    Some(detail)
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
