use super::remote_control_download::propagation_download_request;
use super::*;
use reticulum_daemon::lxmf_bridge::rmpv_to_json;
use rns_rpc::RemoteControlBridge;

use super::remote_fetch::{
    propagation_payload_ack_transient_id, rmpv_binary_array, LocalPropagationImportOutcome,
};
use super::remote_request::remote_control_request;

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

    fn propagation_remote_fetch(
        &self,
        remote: &str,
        identity_private_key_hex: Option<&str>,
        timeout_secs: f64,
        transfer_limit_kb: Option<f64>,
    ) -> Result<JsonValue, std::io::Error> {
        let available = self.run_remote_control_raw(
            remote,
            identity_private_key_hex,
            timeout_secs,
            "/get",
            rmpv::Value::Array(vec![rmpv::Value::Nil, rmpv::Value::Nil]),
        )?;
        let transient_ids = rmpv_binary_array(&available)?;
        if transient_ids.is_empty() {
            return Ok(propagation_remote_fetch_summary(0, &[], 0, 0, 0));
        }

        let fetched = self.run_remote_control_raw(
            remote,
            identity_private_key_hex,
            timeout_secs,
            "/get",
            rmpv::Value::Array(vec![
                rmpv::Value::Array(
                    transient_ids.iter().cloned().map(rmpv::Value::Binary).collect(),
                ),
                rmpv::Value::Nil,
                transfer_limit_kb
                    .map(rmpv::Value::F64)
                    .unwrap_or_else(|| rmpv::Value::from(10_240u64)),
            ]),
        )?;
        let payloads = rmpv_binary_array(&fetched)?;
        let daemon = self
            .daemon
            .lock()
            .ok()
            .and_then(|guard| guard.clone())
            .ok_or_else(|| std::io::Error::other("daemon unavailable"))?;

        let mut imported_count = 0usize;
        let mut duplicate_count = 0usize;
        let mut rejected_count = 0usize;
        let mut import_outcomes = Vec::with_capacity(payloads.len());
        for payload in &payloads {
            let outcome = self.accept_local_propagated_payload(daemon.clone(), payload.clone())?;
            match outcome {
                LocalPropagationImportOutcome::Imported => {
                    imported_count = imported_count.saturating_add(1);
                }
                LocalPropagationImportOutcome::Duplicate => {
                    duplicate_count = duplicate_count.saturating_add(1);
                }
                LocalPropagationImportOutcome::Rejected => {
                    rejected_count = rejected_count.saturating_add(1);
                }
            }
            import_outcomes.push((payload.as_slice(), outcome));
        }
        let ack_payload = propagation_remote_fetch_ack_payload(import_outcomes.as_slice());
        if ack_payload
            .as_array()
            .and_then(|entries| entries.get(1))
            .and_then(rmpv::Value::as_array)
            .is_some_and(|haves| !haves.is_empty())
        {
            let _ = self.run_remote_control_raw(
                remote,
                identity_private_key_hex,
                timeout_secs,
                "/get",
                ack_payload,
            )?;
        }

        Ok(propagation_remote_fetch_summary(
            transient_ids.len(),
            &payloads,
            imported_count,
            duplicate_count,
            rejected_count,
        ))
    }

    fn propagation_remote_download(
        &self,
        remote: &str,
        identity_private_key_hex: Option<&str>,
        timeout_secs: f64,
        transfer_limit_kb: Option<f64>,
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
                    transfer_limit_kb,
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

fn propagation_remote_fetch_summary(
    available_count: usize,
    payloads: &[Vec<u8>],
    imported_count: usize,
    duplicate_count: usize,
    rejected_count: usize,
) -> JsonValue {
    let transferred_bytes = payloads.iter().map(Vec::len).sum::<usize>();
    json!({
        "available_count": available_count,
        "fetched_count": payloads.len(),
        "imported_count": imported_count,
        "duplicate_count": duplicate_count,
        "rejected_count": rejected_count,
        "transferred_bytes": transferred_bytes,
    })
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
        .map(|(payload, _outcome)| {
            rmpv::Value::Binary(propagation_payload_ack_transient_id(payload))
        })
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

pub(super) fn response_to_result(response: rmpv::Value) -> Result<rmpv::Value, std::io::Error> {
    if let Some(error) = response_code_error(&response) {
        return Err(error);
    }
    Ok(response)
}

pub(super) fn response_code_error(response: &rmpv::Value) -> Option<std::io::Error> {
    if let Some(code) = response.as_u64().or_else(|| response.as_i64().map(|value| value as u64)) {
        let (kind, message) = match code as u8 {
            0xF0 => (std::io::ErrorKind::PermissionDenied, "propagation node requires identity"),
            0xF1 => (std::io::ErrorKind::PermissionDenied, "propagation node denied access"),
            0xF3 => (std::io::ErrorKind::PermissionDenied, "propagation peer invalid peering key"),
            0xF4 => (std::io::ErrorKind::InvalidInput, "propagation node rejected the request"),
            0xF5 => (std::io::ErrorKind::PermissionDenied, "propagation peer invalid stamp"),
            0xF6 => (std::io::ErrorKind::WouldBlock, "propagation peer throttled"),
            0xFD => (std::io::ErrorKind::NotFound, "propagation peer not found"),
            0xFE => (std::io::ErrorKind::TimedOut, "propagation peer timed out"),
            _ => (std::io::ErrorKind::InvalidData, "unexpected propagation control response"),
        };
        return Some(std::io::Error::new(kind, message));
    }
    None
}

#[cfg(test)]
#[path = "bridge_remote_control_tests.rs"]
mod tests;
