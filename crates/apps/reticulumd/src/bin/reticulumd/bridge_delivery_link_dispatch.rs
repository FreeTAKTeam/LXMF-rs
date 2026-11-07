use super::*;
use crate::outbound_resources::{retire_owned_resource, track_owned_resource};
use rns_transport::error::RnsError;
use rns_transport::hash::Hash;
use rns_transport::resource::ResourceCompletion;
use rns_transport::transport::LinkPacketReceipt;

pub(super) struct PacketAttempt {
    pub(super) receipt: LinkPacketReceipt,
    pub(super) event: ReceiptEvent,
}
pub(super) enum Transfer {
    Packet {
        link: Arc<Mutex<Link>>,
        receipt: LinkPacketReceipt,
    },
    Resource {
        link: Arc<Mutex<Link>>,
        link_id: AddressHash,
        hash: Hash,
        completion: ResourceCompletion,
        peer: String,
        bytes: usize,
        method: Method,
        registration_error: Option<String>,
    },
    NoReceipt {
        link: Arc<Mutex<Link>>,
    },
}
impl Transfer {
    pub(super) async fn changed(&mut self) -> std::io::Result<()> {
        match self {
            Self::Packet { receipt, .. } => receipt.wait_proved().await,
            Self::Resource { completion, .. } => {
                completion.wait().await.map(|_| ()).map_err(std::io::Error::other)
            }
            Self::NoReceipt { .. } => std::future::pending().await,
        }
    }
    pub(super) async fn cancel(self, task: &DeliveryTask) {
        if let Self::Resource { link, link_id, hash, mut completion, .. } = self {
            let mut stopping = task.stopping.clone();
            // A native terminal already consumed by this owner needs no cancel.
            let mut cleaned = if completion.current().is_some() {
                true
            } else {
                let cancel = task.transport.cancel_resource_on_link(&link, hash);
                tokio::pin!(cancel);
                tokio::select! {
                    biased;
                    result = &mut cancel => match result {
                        Ok(_) => completion.current().is_some(),
                        Err(error) => {
                            log::warn!("owned Resource cleanup failed message_id={} hash={hash}: {error}", task.message_id);
                            completion.current().is_some()
                        }
                    },
                    result = completion.wait() => match result {
                        Ok(_) => true, // local cleanup precedes optional frame handoff
                        Err(error) => {
                            log::error!("Resource cleanup observation unavailable message_id={} hash={hash}: {error}", task.message_id);
                            tokio::select! {
                                result = &mut cancel => match result {
                                    Ok(_) => completion.current().is_some(),
                                    Err(error) => { log::error!("Resource cleanup failed message_id={} hash={hash}: {error}", task.message_id); false }
                                },
                                _ = wait_stopping(&mut stopping) => false,
                            }
                        }
                    },
                    _ = wait_stopping(&mut stopping) => false,
                }
            };
            let same_link = if cleaned {
                false
            } else {
                tokio::select! {
                    biased;
                    guard = link.lock() => *guard.id() == link_id,
                    _ = wait_stopping(&mut stopping) => false,
                }
            };
            if !cleaned && same_link {
                // A peer close can clear encryption keys before maintenance
                // removes its Resource; local teardown needs no cancel cipher.
                super::teardown(task, &link).await;
                cleaned = completion.current().is_some();
            }
            if !cleaned {
                log::error!(
                    "Resource cleanup is unresolved message_id={} hash={hash}; retaining owner",
                    task.message_id
                );
                tokio::select! {
                    result = completion.wait() => match result {
                        Ok(_) => cleaned = true,
                        Err(error) => {
                            log::error!("Resource cleanup producer lost message_id={} hash={hash}: {error}", task.message_id);
                            wait_stopping(&mut stopping).await;
                        }
                    },
                    _ = wait_stopping(&mut stopping) => {},
                }
            }
            #[cfg(test)]
            if let Some(control) = task.wakeup.trace() {
                control.observation.lock().expect("native cleanup observation").resource_outcome =
                    completion.current();
            }
            if cleaned {
                if let Err(error) = retire_owned_resource(
                    &task.outbound_resource_map,
                    &hash.to_string(),
                    &task.message_id,
                ) {
                    log::error!(
                        "owned Resource retirement failed message_id={}: {error}",
                        task.message_id
                    );
                }
            } else {
                log::error!("Resource remains native-owned after cleanup failure message_id={} hash={hash}; correlation retained", task.message_id);
            }
        }
    }
}

pub(super) fn accepting(
    task: &DeliveryTask,
    stopping: &watch::Receiver<bool>,
) -> std::io::Result<bool> {
    if *stopping.borrow() || stopping.has_changed().is_err() {
        return Ok(false);
    }
    task.observe_delivery_terminal().map(|terminal| !terminal)
}

/// The caller holds execution permits only through this active send. Every
/// registered Resource remains observable even when handoff fails/interruption
/// drops the future. The caller then cleans it up outside those permits.
pub(super) async fn dispatch(
    task: &DeliveryTask,
    context: &Context,
    payload: &[u8],
    packets: &mut Vec<PacketAttempt>,
    stopping: &mut watch::Receiver<bool>,
) -> std::io::Result<Option<Transfer>> {
    match accepting(task, stopping) {
        Ok(true) => {}
        Ok(false) => return Ok(None),
        Err(error) => {
            log::warn!("pre-dispatch status unavailable message_id={}: {error}", task.message_id);
            return Ok(None);
        }
    }
    let Some((packet, link_id)) = super::while_ready(task, async {
        let guard = context.link.lock().await;
        if guard.status() != LinkStatus::Active {
            return None;
        }
        Some((guard.data_packet(payload), *guard.id()))
    })
    .await
    .flatten() else {
        return Ok(None);
    };
    #[cfg(test)]
    if let Some(control) = task.wakeup.trace() {
        use crate::bridge::delivery_wakeup::trace_control::{DispatchObservation, ProducerFault};
        {
            let mut observation = control.observation.lock().expect("dispatch context");
            observation.link = Some(context.link.clone());
            observation.creator = context.signal.is_some();
        }
        control.dispatches.lock().expect("trace dispatches").push(DispatchObservation {
            link: context.link.clone(),
            bytes: payload.to_vec(),
            resource: packet.is_err(),
        });
        match std::mem::take(&mut *control.fault.lock().expect("trace producer fault")) {
            ProducerFault::Error => {
                return Err(std::io::Error::other("controlled producer send failure"))
            }
            ProducerFault::NoReceipt => {
                return Ok(Some(Transfer::NoReceipt { link: context.link.clone() }))
            }
            ProducerFault::None => {}
        }
    }
    match packet {
        Ok(packet) => {
            let mut tracked = None;
            let mut registration_error = None;
            let mut awaiting_status = false;
            let result = {
                let send = task.transport.send_link_packet_on_bound_iface_observed(
                    &context.link,
                    packet,
                    |receipt| {
                        match accepting(task, stopping) {
                            Ok(true) => {}
                            Ok(false) => return false,
                            Err(error) => {
                                log::warn!("packet admission status unavailable message_id={}: {error}", task.message_id);
                                awaiting_status = true;
                                return false;
                            }
                        }
                        let event = context
                            .method
                            .completion(&task.message_id, false)
                            .with_peer(&context.peer)
                            .with_method(context.method.name())
                            .with_delivery_kind("link-packet")
                            .with_bytes(payload.len())
                            .with_packet_hash(hex::encode(receipt.packet_hash().to_bytes()))
                            .with_link_id(hex::encode(receipt.link_id().as_slice()));
                        match task.receipt_tx.track_link_packet(event.clone(), receipt.clone()) {
                            Ok(inserted) => {
                                // A terminal commit can race the first check and
                                // prune before insertion. Reconcile before handoff.
                                match accepting(task, stopping) {
                                    Ok(true) => {
                                        tracked = Some((receipt, event, inserted));
                                        true
                                    }
                                    result => {
                                        if inserted {
                                            if let Err(error) = task.receipt_tx.forget_link_packet(
                                                receipt.packet_hash().to_bytes(),
                                                &task.message_id,
                                            ) {
                                                registration_error = Some(error);
                                            }
                                        }
                                        if let Err(error) = result {
                                            log::warn!("packet admission status changed message_id={}: {error}", task.message_id);
                                            awaiting_status = true;
                                        }
                                        false
                                    }
                                }
                            }
                            Err(error) => {
                                registration_error = Some(error);
                                false
                            }
                        }
                    },
                );
                tokio::pin!(send);
                loop {
                    tokio::select! {
                        result = &mut send => break Some(result),
                        _ = tokio::time::sleep(Duration::from_millis(50)) => {
                            // The send owns the callback's stopping borrow.
                            if stopping_during_send(task) { break None; }
                            if task.stopping.as_ref().is_some_and(|s| *s.borrow() || s.has_changed().is_err()) { break None; }
                        }
                    }
                }
            };
            if let Some(error) = registration_error {
                return Err(error);
            }
            if awaiting_status {
                return Ok(None);
            }
            let Some((receipt, event, inserted)) = tracked else {
                return match result {
                    Some(Err(error)) => Err(std::io::Error::other(error)),
                    _ => Ok(None),
                };
            };
            let handed_off = receipt.sent_at().is_some();
            if !handed_off {
                if inserted {
                    task.receipt_tx
                        .forget_link_packet(receipt.packet_hash().to_bytes(), &task.message_id)?;
                }
                return match result {
                    None | Some(Ok(None)) => Ok(None),
                    Some(Ok(Some(_))) => {
                        Ok(Some(Transfer::NoReceipt { link: context.link.clone() }))
                    }
                    Some(Err(error)) => Err(std::io::Error::other(error)),
                };
            }
            task.daemon.record_outbound_peer_sent(&context.peer, payload.len());
            packets.push(PacketAttempt { receipt: receipt.clone(), event });
            // Retire a registration made while a prior terminal was committing.
            match task.daemon.message_receipt_status(&task.message_id) {
                Ok(status)
                    if status.as_deref().is_some_and(DeliveryTask::is_terminal_status)
                        && inserted =>
                {
                    if let Err(error) = task
                        .receipt_tx
                        .forget_link_packet(receipt.packet_hash().to_bytes(), &task.message_id)
                    {
                        log::error!(
                            "terminal packet retirement unavailable message_id={}: {error}",
                            task.message_id
                        );
                    }
                }
                Err(error) => log::warn!(
                    "accepted packet status unavailable message_id={}; retaining proof: {error}",
                    task.message_id
                ),
                _ => {}
            }
            Ok(Some(Transfer::Packet { link: context.link.clone(), receipt }))
        }
        Err(RnsError::OutOfMemory | RnsError::InvalidArgument) => {
            let mut observed = None;
            let mut tracking_error = None;
            let result = {
                let send = task.transport.send_resource_on_link_observed_with_completion(
                    &context.link,
                    payload.to_vec(),
                    None,
                    |hash, completion| {
                        tracking_error = track_owned_resource(
                            &task.outbound_resource_map,
                            hash.to_string(),
                            task.message_id.clone(),
                        )
                        .err();
                        observed = Some(Transfer::Resource {
                            link: context.link.clone(),
                            link_id,
                            hash,
                            completion,
                            peer: context.peer.clone(),
                            bytes: payload.len(),
                            method: context.method,
                            registration_error: None,
                        });
                    },
                );
                tokio::pin!(send);
                loop {
                    tokio::select! {
                        result = &mut send => break Some(result),
                        _ = tokio::time::sleep(Duration::from_millis(50)) => {
                            if stopping_during_send(task) { break None; }
                        }
                    }
                }
                // Drop the send before any explicit cancellation of its observed
                // registration; queuing and native confirmation may be separated.
            };
            if observed.is_none() {
                return match result {
                    Some(result) => result.map(|_| None).map_err(std::io::Error::other),
                    None => Ok(None),
                };
            }
            if let (Some(error), Some(Transfer::Resource { registration_error, .. })) =
                (tracking_error, observed.as_mut())
            {
                *registration_error = Some(error.to_string());
            }
            Ok(observed)
        }
        Err(error) => Err(std::io::Error::other(error)),
    }
}

fn stopping_during_send(task: &DeliveryTask) -> bool {
    if task.stopping.as_ref().is_some_and(|stop| *stop.borrow() || stop.has_changed().is_err()) {
        return true;
    }
    match task.observe_delivery_terminal() {
        Ok(terminal) => terminal,
        Err(error) => {
            // This handoff was already accepted. Keep driving or explicitly
            // cancel on shutdown; a failed read cannot manufacture a terminal.
            log::warn!(
                "in-flight status observation unavailable message_id={}: {error}",
                task.message_id
            );
            false
        }
    }
}

pub(super) async fn wait_stopping(stopping: &mut Option<watch::Receiver<bool>>) {
    match stopping {
        Some(stop) => super::completion::wait_for_shutdown(stop).await,
        None => std::future::pending().await,
    }
}

#[cfg(test)]
#[path = "bridge_delivery_link_dispatch_tests.rs"]
mod tests;
