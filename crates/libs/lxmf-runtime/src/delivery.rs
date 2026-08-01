use std::time::Duration;

use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use base64::Engine as _;
use lxmf_core::{decide_delivery, LxmfError, Message, MessageMethod, TransportMethod, WireMessage};
use lxmf_sdk::{MessageId, SdkError, SendRequest};
use rand_core::OsRng;
use rns_transport::destination::{DestinationDesc, DestinationName};
use rns_transport::hash::AddressHash;
use rns_transport::identity::PrivateIdentity;
use rns_transport::packet::{
    ContextFlag, DestinationType, Header, HeaderType, IfacFlag, Packet, PacketContext,
    PacketDataBuffer, PacketType, PropagationType, LXMF_MAX_PAYLOAD,
};
use rns_transport::transport::{SendPacketOutcome, Transport};
use serde_json::Value as JsonValue;

use crate::link_delivery::send_link_payload;
use crate::{
    EXT_ACCEPTED_RESULT_ACK, EXT_DIRECT_PACKET_MAX_WIRE_BYTES, EXT_FIELDS_BASE64,
    EXT_LINK_CONNECT_TIMEOUT_MS, EXT_PROPAGATION_RELAY_HEX, EXT_RAW_BYTES_BASE64, EXT_SEND_MODE,
    EXT_USE_PROPAGATION_NODE,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DeliveryMethod {
    Opportunistic,
    Direct,
    Propagated,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DeliveryRepresentation {
    Packet,
    Resource,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DeliveryOutcome {
    SentDirect,
    SentBroadcast,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InProcessSendReport {
    pub message_id: MessageId,
    pub resolved_destination: String,
    pub method: DeliveryMethod,
    pub representation: DeliveryRepresentation,
    pub outcome: DeliveryOutcome,
    pub relay_destination: Option<String>,
    pub receipt_hash: Option<String>,
}

pub(crate) struct SendContext<'a> {
    pub transport: &'a Transport,
    pub identity: &'a PrivateIdentity,
    pub source_destination: AddressHash,
    pub propagation_relay: Option<AddressHash>,
    pub link_connect_timeout: Duration,
    pub link_connect_attempts: usize,
    pub resource_transfer_timeout: Duration,
}

pub(crate) async fn send(
    context: SendContext<'_>,
    request: &SendRequest,
) -> Result<InProcessSendReport, SdkError> {
    let requested_destination = parse_hash(&request.destination)?;
    let remote = resolve_destination(
        context.transport,
        requested_destination,
        "delivery",
        context.link_connect_timeout,
    )
    .await?;
    let wire = encode_wire(context.identity, context.source_destination, &remote, request)?;
    let message_id = MessageId(hex::encode(
        WireMessage::unpack(&wire)
            .map_err(|err| internal(format!("failed to decode encoded LXMF message: {err}")))?
            .try_message_id()
            .map_err(|err| internal(format!("failed to hash encoded LXMF message: {err}")))?,
    ));
    let mut desired_method = requested_method(request)?;
    if matches!(desired_method, TransportMethod::Opportunistic)
        && request_uses_auto_mode(request)
        && context.transport.delivery_link_available(&remote.address_hash).await
    {
        desired_method = TransportMethod::Direct;
    }
    let decision = decide_delivery(desired_method, false, wire.len())
        .map_err(|err| validation(format!("failed to select delivery representation: {err}")))?;
    let representation =
        forced_representation(request, decision.method, decision.representation, wire.len());

    match decision.method {
        TransportMethod::Opportunistic => {
            send_opportunistic(context.transport, remote.address_hash, &wire, message_id).await
        }
        TransportMethod::Direct => {
            send_link_payload(
                context.transport,
                remote,
                &wire,
                message_id,
                DeliveryMethod::Direct,
                representation,
                context.link_connect_timeout,
                context.link_connect_attempts,
                context.resource_transfer_timeout,
                None,
            )
            .await
        }
        TransportMethod::Propagated => {
            send_propagated(context, request, &remote, &wire, message_id).await
        }
        TransportMethod::Paper => Err(validation("paper delivery is not supported in-process")),
    }
}

async fn send_opportunistic(
    transport: &Transport,
    destination: AddressHash,
    wire: &[u8],
    message_id: MessageId,
) -> Result<InProcessSendReport, SdkError> {
    let packet = data_packet(destination, PropagationType::Transport, wire)?;
    let receipt_hash = hex::encode(packet.hash().to_bytes());
    let outcome =
        ensure_sent(transport.send_packet_with_outcome(packet).await, "opportunistic send")?;
    Ok(InProcessSendReport {
        message_id,
        resolved_destination: destination.to_hex_string(),
        method: DeliveryMethod::Opportunistic,
        representation: DeliveryRepresentation::Packet,
        outcome,
        relay_destination: None,
        receipt_hash: Some(receipt_hash),
    })
}

async fn send_propagated(
    context: SendContext<'_>,
    request: &SendRequest,
    remote: &DestinationDesc,
    wire: &[u8],
    message_id: MessageId,
) -> Result<InProcessSendReport, SdkError> {
    let relay_hash = request
        .extensions
        .get(EXT_PROPAGATION_RELAY_HEX)
        .and_then(JsonValue::as_str)
        .map(parse_hash)
        .transpose()?
        .or(context.propagation_relay)
        .ok_or_else(|| transport_error("no propagation relay selected"))?;
    let relay = resolve_destination(
        context.transport,
        relay_hash,
        "propagation",
        context.link_connect_timeout,
    )
    .await?;
    let recipient = lxmf_core::identity::Identity::new_from_slices(
        remote.identity.public_key_bytes(),
        remote.identity.verifying_key_bytes(),
    );
    let propagated = WireMessage::unpack(wire)
        .and_then(|message| {
            let (lxmf_data, transient_id) =
                message.pack_propagation_transient_with_rng(&recipient, OsRng)?;
            // Issue #519: the trailing 32 bytes of the transient payload
            // are the LXMF propagation stamp — a proof-of-work anti-spam
            // value validated by the relay, not a salt/nonce. A fixed
            // all-zero stamp is rejected by nodes enforcing a stamp cost
            // (Python default minimum accepted: 13), so generate a real
            // stamp at the Python default target cost. Using the relay's
            // announced cost (pn_announce data) is a tracked follow-up.
            let stamp = lxmf_core::stamp::generate_propagation_stamp(
                &transient_id,
                lxmf_core::stamp::DEFAULT_PROPAGATION_STAMP_COST,
            )
            .ok_or_else(|| LxmfError::Encode("propagation stamp generation exhausted".into()))?;
            WireMessage::pack_propagation_envelope(
                crate::state::now_ms() as f64 / 1000.0,
                &lxmf_data,
                Some(&stamp),
            )
        })
        .map_err(|err| internal(format!("failed to encode propagated LXMF payload: {err}")))?;
    let representation = if propagated.len() > LXMF_MAX_PAYLOAD {
        MessageMethod::Resource
    } else {
        MessageMethod::Packet
    };
    send_link_payload(
        context.transport,
        relay,
        &propagated,
        message_id,
        DeliveryMethod::Propagated,
        representation,
        context.link_connect_timeout,
        context.link_connect_attempts,
        context.resource_transfer_timeout,
        Some(remote.address_hash.to_hex_string()),
    )
    .await
}

fn encode_wire(
    identity: &PrivateIdentity,
    source: AddressHash,
    remote: &DestinationDesc,
    request: &SendRequest,
) -> Result<Vec<u8>, SdkError> {
    let content = decode_required_base64(request, EXT_RAW_BYTES_BASE64, "content_base64")?;
    let fields = request
        .extensions
        .get(EXT_FIELDS_BASE64)
        .and_then(JsonValue::as_str)
        .map(|value| BASE64_STANDARD.decode(value).map_err(|_| validation("invalid fields base64")))
        .transpose()?
        .map(|bytes| {
            rmp_serde::from_slice(&bytes).map_err(|_| validation("invalid msgpack fields"))
        })
        .transpose()?;
    let title = request.payload.get("title").and_then(JsonValue::as_str).unwrap_or_default();
    let mut message = Message::new();
    message.source_hash = Some(copy_hash(source));
    message.destination_hash = Some(copy_hash(remote.address_hash));
    message.set_content_from_bytes(&content);
    message.set_title_from_string(title);
    message.fields = fields;
    let signer = lxmf_core::identity::PrivateIdentity::from_private_key_bytes(
        &identity.to_private_key_bytes(),
    )
    .map_err(|err| internal(format!("invalid local identity: {err:?}")))?;
    message
        .to_wire(Some(&signer))
        .map_err(|err| internal(format!("failed to encode LXMF message: {err}")))
}

pub(crate) async fn resolve_destination(
    transport: &Transport,
    hash: AddressHash,
    aspect: &str,
    timeout: Duration,
) -> Result<DestinationDesc, SdkError> {
    let identity = if let Some(identity) = transport.destination_identity(&hash).await {
        identity
    } else {
        transport.await_path(&hash, timeout, None).await;
        transport.destination_identity(&hash).await.ok_or_else(|| {
            transport_error(format!(
                "destination identity unavailable for {hash} after path resolution"
            ))
        })?
    };
    Ok(DestinationDesc { identity, name: DestinationName::new("lxmf", aspect), address_hash: hash })
}

pub(crate) fn requested_method(request: &SendRequest) -> Result<TransportMethod, SdkError> {
    if request
        .extensions
        .get(EXT_USE_PROPAGATION_NODE)
        .and_then(JsonValue::as_bool)
        .unwrap_or(false)
    {
        return Ok(TransportMethod::Propagated);
    }
    let value = request
        .extensions
        .get(EXT_SEND_MODE)
        .and_then(JsonValue::as_str)
        .or(request.delivery_method.as_deref())
        .unwrap_or("auto")
        .to_ascii_lowercase();
    match value.as_str() {
        "auto" | "opportunistic" => Ok(TransportMethod::Opportunistic),
        "direct" | "directonly" | "direct_only" => Ok(TransportMethod::Direct),
        "propagated" | "propagationonly" | "propagation_only" => Ok(TransportMethod::Propagated),
        _ => Err(validation(format!("unsupported delivery method: {value}"))),
    }
}

fn request_uses_auto_mode(request: &SendRequest) -> bool {
    request
        .extensions
        .get(EXT_SEND_MODE)
        .and_then(JsonValue::as_str)
        .or(request.delivery_method.as_deref())
        .is_none_or(|value| value.eq_ignore_ascii_case("auto"))
}

pub(crate) fn forced_representation(
    request: &SendRequest,
    method: TransportMethod,
    representation: MessageMethod,
    wire_len: usize,
) -> MessageMethod {
    if matches!(method, TransportMethod::Direct)
        && request
            .extensions
            .get(EXT_DIRECT_PACKET_MAX_WIRE_BYTES)
            .and_then(JsonValue::as_u64)
            .is_some_and(|limit| wire_len as u64 > limit)
    {
        MessageMethod::Resource
    } else {
        representation
    }
}

pub(crate) fn request_link_timeout(request: &SendRequest, fallback: Duration) -> Duration {
    let accepted = request
        .extensions
        .get(EXT_ACCEPTED_RESULT_ACK)
        .and_then(JsonValue::as_bool)
        .unwrap_or(false);
    request
        .extensions
        .get(EXT_LINK_CONNECT_TIMEOUT_MS)
        .and_then(JsonValue::as_u64)
        .map(Duration::from_millis)
        .unwrap_or_else(|| if accepted { Duration::from_secs(5) } else { fallback })
        .clamp(Duration::from_millis(1), Duration::from_secs(120))
}

pub(crate) fn request_link_attempts(request: &SendRequest, fallback: usize) -> usize {
    if request.extensions.get(EXT_ACCEPTED_RESULT_ACK).and_then(JsonValue::as_bool).unwrap_or(false)
    {
        1
    } else {
        fallback.max(1)
    }
}

pub(crate) fn request_resource_timeout(request: &SendRequest, fallback: Duration) -> Duration {
    if request.extensions.get(EXT_ACCEPTED_RESULT_ACK).and_then(JsonValue::as_bool).unwrap_or(false)
    {
        Duration::from_secs(8)
    } else {
        fallback
    }
}

fn decode_required_base64(
    request: &SendRequest,
    extension: &str,
    payload_key: &str,
) -> Result<Vec<u8>, SdkError> {
    let value = request
        .extensions
        .get(extension)
        .and_then(JsonValue::as_str)
        .or_else(|| request.payload.get(payload_key).and_then(JsonValue::as_str))
        .ok_or_else(|| validation("missing raw payload"))?;
    BASE64_STANDARD.decode(value).map_err(|_| validation("invalid payload base64"))
}

fn data_packet(
    destination: AddressHash,
    propagation_type: PropagationType,
    payload: &[u8],
) -> Result<Packet, SdkError> {
    Ok(Packet {
        header: Header {
            ifac_flag: IfacFlag::Open,
            header_type: HeaderType::Type1,
            context_flag: ContextFlag::Unset,
            propagation_type,
            destination_type: DestinationType::Single,
            packet_type: PacketType::Data,
            hops: 0,
        },
        ifac: None,
        destination,
        transport: None,
        context: PacketContext::None,
        data: PacketDataBuffer::new_from_slice(payload),
    })
}

fn parse_hash(value: &str) -> Result<AddressHash, SdkError> {
    let value = value.trim();
    if value.len() != 32 || !value.chars().all(|character| character.is_ascii_hexdigit()) {
        return Err(validation("invalid destination hash"));
    }
    AddressHash::new_from_hex_string(value).map_err(|_| validation("invalid destination hash"))
}

fn copy_hash(hash: AddressHash) -> [u8; 16] {
    let mut bytes = [0_u8; 16];
    bytes.copy_from_slice(hash.as_slice());
    bytes
}

fn ensure_sent(outcome: SendPacketOutcome, action: &str) -> Result<DeliveryOutcome, SdkError> {
    match outcome {
        SendPacketOutcome::SentDirect => Ok(DeliveryOutcome::SentDirect),
        SendPacketOutcome::SentBroadcast => Ok(DeliveryOutcome::SentBroadcast),
        _ => Err(transport_error(format!("{action} failed: {outcome:?}"))),
    }
}

fn validation(message: impl Into<String>) -> SdkError {
    SdkError::new(
        lxmf_sdk::error_code::VALIDATION_INVALID_ARGUMENT,
        lxmf_sdk::ErrorCategory::Validation,
        message,
    )
    .with_user_actionable(true)
}

pub(crate) fn transport_error(message: impl Into<String>) -> SdkError {
    SdkError::new(lxmf_sdk::error_code::INTERNAL, lxmf_sdk::ErrorCategory::Transport, message)
}

fn internal(message: impl Into<String>) -> SdkError {
    crate::state::internal_error(message)
}
