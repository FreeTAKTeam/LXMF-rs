use std::future::Future;
use std::sync::Arc;
use std::time::Duration;

use lxmf_core::MessageMethod;
use lxmf_sdk::{MessageId, SdkError};
use rns_transport::delivery::{await_link_activation, send_on_link, LinkSendResult};
use rns_transport::destination::link::Link;
use rns_transport::destination::DestinationDesc;
use rns_transport::resource::ResourceEventKind;
use rns_transport::transport::Transport;
use tokio::sync::Mutex;

use crate::delivery::{
    transport_error, DeliveryMethod, DeliveryOutcome, DeliveryRepresentation, InProcessSendReport,
};

#[expect(
    clippy::too_many_arguments,
    reason = "transport send contract keeps routing and timeout choices explicit"
)]
pub(crate) async fn send_link_payload(
    transport: &Transport,
    destination: DestinationDesc,
    payload: &[u8],
    message_id: MessageId,
    method: DeliveryMethod,
    representation: MessageMethod,
    link_timeout: Duration,
    link_attempts: usize,
    resource_timeout: Duration,
    propagated_recipient: Option<String>,
) -> Result<InProcessSendReport, SdkError> {
    let resolved_destination = destination.address_hash.to_hex_string();
    let link = activate_link(transport, destination, link_timeout, link_attempts).await?;
    let mut resource_events = transport.resource_events();
    let (actual_representation, receipt_hash) = if matches!(representation, MessageMethod::Resource)
    {
        let link_id = *link.lock().await.id();
        let resource_hash = transport
            .send_resource(&link_id, payload.to_vec(), None)
            .await
            .map_err(|err| transport_error(format!("resource send failed: {err:?}")))?;
        await_resource_completion_with_cancel(
            &mut resource_events,
            resource_hash,
            resource_timeout,
            cancel_resource(transport, link_id, resource_hash),
        )
        .await?;
        (DeliveryRepresentation::Resource, None)
    } else {
        match send_on_link(transport, &link, payload)
            .await
            .map_err(|err| transport_error(format!("link send failed: {err}")))?
        {
            LinkSendResult::Packet(packet) => {
                (DeliveryRepresentation::Packet, Some(hex::encode(packet.hash().to_bytes())))
            }
            LinkSendResult::Resource(resource_hash) => {
                let link_id = *link.lock().await.id();
                await_resource_completion_with_cancel(
                    &mut resource_events,
                    resource_hash,
                    resource_timeout,
                    cancel_resource(transport, link_id, resource_hash),
                )
                .await?;
                (DeliveryRepresentation::Resource, None)
            }
        }
    };
    Ok(InProcessSendReport {
        message_id,
        resolved_destination: propagated_recipient.unwrap_or(resolved_destination.clone()),
        method,
        representation: actual_representation,
        outcome: DeliveryOutcome::SentDirect,
        relay_destination: matches!(method, DeliveryMethod::Propagated)
            .then_some(resolved_destination),
        receipt_hash,
    })
}

async fn activate_link(
    transport: &Transport,
    destination: DestinationDesc,
    timeout: Duration,
    attempts: usize,
) -> Result<Arc<Mutex<Link>>, SdkError> {
    let mut last_error = None;
    for _ in 0..attempts.max(1) {
        let link = transport.link(destination).await;
        match await_link_activation(transport, &link, timeout).await {
            Ok(()) => return Ok(link),
            Err(error) => {
                last_error = Some(error);
                transport.reset_out_link(&destination.address_hash).await;
            }
        }
    }
    Err(transport_error(format!(
        "link activation failed: {}",
        last_error.map_or_else(|| "no attempts made".to_owned(), |error| error.to_string())
    )))
}

async fn cancel_resource(
    transport: &Transport,
    link_id: rns_transport::hash::AddressHash,
    resource_hash: rns_transport::hash::Hash,
) -> Result<(), SdkError> {
    transport.cancel_resource(&link_id, resource_hash).await.map(|_| ()).map_err(|error| {
        transport_error(format!(
            "resource cancellation failed link={link_id} hash={resource_hash}: {error}"
        ))
    })
}

pub(crate) async fn await_resource_completion_with_cancel<F>(
    events: &mut tokio::sync::broadcast::Receiver<rns_transport::resource::ResourceEvent>,
    resource_hash: rns_transport::hash::Hash,
    timeout: Duration,
    cancel: F,
) -> Result<(), SdkError>
where
    F: Future<Output = Result<(), SdkError>>,
{
    let result = await_resource_completion(events, resource_hash, timeout).await;
    if let Err(transfer_error) = result {
        return match cancel.await {
            Ok(()) => Err(transfer_error),
            Err(cancel_error) => Err(transport_error(format!(
                "{}; cleanup failed: {}",
                transfer_error.message, cancel_error.message
            ))),
        };
    }
    Ok(())
}

pub(crate) async fn await_resource_completion(
    events: &mut tokio::sync::broadcast::Receiver<rns_transport::resource::ResourceEvent>,
    resource_hash: rns_transport::hash::Hash,
    timeout: Duration,
) -> Result<(), SdkError> {
    let deadline = tokio::time::Instant::now() + timeout;
    loop {
        let remaining = deadline.saturating_duration_since(tokio::time::Instant::now());
        if remaining.is_zero() {
            return Err(transport_error("resource transfer timed out"));
        }
        match tokio::time::timeout(remaining, events.recv()).await {
            Ok(Ok(event)) if event.hash != resource_hash => continue,
            Ok(Ok(event)) => match event.kind {
                ResourceEventKind::OutboundComplete => return Ok(()),
                ResourceEventKind::OutboundFailed => {
                    return Err(transport_error("resource transfer failed"))
                }
                ResourceEventKind::OutboundCancelled => {
                    return Err(transport_error("resource transfer cancelled"))
                }
                _ => continue,
            },
            Ok(Err(tokio::sync::broadcast::error::RecvError::Lagged(_))) => continue,
            Ok(Err(tokio::sync::broadcast::error::RecvError::Closed)) => {
                return Err(transport_error("resource event stream closed"));
            }
            Err(_) => return Err(transport_error("resource transfer timed out")),
        }
    }
}
