//! A retained send owner keeps admission until native cancellation is observed.
use crate::delivery::transport_error;
use crate::{
    DeliveryMethod, DeliveryOutcome, DeliveryRepresentation, InProcessBackendConfig,
    InProcessSendReport,
};
use lxmf_core::MessageMethod;
use lxmf_sdk::{MessageId, SdkError};
use rns_transport::{
    destination::{link::Link, DestinationDesc},
    resource::{ResourceCompletion, ResourceOutcome},
    transport::Transport,
};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{oneshot, Mutex};

pub(crate) type CompletionHook =
    Box<dyn FnOnce(&Result<InProcessSendReport, SdkError>) -> Result<(), SdkError> + Send + Sync>;
pub(crate) struct OwnedSend {
    pub starting: Option<Box<dyn FnOnce() -> Result<(), SdkError> + Send + Sync>>,
    pub config: Arc<InProcessBackendConfig>,
    pub destination: DestinationDesc,
    pub payload: Vec<u8>,
    pub message_id: MessageId,
    pub method: DeliveryMethod,
    pub representation: MessageMethod,
    pub timeout: Duration,
    pub link_connect_timeout: Duration,
    pub link_connect_attempts: usize,
    pub propagated_recipient: Option<String>,
    pub admission: Box<dyn Send + Sync>,
    pub completed: Option<CompletionHook>,
}

pub(crate) async fn send_owned(request: OwnedSend) -> Result<InProcessSendReport, SdkError> {
    let (cancel_tx, cancel_rx) = oneshot::channel();
    let (result_tx, result_rx) = oneshot::channel();
    tokio::spawn(async move {
        let mut request = request;
        let mut result = dispatch(&mut request, cancel_rx).await;
        if let Some(completed) = request.completed.take() {
            if let Err(error) = completed(&result) {
                result = Err(error);
            }
        }
        // This owner, rather than the caller's waiter, releases admission after cleanup.
        drop(request.admission);
        if result_tx.send(result).is_err() {
            log::debug!("[local-propagation] send waiter closed after native cleanup");
        }
    });
    let _cancel = CancelOnDrop(Some(cancel_tx));
    result_rx.await.map_err(|_| transport_error("retained send owner stopped"))?
}
struct CancelOnDrop(Option<oneshot::Sender<()>>);
impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        if let Some(tx) = self.0.take() {
            if tx.send(()).is_err() {
                log::debug!("[local-propagation] send owner already finished at cancellation");
            }
        }
    }
}

async fn dispatch(
    request: &mut OwnedSend,
    mut cancel: oneshot::Receiver<()>,
) -> Result<InProcessSendReport, SdkError> {
    let deadline = tokio::time::Instant::now() + request.timeout;
    let activation = crate::link_delivery::activate_link(
        &request.config.transport,
        request.destination,
        request.link_connect_timeout.min(request.timeout),
        request.link_connect_attempts,
    );
    let link = tokio::select! {
        result=activation=>result?,
        _=&mut cancel=>return Err(transport_error("send cancelled before link activation")),
        _=tokio::time::sleep_until(deadline)=>return Err(transport_error("link activation timed out")),
    };
    if let Some(starting) = request.starting.take() {
        starting()?;
    }
    let packet = if request.representation == MessageMethod::Resource {
        None
    } else {
        match link.lock().await.data_packet(&request.payload) {
            Ok(packet) => Some(packet),
            Err(
                rns_transport::error::RnsError::OutOfMemory
                | rns_transport::error::RnsError::InvalidArgument,
            ) => None,
            Err(error) => {
                return Err(transport_error(format!("link packet construction: {error:?}")))
            }
        }
    };
    let (representation, receipt_hash) = if let Some(packet) = packet {
        let hash = hex::encode(packet.hash().to_bytes());
        let outcome = tokio::select! {
            outcome=request.config.transport.send_link_packet_on_bound_iface(&link,packet)=>outcome,
            _=&mut cancel=>return Err(transport_error("packet send cancelled")),
            _=tokio::time::sleep_until(deadline)=>return Err(transport_error("packet send timed out")),
        };
        if !rns_transport::delivery::send_outcome_is_sent(outcome) {
            return Err(transport_error("link packet not sent"));
        }
        (DeliveryRepresentation::Packet, Some(hash))
    } else {
        send_resource(request, &link, &mut cancel, deadline).await?;
        (DeliveryRepresentation::Resource, None)
    };
    let destination = request.destination.address_hash.to_hex_string();
    Ok(InProcessSendReport {
        message_id: request.message_id.clone(),
        resolved_destination: request
            .propagated_recipient
            .clone()
            .unwrap_or_else(|| destination.clone()),
        method: request.method,
        representation,
        outcome: DeliveryOutcome::SentDirect,
        relay_destination: request.propagated_recipient.as_ref().map(|_| destination),
        receipt_hash,
    })
}

async fn send_resource(
    request: &OwnedSend,
    link: &Arc<Mutex<Link>>,
    cancel: &mut oneshot::Receiver<()>,
    deadline: tokio::time::Instant,
) -> Result<(), SdkError> {
    let transport = &request.config.transport;
    let mut registration = None;
    // Drop the registered send before cancelling it; driving both concurrently is invalid.
    let dispatch = {
        let send = transport.send_resource_on_link_observed_with_completion(
            link,
            request.payload.clone(),
            None,
            |hash, completion| registration = Some((hash, completion)),
        );
        tokio::pin!(send);
        tokio::select! {
            result=&mut send=>result.map(|_|()).map_err(|error|transport_error(format!("resource dispatch: {error:?}"))),
            _=&mut *cancel=>Err(transport_error("resource send cancelled")),
            _=tokio::time::sleep_until(deadline)=>Err(transport_error("resource send timed out")),
        }
    };
    let Some((hash, mut completion)) = registration else {
        return dispatch.and(Err(transport_error("resource not registered")));
    };
    let result = if dispatch.is_ok() {
        tokio::select! {
            outcome=completion.wait()=>match outcome {
                Ok(ResourceOutcome::Complete)=>Ok(()),
                Ok(outcome)=>Err(transport_error(format!("resource transfer {outcome:?}"))),
                Err(_)=>Err(transport_error("resource completion unavailable")),
            },
            _=&mut *cancel=>Err(transport_error("resource send cancelled")),
            _=tokio::time::sleep_until(deadline)=>Err(transport_error("resource send timed out")),
        }
    } else {
        dispatch
    };
    if result.is_err() {
        cancel_and_observe(transport, link, hash, &mut completion).await?;
    }
    result
}

async fn cancel_and_observe(
    transport: &Transport,
    link: &Arc<Mutex<Link>>,
    hash: rns_transport::hash::Hash,
    completion: &mut ResourceCompletion,
) -> Result<(), SdkError> {
    if completion.current().is_none() {
        let _ = transport.cancel_resource_on_link(link, hash).await;
    }
    if completion.current().is_none() {
        // Teardown observes native cleanup even if no cancel frame can be handed off.
        let mut locally_closed = false;
        let _ = transport.teardown_link_observed(link, || locally_closed = true).await;
        if locally_closed {
            return Ok(());
        }
    }
    if completion.current().is_none() {
        return Err(transport_error("resource cleanup could not be confirmed"));
    }
    Ok(())
}

#[cfg(test)]
#[path = "owned_link_delivery_tests.rs"]
mod tests;
