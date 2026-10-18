use super::{completion, prepare_propagation, DeliverySchedulerMetrics};
use crate::bridge::delivery_attempt::DeliveryAttemptOutcome;
use crate::bridge::delivery_task::{DeliveryTask, PreparedDeliveryPayload};
use crate::bridge::{propagation, RequestedDeliveryMethod};
use reticulum_daemon::receipt_bridge::ReceiptEvent;
use rns_transport::destination::{
    link::{Link, LinkStatus},
    DestinationDesc, DestinationName,
};
use rns_transport::hash::AddressHash;
use std::{sync::Arc, time::Duration};
use tokio::sync::{watch, Mutex};
use tokio::time::Instant;

#[path = "bridge_delivery_link_acquisition.rs"]
mod acquisition;
#[path = "bridge_delivery_link_dispatch.rs"]
mod dispatch;
#[path = "bridge_delivery_link_state.rs"]
mod state;
use dispatch::{PacketAttempt, Transfer};
use rns_transport::resource::ResourceOutcome;

include!("bridge_delivery_link_context.rs");

async fn run_inner(
    task: &mut DeliveryTask,
    prepared: &mut PreparedDeliveryPayload,
    limits: &super::DeliveryLimits,
    metrics: &Arc<DeliverySchedulerMetrics>,
    peer: &str,
    stopping: &mut watch::Receiver<bool>,
    transfer: &mut Option<Transfer>,
) {
    let origin = Instant::now();
    let mut retry = state::Retry::default();
    let mut context: Option<Context> = None;
    let mut packets = Vec::<PacketAttempt>::new();
    let mut sending = false;
    let mut terminal = task.receipt_tx.terminal_completion_events();
    let mut out_links = task.transport.out_link_events();
    let mut in_links = task.transport.in_link_events();
    loop {
        #[cfg(test)]
        if let Some(control) = task.wakeup.trace() {
            if !control
                .enter(
                    trace_observation(&retry, origin, &context, sending, &packets, transfer),
                    stopping.clone(),
                )
                .await
            {
                return;
            }
        }
        match dispatch::accepting(task, stopping) {
            Ok(false) => {
                if let Some(transfer) = transfer.take() {
                    transfer.cancel(task).await;
                }
                return;
            }
            Err(error) => {
                log::warn!("Link owner status unavailable message_id={}: {error}", task.message_id);
                pause(task, stopping, &mut terminal, Duration::from_millis(250)).await;
                continue;
            }
            Ok(true) => {}
        }
        if received_outcome(task, &context, &packets, transfer, stopping).await {
            return;
        }
        let completed = match transfer.as_ref() {
            Some(Transfer::Resource { completion, .. }) => completion.current(),
            _ => None,
        };
        if let Some(outcome) = completed {
            let Some(Transfer::Resource { link, hash, peer: activity_peer, bytes, method, .. }) =
                transfer.take()
            else {
                unreachable!("Resource result belongs to Resource transfer");
            };
            if let Err(error) = crate::outbound_resources::retire_owned_resource(
                &task.outbound_resource_map,
                &hash.to_string(),
                &task.message_id,
            ) {
                log::error!(
                    "Resource owner retirement unavailable message_id={}: {error}",
                    task.message_id
                );
                completion::wait_for_shutdown(stopping).await;
                return;
            }
            match outcome {
                ResourceOutcome::Complete => {
                    task.daemon.record_outbound_peer_sent(&activity_peer, bytes);
                    publish_link_outcome(
                        task,
                        method
                            .completion(&task.message_id, true)
                            .with_resource_hash(hash.to_string())
                            .with_peer(activity_peer)
                            .with_bytes(bytes)
                            .with_method(method.name())
                            .with_delivery_kind("link-resource"),
                        &context,
                        stopping,
                    )
                    .await;
                    return;
                }
                ResourceOutcome::Rejected if method != Method::Propagated => {
                    publish(
                        task,
                        ReceiptEvent::new(&task.message_id, "rejected")
                            .with_resource_hash(hash.to_string())
                            .with_method(method.name()),
                        stopping,
                    )
                    .await;
                    return;
                }
                ResourceOutcome::Cancelled => {
                    publish(
                        task,
                        ReceiptEvent::new(&task.message_id, "cancelled")
                            .with_resource_hash(hash.to_string())
                            .with_method(method.name()),
                        stopping,
                    )
                    .await;
                    return;
                }
                ResourceOutcome::Failed | ResourceOutcome::Rejected => {
                    if outcome == ResourceOutcome::Failed {
                        task.daemon.record_outbound_peer_activity(&activity_peer, bytes, false);
                    }
                    teardown(task, &link).await;
                    sending = false;
                    // Preserve the closed context for the method-specific closure
                    // branch, including Direct's request and old attempt limit.
                }
            }
        }
        let timed_out = matches!(transfer.as_ref(), Some(Transfer::Packet { receipt, .. }) if packet_expired(task, receipt));
        if timed_out {
            if let Some(Transfer::Packet { link, .. }) = transfer.take() {
                teardown(task, &link).await;
            }
            sending = false;
        }
        let method = Method::of(task);
        let preparation = if method == Method::Propagated {
            let Some(result) = while_ready(
                task,
                prepare_propagation(task, prepared, &limits.stamp, metrics, peer),
            )
            .await
            else {
                return;
            };
            result
        } else {
            Ok(())
        };
        let mut waiting_identity = None;
        match preparation {
            Ok(()) => {}
            Err(DeliveryAttemptOutcome::AwaitPath { destination }) => {
                waiting_identity = Some(destination)
            }
            Err(DeliveryAttemptOutcome::AwaitStatus) => {
                pause(task, stopping, &mut terminal, Duration::from_millis(250)).await;
                continue;
            }
            Err(DeliveryAttemptOutcome::DeferredStorage) => {
                task.deferred_dispatch.store(true, std::sync::atomic::Ordering::Release);
                return;
            }
            Err(_) => return,
        }
        if store_locally(task, prepared, method, stopping).await {
            return;
        }
        let target = match target(task, prepared, method) {
            Ok(target) => target,
            Err(error) => {
                publish(
                    task,
                    ReceiptEvent::new(&task.message_id, format!("failed: {error}")),
                    stopping,
                )
                .await;
                return;
            }
        };
        task.wakeup.wait_for(waiting_identity.unwrap_or(target));
        let Some(measurements) = while_ready(task, task.transport.path_timing(&target)).await
        else {
            return;
        };
        let timing = match task.delivery_timing(measurements) {
            Ok(timing) => timing,
            Err(error) => {
                publish(
                    task,
                    ReceiptEvent::new(&task.message_id, format!("failed: {error}")),
                    stopping,
                )
                .await;
                return;
            }
        };
        if context.is_none() && waiting_identity.is_none() {
            let Some(selected) = acquisition::select(task, target, method).await else {
                return;
            };
            context = selected;
        }
        if let Some(ctx) = context.as_mut() {
            let Some((status, activated)) = while_ready(task, async {
                let guard = ctx.link.lock().await;
                (guard.status(), guard.was_activated())
            })
            .await
            else {
                return;
            };
            match status {
                LinkStatus::Closed => {
                    if matches!(transfer, Some(Transfer::Resource { .. })) {
                        teardown(task, &ctx.link).await;
                        continue;
                    }
                    let mut closed_timing = timing;
                    if method != Method::Propagated && (activated || !retry.never_active_requested)
                    {
                        let Some(measured) = request_path(task, target).await else {
                            return;
                        };
                        closed_timing = measured;
                        if !activated {
                            retry.never_active_requested = true;
                        }
                    }
                    if method == Method::Propagated
                        && while_ready(
                            task,
                            propagation::forget_propagation_link_context(
                                &task.outbound_propagation_link,
                                &ctx.link,
                            ),
                        )
                        .await
                        .is_none()
                    {
                        return;
                    }
                    retry.closed(origin.elapsed(), closed_timing);
                    context = None;
                    continue;
                }
                LinkStatus::Active if !sending && waiting_identity.is_none() => {
                    let permits = super::opportunistic::acquire(
                        task,
                        stopping,
                        &mut terminal,
                        &limits.global,
                        &limits.peer,
                        None,
                    )
                    .await;
                    let permits = match permits {
                        super::opportunistic::Acquisition::Ready(permits) => permits,
                        super::opportunistic::Acquisition::AwaitStatus => continue,
                        _ => return,
                    };
                    // Native state may change while queued for execution.
                    let Some(active) = while_ready(task, async {
                        ctx.link.lock().await.status() == LinkStatus::Active
                    })
                    .await
                    else {
                        return;
                    };
                    if !matches!(dispatch::accepting(task, stopping), Ok(true)) || !active {
                        drop(permits);
                        continue;
                    }
                    metrics.record_dequeued_for_peer(peer);
                    metrics.record_started_for_peer(peer);
                    if ctx.identify {
                        let Some(id) =
                            while_ready(task, async { *ctx.link.lock().await.id() }).await
                        else {
                            return;
                        };
                        task.direct_backchannel_links
                            .record_outbound_local_source(id, task.source_hash);
                        match while_ready(task, task.identify_link_for_backchannel(&ctx.link)).await
                        {
                            Some(Ok(())) => {}
                            Some(Err(error)) => log::warn!(
                                "Link identification failed message_id={} link={id}: {error}",
                                task.message_id
                            ),
                            None => {
                                drop(permits);
                                metrics.record_waiting_for_peer(peer);
                                continue;
                            }
                        }
                        ctx.identify = false;
                    }
                    let payload = if method == Method::Propagated {
                        prepared
                            .propagation
                            .as_ref()
                            .expect("prepared propagation")
                            .payload
                            .bytes
                            .as_slice()
                    } else {
                        &prepared.lxmf_payload
                    };
                    let result =
                        dispatch::dispatch(task, ctx, payload, &mut packets, stopping).await;
                    drop(permits);
                    metrics.record_waiting_for_peer(peer);
                    match result {
                        Ok(Some(next)) => {
                            if let Transfer::Resource { registration_error: Some(error), .. } =
                                &next
                            {
                                let error = error.clone();
                                next.cancel(task).await;
                                publish(
                                    task,
                                    ReceiptEvent::new(
                                        &task.message_id,
                                        format!("failed: Resource ownership registration: {error}"),
                                    ),
                                    stopping,
                                )
                                .await;
                                return;
                            }
                            if let Transfer::NoReceipt { link } = &next {
                                teardown(task, link).await;
                            }
                            *transfer = Some(next);
                            sending = true;
                        }
                        Ok(None) => {}
                        Err(error) => {
                            if fallback(
                                task,
                                method,
                                &mut retry,
                                &mut context,
                                transfer,
                                &mut sending,
                            )
                            .await
                            {
                                continue;
                            }
                            publish(
                                task,
                                ReceiptEvent::new(&task.message_id, format!("failed: {error}"))
                                    .with_method(method.name()),
                                stopping,
                            )
                            .await;
                            return;
                        }
                    }
                    continue;
                }
                _ => {} // Pending/Stale and Active+SENDING retain the owner.
            }
        } else {
            let wait_target = waiting_identity.unwrap_or(target);
            let Some(ready) = while_ready(task, task.discovery_ready(wait_target)).await else {
                return;
            };
            let ready = measurements.path_known && ready;
            if let Some(allowed) = retry.begin(origin.elapsed(), ready, timing) {
                #[cfg(test)]
                if let Some(control) = task.wakeup.trace() {
                    control.observe(trace_observation(
                        &retry, origin, &context, sending, &packets, transfer,
                    ));
                }
                if !allowed {
                    if fallback(task, method, &mut retry, &mut context, transfer, &mut sending)
                        .await
                    {
                        continue;
                    }
                    publish(
                        task,
                        ReceiptEvent::new(
                            &task.message_id,
                            "failed: Link delivery attempts exhausted",
                        )
                        .with_method(method.name()),
                        stopping,
                    )
                    .await;
                    return;
                }
                let Some(identity) = while_ready(
                    task,
                    task.resolve_identity(
                        Some(&target.to_hex_string()),
                        target,
                        None,
                        "link-owner",
                    ),
                )
                .await
                else {
                    return;
                };
                if let Ok(identity) = identity {
                    if waiting_identity.is_none() && measurements.path_known {
                        let Some(next) = acquisition::open(task, target, identity, method).await
                        else {
                            return;
                        };
                        context = Some(next);
                        continue;
                    }
                }
                let Some(timing) = request_path(task, wait_target).await else {
                    return;
                };
                // Anchor after the awaited request even when coalescing suppresses it.
                retry.schedule.schedule_path(origin.elapsed(), timing);
            }
        }
        if let Some(ctx) = context.as_mut() {
            if ctx
                .signal
                .as_ref()
                .is_some_and(|signal| signal.has_changed().is_err() && !*signal.borrow())
            {
                ctx.signal = None;
            }
        }
        #[cfg(test)]
        if let Some(control) = task.wakeup.trace() {
            control
                .observe(trace_observation(&retry, origin, &context, sending, &packets, transfer));
            control.finish_turn();
        }
        let native = changed_transfer(transfer);
        let delay = if context.is_none() {
            retry.schedule.deadline().map_or(Duration::from_millis(250), |deadline| {
                deadline.saturating_sub(origin.elapsed()).saturating_add(Duration::from_nanos(1))
            })
        } else {
            Duration::from_millis(250)
        };
        let signal = changed_signal(&mut context);
        tokio::select! {
            result = native => if let Err(error) = result {
                log::error!("native transfer observation unavailable message_id={}; retaining until shutdown: {error}", task.message_id);
                completion::wait_for_shutdown(stopping).await;
                if let Some(transfer) = transfer.take() { transfer.cancel(task).await; }
                return;
            },
            _ = signal => {},
            _ = out_links.recv() => {},
            _ = in_links.recv() => {},
            _ = pause(task, stopping, &mut terminal, delay) => {},
        }
    }
}

#[cfg(test)]
#[path = "bridge_delivery_link_state_tests.rs"]
mod state_tests;
