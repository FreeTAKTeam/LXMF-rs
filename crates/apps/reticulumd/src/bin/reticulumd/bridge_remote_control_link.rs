use super::*;

pub(super) async fn resolve_remote_identity(
    transport: &Transport,
    remote_hash: &AddressHash,
    timeout: Duration,
) -> Result<Option<Identity>, std::io::Error> {
    transport.request_path(remote_hash, None, None).await;
    let deadline = tokio::time::Instant::now() + timeout.min(Duration::from_secs(12));
    let mut remote_identity = transport.destination_identity(remote_hash).await;

    while remote_identity.is_none() && tokio::time::Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(250)).await;
        remote_identity = transport.destination_identity(remote_hash).await;
    }

    if remote_identity.is_some() {
        tokio::time::sleep(Duration::from_millis(500)).await;
    }

    Ok(remote_identity)
}

pub(super) async fn open_refreshed_remote_link(
    transport: &Transport,
    remote_hash: &AddressHash,
    destination: DestinationDesc,
    timeout: Duration,
) -> Result<Arc<tokio::sync::Mutex<Link>>, std::io::Error> {
    let link = transport.link(destination).await;
    match await_link_activation(transport, &link, timeout).await {
        Ok(()) => Ok(link),
        Err(err) if err.kind() == std::io::ErrorKind::TimedOut => {
            link.lock().await.close();
            transport.request_path(remote_hash, None, None).await;
            tokio::time::sleep(Duration::from_secs(1)).await;
            let retry_link = transport.link(destination).await;
            await_link_activation(transport, &retry_link, timeout).await.map_err(|retry_err| {
                std::io::Error::new(
                    retry_err.kind(),
                    format!(
                        "{retry_err}; retried after refreshing propagation node path due to {err}"
                    ),
                )
            })?;
            Ok(retry_link)
        }
        Err(err) => Err(err),
    }
}

pub(super) fn build_link_identify_payload(
    identity: &PrivateIdentity,
    link_id: &AddressHash,
) -> Vec<u8> {
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

pub(super) fn build_link_request_payload(
    path: &str,
    data: rmpv::Value,
) -> Result<Vec<u8>, std::io::Error> {
    let timestamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs_f64();
    let path_hash = address_hash(path.as_bytes());
    rmp_serde::to_vec(&rmpv::Value::Array(vec![
        rmpv::Value::F64(timestamp),
        rmpv::Value::Binary(path_hash.to_vec()),
        data,
    ]))
    .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidData, err))
}

pub(super) async fn send_link_context_packet(
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

pub(super) async fn wait_for_link_request_response(
    data_rx: &mut tokio::sync::broadcast::Receiver<rns_transport::transport::ReceivedData>,
    resource_rx: &mut tokio::sync::broadcast::Receiver<rns_transport::resource::ResourceEvent>,
    expected_destination: AddressHash,
    expected_link_id: AddressHash,
    request_id: [u8; 16],
    timeout: Duration,
) -> Result<rmpv::Value, std::io::Error> {
    wait_for_link_request_response_with_terminal_policy(
        data_rx,
        resource_rx,
        expected_destination,
        expected_link_id,
        request_id,
        false,
        timeout,
    )
    .await
}

pub(super) async fn wait_for_link_request_response_with_terminal_policy(
    data_rx: &mut tokio::sync::broadcast::Receiver<rns_transport::transport::ReceivedData>,
    resource_rx: &mut tokio::sync::broadcast::Receiver<rns_transport::resource::ResourceEvent>,
    expected_destination: AddressHash,
    expected_link_id: AddressHash,
    request_id: [u8; 16],
    fail_on_terminal_resource_events: bool,
    timeout: Duration,
) -> Result<rmpv::Value, std::io::Error> {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        let now = tokio::time::Instant::now();
        if now >= deadline {
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "propagation control response timed out",
            ));
        }
        let remaining = deadline.saturating_duration_since(now);

        tokio::select! {
            _ = tokio::time::sleep(remaining) => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "propagation control response timed out",
                ));
            }
            result = data_rx.recv() => {
                match result {
                    Ok(event) => {
                        if event.destination != expected_link_id
                            && event.destination != expected_destination
                        {
                            continue;
                        }
                        if let Some(error) = link_close_signal_error(&event) {
                            return Err(error);
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
                        return Err(std::io::Error::new(
                            std::io::ErrorKind::BrokenPipe,
                            "propagation control response channel closed",
                        ));
                    }
                }
            }
            result = resource_rx.recv() => {
                match result {
                    Ok(event) => {
                        if event.link_id != expected_link_id {
                            continue;
                        }
                        match event.kind {
                            rns_transport::resource::ResourceEventKind::Complete(complete) => {
                                if let Some((response_id, payload)) =
                                    parse_link_response_frame(complete.data.as_slice())
                                {
                                    if response_id == request_id {
                                        return Ok(payload);
                                    }
                                }
                            }
                            rns_transport::resource::ResourceEventKind::OutboundFailed => {
                                if fail_on_terminal_resource_events {
                                    return Err(std::io::Error::new(
                                        std::io::ErrorKind::BrokenPipe,
                                        "propagation control resource transfer failed",
                                    ));
                                }
                            }
                            rns_transport::resource::ResourceEventKind::OutboundCancelled => {
                                if fail_on_terminal_resource_events {
                                    return Err(std::io::Error::new(
                                        std::io::ErrorKind::BrokenPipe,
                                        "propagation control resource transfer cancelled",
                                    ));
                                }
                            }
                            rns_transport::resource::ResourceEventKind::OutboundComplete
                            | rns_transport::resource::ResourceEventKind::Progress(_) => {}
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                        return Err(std::io::Error::new(
                            std::io::ErrorKind::BrokenPipe,
                            "propagation control resource channel closed",
                        ));
                    }
                }
            }
        }
    }
}

fn link_close_signal_error(
    event: &rns_transport::transport::ReceivedData,
) -> Option<std::io::Error> {
    if event.context != Some(PacketContext::LinkClose) {
        return None;
    }
    let value = rmp_serde::from_slice::<rmpv::Value>(event.data.as_slice()).inspect_err(|err| {
        log::warn!("[daemon-control] failed to decode link close signal: {err}")
    });
    let value = value.ok()?;
    let rmpv::Value::Array(entries) = value else {
        return Some(std::io::Error::new(
            std::io::ErrorKind::ConnectionAborted,
            "propagation control link closed",
        ));
    };
    let Some(signal) = entries.first() else {
        return Some(std::io::Error::new(
            std::io::ErrorKind::ConnectionAborted,
            "propagation control link closed",
        ));
    };
    let Some(error) = super::remote_control::response_code_error(signal) else {
        return Some(std::io::Error::new(
            std::io::ErrorKind::ConnectionAborted,
            "propagation control link closed",
        ));
    };
    Some(error)
}

fn parse_link_response_frame(bytes: &[u8]) -> Option<([u8; 16], rmpv::Value)> {
    let value = rmp_serde::from_slice::<rmpv::Value>(bytes).inspect_err(|err| {
        log::warn!("[daemon-control] failed to decode link response frame: {err}")
    });
    let value = value.ok()?;
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
    use rns_transport::hash::Hash;
    use rns_transport::packet::PacketDataBuffer;
    use rns_transport::resource::{ResourceEvent, ResourceEventKind};
    use rns_transport::transport::{ReceivedData, ReceivedPayloadMode};

    #[test]
    fn build_link_request_payload_encodes_peer_sync_data_as_msgpack_binary() {
        let peer_hash = vec![0xab; 16];
        let payload =
            build_link_request_payload("/pn/peer/sync", rmpv::Value::Binary(peer_hash.clone()))
                .expect("encode peer sync request");

        assert_eq!(payload.first(), Some(&0x93), "request frame must be a 3-item array");
        assert_eq!(
            payload.get(10..12),
            Some(&[0xc4, 0x10][..]),
            "path hash must be MessagePack bin8 bytes"
        );
        assert_eq!(
            payload.get(28..30),
            Some(&[0xc4, 0x10][..]),
            "peer sync data must be MessagePack bin8 bytes for Python LXMF"
        );
        assert_eq!(payload.get(30..46), Some(peer_hash.as_slice()));
    }

    async fn resource_terminal_error(kind: ResourceEventKind) -> std::io::Error {
        let (_data_tx, mut data_rx) = tokio::sync::broadcast::channel(4);
        let (resource_tx, mut resource_rx) = tokio::sync::broadcast::channel(4);
        let destination = AddressHash::new([0x11; 16]);
        let link_id = AddressHash::new([0x22; 16]);
        let request_id = [0x33; 16];

        resource_tx
            .send(ResourceEvent {
                hash: Hash::new_from_slice(b"terminal propagation control resource"),
                link_id,
                kind,
            })
            .expect("send terminal resource event");

        wait_for_link_request_response_with_terminal_policy(
            &mut data_rx,
            &mut resource_rx,
            destination,
            link_id,
            request_id,
            true,
            Duration::from_millis(50),
        )
        .await
        .expect_err("terminal resource event should fail immediately")
    }

    #[tokio::test]
    async fn wait_for_link_request_response_fails_on_resource_failure() {
        let err = resource_terminal_error(ResourceEventKind::OutboundFailed).await;

        assert_eq!(err.kind(), std::io::ErrorKind::BrokenPipe);
        assert_eq!(err.to_string(), "propagation control resource transfer failed");
    }

    #[tokio::test]
    async fn wait_for_link_request_response_fails_on_resource_cancel() {
        let err = resource_terminal_error(ResourceEventKind::OutboundCancelled).await;

        assert_eq!(err.kind(), std::io::ErrorKind::BrokenPipe);
        assert_eq!(err.to_string(), "propagation control resource transfer cancelled");
    }

    #[tokio::test]
    async fn wait_for_link_request_response_ignores_terminal_resource_without_policy() {
        let (data_tx, mut data_rx) = tokio::sync::broadcast::channel(4);
        let (resource_tx, mut resource_rx) = tokio::sync::broadcast::channel(4);
        let destination = AddressHash::new([0x11; 16]);
        let link_id = AddressHash::new([0x22; 16]);
        let stale_request_id = [0x33; 16];
        let request_id = [0x44; 16];
        let response_payload = rmpv::Value::Array(vec![
            rmpv::Value::Binary(request_id.to_vec()),
            rmpv::Value::String("ok".into()),
        ]);
        let response_frame = rmp_serde::to_vec(&response_payload).expect("encode response frame");

        resource_tx
            .send(ResourceEvent {
                hash: Hash::new_from_slice(b"stale propagation control resource"),
                link_id,
                kind: ResourceEventKind::OutboundFailed,
            })
            .expect("send stale terminal resource event");
        assert!(data_tx
            .send(ReceivedData {
                destination: link_id,
                data: PacketDataBuffer::new_from_slice(&response_frame),
                payload_mode: ReceivedPayloadMode::FullWire,
                ratchet_used: false,
                context: Some(PacketContext::None),
                request_id: None,
                hops: None,
                interface: None,
            })
            .is_ok());

        let response = wait_for_link_request_response_with_terminal_policy(
            &mut data_rx,
            &mut resource_rx,
            destination,
            link_id,
            request_id,
            false,
            Duration::from_millis(50),
        )
        .await
        .expect("stale terminal event should not fail the current request");

        assert_eq!(response.as_str(), Some("ok"));
        assert_ne!(stale_request_id, request_id);
    }

    #[tokio::test]
    async fn wait_for_link_request_response_fails_on_link_close_signal() {
        let (data_tx, mut data_rx) = tokio::sync::broadcast::channel(4);
        let (_resource_tx, mut resource_rx) = tokio::sync::broadcast::channel::<ResourceEvent>(4);
        let expected_destination = AddressHash::new_from_slice(&[0x11; 16]);
        let expected_link_id = AddressHash::new_from_slice(&[0x22; 16]);
        let request_id = [0x33; 16];
        let signal_payload = rmp_serde::to_vec(&vec![0xf1u8]).expect("signal msgpack");

        assert!(
            data_tx
                .send(ReceivedData {
                    destination: expected_link_id,
                    data: PacketDataBuffer::new_from_slice(&signal_payload),
                    payload_mode: ReceivedPayloadMode::FullWire,
                    ratchet_used: false,
                    context: Some(PacketContext::LinkClose),
                    request_id: None,
                    hops: None,
                    interface: None,
                })
                .is_ok(),
            "send link-close signal"
        );

        let err = wait_for_link_request_response(
            &mut data_rx,
            &mut resource_rx,
            expected_destination,
            expected_link_id,
            request_id,
            Duration::from_secs(10),
        )
        .await
        .expect_err("link-close signal should fail the active request");

        assert_eq!(err.kind(), std::io::ErrorKind::PermissionDenied);
        assert!(err.to_string().contains("propagation node denied access"));
    }
}
