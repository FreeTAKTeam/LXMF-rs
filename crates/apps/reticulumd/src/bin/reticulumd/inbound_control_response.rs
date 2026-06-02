use super::*;

pub(super) enum ControlResponse {
    Code(u8),
    Bool(bool),
    Rmpv(rmpv::Value),
    Value(Value),
}

pub(super) async fn send_control_response(
    transport: &Transport,
    link_id: &AddressHash,
    request_id: [u8; 16],
    response: ControlResponse,
) -> Result<(), std::io::Error> {
    let Some(link) = transport.find_in_link(link_id).await else {
        return Err(std::io::Error::other("control link not found"));
    };
    let response_value = match response {
        ControlResponse::Code(code) => rmpv::Value::from(code),
        ControlResponse::Bool(value) => rmpv::Value::Boolean(value),
        ControlResponse::Rmpv(value) => value,
        ControlResponse::Value(value) => json_to_rmpv(&value),
    };
    let frame = rmpv::Value::Array(vec![rmpv::Value::Binary(request_id.to_vec()), response_value]);
    let payload = rmp_serde::to_vec(&frame).map_err(std::io::Error::other)?;
    let (packet, ingress_iface) = build_link_response_packet(&link, payload.as_slice()).await?;
    let Some(ingress_iface) = ingress_iface else {
        return Err(std::io::Error::other("control link ingress interface unavailable"));
    };
    match packet {
        LinkResponsePacket::Direct(packet) => {
            transport.send_direct(ingress_iface, *packet).await;
            Ok(())
        }
        LinkResponsePacket::Resource(payload) => transport
            .send_resource_direct(link_id, ingress_iface, payload, None)
            .await
            .map(|_| ())
            .map_err(|err| std::io::Error::other(format!("{err:?}"))),
    }
}

enum LinkResponsePacket {
    Direct(Box<Packet>),
    Resource(Vec<u8>),
}

async fn build_link_response_packet(
    link: &Arc<tokio::sync::Mutex<Link>>,
    payload: &[u8],
) -> Result<(LinkResponsePacket, Option<AddressHash>), std::io::Error> {
    let guard = link.lock().await;
    let ingress_iface = guard.ingress_iface();
    let mut packet_data = PacketDataBuffer::new();
    let cipher_len = match guard.encrypt(payload, packet_data.accuire_buf_max()) {
        Ok(ciphertext) => ciphertext.len(),
        Err(_) => return Ok((LinkResponsePacket::Resource(payload.to_vec()), ingress_iface)),
    };
    packet_data.resize(cipher_len);
    Ok((
        LinkResponsePacket::Direct(Box::new(Packet {
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
        })),
        ingress_iface,
    ))
}

fn json_to_rmpv(value: &Value) -> rmpv::Value {
    match value {
        Value::Null => rmpv::Value::Nil,
        Value::Bool(value) => rmpv::Value::Boolean(*value),
        Value::Number(value) => {
            if let Some(value) = value.as_i64() {
                rmpv::Value::from(value)
            } else if let Some(value) = value.as_u64() {
                rmpv::Value::from(value)
            } else if let Some(value) = value.as_f64() {
                rmpv::Value::F64(value)
            } else {
                rmpv::Value::Nil
            }
        }
        Value::String(value) => rmpv::Value::from(value.as_str()),
        Value::Array(values) => rmpv::Value::Array(values.iter().map(json_to_rmpv).collect()),
        Value::Object(map) => rmpv::Value::Map(
            map.iter()
                .map(|(key, value)| (rmpv::Value::from(key.as_str()), json_to_rmpv(value)))
                .collect(),
        ),
    }
}
