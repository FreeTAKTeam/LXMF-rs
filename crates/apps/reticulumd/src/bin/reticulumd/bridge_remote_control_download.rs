use super::remote_control::response_code_error;
use super::remote_control_link::{
    build_link_identify_payload, build_link_request_payload, open_refreshed_remote_link,
    resolve_remote_identity, send_link_context_packet, wait_for_link_request_response,
};
use super::remote_fetch::propagation_payload_ack_transient_id;
use super::*;
use lxmf::inbound_decode::InboundPayloadMode;
use reticulum_daemon::inbound_delivery::{
    annotate_inbound_record_stamp_status, decode_inbound_payload, evaluate_inbound_stamp_policy,
    inbound_record_allowed_by_delivery_policy,
};
use rns_transport::identity::DecryptIdentity;
use x25519_dalek::PublicKey;

type RemoteTransientPartition = (Vec<Vec<u8>>, Vec<Vec<u8>>);

pub(super) async fn propagation_download_request(
    transport: &Transport,
    daemon: &RpcDaemon,
    delivery_destination: &Arc<tokio::sync::Mutex<SingleInputDestination>>,
    request_identity: &PrivateIdentity,
    remote: &str,
    timeout: Duration,
    transfer_limit_kb: Option<f64>,
) -> Result<(JsonValue, Identity), std::io::Error> {
    let remote_hash = AddressHash::new(parse_destination_hash_required(remote)?);
    let remote_identity = resolve_remote_identity(transport, &remote_hash, timeout).await?;
    let remote_identity = remote_identity.ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::NotFound, "no path known for propagation node")
    })?;

    let destination =
        SingleOutputDestination::new(remote_identity, DestinationName::new("lxmf", "propagation"));
    let link =
        open_refreshed_remote_link(transport, &remote_hash, destination.desc, timeout).await?;
    let link_id = *link.lock().await.id();

    let identify_payload = build_link_identify_payload(request_identity, &link_id);
    send_link_context_packet(
        transport,
        &link,
        PacketContext::LinkIdentify,
        identify_payload.as_slice(),
    )
    .await?;

    let mut data_rx = transport.received_data_events();
    let mut resource_rx = transport.resource_events();
    let list_payload = build_link_request_payload(
        "/get",
        rmpv::Value::Array(vec![rmpv::Value::Nil, rmpv::Value::Nil]),
    )?;
    let list_request_id =
        send_link_context_packet(transport, &link, PacketContext::Request, list_payload.as_slice())
            .await?
            .ok_or_else(|| std::io::Error::other("missing propagation list request id"))?;
    let list_response = wait_for_link_request_response(
        &mut data_rx,
        &mut resource_rx,
        destination.desc.address_hash,
        link_id,
        list_request_id,
        timeout,
    )
    .await
    .map_err(|err| std::io::Error::new(std::io::ErrorKind::TimedOut, err))?;
    let available = binary_array_response(&list_response)?;
    let (wanted, haves) = classify_remote_transient_ids(daemon, available)?;
    let available_count = wanted.len().saturating_add(haves.len());

    if wanted.is_empty() && haves.is_empty() {
        return Ok((propagation_download_summary_json(0, &[], 0, 0, 0), remote_identity));
    }

    if wanted.is_empty() {
        let ack_response = PropagationDownloadAckWait {
            transport,
            link: &link,
            data_rx: &mut data_rx,
            resource_rx: &mut resource_rx,
            expected_destination: destination.desc.address_hash,
            link_id,
            timeout,
        }
        .send_haves(haves.as_slice())
        .await?;
        return Ok((
            propagation_download_haves_only_summary(available_count, &ack_response)?,
            remote_identity,
        ));
    }

    let get_payload = propagation_download_get_payload(
        Some(wanted.as_slice()),
        haves.as_slice(),
        transfer_limit_kb,
    )?;
    let get_request_id =
        send_link_context_packet(transport, &link, PacketContext::Request, get_payload.as_slice())
            .await?
            .ok_or_else(|| std::io::Error::other("missing propagation get request id"))?;
    let get_response = wait_for_link_request_response(
        &mut data_rx,
        &mut resource_rx,
        destination.desc.address_hash,
        link_id,
        get_request_id,
        timeout,
    )
    .await
    .map_err(|err| std::io::Error::new(std::io::ErrorKind::TimedOut, err))?;
    let payloads = binary_array_response(&get_response)?;

    let mut accepted_haves = haves;
    let mut downloaded = 0usize;
    let mut duplicates = 0usize;
    let mut rejected = 0usize;
    for payload in &payloads {
        let transient_id = propagation_payload_ack_transient_id(payload);
        match accept_downloaded_propagation_payload(daemon, delivery_destination, payload).await? {
            DownloadAcceptOutcome::Stored => {
                downloaded += 1;
                accepted_haves.push(transient_id.to_vec());
            }
            DownloadAcceptOutcome::Duplicate => {
                duplicates += 1;
                accepted_haves.push(transient_id.to_vec());
            }
            DownloadAcceptOutcome::Rejected => rejected += 1,
        }
    }

    if !accepted_haves.is_empty() {
        let ack_response = PropagationDownloadAckWait {
            transport,
            link: &link,
            data_rx: &mut data_rx,
            resource_rx: &mut resource_rx,
            expected_destination: destination.desc.address_hash,
            link_id,
            timeout,
        }
        .send_haves(accepted_haves.as_slice())
        .await?;
        propagation_download_ack_response_result(&ack_response)?;
    }

    Ok((
        propagation_download_summary_json(
            available_count,
            &payloads,
            downloaded,
            duplicates,
            rejected,
        ),
        remote_identity,
    ))
}

struct PropagationDownloadAckWait<'a> {
    transport: &'a Transport,
    link: &'a Arc<tokio::sync::Mutex<Link>>,
    data_rx: &'a mut tokio::sync::broadcast::Receiver<rns_transport::transport::ReceivedData>,
    resource_rx: &'a mut tokio::sync::broadcast::Receiver<rns_transport::resource::ResourceEvent>,
    expected_destination: AddressHash,
    link_id: AddressHash,
    timeout: Duration,
}

impl PropagationDownloadAckWait<'_> {
    async fn send_haves(&mut self, haves: &[Vec<u8>]) -> Result<rmpv::Value, std::io::Error> {
        let ack_payload = propagation_download_get_payload(None, haves, None)?;
        let ack_request_id = send_link_context_packet(
            self.transport,
            self.link,
            PacketContext::Request,
            ack_payload.as_slice(),
        )
        .await?
        .ok_or_else(|| std::io::Error::other("missing propagation haves ack request id"))?;
        wait_for_link_request_response(
            self.data_rx,
            self.resource_rx,
            self.expected_destination,
            self.link_id,
            ack_request_id,
            self.timeout,
        )
        .await
        .map_err(|err| std::io::Error::new(std::io::ErrorKind::TimedOut, err))
    }
}

fn classify_remote_transient_ids(
    daemon: &RpcDaemon,
    remote_ids: Vec<Vec<u8>>,
) -> Result<RemoteTransientPartition, std::io::Error> {
    classify_remote_transient_ids_with(remote_ids, |transient_id| {
        daemon.propagation_transient_exists(hex::encode(transient_id).as_str())
    })
}

fn classify_remote_transient_ids_with<F>(
    remote_ids: Vec<Vec<u8>>,
    mut exists: F,
) -> Result<RemoteTransientPartition, std::io::Error>
where
    F: FnMut(&[u8]) -> Result<bool, std::io::Error>,
{
    let mut wants = Vec::new();
    let mut haves = Vec::new();
    for transient_id in remote_ids {
        if exists(transient_id.as_slice())? {
            haves.push(transient_id);
        } else {
            wants.push(transient_id);
        }
    }
    Ok((wants, haves))
}

fn propagation_download_get_payload(
    wants: Option<&[Vec<u8>]>,
    haves: &[Vec<u8>],
    transfer_limit_kb: Option<f64>,
) -> Result<Vec<u8>, std::io::Error> {
    let wants = wants
        .map(|ids| rmpv::Value::Array(ids.iter().cloned().map(rmpv::Value::Binary).collect()))
        .unwrap_or(rmpv::Value::Nil);
    let haves = rmpv::Value::Array(haves.iter().cloned().map(rmpv::Value::Binary).collect());
    let mut entries = vec![wants, haves];
    if let Some(limit) = transfer_limit_kb {
        entries.push(rmpv::Value::F64(limit));
    } else if entries.first().is_some_and(|value| !value.is_nil()) {
        entries.push(rmpv::Value::F64(1000.0));
    }
    build_link_request_payload("/get", rmpv::Value::Array(entries))
}

#[cfg(test)]
fn decode_link_request_payload(payload: &[u8]) -> rmpv::Value {
    let decoded: rmpv::Value = rmp_serde::from_slice(payload).expect("decode link request");
    let rmpv::Value::Array(entries) = decoded else {
        panic!("request should be an array");
    };
    entries.get(2).cloned().expect("request data")
}

fn propagation_download_summary_json(
    available: usize,
    payloads: &[Vec<u8>],
    downloaded: usize,
    duplicates: usize,
    rejected: usize,
) -> JsonValue {
    let transferred_bytes = payloads.iter().map(Vec::len).sum::<usize>();
    json!({
        "available_count": available,
        "downloaded_count": downloaded,
        "duplicate_count": duplicates,
        "rejected_count": rejected,
        "available": available,
        "downloaded": downloaded,
        "duplicates": duplicates,
        "rejected": rejected,
        "transferred_bytes": transferred_bytes,
    })
}

fn propagation_download_ack_response_result(response: &rmpv::Value) -> Result<(), std::io::Error> {
    if let Some(error) = response_code_error(response) {
        return Err(error);
    }
    Ok(())
}

fn propagation_download_haves_only_summary(
    available_count: usize,
    ack_response: &rmpv::Value,
) -> Result<JsonValue, std::io::Error> {
    propagation_download_ack_response_result(ack_response)?;
    Ok(propagation_download_summary_json(available_count, &[], 0, 0, 0))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DownloadAcceptOutcome {
    Stored,
    Duplicate,
    Rejected,
}

async fn accept_downloaded_propagation_payload(
    daemon: &RpcDaemon,
    delivery_destination: &Arc<tokio::sync::Mutex<SingleInputDestination>>,
    transient_payload: &[u8],
) -> Result<DownloadAcceptOutcome, std::io::Error> {
    let (destination_hash, wire) = {
        let destination = delivery_destination.lock().await;
        if transient_payload.len() <= 16 + 32 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "propagated LXMF payload too short",
            ));
        }
        if &transient_payload[..16] != destination.desc.address_hash.as_slice() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "propagated LXMF payload is not addressed to local delivery destination",
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
            "failed to decode downloaded propagated LXMF payload",
        ));
    };

    annotate_inbound_record_stamp_status(&mut record, stamp_status);
    if !inbound_record_allowed_by_delivery_policy(daemon, &record) {
        return Ok(DownloadAcceptOutcome::Rejected);
    }
    if daemon.message_exists(record.id.as_str())? {
        return Ok(DownloadAcceptOutcome::Duplicate);
    }
    daemon.record_inbound_peer_activity(&record.source, wire.len());
    daemon.accept_inbound_with_raw(record, &wire)?;
    Ok(DownloadAcceptOutcome::Stored)
}

fn decrypt_local_propagated_wire(
    destination: &SingleInputDestination,
    transient_payload: &[u8],
) -> Result<Vec<u8>, std::io::Error> {
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
        "failed to decrypt downloaded propagated LXMF payload",
    ))
}

fn binary_array_response(response: &rmpv::Value) -> Result<Vec<Vec<u8>>, std::io::Error> {
    match response {
        rmpv::Value::Array(entries) => entries
            .iter()
            .map(|entry| match entry {
                rmpv::Value::Binary(bytes) => Ok(bytes.clone()),
                _ => Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "propagation node returned non-binary message entry",
                )),
            })
            .collect(),
        _ => Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "propagation node returned non-list response",
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lxmf::WireMessage;
    use rand_core::OsRng;
    use reticulum_daemon::lxmf_bridge::build_wire_message_with_options;
    use rns_transport::destination::DestinationName;
    use rns_transport::identity::PrivateIdentity;
    use rns_transport::identity_bridge::{to_core_identity, to_core_private_identity};
    use tokio::sync::Mutex as TokioMutex;

    #[test]
    fn propagation_download_summary_reports_transferred_bytes() {
        let payloads = vec![b"downloaded".to_vec(), b"payload-two".to_vec()];

        let summary = propagation_download_summary_json(5, &payloads, 1, 1, 2);

        assert_eq!(summary["available_count"].as_u64(), Some(5));
        assert_eq!(summary["downloaded_count"].as_u64(), Some(1));
        assert_eq!(summary["duplicate_count"].as_u64(), Some(1));
        assert_eq!(summary["rejected_count"].as_u64(), Some(2));
        assert_eq!(summary["available"].as_u64(), Some(5));
        assert_eq!(summary["downloaded"].as_u64(), Some(1));
        assert_eq!(summary["duplicates"].as_u64(), Some(1));
        assert_eq!(summary["rejected"].as_u64(), Some(2));
        assert_eq!(
            summary["transferred_bytes"].as_u64(),
            Some(payloads.iter().map(Vec::len).sum::<usize>() as u64)
        );
    }

    #[test]
    fn classify_remote_transient_ids_reports_known_entries_as_haves() {
        let known = [0x11; 32];
        let unknown = [0x22; 32];

        let (wants, haves) = classify_remote_transient_ids_with(
            vec![known.to_vec(), unknown.to_vec()],
            |transient_id| Ok(transient_id == known.as_slice()),
        )
        .expect("classify remote ids");

        assert_eq!(wants, vec![unknown.to_vec()]);
        assert_eq!(haves, vec![known.to_vec()]);
    }

    #[test]
    fn propagation_download_get_payload_sends_mixed_wants_and_haves() {
        let wanted = vec![vec![0x11; 32]];
        let haves = vec![vec![0x22; 32]];

        let data = decode_link_request_payload(
            propagation_download_get_payload(Some(wanted.as_slice()), haves.as_slice(), Some(42.0))
                .expect("build get payload")
                .as_slice(),
        );

        let rmpv::Value::Array(entries) = data else {
            panic!("request data should be an array");
        };
        assert_eq!(
            entries.first(),
            Some(&rmpv::Value::Array(vec![rmpv::Value::Binary(wanted[0].clone())]))
        );
        assert_eq!(
            entries.get(1),
            Some(&rmpv::Value::Array(vec![rmpv::Value::Binary(haves[0].clone())]))
        );
        assert_eq!(entries.get(2).and_then(rmpv::Value::as_f64), Some(42.0));
    }

    #[test]
    fn propagation_download_get_payload_sends_purge_only_when_no_wants() {
        let haves = vec![vec![0x33; 32]];

        let data = decode_link_request_payload(
            propagation_download_get_payload(None, haves.as_slice(), None)
                .expect("build purge payload")
                .as_slice(),
        );

        let rmpv::Value::Array(entries) = data else {
            panic!("request data should be an array");
        };
        assert!(entries.first().is_some_and(rmpv::Value::is_nil));
        assert_eq!(
            entries.get(1),
            Some(&rmpv::Value::Array(vec![rmpv::Value::Binary(haves[0].clone())]))
        );
        assert_eq!(entries.len(), 2);
    }

    #[test]
    fn propagation_download_ack_rejects_remote_error_code() {
        let err = propagation_download_ack_response_result(&rmpv::Value::from(0xF6_u8))
            .expect_err("throttled ack response should fail");

        assert_eq!(err.kind(), std::io::ErrorKind::WouldBlock);
        assert!(err.to_string().contains("throttled"));
    }

    #[test]
    fn propagation_download_haves_only_summary_requires_ack_success() {
        let err = propagation_download_haves_only_summary(2, &rmpv::Value::from(0xF4_u64))
            .expect_err("remote cleanup rejection must fail purge-only download");

        assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
        assert!(err.to_string().contains("rejected"));

        let summary = propagation_download_haves_only_summary(2, &rmpv::Value::Boolean(true))
            .expect("successful ack returns summary");

        assert_eq!(summary["available_count"].as_u64(), Some(2));
        assert_eq!(summary["downloaded_count"].as_u64(), Some(0));
        assert_eq!(summary["duplicate_count"].as_u64(), Some(0));
        assert_eq!(summary["rejected_count"].as_u64(), Some(0));
        assert_eq!(summary["transferred_bytes"].as_u64(), Some(0));
    }

    #[tokio::test]
    async fn policy_rejected_downloaded_payload_is_not_reported_as_duplicate_have() {
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
                id: 70,
                method: "set_delivery_policy".to_string(),
                params: Some(json!({
                    "ignored_destinations": [hex::encode(source_hash)],
                })),
            })
            .expect("set delivery policy");

        let wire = build_wire_message_with_options(
            source_hash,
            destination_hash,
            "ignored remote title",
            "ignored remote content",
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

        let outcome = accept_downloaded_propagation_payload(
            &daemon,
            &delivery_destination,
            transient_payload.as_slice(),
        )
        .await
        .expect("accept downloaded payload");

        assert_eq!(
            outcome,
            DownloadAcceptOutcome::Rejected,
            "policy-rejected downloads are not local haves and must not be acked"
        );
    }

    #[test]
    fn propagation_download_ack_rejects_remote_rejection_code() {
        let err = propagation_download_ack_response_result(&rmpv::Value::from(0xF4_u64))
            .expect_err("remote ack rejection must fail the download");

        assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
        assert!(err.to_string().contains("rejected"));
    }
}
