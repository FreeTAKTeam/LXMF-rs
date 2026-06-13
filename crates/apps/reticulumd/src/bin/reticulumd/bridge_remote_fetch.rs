use super::*;
use lxmf::inbound_decode::InboundPayloadMode;
use reticulum_daemon::inbound_delivery::{
    annotate_inbound_record_stamp_status, decode_inbound_payload, evaluate_inbound_stamp_policy,
    inbound_record_allowed_by_delivery_policy,
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
    use super::*;
    use lxmf::WireMessage;
    use rand_core::OsRng;
    use reticulum_daemon::lxmf_bridge::build_wire_message_with_options;
    use reticulum_daemon::lxmf_stamps::generate_propagation_stamp;
    use rns_transport::destination::DestinationName;
    use rns_transport::identity::PrivateIdentity;
    use rns_transport::identity_bridge::{to_core_identity, to_core_private_identity};
    use tokio::sync::Mutex as TokioMutex;

    #[tokio::test]
    async fn policy_rejected_fetched_payload_is_reported_separately_from_duplicate() {
        let daemon = RpcDaemon::test_instance();
        let delivery_private = PrivateIdentity::new_from_rand(OsRng);
        let source_private = PrivateIdentity::new_from_rand(OsRng);
        let delivery_destination = Arc::new(TokioMutex::new(SingleInputDestination::new(
            delivery_private.clone(),
            DestinationName::new("lxmf", "delivery"),
        )));
        let source_destination = SingleInputDestination::new(
            source_private.clone(),
            DestinationName::new("lxmf", "delivery"),
        );
        let mut destination_hash = [0u8; 16];
        {
            let destination = delivery_destination.lock().await;
            destination_hash.copy_from_slice(destination.desc.address_hash.as_slice());
        }
        let mut source_hash = [0u8; 16];
        source_hash.copy_from_slice(source_destination.desc.address_hash.as_slice());
        daemon
            .handle_rpc(RpcRequest {
                id: 80,
                method: "set_delivery_policy".to_string(),
                params: Some(json!({
                    "ignored_destinations": [hex::encode(source_hash)],
                })),
            })
            .expect("set delivery policy");

        let wire = build_wire_message_with_options(
            source_hash,
            destination_hash,
            "ignored fetch title",
            "ignored fetch content",
            None,
            &to_core_private_identity(&source_private),
            None,
            None,
            None,
        )
        .expect("wire");
        let transient_payload = {
            let destination = delivery_destination.lock().await;
            let message = WireMessage::unpack(&wire).expect("wire unpack");
            message
                .pack_propagation_transient_with_rng(
                    &to_core_identity(destination.identity.as_identity()),
                    OsRng,
                )
                .expect("propagation transient")
                .0
        };

        let outcome = accept_local_propagated_payload_inner(
            &daemon,
            delivery_destination,
            &transient_payload,
        )
        .await
        .expect("accept fetched payload");

        assert_eq!(
            outcome,
            LocalPropagationImportOutcome::Rejected,
            "policy-rejected fetched payloads should not be counted as duplicates"
        );
    }

    #[tokio::test]
    async fn duplicate_fetched_payload_is_reported_separately_from_rejection() {
        let daemon = RpcDaemon::test_instance();
        let delivery_private = PrivateIdentity::new_from_rand(OsRng);
        let source_private = PrivateIdentity::new_from_rand(OsRng);
        let delivery_destination = Arc::new(TokioMutex::new(SingleInputDestination::new(
            delivery_private.clone(),
            DestinationName::new("lxmf", "delivery"),
        )));
        let source_destination = SingleInputDestination::new(
            source_private.clone(),
            DestinationName::new("lxmf", "delivery"),
        );
        let mut destination_hash = [0u8; 16];
        {
            let destination = delivery_destination.lock().await;
            destination_hash.copy_from_slice(destination.desc.address_hash.as_slice());
        }
        let mut source_hash = [0u8; 16];
        source_hash.copy_from_slice(source_destination.desc.address_hash.as_slice());

        let wire = build_wire_message_with_options(
            source_hash,
            destination_hash,
            "duplicate fetch title",
            "duplicate fetch content",
            None,
            &to_core_private_identity(&source_private),
            None,
            None,
            None,
        )
        .expect("wire");
        let transient_payload = {
            let destination = delivery_destination.lock().await;
            let message = WireMessage::unpack(&wire).expect("wire unpack");
            message
                .pack_propagation_transient_with_rng(
                    &to_core_identity(destination.identity.as_identity()),
                    OsRng,
                )
                .expect("propagation transient")
                .0
        };

        let first = accept_local_propagated_payload_inner(
            &daemon,
            delivery_destination.clone(),
            &transient_payload,
        )
        .await
        .expect("first fetch accept");
        let second = accept_local_propagated_payload_inner(
            &daemon,
            delivery_destination,
            &transient_payload,
        )
        .await
        .expect("second fetch accept");

        assert_eq!(first, LocalPropagationImportOutcome::Imported);
        assert_eq!(second, LocalPropagationImportOutcome::Duplicate);
    }

    #[test]
    fn ack_transient_id_uses_lxm_data_for_stamped_payloads() {
        let lxm_data = vec![0x42; 160];
        let transient_id = Sha256::digest(&lxm_data);
        let stamp = generate_propagation_stamp(
            transient_id.as_slice().try_into().expect("transient id width"),
            1,
        )
        .expect("propagation stamp");
        let mut transient_payload = lxm_data.clone();
        transient_payload.extend_from_slice(stamp.as_slice());

        let ack_id = propagation_payload_ack_transient_id(transient_payload.as_slice());

        assert_eq!(ack_id, transient_id.to_vec());
        assert_ne!(ack_id, Sha256::digest(transient_payload).to_vec());
    }

    #[test]
    fn ack_transient_id_keeps_unstamped_payload_hash() {
        let transient_payload = b"ack-unstamped-lxm-data".to_vec();

        let ack_id = propagation_payload_ack_transient_id(transient_payload.as_slice());

        assert_eq!(ack_id, Sha256::digest(transient_payload).to_vec());
    }
}
