use super::remote_control_link::{
    build_link_identify_payload, build_link_request_payload, open_refreshed_remote_link,
    resolve_remote_identity, send_link_context_packet, wait_for_link_request_response,
};
use super::*;

pub(super) async fn remote_control_request(
    transport: &Transport,
    request_identity: &PrivateIdentity,
    remote: &str,
    path: &str,
    data: rmpv::Value,
    timeout: Duration,
) -> Result<(rmpv::Value, Identity), std::io::Error> {
    let remote_hash = AddressHash::new(parse_destination_hash_required(remote)?);
    let remote_identity = resolve_remote_identity(transport, &remote_hash, timeout).await?;
    let remote_identity = remote_identity.ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "no path known for propagation control node",
        )
    })?;

    let destination_name = if matches!(path, "/get" | "/offer") {
        DestinationName::new("lxmf", "propagation")
    } else {
        DestinationName::new("lxmf", "propagation.control")
    };
    let destination = SingleOutputDestination::new(remote_identity, destination_name);
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

    Ok((response, remote_identity))
}
