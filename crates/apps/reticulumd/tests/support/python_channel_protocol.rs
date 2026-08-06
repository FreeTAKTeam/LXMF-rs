use std::io;
use std::sync::Arc;
use std::time::Duration;

use rns_transport::destination::link::{Link, LinkEvent, LinkEventData};
use rns_transport::hash::{address_hash, AddressHash};
use rns_transport::identity::{lxmf_sign, Identity, PUBLIC_KEY_LENGTH};
use rns_transport::packet::{
    ContextFlag, DestinationType, Header, HeaderType, IfacFlag, Packet, PacketContext,
    PacketDataBuffer, PacketType, PropagationType,
};
use rns_transport::resource::{ResourceComplete, ResourceEvent, ResourceEventKind};
use rns_transport::transport::{ReceivedData, SendPacketOutcome, Transport};
use tokio::time::timeout;

pub(super) async fn wait_for_link_identify(
    events: &mut tokio::sync::broadcast::Receiver<LinkEventData>,
    link_id: AddressHash,
    duration: Duration,
) -> Identity {
    timeout(duration, async {
        loop {
            let event = events.recv().await.expect("link event");
            if event.id != link_id {
                continue;
            }
            if let LinkEvent::PeerIdentified(identity) = event.event {
                return *identity;
            }
        }
    })
    .await
    .expect("timed out waiting for link identify")
}

pub(super) fn build_link_identify_payload(
    identity: &rns_transport::identity::PrivateIdentity,
    link_id: &AddressHash,
) -> Vec<u8> {
    let mut public_key = Vec::with_capacity(PUBLIC_KEY_LENGTH * 2);
    public_key.extend_from_slice(identity.as_identity().public_key_bytes());
    public_key.extend_from_slice(identity.as_identity().verifying_key_bytes());

    let mut signed_data = Vec::with_capacity(link_id.as_slice().len() + public_key.len());
    signed_data.extend_from_slice(link_id.as_slice());
    signed_data.extend_from_slice(&public_key);

    let signature = lxmf_sign(identity, &signed_data);
    let mut payload = public_key;
    payload.extend_from_slice(&signature);
    payload
}

pub(super) fn build_link_request_payload(
    path: &str,
    data: rmpv::Value,
) -> Result<Vec<u8>, io::Error> {
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs_f64();
    let path_hash = address_hash(path.as_bytes());
    rmp_serde::to_vec(&rmpv::Value::Array(vec![
        rmpv::Value::F64(timestamp),
        rmpv::Value::Binary(path_hash.to_vec()),
        data,
    ]))
    .map_err(io::Error::other)
}

pub(super) async fn send_link_context_packet(
    transport: &Transport,
    link: &Arc<tokio::sync::Mutex<Link>>,
    context: PacketContext,
    payload: &[u8],
) -> Result<Option<[u8; 16]>, io::Error> {
    let packet = {
        let guard = link.lock().await;
        let mut packet_data = PacketDataBuffer::new();
        let cipher_len = {
            let ciphertext = guard
                .encrypt(payload, packet_data.accuire_buf_max())
                .map_err(|_| io::Error::other("failed to encrypt link packet"))?;
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

    match transport.send_packet_with_outcome(packet).await {
        SendPacketOutcome::SentDirect | SendPacketOutcome::SentBroadcast => Ok(request_id),
        other => Err(io::Error::other(format!("request packet not sent: {other:?}"))),
    }
}

pub(super) async fn send_link_response(
    transport: &Transport,
    link_id: AddressHash,
    request_id: [u8; 16],
    response: rmpv::Value,
) -> Result<(), io::Error> {
    let link = transport
        .find_in_link(&link_id)
        .await
        .ok_or_else(|| io::Error::other("inbound link not found"))?;
    let frame = rmpv::Value::Array(vec![rmpv::Value::Binary(request_id.to_vec()), response]);
    let payload = rmp_serde::to_vec(&frame).map_err(io::Error::other)?;
    let (packet, iface) = {
        let guard = link.lock().await;
        let iface = guard
            .ingress_iface()
            .ok_or_else(|| io::Error::other("inbound link ingress iface missing"))?;
        let mut packet_data = PacketDataBuffer::new();
        let cipher_len = {
            let ciphertext = guard
                .encrypt(payload.as_slice(), packet_data.accuire_buf_max())
                .map_err(|_| io::Error::other("failed to encrypt response"))?;
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
                context: PacketContext::Response,
                data: packet_data,
            },
            iface,
        )
    };
    transport.send_direct(iface, packet).await;
    Ok(())
}

pub(super) async fn wait_for_request_response(
    events: &mut tokio::sync::broadcast::Receiver<ReceivedData>,
    link_id: AddressHash,
    request_id: [u8; 16],
    duration: Duration,
) -> rmpv::Value {
    timeout(duration, async {
        loop {
            let event = events.recv().await.expect("received data event");
            if event.destination != link_id {
                continue;
            }
            if let Some((response_id, response)) =
                parse_request_response_frame(event.data.as_slice())
            {
                if response_id == request_id {
                    return response;
                }
            }
        }
    })
    .await
    .expect("timed out waiting for request response")
}

pub(super) async fn wait_for_resource_response(
    events: &mut tokio::sync::broadcast::Receiver<ResourceEvent>,
    link_id: AddressHash,
    request_id: [u8; 16],
    duration: Duration,
) -> rmpv::Value {
    timeout(duration, async {
        loop {
            let event = events.recv().await.expect("resource event");
            if event.link_id != link_id {
                continue;
            }
            if let ResourceEventKind::Complete(complete) = event.kind {
                if complete.is_response
                    && complete.request_id.as_deref() == Some(request_id.as_slice())
                {
                    if let Some((response_id, response)) =
                        parse_request_response_frame(complete.data.as_slice())
                    {
                        if response_id == request_id {
                            return response;
                        }
                    }
                }
            }
        }
    })
    .await
    .expect("timed out waiting for resource response")
}

pub(super) async fn wait_for_file_resource_response(
    events: &mut tokio::sync::broadcast::Receiver<ResourceEvent>,
    link_id: AddressHash,
    request_id: [u8; 16],
    duration: Duration,
) -> ResourceComplete {
    timeout(duration, async {
        loop {
            let event = events.recv().await.expect("resource event");
            if event.link_id != link_id {
                continue;
            }
            if let ResourceEventKind::Complete(complete) = event.kind {
                if complete.is_response
                    && !complete.is_request
                    && complete.request_id.as_deref() == Some(request_id.as_slice())
                {
                    return complete;
                }
            }
        }
    })
    .await
    .expect("timed out waiting for file resource response")
}

pub(super) async fn wait_for_request(
    events: &mut tokio::sync::broadcast::Receiver<ReceivedData>,
    link_id: AddressHash,
    duration: Duration,
) -> ([u8; 16], rmpv::Value) {
    timeout(duration, async {
        loop {
            let event = events.recv().await.expect("received data event");
            if event.destination != link_id || event.context != Some(PacketContext::Request) {
                continue;
            }
            let Some(request_id) = event.request_id else {
                continue;
            };
            let Some(data) = parse_request_payload(event.data.as_slice()) else {
                continue;
            };
            return (request_id, data);
        }
    })
    .await
    .expect("timed out waiting for request")
}

pub(super) fn parse_request_payload(bytes: &[u8]) -> Option<rmpv::Value> {
    let value = rmp_serde::from_slice::<rmpv::Value>(bytes).ok()?;
    let rmpv::Value::Array(entries) = value else {
        return None;
    };
    if entries.len() != 3 {
        return None;
    }
    entries.get(2).cloned()
}

fn parse_request_response_frame(bytes: &[u8]) -> Option<([u8; 16], rmpv::Value)> {
    let value = rmp_serde::from_slice::<rmpv::Value>(bytes).ok()?;
    let rmpv::Value::Array(entries) = value else {
        return None;
    };
    if entries.len() != 2 {
        return None;
    }
    let rmpv::Value::Binary(request_bytes) = entries.first()? else {
        return None;
    };
    if request_bytes.len() != 16 {
        return None;
    }
    let mut request_id = [0u8; 16];
    request_id.copy_from_slice(request_bytes.as_slice());
    Some((request_id, entries.get(1)?.clone()))
}

pub(super) fn rmpv_to_string(value: &rmpv::Value) -> Option<String> {
    match value {
        rmpv::Value::String(text) => text.as_str().map(ToOwned::to_owned),
        rmpv::Value::Binary(bytes) => String::from_utf8(bytes.clone()).ok(),
        _ => None,
    }
}
