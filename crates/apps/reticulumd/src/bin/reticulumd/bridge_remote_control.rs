use super::remote_control_download::propagation_download_request;
use super::remote_control_link::{
    build_link_identify_payload, build_link_request_payload, open_refreshed_remote_link,
    resolve_remote_identity, send_link_context_packet, wait_for_link_request_response,
};
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

async fn remote_control_request(
    transport: &Transport,
    request_identity: &PrivateIdentity,
    remote: &str,
    path: &str,
    data: rmpv::Value,
    timeout: Duration,
) -> Result<(JsonValue, Identity), std::io::Error> {
    let remote_hash = AddressHash::new(parse_destination_hash_required(remote)?);
    let remote_identity = resolve_remote_identity(transport, &remote_hash, timeout).await?;
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
