use super::{
    send_outcome_is_sent, send_outcome_status, PR_LINK_FAILED, PR_NO_ACCESS, PR_NO_IDENTITY_RCVD,
    PR_NO_PATH, PR_TRANSFER_FAILED,
};
use reticulum::destination::link::{Link, LinkStatus};
use reticulum::hash::{address_hash, AddressHash};
use reticulum::identity::PrivateIdentity;
use reticulum::packet::{
    ContextFlag, DestinationType, Header, HeaderType, IfacFlag, Packet, PacketContext,
    PacketDataBuffer, PacketType, PropagationType,
};
use reticulum::transport::Transport;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

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
    let packet = {
        let guard = link.lock().await;
        if guard.status() != LinkStatus::Active {
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "propagation link is not active",
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
        return Err(std::io::Error::other(send_outcome_status("propagation request", outcome)));
    }
    Ok(request_id)
}

pub(super) async fn wait_for_link_request_response(
    data_rx: &mut tokio::sync::broadcast::Receiver<reticulum::transport::ReceivedData>,
    resource_rx: &mut tokio::sync::broadcast::Receiver<reticulum::resource::ResourceEvent>,
    expected_destination: AddressHash,
    expected_link_id: AddressHash,
    request_id: [u8; 16],
    timeout: Duration,
) -> Result<rmpv::Value, String> {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        let now = tokio::time::Instant::now();
        if now >= deadline {
            return Err("propagation response timed out".to_string());
        }
        let remaining = deadline.saturating_duration_since(now);

        tokio::select! {
            _ = tokio::time::sleep(remaining) => {
                return Err("propagation response timed out".to_string());
            }
            result = data_rx.recv() => {
                match result {
                    Ok(event) => {
                        if event.destination != expected_destination {
                            continue;
                        }
                        if let Some((response_id, payload)) = parse_link_response_frame(event.data.as_slice()) {
                            if response_id == request_id {
                                return Ok(payload);
                            }
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                        return Err("propagation response channel closed".to_string());
                    }
                }
            }
            result = resource_rx.recv() => {
                match result {
                    Ok(event) => {
                        if event.link_id != expected_link_id {
                            continue;
                        }
                        let reticulum::resource::ResourceEventKind::Complete(complete) = event.kind else {
                            continue;
                        };
                        if let Some((response_id, payload)) = parse_link_response_frame(complete.data.as_slice()) {
                            if response_id == request_id {
                                return Ok(payload);
                            }
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                        return Err("propagation resource channel closed".to_string());
                    }
                }
            }
        }
    }
}

pub(super) fn propagation_error_from_response_value(
    value: &rmpv::Value,
) -> Option<(u32, &'static str, &'static str, &'static str)> {
    let code = value.as_u64().or_else(|| value.as_i64().map(|raw| raw as u64))?;
    match code as u32 {
        PR_NO_PATH => {
            Some((PR_NO_PATH, "no_path", "No path known for propagation node", "NO_PATH"))
        }
        PR_LINK_FAILED => {
            Some((PR_LINK_FAILED, "link_failed", "Propagation link failed", "LINK_FAILED"))
        }
        PR_TRANSFER_FAILED => Some((
            PR_TRANSFER_FAILED,
            "transfer_failed",
            "Propagation transfer failed",
            "TRANSFER_FAILED",
        )),
        PR_NO_IDENTITY_RCVD => Some((
            PR_NO_IDENTITY_RCVD,
            "no_identity_rcvd",
            "Propagation node requires identity",
            "NO_IDENTITY_RCVD",
        )),
        PR_NO_ACCESS => {
            Some((PR_NO_ACCESS, "no_access", "Propagation node denied access", "NO_ACCESS"))
        }
        _ => None,
    }
}

pub(super) fn parse_binary_array(value: &rmpv::Value) -> Option<Vec<Vec<u8>>> {
    let rmpv::Value::Array(entries) = value else {
        return None;
    };
    let mut out = Vec::with_capacity(entries.len());
    for entry in entries {
        let value = value_to_bytes(entry)?;
        out.push(value);
    }
    Some(out)
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
