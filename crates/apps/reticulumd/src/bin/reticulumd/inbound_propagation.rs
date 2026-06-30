use super::delivery_events::{
    annotate_inbound_signature_status, emit_inbound_drop_event,
    emit_propagation_duplicate_drop_event, emit_propagation_predecode_drop_event,
    InboundDeliveryKind, InboundDropEvent,
};
use super::*;
use lxmf::inbound_decode::InboundPayloadMode;
use reticulum_daemon::inbound_delivery::{
    annotate_inbound_record_stamp_status, decode_inbound_payload, evaluate_inbound_stamp_policy,
    inbound_record_allowed_by_delivery_policy,
};
use reticulum_daemon::lxmf_stamps::validate_propagation_stamp;
use serde_json::{Map as JsonMap, Value as JsonValue};
use x25519_dalek::PublicKey;

pub(super) fn is_lxmf_propagation_destination(
    destination: &AddressHash,
    control: &PropagationControlContext,
) -> bool {
    let destination_hex = hex::encode(destination.as_slice());
    control.propagation_destination_hash_hex.as_deref() == Some(destination_hex.as_str())
}

#[cfg(test)]
pub(super) async fn ingest_propagation_envelope(
    daemon: &RpcDaemon,
    payload: &[u8],
    delivery_destination: Option<&Arc<tokio::sync::Mutex<SingleInputDestination>>>,
) -> Result<usize, std::io::Error> {
    ingest_propagation_envelope_with_transport(daemon, payload, delivery_destination, None).await
}

pub(super) async fn ingest_propagation_envelope_with_transport(
    daemon: &RpcDaemon,
    payload: &[u8],
    delivery_destination: Option<&Arc<tokio::sync::Mutex<SingleInputDestination>>>,
    signature_transport: Option<&Transport>,
) -> Result<usize, std::io::Error> {
    ingest_propagation_envelope_from_peer_with_transport(
        daemon,
        payload,
        delivery_destination,
        None,
        signature_transport,
    )
    .await
}

#[cfg(test)]
pub(super) async fn ingest_propagation_resource_from_peer(
    daemon: &RpcDaemon,
    payload: &[u8],
    delivery_destination: Option<&Arc<tokio::sync::Mutex<SingleInputDestination>>>,
    remote_propagation_peer: Option<&str>,
    peer_link_validated: bool,
) -> Result<usize, std::io::Error> {
    ingest_propagation_resource_from_peer_with_transport(
        daemon,
        payload,
        delivery_destination,
        remote_propagation_peer,
        peer_link_validated,
        None,
    )
    .await
}

pub(super) async fn ingest_propagation_resource_from_peer_with_transport(
    daemon: &RpcDaemon,
    payload: &[u8],
    delivery_destination: Option<&Arc<tokio::sync::Mutex<SingleInputDestination>>>,
    remote_propagation_peer: Option<&str>,
    peer_link_validated: bool,
    signature_transport: Option<&Transport>,
) -> Result<usize, std::io::Error> {
    if !peer_link_validated && propagation_envelope_message_count(payload)? > 1 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "received multiple propagation messages without valid peering key",
        ));
    }
    ingest_propagation_envelope_from_peer_with_transport(
        daemon,
        payload,
        delivery_destination,
        remote_propagation_peer,
        signature_transport,
    )
    .await
}

async fn ingest_propagation_envelope_from_peer_with_transport(
    daemon: &RpcDaemon,
    payload: &[u8],
    delivery_destination: Option<&Arc<tokio::sync::Mutex<SingleInputDestination>>>,
    remote_propagation_peer: Option<&str>,
    signature_transport: Option<&Transport>,
) -> Result<usize, std::io::Error> {
    ingest_propagation_envelope_from_peer_inner(
        daemon,
        payload,
        delivery_destination,
        remote_propagation_peer,
        signature_transport,
    )
    .await
}

#[cfg(test)]
pub(super) async fn ingest_propagation_envelope_from_peer(
    daemon: &RpcDaemon,
    payload: &[u8],
    delivery_destination: Option<&Arc<tokio::sync::Mutex<SingleInputDestination>>>,
    remote_propagation_peer: Option<&str>,
) -> Result<usize, std::io::Error> {
    ingest_propagation_envelope_from_peer_inner(
        daemon,
        payload,
        delivery_destination,
        remote_propagation_peer,
        None,
    )
    .await
}

async fn ingest_propagation_envelope_from_peer_inner(
    daemon: &RpcDaemon,
    payload: &[u8],
    delivery_destination: Option<&Arc<tokio::sync::Mutex<SingleInputDestination>>>,
    remote_propagation_peer: Option<&str>,
    signature_transport: Option<&Transport>,
) -> Result<usize, std::io::Error> {
    let (_timestamp, messages): (f64, Vec<Vec<u8>>) =
        rmp_serde::from_slice(payload).map_err(|err| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("invalid propagation envelope: {err}"),
            )
        })?;
    let accepted_stamp_cost = daemon.propagation_min_accepted_stamp_cost();
    let mut invalid_stamp_error = None;
    for message in messages.iter() {
        let transient_id = match daemon
            .canonical_propagation_payload_bytes_at_cost(message, accepted_stamp_cost)
        {
            Ok(transient_id) => transient_id,
            Err(error) => {
                if let Some(peer) = remote_propagation_peer {
                    daemon.throttle_propagation_peer_for_invalid_stamp(peer);
                }
                if invalid_stamp_error.is_none() {
                    invalid_stamp_error = Some(error);
                }
                continue;
            }
        };
        match try_accept_local_propagated_message(
            daemon,
            delivery_destination,
            message,
            transient_id.as_str(),
            remote_propagation_peer,
            signature_transport,
        )
        .await?
        {
            LocalPropagationOutcome::Accepted { counted } => {
                if let Some(peer) = remote_propagation_peer {
                    daemon.relay_accepted_peer_propagation_payload_bytes_at_cost(
                        message,
                        Some(transient_id.as_str()),
                        accepted_stamp_cost,
                        peer,
                    )?;
                } else {
                    daemon.note_client_propagation_messages_received(usize::from(counted));
                }
                continue;
            }
            LocalPropagationOutcome::Dropped => continue,
            LocalPropagationOutcome::NotLocal => {}
        }
        if let Some(peer) = remote_propagation_peer {
            daemon.ingest_peer_propagation_payload_bytes_at_cost(
                message,
                Some(transient_id.as_str()),
                accepted_stamp_cost,
                peer,
            )?;
        } else {
            daemon.ingest_propagation_payload_bytes_at_cost(
                message,
                Some(transient_id.as_str()),
                accepted_stamp_cost,
            )?;
        }
    }
    if let Some(error) = invalid_stamp_error {
        return Err(error);
    }
    Ok(messages.len())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum LocalPropagationOutcome {
    Accepted { counted: bool },
    Dropped,
    NotLocal,
}

fn propagation_envelope_message_count(payload: &[u8]) -> Result<usize, std::io::Error> {
    let (_timestamp, messages): (f64, Vec<Vec<u8>>) =
        rmp_serde::from_slice(payload).map_err(|err| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("invalid propagation envelope: {err}"),
            )
        })?;
    Ok(messages.len())
}

async fn try_accept_local_propagated_message(
    daemon: &RpcDaemon,
    delivery_destination: Option<&Arc<tokio::sync::Mutex<SingleInputDestination>>>,
    transient_payload: &[u8],
    transient_id: &str,
    remote_propagation_peer: Option<&str>,
    signature_transport: Option<&Transport>,
) -> Result<LocalPropagationOutcome, std::io::Error> {
    let Some(delivery_destination) = delivery_destination else {
        return Ok(LocalPropagationOutcome::NotLocal);
    };
    let (destination_hash, wire) = {
        let destination = delivery_destination.lock().await;
        let mut destination_hash = [0u8; 16];
        destination_hash.copy_from_slice(destination.desc.address_hash.as_slice());
        if transient_payload.len() <= 16 + 32 {
            if transient_payload.len() >= 16
                && &transient_payload[..16] == destination.desc.address_hash.as_slice()
            {
                emit_propagation_predecode_drop_event(
                    daemon,
                    destination_hash,
                    transient_payload,
                    "payload_too_short",
                    "propagated LXMF payload too short",
                );
                return Ok(LocalPropagationOutcome::Dropped);
            }
            return Ok(LocalPropagationOutcome::NotLocal);
        }
        if &transient_payload[..16] != destination.desc.address_hash.as_slice() {
            return Ok(LocalPropagationOutcome::NotLocal);
        }
        if daemon.local_propagation_processed_mark_exists(transient_id)? {
            emit_propagation_duplicate_drop_event(
                daemon,
                destination_hash,
                transient_payload,
                transient_id,
                "transient already processed locally",
            );
            return Ok(LocalPropagationOutcome::Accepted { counted: false });
        }
        let wire = match decrypt_local_propagated_wire(&destination, transient_payload) {
            Ok(wire) => wire,
            Err(error) => {
                emit_propagation_predecode_drop_event(
                    daemon,
                    destination_hash,
                    transient_payload,
                    "decrypt_failed",
                    error.to_string(),
                );
                return Err(error);
            }
        };
        (destination_hash, wire)
    };
    let raw_destination_hex = hex::encode(destination_hash);

    let stamp_status = match evaluate_inbound_stamp_policy(
        daemon,
        destination_hash,
        &wire,
        InboundPayloadMode::FullWire,
    ) {
        Ok(status) => status,
        Err(error) => {
            emit_inbound_drop_event(
                daemon,
                InboundDropEvent {
                    reason: "stamp_policy_rejected",
                    delivery_kind: InboundDeliveryKind::Propagation,
                    raw_destination_hex: raw_destination_hex.as_str(),
                    destination: destination_hash,
                    payload_mode: InboundPayloadMode::FullWire,
                    bytes_len: wire.len(),
                    detail: Some(error.to_string()),
                    record: None,
                },
            );
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, error));
        }
    };

    let Some(mut record) =
        decode_inbound_payload(destination_hash, &wire, InboundPayloadMode::FullWire)
    else {
        emit_inbound_drop_event(
            daemon,
            InboundDropEvent {
                reason: "decode_failed",
                delivery_kind: InboundDeliveryKind::Propagation,
                raw_destination_hex: raw_destination_hex.as_str(),
                destination: destination_hash,
                payload_mode: InboundPayloadMode::FullWire,
                bytes_len: wire.len(),
                detail: Some("failed to decode locally delivered propagated LXMF payload".into()),
                record: None,
            },
        );
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "failed to decode locally delivered propagated LXMF payload",
        ));
    };

    annotate_inbound_record_stamp_status(&mut record, stamp_status);
    annotate_inbound_record_propagation_stamp_status(
        &mut record,
        transient_payload,
        daemon.propagation_target_cost(),
        daemon.propagation_min_accepted_stamp_cost(),
    );
    annotate_inbound_signature_status(
        signature_transport,
        &mut record,
        destination_hash,
        &wire,
        InboundPayloadMode::FullWire,
    )
    .await;
    if let Some(peer) = remote_propagation_peer {
        let propagation_bytes = if daemon.propagation_target_cost() > 0 {
            transient_payload.len().saturating_sub(32)
        } else {
            transient_payload.len()
        };
        if !daemon.record_inbound_propagation_peer_activity(peer, propagation_bytes) {
            daemon.record_unpeered_propagation_attempt(propagation_bytes);
        }
    }
    if !inbound_record_allowed_by_delivery_policy(daemon, &record) {
        emit_inbound_drop_event(
            daemon,
            InboundDropEvent {
                reason: "delivery_policy_rejected",
                delivery_kind: InboundDeliveryKind::Propagation,
                raw_destination_hex: raw_destination_hex.as_str(),
                destination: destination_hash,
                payload_mode: InboundPayloadMode::FullWire,
                bytes_len: wire.len(),
                detail: None,
                record: Some(&record),
            },
        );
        daemon.mark_local_propagation_processed(transient_id)?;
        return Ok(LocalPropagationOutcome::Accepted { counted: true });
    }
    if daemon.message_exists(record.id.as_str())? {
        daemon.mark_local_propagation_processed(transient_id)?;
        emit_propagation_duplicate_drop_event(
            daemon,
            destination_hash,
            transient_payload,
            transient_id,
            "message already exists locally",
        );
        return Ok(LocalPropagationOutcome::Accepted { counted: false });
    }
    if remote_propagation_peer.is_none() {
        daemon.record_inbound_peer_activity(&record.source, wire.len());
    }
    daemon.accept_inbound_with_raw(record, &wire)?;
    daemon.mark_local_propagation_processed(transient_id)?;
    Ok(LocalPropagationOutcome::Accepted { counted: true })
}

fn annotate_inbound_record_propagation_stamp_status(
    record: &mut rns_rpc::MessageRecord,
    transient_payload: &[u8],
    target_cost: u32,
    accepted_cost: u32,
) {
    if target_cost == 0 {
        return;
    }
    let validation_cost = if accepted_cost == 0 { target_cost } else { accepted_cost };
    let Some(value) = validate_propagation_stamp(transient_payload, validation_cost) else {
        return;
    };

    let mut root = match record.fields.take() {
        Some(JsonValue::Object(map)) => map,
        Some(other) => {
            let mut map = JsonMap::new();
            map.insert("_fields_raw".into(), other);
            map
        }
        None => JsonMap::new(),
    };
    let mut lxmf = match root.remove("_lxmf") {
        Some(JsonValue::Object(map)) => map,
        _ => JsonMap::new(),
    };
    lxmf.insert("propagation_stamp_checked".into(), JsonValue::Bool(true));
    lxmf.insert("propagation_stamp_valid".into(), JsonValue::Bool(true));
    lxmf.insert(
        "propagation_stamp_target_cost".into(),
        JsonValue::Number(serde_json::Number::from(validation_cost)),
    );
    lxmf.insert(
        "propagation_stamp_value".into(),
        JsonValue::Number(serde_json::Number::from(value)),
    );
    root.insert("_lxmf".into(), JsonValue::Object(lxmf));
    record.fields = Some(JsonValue::Object(root));
}

fn decrypt_local_propagated_wire(
    destination: &SingleInputDestination,
    transient_payload: &[u8],
) -> Result<Vec<u8>, std::io::Error> {
    if transient_payload.len() <= 16 + 32 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "propagated LXMF payload too short",
        ));
    }

    for strip_stamp in [false, true] {
        let payload = if strip_stamp {
            if transient_payload.len() <= 16 + 32 + 32 {
                continue;
            }
            &transient_payload[..transient_payload.len() - 32]
        } else {
            transient_payload
        };

        let ciphertext = &payload[16..];
        if ciphertext.len() <= 32 {
            continue;
        }
        let Ok(ephemeral_key) = <[u8; 32]>::try_from(&ciphertext[..32]) else {
            continue;
        };
        let public_key = PublicKey::from(ephemeral_key);
        let derived_key = destination
            .identity
            .derive_key(&public_key, Some(destination.identity.address_hash().as_slice()));
        let token = &ciphertext[32..];
        let mut plaintext = vec![0u8; token.len()];
        let Ok(decrypted) =
            destination.identity.decrypt(rand_core::OsRng, token, &derived_key, &mut plaintext)
        else {
            continue;
        };

        let mut wire = Vec::with_capacity(16 + decrypted.len());
        wire.extend_from_slice(destination.desc.address_hash.as_slice());
        wire.extend_from_slice(decrypted);
        return Ok(wire);
    }

    Err(std::io::Error::new(
        std::io::ErrorKind::InvalidData,
        "failed to decrypt propagated LXMF payload for local delivery",
    ))
}
