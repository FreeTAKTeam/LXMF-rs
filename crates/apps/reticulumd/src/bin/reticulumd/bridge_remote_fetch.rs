use super::*;
use lxmf::inbound_decode::InboundPayloadMode;
use reticulum_daemon::inbound_delivery::{
    annotate_inbound_record_stamp_status, decode_inbound_payload, evaluate_inbound_stamp_policy,
    inbound_record_allowed_by_delivery_policy,
};
use rns_transport::identity::DecryptIdentity;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum LocalPropagationImportOutcome {
    Imported,
    Skipped,
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

impl TransportBridge {
    pub(super) fn accept_local_propagated_payload(
        &self,
        daemon: Arc<RpcDaemon>,
        transient_payload: Vec<u8>,
    ) -> Result<LocalPropagationImportOutcome, std::io::Error> {
        let destination = self.announce_destination.clone();
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
) -> Result<LocalPropagationImportOutcome, std::io::Error> {
    if transient_payload.len() <= 16 + 32 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "propagated LXMF payload too short",
        ));
    }

    let (destination_hash, wire) = {
        let destination = delivery_destination.lock().await;
        if &transient_payload[..16] != destination.desc.address_hash.as_slice() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "propagated LXMF payload is not addressed to the local delivery destination",
            ));
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
            "failed to decode fetched propagated LXMF payload",
        ));
    };

    annotate_inbound_record_stamp_status(&mut record, stamp_status);
    if !inbound_record_allowed_by_delivery_policy(daemon, &record) {
        return Ok(LocalPropagationImportOutcome::Skipped);
    }
    if daemon.message_exists(record.id.as_str())? {
        return Ok(LocalPropagationImportOutcome::Skipped);
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
