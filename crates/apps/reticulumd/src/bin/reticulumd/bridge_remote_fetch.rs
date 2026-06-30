use super::*;
use crate::inbound_worker::delivery_events::{
    annotate_inbound_signature_status, emit_inbound_drop_event,
    emit_propagation_predecode_drop_event, InboundDeliveryKind, InboundDropEvent,
};
use lxmf::inbound_decode::InboundPayloadMode;
use reticulum_daemon::inbound_delivery::{
    annotate_inbound_record_stamp_status, decode_inbound_payload_with_diagnostics,
    evaluate_inbound_stamp_policy, inbound_record_allowed_by_delivery_policy,
};
use reticulum_daemon::lxmf_stamps::{validate_propagation_stamp, PROPAGATION_STAMP_SIZE};
use rns_transport::identity::DecryptIdentity;
use sha2::{Digest, Sha256};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum LocalPropagationImportOutcome {
    Imported,
    Duplicate,
    Rejected,
}

pub(super) fn rmpv_binary_array(value: &rmpv::Value) -> Result<Vec<Vec<u8>>, std::io::Error> {
    let rmpv::Value::Array(values) = value else {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "propagation node returned non-array payload",
        ));
    };
    values
        .iter()
        .map(|value| match value {
            rmpv::Value::Binary(bytes) => Ok(bytes.clone()),
            _ => Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "propagation node returned non-binary item",
            )),
        })
        .collect()
}

pub(super) fn propagation_payload_ack_transient_id(transient_payload: &[u8]) -> Vec<u8> {
    if validate_propagation_stamp(transient_payload, 1).is_some() {
        let lxm_data_len = transient_payload.len().saturating_sub(PROPAGATION_STAMP_SIZE);
        return Sha256::digest(&transient_payload[..lxm_data_len]).to_vec();
    }
    Sha256::digest(transient_payload).to_vec()
}

impl TransportBridge {
    pub(super) fn accept_local_propagated_payload(
        &self,
        daemon: Arc<RpcDaemon>,
        transient_payload: Vec<u8>,
    ) -> Result<LocalPropagationImportOutcome, std::io::Error> {
        let destination = self.announce_destination.clone();
        let transport = self.transport.clone();
        std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|err| {
                    std::io::Error::other(format!(
                        "failed to build propagation import runtime: {err}"
                    ))
                })?;
            runtime.block_on(async move {
                accept_local_propagated_payload_inner(
                    daemon.as_ref(),
                    destination,
                    transient_payload.as_slice(),
                    Some(transport.as_ref()),
                )
                .await
            })
        })
        .join()
        .map_err(|_| std::io::Error::other("propagation import helper thread panicked"))?
    }
}

async fn accept_local_propagated_payload_inner(
    daemon: &RpcDaemon,
    delivery_destination: Arc<tokio::sync::Mutex<SingleInputDestination>>,
    transient_payload: &[u8],
    signature_transport: Option<&Transport>,
) -> Result<LocalPropagationImportOutcome, std::io::Error> {
    let (destination_hash, wire) = {
        let destination = delivery_destination.lock().await;
        let mut destination_hash = [0u8; 16];
        destination_hash.copy_from_slice(destination.desc.address_hash.as_slice());
        if transient_payload.len() <= 16 + 32 {
            emit_propagation_predecode_drop_event(
                daemon,
                destination_hash,
                transient_payload,
                "payload_too_short",
                "propagated LXMF payload too short",
            );
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "propagated LXMF payload too short",
            ));
        }
        if &transient_payload[..16] != destination.desc.address_hash.as_slice() {
            emit_propagation_predecode_drop_event(
                daemon,
                destination_hash,
                transient_payload,
                "destination_mismatch",
                "propagated LXMF payload is not addressed to the local delivery destination",
            );
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "propagated LXMF payload is not addressed to the local delivery destination",
            ));
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
    let (record, diagnostics) = decode_inbound_payload_with_diagnostics(
        destination_hash,
        &wire,
        InboundPayloadMode::FullWire,
    );
    let Some(mut record) = record else {
        emit_inbound_drop_event(
            daemon,
            InboundDropEvent {
                reason: "decode_failed",
                delivery_kind: InboundDeliveryKind::Propagation,
                raw_destination_hex: raw_destination_hex.as_str(),
                destination: destination_hash,
                payload_mode: InboundPayloadMode::FullWire,
                bytes_len: wire.len(),
                detail: Some(diagnostics.summary()),
                record: None,
            },
        );
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "failed to decode fetched propagated LXMF payload",
        ));
    };

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
                    record: Some(&record),
                },
            );
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, error));
        }
    };

    annotate_inbound_record_stamp_status(&mut record, stamp_status);
    annotate_inbound_signature_status(
        signature_transport,
        &mut record,
        destination_hash,
        &wire,
        InboundPayloadMode::FullWire,
    )
    .await;
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
        return Ok(LocalPropagationImportOutcome::Rejected);
    }
    if daemon.message_exists(record.id.as_str())? {
        return Ok(LocalPropagationImportOutcome::Duplicate);
    }
    daemon.record_inbound_peer_activity(&record.source, wire.len());
    daemon.accept_inbound_with_raw(record, &wire)?;
    Ok(LocalPropagationImportOutcome::Imported)
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
        let public_key = x25519_dalek::PublicKey::from(ephemeral_key);
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

#[cfg(test)]
mod tests {
    include!("bridge_remote_fetch_tests.rs");
}
