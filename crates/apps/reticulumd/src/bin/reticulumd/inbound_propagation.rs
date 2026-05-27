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

pub(super) async fn ingest_propagation_envelope(
    daemon: &RpcDaemon,
    payload: &[u8],
    delivery_destination: Option<&Arc<tokio::sync::Mutex<SingleInputDestination>>>,
) -> Result<usize, std::io::Error> {
    let (_timestamp, messages): (f64, Vec<Vec<u8>>) =
        rmp_serde::from_slice(payload).map_err(|err| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("invalid propagation envelope: {err}"),
            )
        })?;
    let accepted_stamp_cost = daemon.propagation_min_accepted_stamp_cost();
    for message in messages.iter() {
        let transient_id =
            daemon.canonical_propagation_payload_bytes_at_cost(message, accepted_stamp_cost)?;
        if try_accept_local_propagated_message(daemon, delivery_destination, message).await? {
            daemon.note_client_propagation_messages_received(1);
            continue;
        }
        daemon.ingest_propagation_payload_bytes_at_cost(
            message,
            Some(transient_id.as_str()),
            accepted_stamp_cost,
        )?;
    }
    Ok(messages.len())
}

async fn try_accept_local_propagated_message(
    daemon: &RpcDaemon,
    delivery_destination: Option<&Arc<tokio::sync::Mutex<SingleInputDestination>>>,
    transient_payload: &[u8],
) -> Result<bool, std::io::Error> {
    let Some(delivery_destination) = delivery_destination else {
        return Ok(false);
    };
    if transient_payload.len() <= 16 + 32 {
        return Ok(false);
    }

    let (destination_hash, wire) = {
        let destination = delivery_destination.lock().await;
        if &transient_payload[..16] != destination.desc.address_hash.as_slice() {
            return Ok(false);
        }
        let wire = decrypt_local_propagated_wire(&destination, transient_payload)?;
        let mut destination_hash = [0u8; 16];
        destination_hash.copy_from_slice(destination.desc.address_hash.as_slice());
        (destination_hash, wire)
    };

    let stamp_status = evaluate_inbound_stamp_policy(
        daemon,
        destination_hash,
        &wire,
        InboundPayloadMode::FullWire,
    )
    .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;

    let Some(mut record) =
        decode_inbound_payload(destination_hash, &wire, InboundPayloadMode::FullWire)
    else {
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
    );
    if !inbound_record_allowed_by_delivery_policy(daemon, &record) {
        return Ok(true);
    }
    if daemon.message_exists(record.id.as_str())? {
        return Ok(true);
    }
    daemon.record_inbound_peer_activity(&record.source, wire.len());
    daemon.accept_inbound_with_raw(record, &wire)?;
    Ok(true)
}

fn annotate_inbound_record_propagation_stamp_status(
    record: &mut rns_rpc::MessageRecord,
    transient_payload: &[u8],
    target_cost: u32,
) {
    if target_cost == 0 {
        return;
    }
    let Some(value) = validate_propagation_stamp(transient_payload, target_cost) else {
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
        JsonValue::Number(serde_json::Number::from(target_cost)),
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
