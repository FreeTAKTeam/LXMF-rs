use super::*;
use reticulum_daemon::lxmf_bridge::rmpv_to_json;
use rns_rpc::RemoteControlBridge;

impl TransportBridge {
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
                result.map(|(json, _)| json)
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
    ) -> Result<JsonValue, std::io::Error> {
        self.run_remote_control(
            remote,
            identity_private_key_hex,
            timeout_secs,
            "/pn/peer/sync",
            remote_peer_value(peer)?,
        )
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

fn response_to_json(response: &rmpv::Value) -> Result<JsonValue, std::io::Error> {
    if let Some(code) = response.as_u64().or_else(|| response.as_i64().map(|value| value as u64)) {
        let (kind, message) = match code as u8 {
            0xF0 => (std::io::ErrorKind::PermissionDenied, "propagation node requires identity"),
            0xF1 => (std::io::ErrorKind::PermissionDenied, "propagation node denied access"),
            0xF4 => (std::io::ErrorKind::InvalidInput, "propagation node rejected the request"),
            0xFD => (std::io::ErrorKind::NotFound, "propagation peer not found"),
            _ => (std::io::ErrorKind::InvalidData, "unexpected propagation control response"),
        };
        return Err(std::io::Error::new(kind, message));
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
    let packet = {
        let guard = link.lock().await;
        if guard.status() != LinkStatus::Active {
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "propagation control link is not active",
            ));
        }

        let mut packet_data = PacketDataBuffer::new();
        let cipher_len = {
            let ciphertext = guard
                .encrypt(payload, packet_data.accuire_buf_max())
                .map_err(|_| std::io::Error::other("failed to encrypt link packet"))?;
            ciphertext.len()
        };
        packet_data.resize(cipher_len);

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
        }
    };

    let request_id = if context == PacketContext::Request {
        let hash = packet.hash().to_bytes();
        let mut request_id = [0u8; 16];
        request_id.copy_from_slice(&hash[..16]);
        Some(request_id)
    } else {
        None
    };

    let outcome = transport.send_packet_with_outcome(packet).await;
    if !send_outcome_is_sent(outcome) {
        return Err(std::io::Error::other(send_outcome_status(
            "propagation control request",
            outcome,
        )));
    }
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
