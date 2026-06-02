use crate::bridge_helpers::{diagnostics_enabled, payload_preview};
use lxmf::{inbound_decode::InboundPayloadMode, WireMessage};
use reticulum_daemon::inbound_delivery::{
    annotate_inbound_record_stamp_status, decode_inbound_payload,
    decode_inbound_payload_with_diagnostics, evaluate_inbound_stamp_policy,
    inbound_record_allowed_by_delivery_policy,
};
use rns_rpc::{MessageRecord, RpcDaemon};
use rns_transport::hash::AddressHash;
use rns_transport::identity_bridge::to_core_identity;
use rns_transport::transport::{ReceivedPayloadMode, Transport};
use serde_json::{Map, Value};
use std::borrow::Cow;

pub(super) async fn accept_delivery_resource(
    daemon: &RpcDaemon,
    transport: &Transport,
    destination: [u8; 16],
    data: &[u8],
) {
    let stamp_status = match evaluate_inbound_stamp_policy(
        daemon,
        destination,
        data,
        InboundPayloadMode::FullWire,
    ) {
        Ok(status) => status,
        Err(error) => {
            if diagnostics_enabled() {
                log::warn!("[daemon-rx] dropping inbound resource due to stamp policy: {}", error);
            }
            return;
        }
    };
    if let Some(mut record) =
        decode_inbound_payload(destination, data, InboundPayloadMode::FullWire)
    {
        annotate_inbound_record_stamp_status(&mut record, stamp_status);
        annotate_inbound_signature_status(
            transport,
            &mut record,
            destination,
            data,
            InboundPayloadMode::FullWire,
        )
        .await;
        annotate_direct_delivery_transport_metadata(&mut record, 2);
        if !inbound_record_allowed_by_delivery_policy(daemon, &record) {
            return;
        }
        let _ = daemon.accept_inbound_with_raw(record, data);
    }
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
    let record = if diagnostics_enabled() {
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
            log::debug!(
                "[daemon-rx] decode-failed raw_dst={} resolved_dst={} attempts={}",
                raw_destination_hex,
                hex::encode(destination),
                diagnostics.summary()
            );
        }
        record
    } else {
        decode_inbound_payload(destination, data, payload_mode)
    };
    let stamp_status = if record.is_some() {
        match evaluate_inbound_stamp_policy(daemon, destination, data, payload_mode) {
            Ok(status) => Some(status),
            Err(_) => {
                if diagnostics_enabled() {
                    log::warn!(
                        "[daemon-rx] dropping inbound payload due to stamp policy: raw_dst={} resolved_dst={}",
                        raw_destination_hex,
                        hex::encode(destination)
                    );
                }
                return;
            }
        }
    } else {
        None
    };
    if let Some(mut record) = record {
        if let Some(stamp_status) = stamp_status {
            annotate_inbound_record_stamp_status(&mut record, stamp_status);
        }
        annotate_inbound_signature_status(transport, &mut record, destination, data, payload_mode)
            .await;
        let method = match payload_mode {
            InboundPayloadMode::DestinationStripped => 1,
            InboundPayloadMode::FullWire => 2,
        };
        annotate_direct_delivery_transport_metadata(&mut record, method);
        if !inbound_record_allowed_by_delivery_policy(daemon, &record) {
            return;
        }
        if matches!(daemon.message_exists(record.id.as_str()), Ok(true)) {
            return;
        }
        daemon.record_inbound_peer_activity(&record.source, data.len());
        let _ = daemon.accept_inbound_with_raw(record, data);
    }
}

pub(super) fn log_resolved_packet(
    raw_destination_hex: &str,
    resolved_destination: impl std::fmt::Debug,
    payload_mode: ReceivedPayloadMode,
    ratchet_used: bool,
    data: &[u8],
) {
    if diagnostics_enabled() {
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
}

fn inbound_payload_mode(mode: ReceivedPayloadMode) -> InboundPayloadMode {
    match mode {
        ReceivedPayloadMode::FullWire => InboundPayloadMode::FullWire,
        ReceivedPayloadMode::DestinationStripped => InboundPayloadMode::DestinationStripped,
    }
}

async fn annotate_inbound_signature_status(
    transport: &Transport,
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
            if let Some(identity) = transport.destination_identity(&source_hash).await {
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
