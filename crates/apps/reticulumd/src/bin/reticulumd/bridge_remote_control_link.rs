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
    let signer = rns_transport::identity::PrivateIdentity::from_private_key_bytes(
        &identity.to_private_key_bytes(),
    )
    .expect("a private identity's own key bytes round-trip");
    rns_transport::destination::link::identify_payload(&signer, link_id)
}

pub(super) fn build_link_request_payload(
    path: &str,
    data: rmpv::Value,
) -> Result<Vec<u8>, std::io::Error> {
    Link::request_payload(path, data)
        .map(|request| request.packed)
        .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidData, err.to_string()))
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
                            rns_transport::resource::ResourceEventKind::InboundFailed(failure) => {
                                if fail_on_terminal_resource_events {
                                    let msg = format!(
                                        "propagation control inbound resource transfer failed: {}",
                                        failure.reason
                                    );
                                    return Err(std::io::Error::new(std::io::ErrorKind::BrokenPipe, msg));
                                }
                            }
                            rns_transport::resource::ResourceEventKind::OutboundComplete
                            | rns_transport::resource::ResourceEventKind::SegmentComplete(_)
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
#[path = "bridge_remote_control_link_tests.rs"]
mod tests;
