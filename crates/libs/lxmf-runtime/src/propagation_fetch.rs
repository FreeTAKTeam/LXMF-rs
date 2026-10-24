//! Standard recipient `/get`, with retained native ownership and receiver-confirmed custody.
use crate::{delivery::transport_error, InProcessBackendConfig};
use lxmf_core::WireMessage;
use lxmf_sdk::SdkError;
use rns_transport::{
    destination::link::{unpack_response_envelope, Link},
    hash::{AddressHash, Hash},
    transport::Transport,
};
use std::{collections::HashSet, future::Future, sync::Arc, time::Duration};
use tokio::sync::{oneshot, Mutex, OwnedSemaphorePermit};

#[cfg(test)]
#[path = "propagation_fetch_tests.rs"]
mod tests;

#[derive(Clone, Copy, Debug)]
pub struct PropagationFetchLimits {
    pub max_messages: usize,
    /// Applies before native Resource acceptance, including the response envelope.
    pub max_response_bytes: usize,
    pub request_timeout: Duration,
    pub link_timeout: Duration,
    pub link_attempts: usize,
}

#[derive(Default, Debug)]
pub struct PropagationFetchReport {
    pub available: usize,
    pub fetched: usize,
    pub imported: usize,
    pub confirmed: usize,
    pub failed: usize,
    pub deferred: usize,
}

pub(crate) async fn fetch_retained<F, Fut>(
    config: Arc<InProcessBackendConfig>,
    relay: AddressHash,
    limits: PropagationFetchLimits,
    admission: Option<OwnedSemaphorePermit>,
    import: F,
) -> Result<PropagationFetchReport, SdkError>
where
    F: FnMut(Vec<u8>) -> Fut + Send + 'static,
    Fut: Future<Output = bool> + Send,
{
    if limits.max_response_bytes < 64 || limits.request_timeout.is_zero() {
        return Err(transport_error("invalid propagation fetch limits"));
    }
    let (tx, rx) = oneshot::channel();
    tokio::spawn(async move {
        // The task owns admission even if the caller drops its waiter.
        let mut admission = admission;
        let result = fetch_session(&config, relay, limits, &mut admission, import).await;
        if tx.send(result).is_err() {
            log::debug!("propagation fetch waiter closed after native cleanup");
        }
    });
    rx.await.map_err(|_| transport_error("propagation fetch owner unavailable"))?
}

async fn fetch_session<F, Fut>(
    config: &InProcessBackendConfig,
    relay: AddressHash,
    limits: PropagationFetchLimits,
    admission: &mut Option<OwnedSemaphorePermit>,
    import: F,
) -> Result<PropagationFetchReport, SdkError>
where
    F: FnMut(Vec<u8>) -> Fut,
    Fut: Future<Output = bool>,
{
    let destination = crate::delivery::resolve_destination(
        &config.transport,
        relay,
        "propagation",
        limits.link_timeout,
    )
    .await?;
    let link = crate::link_delivery::activate_link(
        &config.transport,
        destination,
        limits.link_timeout,
        limits.link_attempts,
    )
    .await?;
    let result = retrieve_on_link(config, &link, limits, import).await;
    // Local cleanup precedes optional teardown-frame handoff and permit release.
    let mut locally_closed = false;
    let cleanup = config.transport.teardown_link_observed(&link, || {
        locally_closed = true;
        drop(admission.take());
    });
    match tokio::time::timeout(limits.request_timeout, cleanup).await {
        Ok(Ok(_)) => {}
        Ok(Err(error)) => log::debug!("propagation fetch teardown frame: {error:?}"),
        Err(_) if locally_closed => log::debug!("propagation fetch teardown frame timed out"),
        Err(_) => {
            // Never release admission while native work can still use this exact Link.
            let cleanup =
                config.transport.teardown_link_observed(&link, || drop(admission.take())).await;
            if let Err(error) = cleanup {
                log::debug!("propagation fetch cleanup frame: {error:?}");
            }
        }
    }
    result
}

async fn retrieve_on_link<F, Fut>(
    config: &InProcessBackendConfig,
    link: &Arc<Mutex<Link>>,
    limits: PropagationFetchLimits,
    mut import: F,
) -> Result<PropagationFetchReport, SdkError>
where
    F: FnMut(Vec<u8>) -> Fut,
    Fut: Future<Output = bool>,
{
    let packet = {
        let guard = link.lock().await;
        guard
            .identify_packet(&guard.identify_payload(&config.identity))
            .map_err(|error| transport_error(format!("propagation identify: {error:?}")))?
    };
    let sent = tokio::time::timeout(
        limits.request_timeout,
        config.transport.send_link_packet_on_bound_iface(link, packet),
    )
    .await
    .map_err(|_| transport_error("propagation identify timed out"))?;
    if !rns_transport::delivery::send_outcome_is_sent(sent) {
        return Err(transport_error("propagation identify not sent"));
    }
    let inventory = request(
        &config.transport,
        link,
        limits,
        rmpv::Value::Array(vec![rmpv::Value::Nil, rmpv::Value::Nil]),
    )
    .await?;
    let ids = inventory_ids(inventory)?;
    let mut report = PropagationFetchReport { available: ids.len(), ..Default::default() };
    let recipient = lxmf_core::identity::PrivateIdentity::from_private_key_bytes(
        &config.identity.to_private_key_bytes(),
    )
    .map_err(|error| transport_error(format!("propagation recipient identity: {error}")))?;
    for id in ids.into_iter().take(limits.max_messages) {
        let response = request(
            &config.transport,
            link,
            limits,
            rmpv::Value::Array(vec![
                rmpv::Value::Array(vec![rmpv::Value::Binary(id.clone())]),
                rmpv::Value::Nil,
                // Decimal kB is the standard LXMF wire unit; cap Resource bytes independently.
                rmpv::Value::F64(
                    f64::from(u32::try_from(limits.max_response_bytes).unwrap_or(u32::MAX))
                        / 1000.0,
                ),
            ]),
        )
        .await?;
        let Some(payload) = single_payload(response, &id)? else {
            report.deferred += 1;
            continue;
        };
        report.fetched += 1;
        let wire = match WireMessage::decrypt_propagation(
            &payload,
            &recipient,
            config.source_destination.as_slice(),
        ) {
            Ok(wire) => wire,
            Err(error) => {
                log::warn!("propagated message retained after decode failure: {error}");
                report.failed += 1;
                continue;
            }
        };
        if !source_verified(config, &wire, limits.link_timeout).await {
            report.failed += 1;
            continue;
        }
        if !import(wire).await {
            report.failed += 1;
            continue;
        }
        report.imported += 1;
        let confirmed = request(
            &config.transport,
            link,
            limits,
            rmpv::Value::Array(vec![
                rmpv::Value::Nil,
                rmpv::Value::Array(vec![rmpv::Value::Binary(id)]),
            ]),
        )
        .await?;
        if !matches!(confirmed, rmpv::Value::Boolean(true))
            && !matches!(confirmed, rmpv::Value::Array(ref values) if values.is_empty())
        {
            return Err(transport_error("relay did not confirm recipient haves"));
        }
        report.confirmed += 1;
    }
    Ok(report)
}

async fn source_verified(config: &InProcessBackendConfig, wire: &[u8], timeout: Duration) -> bool {
    let Ok(message) = WireMessage::unpack(wire) else {
        return false;
    };
    let source = if message.source.as_slice() == config.source_destination.as_slice() {
        *config.identity.as_identity()
    } else {
        let Ok(source) = crate::delivery::resolve_destination(
            &config.transport,
            AddressHash::new(message.source),
            "delivery",
            timeout,
        )
        .await
        else {
            log::warn!("propagated message retained: sender identity unavailable");
            return false;
        };
        source.identity
    };
    let identity = lxmf_core::identity::Identity::new_from_slices(
        source.public_key_bytes(),
        source.verifying_key_bytes(),
    );
    lxmf_propagation::prepared::delivery_address(&identity) == message.source
        && matches!(message.verify(&identity), Ok(true))
}

fn inventory_ids(value: rmpv::Value) -> Result<Vec<Vec<u8>>, SdkError> {
    let rmpv::Value::Array(values) = value else {
        return Err(transport_error("invalid propagation inventory"));
    };
    let mut ids = Vec::with_capacity(values.len());
    let mut seen = HashSet::with_capacity(values.len());
    for value in values {
        let rmpv::Value::Binary(id) = value else {
            return Err(transport_error("invalid propagation inventory id"));
        };
        if id.len() != 32 {
            return Err(transport_error("invalid propagation inventory id length"));
        }
        if seen.insert(id.clone()) {
            ids.push(id);
        }
    }
    Ok(ids)
}

fn single_payload(value: rmpv::Value, id: &[u8]) -> Result<Option<Vec<u8>>, SdkError> {
    let mut values = crate::propagation_fetch_decode::payloads(&value)?;
    if values.is_empty() {
        return Ok(None);
    }
    if values.len() != 1 {
        return Err(transport_error("unexpected propagation message count"));
    }
    let Some(payload) = values.pop() else {
        return Err(transport_error("invalid propagation message"));
    };
    let matches = Hash::new_from_slice(&payload).as_slice() == id
        || payload
            .len()
            .checked_sub(32)
            .is_some_and(|len| Hash::new_from_slice(&payload[..len]).as_slice() == id);
    if !matches {
        return Err(transport_error("propagation transient id mismatch"));
    }
    Ok(Some(payload))
}

async fn request(
    transport: &Transport,
    link: &Arc<Mutex<Link>>,
    limits: PropagationFetchLimits,
    data: rmpv::Value,
) -> Result<rmpv::Value, SdkError> {
    let payload = Link::request_payload("/get", data)
        .map_err(|error| transport_error(format!("propagation request encoding: {error:?}")))?;
    let mut receipt = None;
    let dispatch = transport.send_request_on_link_observed(
        link,
        payload,
        limits.request_timeout,
        Some(limits.max_response_bytes),
        |registered| receipt = Some(registered),
    );
    let dispatch = tokio::time::timeout(limits.request_timeout, dispatch).await;
    match dispatch {
        Ok(Ok(_)) => {}
        failure => {
            return Err(transport_error(format!("propagation request dispatch: {failure:?}")))
        }
    }
    let mut receipt =
        receipt.ok_or_else(|| transport_error("propagation request not registered"))?;
    let response = tokio::time::timeout(limits.request_timeout, receipt.wait())
        .await
        .map_err(|_| transport_error("propagation request timed out"))?
        .map_err(|error| transport_error(format!("propagation response: {error}")))?;
    // Native RequestReceipt already enforces this before accepting Resource payload.
    if response.data.len() > limits.max_response_bytes {
        return Err(transport_error("propagation response exceeds configured bound"));
    }
    let (request_id, value) = unpack_response_envelope(&response.data)
        .map_err(|error| transport_error(format!("propagation response envelope: {error:?}")))?;
    if request_id.as_slice() != receipt.request_id() {
        return Err(transport_error("propagation response request id mismatch"));
    }
    Ok(value)
}
