/// The admitted message owns activation, transfer and persistence. Only active
/// packet/Resource dispatch borrows execution permits; every wait releases them.
pub(super) async fn run(
    task: &mut DeliveryTask,
    prepared: &mut PreparedDeliveryPayload,
    limits: super::DeliveryLimits,
    metrics: &Arc<DeliverySchedulerMetrics>,
    peer: &str,
    stopping: &mut watch::Receiver<bool>,
) {
    let mut transfer = None;
    run_inner(task, prepared, &limits, metrics, peer, stopping, &mut transfer)
        .await;
    if let Some(transfer) = transfer {
        transfer.cancel(task).await;
    }
    #[cfg(test)]
    if let Some(control) = task.wakeup.trace() { control.finish(); }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Method {
    Direct,
    Opportunistic,
    Propagated,
}
impl Method {
    fn of(task: &DeliveryTask) -> Self {
        match task.requested_method {
            RequestedDeliveryMethod::Propagated => Self::Propagated,
            RequestedDeliveryMethod::Opportunistic => Self::Opportunistic,
            _ => Self::Direct,
        }
    }
    fn name(self) -> &'static str {
        match self {
            Self::Direct => "direct",
            Self::Opportunistic => "opportunistic-link",
            Self::Propagated => "propagation",
        }
    }
    fn completion(self, id: &str, resource: bool) -> ReceiptEvent {
        if self == Self::Propagated {
            ReceiptEvent::propagation_custody(
                id,
                if resource {
                    rns_rpc::broker::PropagationCustody::Resource
                } else {
                    rns_rpc::broker::PropagationCustody::Packet
                },
            )
        } else {
            ReceiptEvent::new(id, "delivered")
        }
    }
}
struct Context {
    link: Arc<Mutex<Link>>,
    peer: String,
    method: Method,
    identify: bool,
    signal: Option<watch::Receiver<bool>>,
}

fn target(
    task: &DeliveryTask,
    prepared: &PreparedDeliveryPayload,
    method: Method,
) -> std::io::Result<AddressHash> {
    if method != Method::Propagated {
        return Ok(task.destination_hash);
    }
    if let Some(propagation) = &prepared.propagation {
        return Ok(propagation.propagation_hash);
    }
    let hex = task
        .propagation_node_hex
        .as_deref()
        .ok_or_else(|| std::io::Error::other("no propagation node selected"))?;
    let bytes = hex::decode(hex).map_err(std::io::Error::other)?;
    Ok(AddressHash::new(
        bytes.try_into().map_err(|_| std::io::Error::other("invalid propagation node"))?,
    ))
}
async fn fallback(
    task: &mut DeliveryTask,
    method: Method,
    retry: &mut state::Retry,
    context: &mut Option<Context>,
    transfer: &mut Option<Transfer>,
    sending: &mut bool,
) -> bool {
    if method != Method::Direct
        || !task.try_propagation_on_fail
        || task.propagation_node_hex.is_none()
    {
        return false;
    }
    if let Some(previous) = transfer.take() { previous.cancel(task).await; }
    *sending = false;
    task.requested_method = RequestedDeliveryMethod::Propagated;
    // This explicit Rust extension advances one-way to a new method. Identity,
    // path and Link retries never reset the budget within either phase.
    *retry = state::Retry::default();
    *context = None;
    true
}
async fn teardown(task: &DeliveryTask, link: &Arc<Mutex<Link>>) {
    #[cfg(test)]
    if let Some(control) = task.wakeup.trace() { control.teardowns.lock().expect("trace teardowns").push(link.clone()); }
    let (local, mut closed) = watch::channel(false);
    let teardown = task.transport.teardown_link_observed(link, || {
        local.send_replace(true);
    });
    tokio::pin!(teardown);
    let mut stopping = task.stopping.clone();
    tokio::select! {
        biased;
        result = &mut teardown => if let Err(error) = result { log::warn!("Link teardown failed message_id={}: {error}", task.message_id); },
        _ = dispatch::wait_stopping(&mut stopping) => log::warn!("Link cleanup interrupted by coordinated shutdown message_id={}", task.message_id),
        result = closed.changed() => match result {
            Ok(()) if *closed.borrow() => log::debug!("Link locally closed; optional frame remains blocked message_id={}", task.message_id),
            _ => { if let Err(error) = teardown.await { log::warn!("Link teardown observation failed message_id={}: {error}", task.message_id); } },
        }
    }
}
async fn pause(
    task: &DeliveryTask,
    stopping: &mut watch::Receiver<bool>,
    terminal: &mut watch::Receiver<()>,
    delay: Duration,
) {
    tokio::select! {
        _ = task.wakeup.changed.notified() => {},
        _ = terminal.changed() => {},
        _ = stopping.changed() => {},
        _ = tokio::time::sleep(delay.min(Duration::from_millis(250))) => {},
    }
}
async fn publish(task: &DeliveryTask, event: ReceiptEvent, stopping: &mut watch::Receiver<bool>) {
    if event.custody().is_some() {
        completion::await_custody_commit(task, event, stopping).await;
    } else {
        completion::publish_terminal(task, event, stopping).await;
    }
}

async fn while_ready<T>(
    task: &DeliveryTask,
    future: impl std::future::Future<Output = T>,
) -> Option<T> {
    task.drive_delivery_io(future).await
}

async fn changed_transfer(transfer: &mut Option<Transfer>) -> std::io::Result<()> {
    match transfer.as_mut() {
        Some(transfer) => transfer.changed().await,
        None => std::future::pending().await,
    }
}
async fn changed_signal(context: &mut Option<Context>) {
    match context.as_mut().and_then(|ctx| ctx.signal.as_mut()) {
        Some(signal) => {
            if let Err(error) = signal.changed().await {
                log::debug!("Link signal producer closed: {error}");
            }
        }
        None => std::future::pending().await,
    }
}

async fn request_path(task: &DeliveryTask, target: AddressHash) -> Option<lxmf::message::DeliveryTiming> {
    match task.request_delivery_path(target).await {
        Ok(timing) => Some(timing),
        Err(outcome) => {
            log::warn!("Link path request unavailable message_id={}: {outcome:?}", task.message_id);
            None
        }
    }
}

async fn received_outcome(
    task: &DeliveryTask, context: &Option<Context>, packets: &[PacketAttempt],
    transfer: &mut Option<Transfer>, stopping: &mut watch::Receiver<bool>,
) -> bool {
    // A creator signal can override proof while the message is still pending.
    // Queue acceptance is not retirement; observing committed custody retires this owner.
    let event = if context.as_ref().and_then(|ctx| ctx.signal.as_ref()).is_some_and(|signal| *signal.borrow()) {
        Some(ReceiptEvent::new(&task.message_id, "rejected").with_method("propagation")
            .with_delivery_kind("propagation-invalid-stamp"))
    } else {
        packets.iter().find(|attempt| attempt.receipt.is_proved()).map(|attempt| attempt.event.clone())
    };
    let Some(event) = event else { return false; };
    if let Some(transfer) = transfer.take() { transfer.cancel(task).await; }
    publish_link_outcome(task, event, context, stopping).await;
    true
}


async fn store_locally(task: &DeliveryTask, prepared: &PreparedDeliveryPayload, method: Method, stopping: &mut watch::Receiver<bool>) -> bool {
    let Some(propagation) = prepared.propagation.as_ref().filter(|p|
        method == Method::Propagated && task.selected_propagation_node_is_local(&p.propagation_node_hex))
    else { return false; };
    let event = match task.store_local_propagation_payload(&propagation.propagation_node_hex, &propagation.payload) {
        Ok(()) => ReceiptEvent::propagation_custody(&task.message_id, rns_rpc::broker::PropagationCustody::Resource)
            .with_method("propagation").with_delivery_kind("local-propagation-store"),
        Err(error) => ReceiptEvent::new(&task.message_id, format!("failed: {error}")),
    };
    publish(task, event, stopping).await;
    true
}

/// Retire creator callbacks when this owner observes the custody commit. If
/// both watches are ready, the creator signal wins. Accepted custody work is
/// retained and the same receipt worker commits the terminal rejection after it.
async fn publish_link_outcome(
    task: &DeliveryTask, event: ReceiptEvent, context: &Option<Context>,
    stopping: &mut watch::Receiver<bool>,
) {
    if event.custody().is_none() {
        publish(task, event, stopping).await;
        return;
    }
    let mut signal = context.as_ref().and_then(|ctx| ctx.signal.clone());
    let commit = completion::await_custody_commit(task, event, stopping);
    tokio::pin!(commit);
    let rejected = async {
        match signal.as_mut() {
            Some(signal) => loop {
                if *signal.borrow() { return; }
                if signal.changed().await.is_err() { std::future::pending::<()>().await; }
            },
            None => std::future::pending::<()>().await,
        }
    };
    tokio::select! {
        biased;
        _ = rejected => {
            let mut terminal_stopping = task.stopping.clone().expect("delivery owns shutdown");
            completion::publish_terminal(task, ReceiptEvent::new(&task.message_id, "rejected")
                .with_method("propagation").with_delivery_kind("propagation-invalid-stamp"),
                &mut terminal_stopping).await;
            commit.await;
        },
        _ = &mut commit => {},
    }
}

fn packet_expired(task: &DeliveryTask, receipt: &rns_transport::transport::LinkPacketReceipt) -> bool {
    #[cfg(test)]
    if let Some(control) = task.wakeup.trace() { return control.expired(receipt); }
    let _ = task;
    receipt.expired(std::time::Instant::now())
}

#[cfg(test)]
fn trace_observation(retry: &state::Retry, origin: Instant, context: &Option<Context>, sending: bool,
    packets: &[PacketAttempt], transfer: &Option<Transfer>) -> crate::bridge::delivery_wakeup::trace_control::Observation {
    crate::bridge::delivery_wakeup::trace_control::Observation {
        attempts: retry.attempts, schedule: retry.schedule, never_active_requested: retry.never_active_requested,
        origin: Some(origin), link: context.as_ref().map(|ctx| ctx.link.clone()),
        creator: context.as_ref().is_some_and(|ctx| ctx.signal.is_some()), sending,
        packets: packets.iter().map(|p| p.receipt.clone()).collect(),
        resource: match transfer { Some(Transfer::Resource { hash, .. }) => Some(*hash), _ => None },
        resource_outcome: match transfer { Some(Transfer::Resource { completion, .. }) => completion.current(), _ => None },
    }
}
