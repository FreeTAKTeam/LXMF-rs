use super::remote_control_download::propagation_download_request;
use super::*;
use lxmf::inbound_decode::InboundPayloadMode;
use reticulum_daemon::inbound_delivery::{
    annotate_inbound_record_stamp_status, decode_inbound_payload, evaluate_inbound_stamp_policy,
    inbound_record_allowed_by_delivery_policy,
};
use reticulum_daemon::lxmf_bridge::rmpv_to_json;
use rns_rpc::RemoteControlBridge;
use rns_transport::identity::DecryptIdentity;
use sha2::{Digest, Sha256};
use x25519_dalek::PublicKey;

impl TransportBridge {
    pub(super) fn run_remote_control_raw(
        &self,
        remote: &str,
        identity_private_key_hex: Option<&str>,
        timeout_secs: f64,
        path: &str,
        data: rmpv::Value,
    ) -> Result<rmpv::Value, std::io::Error> {
        let remote = remote.trim().to_string();
        let identity_override = identity_private_key_hex
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(|value| {
                let bytes = hex::decode(value).map_err(|err| {
                    std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        format!("identity_private_key_hex must be hex-encoded: {err}"),
                    )
                })?;
                PrivateIdentity::from_private_key_bytes(&bytes).map_err(|err| {
                    std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        format!("invalid identity private key: {err:?}"),
                    )
                })
            })
            .transpose()?;
        let request_identity = identity_override.unwrap_or_else(|| self.signer.clone());
        let timeout = Duration::from_secs_f64(timeout_secs.max(0.1));
        let path = path.to_string();
        let transport = self.transport.clone();
        let identity_cache = self.outbound_propagation_identities.clone();

        std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|err| {
                    std::io::Error::other(format!("failed to build remote control runtime: {err}"))
                })?;
            runtime.block_on(async move {
                let result = remote_control_request(
                    transport.as_ref(),
                    &request_identity,
                    &remote,
                    &path,
                    data,
                    timeout,
                )
                .await;
                if let Ok((_, identity)) = &result {
                    if let Ok(mut guard) = identity_cache.lock() {
                        guard.insert(remote.clone(), *identity);
                    }
                }
                result.and_then(|(value, _)| response_to_result(value))
            })
        })
        .join()
        .map_err(|_| std::io::Error::other("remote control helper thread panicked"))?
    }

    pub(super) fn run_remote_control(
        &self,
        remote: &str,
        identity_private_key_hex: Option<&str>,
        timeout_secs: f64,
        path: &str,
        data: rmpv::Value,
    ) -> Result<JsonValue, std::io::Error> {
        let remote = remote.trim().to_string();
        let identity_override = identity_private_key_hex
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(|value| {
                let bytes = hex::decode(value).map_err(|err| {
                    std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        format!("identity_private_key_hex must be hex-encoded: {err}"),
                    )
                })?;
                PrivateIdentity::from_private_key_bytes(&bytes).map_err(|err| {
                    std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        format!("invalid identity private key: {err:?}"),
                    )
                })
            })
            .transpose()?;
        let request_identity = identity_override.unwrap_or_else(|| self.signer.clone());
        let timeout = Duration::from_secs_f64(timeout_secs.max(0.1));
        let path = path.to_string();
        let transport = self.transport.clone();
        let identity_cache = self.outbound_propagation_identities.clone();

        std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|err| {
                    std::io::Error::other(format!("failed to build remote control runtime: {err}"))
                })?;
            runtime.block_on(async move {
                let result = remote_control_request(
                    transport.as_ref(),
                    &request_identity,
                    &remote,
                    &path,
                    data,
                    timeout,
                )
                .await;
                if let Ok((_, identity)) = &result {
                    if let Ok(mut guard) = identity_cache.lock() {
                        guard.insert(remote.clone(), *identity);
                    }
                }
                result.and_then(|(value, _)| response_to_json(&value))
            })
        })
        .join()
        .map_err(|_| std::io::Error::other("remote control helper thread panicked"))?
    }
}

pub(super) fn remote_peer_value(peer: &str) -> Result<rmpv::Value, std::io::Error> {
    let peer_hash = parse_destination_hash_required(peer)?;
    Ok(rmpv::Value::Binary(peer_hash.to_vec()))
}

impl RemoteControlBridge for TransportBridge {
    fn propagation_remote_status(
        &self,
        remote: &str,
        identity_private_key_hex: Option<&str>,
        timeout_secs: f64,
    ) -> Result<JsonValue, std::io::Error> {
        self.run_remote_control(
            remote,
            identity_private_key_hex,
            timeout_secs,
            "/pn/get/stats",
            rmpv::Value::Nil,
        )
    }

    fn propagation_remote_sync(
        &self,
        remote: &str,
        peer: &str,
        identity_private_key_hex: Option<&str>,
        timeout_secs: f64,
        transfer_limit_kb: Option<f64>,
    ) -> Result<JsonValue, std::io::Error> {
        let peer_value = remote_peer_value(peer)?;
        let request = transfer_limit_kb
            .map(|limit| rmpv::Value::Array(vec![peer_value.clone(), rmpv::Value::F64(limit)]))
            .unwrap_or(peer_value);
        self.run_remote_control(
            remote,
            identity_private_key_hex,
            timeout_secs,
            "/pn/peer/sync",
            request,
        )
    }

    fn propagation_remote_download(
        &self,
        remote: &str,
        identity_private_key_hex: Option<&str>,
        timeout_secs: f64,
    ) -> Result<JsonValue, std::io::Error> {
        let remote = remote.trim().to_string();
        let identity_override = identity_private_key_hex
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(|value| {
                let bytes = hex::decode(value).map_err(|err| {
                    std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        format!("identity_private_key_hex must be hex-encoded: {err}"),
                    )
                })?;
                PrivateIdentity::from_private_key_bytes(&bytes).map_err(|err| {
                    std::io::Error::new(
                        std::io::ErrorKind::InvalidInput,
                        format!("invalid identity private key: {err:?}"),
                    )
                })
            })
            .transpose()?;
        let request_identity = identity_override.unwrap_or_else(|| self.signer.clone());
        let timeout = Duration::from_secs_f64(timeout_secs.max(0.1));
        let transport = self.transport.clone();
        let identity_cache = self.outbound_propagation_identities.clone();
        let daemon = self
            .daemon
            .lock()
            .ok()
            .and_then(|guard| guard.clone())
            .ok_or_else(|| std::io::Error::other("rpc daemon unavailable"))?;
        let delivery_destination = self.announce_destination.clone();

        std::thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .map_err(|err| {
                    std::io::Error::other(format!(
                        "failed to build propagation download runtime: {err}"
                    ))
                })?;
            runtime.block_on(async move {
                let result = propagation_download_request(
                    transport.as_ref(),
                    daemon.as_ref(),
                    &delivery_destination,
                    &request_identity,
                    &remote,
                    timeout,
                )
                .await;
                if let Ok((_, identity)) = &result {
                    if let Ok(mut guard) = identity_cache.lock() {
                        guard.insert(remote.clone(), *identity);
                    }
                }
                result.map(|(json, _)| json)
            })
        })
        .join()
        .map_err(|_| std::io::Error::other("propagation download helper thread panicked"))?
    }

    fn propagation_remote_unpeer(
        &self,
        remote: &str,
        peer: &str,
        identity_private_key_hex: Option<&str>,
        timeout_secs: f64,
    ) -> Result<JsonValue, std::io::Error> {
        self.run_remote_control(
            remote,
            identity_private_key_hex,
            timeout_secs,
            "/pn/peer/unpeer",
            remote_peer_value(peer)?,
        )
    }
}

async fn propagation_download_request(
    transport: &Transport,
    daemon: &RpcDaemon,
    delivery_destination: &Arc<tokio::sync::Mutex<SingleInputDestination>>,
    request_identity: &PrivateIdentity,
    remote: &str,
    timeout: Duration,
) -> Result<(JsonValue, Identity), std::io::Error> {
    let remote_hash = AddressHash::new(parse_destination_hash_required(remote)?);
    let mut remote_identity = transport.destination_identity(&remote_hash).await;
    if remote_identity.is_none() {
        transport.request_path(&remote_hash, None, None).await;
        let deadline = tokio::time::Instant::now() + timeout.min(Duration::from_secs(12));
        while remote_identity.is_none() && tokio::time::Instant::now() < deadline {
            tokio::time::sleep(Duration::from_millis(250)).await;
            remote_identity = transport.destination_identity(&remote_hash).await;
        }
    }
    let remote_identity = remote_identity.ok_or_else(|| {
        std::io::Error::new(std::io::ErrorKind::NotFound, "no path known for propagation node")
    })?;

    let destination =
        SingleOutputDestination::new(remote_identity, DestinationName::new("lxmf", "propagation"));
    let link = transport.link(destination.desc).await;
    await_link_activation(transport, &link, timeout).await?;
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
    let wanted = binary_array_response(&list_response)?;

    if wanted.is_empty() {
        return Ok((json!({ "available": 0, "downloaded": 0, "duplicates": 0 }), remote_identity));
    }

    let get_payload = build_link_request_payload(
        "/get",
        rmpv::Value::Array(vec![
            rmpv::Value::Array(wanted.iter().cloned().map(rmpv::Value::Binary).collect()),
            rmpv::Value::Array(Vec::new()),
            rmpv::Value::F64(1000.0),
        ]),
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

    let mut haves = Vec::new();
    let mut downloaded = 0usize;
    let mut duplicates = 0usize;
    for payload in &payloads {
        let transient_id = Sha256::digest(payload);
        haves.push(transient_id.to_vec());
        match accept_downloaded_propagation_payload(daemon, delivery_destination, payload).await? {
            DownloadAcceptOutcome::Stored => downloaded += 1,
            DownloadAcceptOutcome::Duplicate => duplicates += 1,
        }
    }

    if !haves.is_empty() {
        let ack_payload = build_link_request_payload(
            "/get",
            rmpv::Value::Array(vec![
                rmpv::Value::Nil,
                rmpv::Value::Array(haves.into_iter().map(rmpv::Value::Binary).collect()),
            ]),
        )?;
        let _ = send_link_context_packet(
            transport,
            &link,
            PacketContext::Request,
            ack_payload.as_slice(),
        )
        .await?;
    }

    Ok((
        json!({
            "available": wanted.len(),
            "downloaded": downloaded,
            "duplicates": duplicates,
        }),
        remote_identity,
    ))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DownloadAcceptOutcome {
    Stored,
    Duplicate,
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
        return Ok(DownloadAcceptOutcome::Duplicate);
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

async fn remote_control_request(
    transport: &Transport,
    request_identity: &PrivateIdentity,
    remote: &str,
    path: &str,
    data: rmpv::Value,
    timeout: Duration,
) -> Result<(JsonValue, Identity), std::io::Error> {
    let remote_hash = AddressHash::new(parse_destination_hash_required(remote)?);
    let mut remote_identity = transport.destination_identity(&remote_hash).await;
    if remote_identity.is_none() {
        transport.request_path(&remote_hash, None, None).await;
        let deadline = tokio::time::Instant::now() + timeout.min(Duration::from_secs(12));
        while remote_identity.is_none() && tokio::time::Instant::now() < deadline {
            tokio::time::sleep(Duration::from_millis(250)).await;
            remote_identity = transport.destination_identity(&remote_hash).await;
        }
    }
    let remote_identity = remote_identity.ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "no path known for propagation control node",
        )
    })?;

    let destination = SingleOutputDestination::new(
        remote_identity,
        DestinationName::new("lxmf", "propagation.control"),
    );
    let link = transport.link(destination.desc).await;
    await_link_activation(transport, &link, timeout).await?;
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
    let request_payload = build_link_request_payload(path, data)?;
    let request_id = send_link_context_packet(
        transport,
        &link,
        PacketContext::Request,
        request_payload.as_slice(),
    )
    .await?
    .ok_or_else(|| std::io::Error::other("missing remote control request id"))?;

    let response = wait_for_link_request_response(
        &mut data_rx,
        &mut resource_rx,
        destination.desc.address_hash,
        link_id,
        request_id,
        timeout,
    )
    .await
    .map_err(|err| std::io::Error::new(std::io::ErrorKind::TimedOut, err))?;

    response_to_json(&response).map(|json| (json, remote_identity))
}

fn propagation_remote_fetch_ack_payload(
    payload_outcomes: &[(&[u8], LocalPropagationImportOutcome)],
) -> rmpv::Value {
    let haves = payload_outcomes
        .iter()
        .filter(|(_payload, outcome)| {
            matches!(
                outcome,
                LocalPropagationImportOutcome::Imported | LocalPropagationImportOutcome::Duplicate
            )
        })
        .map(|(payload, _outcome)| rmpv::Value::Binary(Sha256::digest(payload).to_vec()))
        .collect();
    rmpv::Value::Array(vec![rmpv::Value::Nil, rmpv::Value::Array(haves)])
}

fn response_to_json(response: &rmpv::Value) -> Result<JsonValue, std::io::Error> {
    if let Some(error) = response_code_error(response) {
        return Err(error);
    }
    if let Some(json) = rmpv_to_json(response) {
        return Ok(json);
    }
    match response {
        rmpv::Value::Boolean(value) => Ok(json!(value)),
        rmpv::Value::Nil => Ok(JsonValue::Null),
        _ => Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "unsupported propagation control response payload",
        )),
    }
}

fn response_to_result(response: rmpv::Value) -> Result<rmpv::Value, std::io::Error> {
    if let Some(error) = response_code_error(&response) {
        return Err(error);
    }
    Ok(response)
}

fn build_link_identify_payload(identity: &PrivateIdentity, link_id: &AddressHash) -> Vec<u8> {
    let mut public_key = Vec::with_capacity(64);
    public_key.extend_from_slice(identity.as_identity().public_key.as_bytes());
    public_key.extend_from_slice(identity.as_identity().verifying_key.as_bytes());

    let mut signed_data = Vec::with_capacity(16 + public_key.len());
    signed_data.extend_from_slice(link_id.as_slice());
    signed_data.extend_from_slice(public_key.as_slice());
    let signature = identity.sign(signed_data.as_slice());

    let mut payload = Vec::with_capacity(public_key.len() + signature.to_bytes().len());
    payload.extend_from_slice(public_key.as_slice());
    payload.extend_from_slice(signature.to_bytes().as_slice());
    payload
}

fn build_link_request_payload(path: &str, data: rmpv::Value) -> Result<Vec<u8>, std::io::Error> {
    let timestamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs_f64();
    let path_hash = address_hash(path.as_bytes());
    rmp_serde::to_vec(&rmpv::Value::Array(vec![
        rmpv::Value::F64(timestamp),
        rmpv::Value::Binary(path_hash.to_vec()),
        data,
    ]))
    .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidData, err))
}

async fn send_link_context_packet(
    transport: &Transport,
    link: &Arc<tokio::sync::Mutex<Link>>,
    context: PacketContext,
    payload: &[u8],
) -> Result<Option<[u8; 16]>, std::io::Error> {
    let (packet, ingress_iface) = {
        let guard = link.lock().await;
        if guard.status() != LinkStatus::Active {
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "propagation control link is not active",
            ));
        }

        let Some(ingress_iface) = guard.ingress_iface() else {
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotConnected,
                "propagation control link has no bound interface",
            ));
        };
        let mut packet_data = PacketDataBuffer::new();
        let cipher_len = {
            let ciphertext = guard
                .encrypt(payload, packet_data.accuire_buf_max())
                .map_err(|_| std::io::Error::other("failed to encrypt link packet"))?;
            ciphertext.len()
        };
        packet_data.resize(cipher_len);

        (
            Packet {
                header: Header {
                    ifac_flag: IfacFlag::Open,
                    header_type: HeaderType::Type1,
                    context_flag: ContextFlag::Unset,
                    propagation_type: PropagationType::Broadcast,
                    destination_type: DestinationType::Link,
                    packet_type: PacketType::Data,
                    hops: 0,
                },
                ifac: None,
                destination: *guard.id(),
                transport: None,
                context,
                data: packet_data,
            },
            ingress_iface,
        )
    };

    let request_id = if context == PacketContext::Request {
        let hash = packet.hash().to_bytes();
        let mut request_id = [0u8; 16];
        request_id.copy_from_slice(&hash[..16]);
        Some(request_id)
    } else {
        None
    };

    transport.send_direct(ingress_iface, packet).await;
    Ok(request_id)
}

async fn wait_for_link_request_response(
    data_rx: &mut tokio::sync::broadcast::Receiver<rns_transport::transport::ReceivedData>,
    resource_rx: &mut tokio::sync::broadcast::Receiver<rns_transport::resource::ResourceEvent>,
    expected_destination: AddressHash,
    expected_link_id: AddressHash,
    request_id: [u8; 16],
    timeout: Duration,
) -> Result<rmpv::Value, String> {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        let now = tokio::time::Instant::now();
        if now >= deadline {
            return Err("propagation control response timed out".to_string());
        }
        let remaining = deadline.saturating_duration_since(now);

        tokio::select! {
            _ = tokio::time::sleep(remaining) => {
                return Err("propagation control response timed out".to_string());
            }
            result = data_rx.recv() => {
                match result {
                    Ok(event) => {
                        if event.destination != expected_link_id
                            && event.destination != expected_destination
                        {
                            continue;
                        }
                        if let Some((response_id, payload)) =
                            parse_link_response_frame(event.data.as_slice())
                        {
                            if response_id == request_id {
                                return Ok(payload);
                            }
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                        return Err("propagation control response channel closed".to_string());
                    }
                }
            }
            result = resource_rx.recv() => {
                match result {
                    Ok(event) => {
                        let rns_transport::resource::ResourceEventKind::Complete(complete) =
                            event.kind
                        else {
                            continue;
                        };
                        if event.link_id != expected_link_id {
                            continue;
                        }
                        if let Some((response_id, payload)) =
                            parse_link_response_frame(complete.data.as_slice())
                        {
                            if response_id == request_id {
                                return Ok(payload);
                            }
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                        return Err("propagation control resource channel closed".to_string());
                    }
                }
            }
        }
    }
}

fn parse_link_response_frame(bytes: &[u8]) -> Option<([u8; 16], rmpv::Value)> {
    let value = rmp_serde::from_slice::<rmpv::Value>(bytes).ok()?;
    let rmpv::Value::Array(entries) = value else {
        return None;
    };
    if entries.len() != 2 {
        return None;
    }
    let request_bytes = value_to_bytes(entries.first()?)?;
    if request_bytes.len() != 16 {
        return None;
    }
    let mut request_id = [0u8; 16];
    request_id.copy_from_slice(request_bytes.as_slice());
    Some((request_id, entries.get(1)?.clone()))
}

fn value_to_bytes(value: &rmpv::Value) -> Option<Vec<u8>> {
    match value {
        rmpv::Value::Binary(bytes) => Some(bytes.clone()),
        rmpv::Value::String(text) => {
            let value = text.as_str()?;
            if let Ok(decoded) = hex::decode(value) {
                return Some(decoded);
            }
            Some(value.as_bytes().to_vec())
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::{Digest, Sha256};

    #[test]
    fn propagation_remote_fetch_summary_reports_transferred_bytes() {
        let payloads = vec![b"first".to_vec(), b"second-payload".to_vec()];

        let summary = propagation_remote_fetch_summary(7, &payloads, 1, 2, 3);

        assert_eq!(summary["available_count"].as_u64(), Some(7));
        assert_eq!(summary["fetched_count"].as_u64(), Some(2));
        assert_eq!(summary["imported_count"].as_u64(), Some(1));
        assert_eq!(summary["duplicate_count"].as_u64(), Some(2));
        assert_eq!(summary["rejected_count"].as_u64(), Some(3));
        assert_eq!(
            summary["transferred_bytes"].as_u64(),
            Some(payloads.iter().map(Vec::len).sum::<usize>() as u64)
        );
    }

    #[test]
    fn propagation_control_response_code_maps_throttled_like_python() {
        let err = response_code_error(&rmpv::Value::from(0xF6_u64))
            .expect("throttled response should map to error");

        assert_eq!(err.kind(), std::io::ErrorKind::WouldBlock);
        assert_eq!(err.to_string(), "propagation peer throttled");
    }

    #[test]
    fn propagation_control_response_code_maps_invalid_peering_key_like_python() {
        let err = response_code_error(&rmpv::Value::from(0xF3_u64))
            .expect("invalid key response should map to error");

        assert_eq!(err.kind(), std::io::ErrorKind::PermissionDenied);
        assert_eq!(err.to_string(), "propagation peer invalid peering key");
    }

    #[test]
    fn propagation_control_response_code_maps_timeout_like_python() {
        let err = response_code_error(&rmpv::Value::from(0xFE_u64))
            .expect("timeout response should map to error");

        assert_eq!(err.kind(), std::io::ErrorKind::TimedOut);
        assert_eq!(err.to_string(), "propagation peer timed out");
    }

    #[test]
    fn propagation_remote_fetch_ack_payload_reports_imported_and_duplicate_haves() {
        let imported_payload = b"imported remote fetch payload".to_vec();
        let duplicate_payload = b"duplicate remote fetch payload".to_vec();
        let rejected_payload = b"rejected remote fetch payload".to_vec();

        let ack = propagation_remote_fetch_ack_payload(&[
            (&imported_payload, LocalPropagationImportOutcome::Imported),
            (&duplicate_payload, LocalPropagationImportOutcome::Duplicate),
            (&rejected_payload, LocalPropagationImportOutcome::Rejected),
        ]);

        let rmpv::Value::Array(entries) = ack else {
            panic!("expected /get acknowledgement array");
        };
        assert!(entries.first().is_some_and(rmpv::Value::is_nil));
        let Some(rmpv::Value::Array(haves)) = entries.get(1) else {
            panic!("expected haves array");
        };
        assert_eq!(haves.len(), 2);
        assert_eq!(haves[0], rmpv::Value::Binary(Sha256::digest(imported_payload).to_vec()));
        assert_eq!(haves[1], rmpv::Value::Binary(Sha256::digest(duplicate_payload).to_vec()));
    }
}
