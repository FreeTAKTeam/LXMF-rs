use super::announce::{release_held_announces, retransmit_announces};
use super::inbound_processing::{
    preprocess_inbound_message, process_inbound_message, rollback_rejected_inbound,
};
use super::*;
use crate::destination::link::LinkWatchdogAction;

#[allow(dead_code)]
const MIN_LINKS_CHECK_DELAY: Duration = Duration::from_millis(10);
const PATH_REQUEST_MI: Duration = Duration::from_secs(20);

/// Reference Reticulum increments a packet's hop count unconditionally on
/// every single receipt, on every interface (`Transport.inbound()`,
/// `packet.hops += 1`, immediately after decode) — this crate never did, so
/// every stored hop count was one lower than what a reference client or
/// `rnsd` itself computes for the same path. Confirmed via a live `rnpath`
/// check on a live node against a destination reachable in one hop: `rnsd`'s
/// own path table said `hops=1`, matching this crate's, while a separate wire
/// capture of a reference client's own log for a *different* destination
/// said "1 hops away" where this crate stored 0 for the same destination.
/// `hash()` deliberately excludes hops (see `Packet::hash`), so this doesn't
/// disturb duplicate detection. `saturating_add` rather than wrapping,
/// matching the header's hop count never being meant to overflow past its
/// practical (u8) ceiling.
pub(super) fn apply_receive_hop_increment(packet: &mut Packet) {
    packet.header.hops = packet.header.hops.saturating_add(1);
}

#[cfg(test)]
mod receive_hop_increment_tests {
    use super::*;
    use crate::packet::Header;

    fn test_packet(hops: u8) -> Packet {
        Packet {
            header: Header { hops, ..Default::default() },
            ifac: None,
            destination: AddressHash::new_empty(),
            transport: None,
            context: PacketContext::None,
            data: PacketDataBuffer::new(),
        }
    }

    #[test]
    fn increments_a_freshly_originated_packet_to_one_real_hop() {
        let mut packet = test_packet(0);
        apply_receive_hop_increment(&mut packet);
        assert_eq!(packet.header.hops, 1);
    }

    #[test]
    fn increments_an_already_relayed_packet_by_exactly_one() {
        let mut packet = test_packet(3);
        apply_receive_hop_increment(&mut packet);
        assert_eq!(packet.header.hops, 4);
    }

    #[test]
    fn saturates_instead_of_wrapping_at_the_u8_ceiling() {
        let mut packet = test_packet(u8::MAX);
        apply_receive_hop_increment(&mut packet);
        assert_eq!(packet.header.hops, u8::MAX);
    }

    #[test]
    fn does_not_change_the_packet_hash() {
        let mut packet = test_packet(0);
        let hash_before = packet.hash();
        apply_receive_hop_increment(&mut packet);
        assert_eq!(packet.hash(), hash_before);
    }
}

include!("jobs_parts/link_table_cleanup.rs");

#[allow(dead_code)]
fn link_check_delay_from_deadline(
    now: std::time::Instant,
    earliest_retry: Option<std::time::Instant>,
) -> Duration {
    let Some(deadline) = earliest_retry else {
        return INTERVAL_LINKS_CHECK;
    };

    if deadline <= now {
        return MIN_LINKS_CHECK_DELAY;
    }

    std::cmp::min(deadline.duration_since(now), INTERVAL_LINKS_CHECK)
}

#[allow(dead_code)]
async fn next_link_check_delay(handler_arc: &Arc<Mutex<TransportHandler>>) -> Duration {
    let (in_links, out_links) = {
        let handler = handler_arc.lock().await;
        (
            handler.in_links.values().cloned().collect::<Vec<_>>(),
            handler.out_links.values().cloned().collect::<Vec<_>>(),
        )
    };

    let now = std::time::Instant::now();
    let mut earliest_deadline = None;
    for link in in_links {
        let link = link.lock().await;
        for deadline in
            [link.next_channel_retry_at(), link.next_watchdog_deadline(false)].into_iter().flatten()
        {
            earliest_deadline = Some(match earliest_deadline {
                Some(current) => std::cmp::min(current, deadline),
                None => deadline,
            });
        }
    }
    for link in out_links {
        let link = link.lock().await;
        for deadline in
            [link.next_channel_retry_at(), link.next_watchdog_deadline(true)].into_iter().flatten()
        {
            earliest_deadline = Some(match earliest_deadline {
                Some(current) => std::cmp::min(current, deadline),
                None => deadline,
            });
        }
    }

    link_check_delay_from_deadline(now, earliest_deadline)
}

#[path = "jobs_parts/link_maintenance.rs"]
mod link_maintenance;
pub(super) use link_maintenance::handle_check_links;

pub(super) async fn handle_cleanup<'a>(mut handler: MutexGuard<'a, TransportHandler>) {
    handler.gc_unicast_ifaces().await;
    {
        let iface_manager = handler.iface_manager.clone();
        let mut iface_manager = iface_manager.lock().await;
        handler
            .path_table
            .remove_stale(std::time::Instant::now(), |iface| iface_manager.mode(iface));
        handler.tunnel_table.remove_stale(std::time::Instant::now());
        iface_manager.cleanup();
    }
}

pub(super) async fn manage_transport(
    handler_arc: Arc<Mutex<TransportHandler>>,
    rx_receiver: Arc<Mutex<InterfaceRxReceiver>>,
    iface_messages_tx: broadcast::Sender<RxMessage>,
) {
    let cancel = handler_arc.lock().await.cancel.clone();
    let transport_enabled = handler_arc.lock().await.config.transport_enabled;

    // Worker supervision (issue #525): every worker handle is retained in
    // this set. The loop at the end of this function fails loudly and
    // cancels the remaining workers if any worker exits before shutdown,
    // so a panicked or silently returned worker can no longer degrade the
    // transport invisibly. Each worker returns its name on exit, and the
    // id-to-name map keeps failures attributable even when a panic means
    // no value comes back (review follow-up on #539).
    let mut workers = WorkerSet::new();
    let mut worker_names = WorkerNames::new();

    let inbound_queues = handler_arc.lock().await.inbound_queues.clone();

    {
        let handler_arc = handler_arc.clone();
        let cancel = cancel.clone();
        let inbound_queues = inbound_queues.clone();

        spawn_named_worker(&mut workers, &mut worker_names, "packet-ingress", async move {
            loop {
                let message = {
                    let mut receiver = rx_receiver.lock().await;
                    tokio::select! {
                        _ = cancel.cancelled() => break,
                        message = receiver.recv() => message,
                    }
                };
                let Some(message) = message else {
                    break;
                };
                let Some((class, message)) =
                    preprocess_inbound_message(&handler_arc, &iface_messages_tx, message).await
                else {
                    continue;
                };
                if let Err(full) = inbound_queues.enqueue(class, message) {
                    rollback_rejected_inbound(&handler_arc, &full.item).await;
                    log::warn!(
                        "dropping inbound packet because {:?} queue is full iface={} hash={}",
                        full.class,
                        full.item.message.address,
                        full.item.message.packet.hash()
                    );
                }
            }
        });
    }

    {
        let handler_arc = handler_arc.clone();
        let cancel = cancel.clone();

        spawn_named_worker(&mut workers, &mut worker_names, "packet-drain", async move {
            loop {
                let message = tokio::select! {
                    _ = cancel.cancelled() => break,
                    message = inbound_queues.dequeue() => message,
                };
                process_inbound_message(handler_arc.clone(), message).await;
            }
        });
    }

    {
        let handler = handler_arc.clone();
        let cancel = cancel.clone();

        spawn_named_worker(&mut workers, &mut worker_names, "link-check", async move {
            loop {
                if cancel.is_cancelled() {
                    break;
                }

                let delay = next_link_check_delay(&handler).await;
                tokio::select! {
                    _ = cancel.cancelled() => {
                        break;
                    },
                    _ = time::sleep(delay) => {
                        handle_check_links(handler.lock().await).await;
                    }
                }
            }
        });
    }

    {
        let iface_manager = handler_arc.lock().await.iface_manager.clone();
        let cancel = cancel.clone();

        spawn_named_worker(&mut workers, &mut worker_names, "traffic-sampler", async move {
            loop {
                tokio::select! {
                    _ = cancel.cancelled() => break,
                    _ = time::sleep(Duration::from_secs(1)) => {
                        iface_manager.lock().await.sample_traffic();
                    }
                }
            }
        });
    }

    {
        let handler = handler_arc.clone();
        let cancel = cancel.clone();

        spawn_named_worker(&mut workers, &mut worker_names, "iface-cleanup", async move {
            loop {
                if cancel.is_cancelled() {
                    break;
                }

                tokio::select! {
                    _ = cancel.cancelled() => {
                        break;
                    },
                    _ = time::sleep(INTERVAL_IFACE_CLEANUP) => {
                        handle_cleanup(handler.lock().await).await;
                    }
                }
            }
        });
    }

    {
        let handler = handler_arc.clone();
        let cancel = cancel.clone();

        spawn_named_worker(&mut workers, &mut worker_names, "packet-cache-cleanup", async move {
            loop {
                if cancel.is_cancelled() {
                    break;
                }

                tokio::select! {
                    _ = cancel.cancelled() => {
                        break;
                    },
                    _ = time::sleep(INTERVAL_PACKET_CACHE_CLEANUP) => {
                        let handler = handler.lock().await;

                        handler
                            .packet_cache
                            .lock()
                            .await
                            .release(INTERVAL_KEEP_PACKET_CACHED);

                        handle_link_table_cleanup(handler).await;
                    },
                }
            }
        });
    }

    {
        let handler = handler_arc.clone();
        let cancel = cancel.clone();

        spawn_named_worker(&mut workers, &mut worker_names, "announce-retransmit", async move {
            loop {
                if cancel.is_cancelled() {
                    break;
                }

                tokio::select! {
                    _ = cancel.cancelled() => {
                        break;
                    },
                    _ = time::sleep(INTERVAL_ANNOUNCES_RETRANSMIT) => {
                        let guard = handler.lock().await;
                        if transport_enabled {
                            retransmit_announces(guard).await;
                        } else {
                            release_held_announces(guard).await;
                            handler.lock().await.iface_manager.lock().await.release_queued_announces().await;
                            continue;
                        }
                        release_held_announces(handler.lock().await).await;
                        handler.lock().await.iface_manager.lock().await.release_queued_announces().await;
                    }
                }
            }
        });
    }

    {
        let handler = handler_arc.clone();
        let cancel = cancel.clone();
        let retry_interval = Duration::from_secs(
            handler_arc.lock().await.config.resource_retry_interval_secs.max(1),
        );

        spawn_named_worker(&mut workers, &mut worker_names, "resource-retry", async move {
            loop {
                if cancel.is_cancelled() {
                    break;
                }

                tokio::select! {
                    _ = cancel.cancelled() => {
                        break;
                    },
                    _ = time::sleep(retry_interval) => {
                        let mut handler = handler.lock().await;
                        let now = Instant::now();
                        let requests = handler.resource_manager.retry_requests(now);
                        let advertisements = handler.resource_manager.poll_outgoing(now);
                        for (link_id, request) in requests {
                            let link = handler
                                .in_links
                                .get(&link_id)
                                .cloned()
                                .or_else(|| handler.out_links.get(&link_id).cloned());
                            if let Some(link) = link {
                                let link_guard = link.lock().await;
                                let packet = build_resource_request_packet(&link_guard, &request);
                                drop(link_guard);
                                handler.send_packet(packet).await;
                            }
                        }
                        for (_link_id, packet) in advertisements {
                            handler.send_packet(packet).await;
                        }
                        let events = handler.resource_manager.drain_events();
                        super::resource_wire::publish_resource_events(&handler, events);
                    }
                }
            }
        });
    }

    // Supervision loop (issue #525) — extracted so the failure semantics
    // are directly testable: normal shutdown cancels every worker and
    // drains this set quietly; any worker that exits while the transport
    // is still running cancels the rest and is logged with its name and
    // failure.
    supervise_workers(&mut workers, &worker_names, &cancel).await;
}

type WorkerSet = tokio::task::JoinSet<()>;
type WorkerNames = std::collections::HashMap<tokio::task::Id, &'static str>;

/// Spawns a named transport worker, recording the task id so failures
/// (including panics, which return no value) stay attributable to the
/// worker that caused them.
fn spawn_named_worker<F>(
    workers: &mut WorkerSet,
    worker_names: &mut WorkerNames,
    name: &'static str,
    future: F,
) where
    F: std::future::Future<Output = ()> + Send + 'static,
{
    worker_names.insert(workers.spawn(future).id(), name);
}

async fn supervise_workers(
    workers: &mut WorkerSet,
    worker_names: &WorkerNames,
    cancel: &tokio_util::sync::CancellationToken,
) {
    while let Some(result) = workers.join_next_with_id().await {
        match result {
            Ok((id, ())) => {
                if !cancel.is_cancelled() {
                    let name = worker_names.get(&id).copied().unwrap_or("<unnamed>");
                    log::error!(
                        "tp: transport worker '{name}' exited before shutdown; cancelling remaining workers"
                    );
                    cancel.cancel();
                }
            }
            Err(err) => {
                // JoinError carries no return value, but it does carry
                // the failed task id — look the worker name up so panics
                // stay attributable (review follow-up on #539).
                let name = worker_names.get(&err.id()).copied().unwrap_or("<unnamed>");
                log::error!(
                    "tp: transport worker '{name}' failed ({err}); cancelling remaining workers"
                );
                cancel.cancel();
            }
        }
    }
}

#[cfg(test)]
include!("jobs_parts/tests.rs");
