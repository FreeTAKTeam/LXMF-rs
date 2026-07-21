use super::announce::{handle_announce, release_held_announces, retransmit_announces};
use super::path::{handle_fixed_destinations, handle_link_request};
use super::wire::{handle_data, handle_proof};
use super::*;
use crate::destination::link::LinkWatchdogAction;

#[allow(dead_code)]
const MIN_LINKS_CHECK_DELAY: Duration = Duration::from_millis(10);
const PATH_REQUEST_MI: Duration = Duration::from_secs(20);

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

#[allow(dead_code)]
pub(super) async fn handle_check_links<'a>(mut handler: MutexGuard<'a, TransportHandler>) {
    let mut links_to_remove: Vec<AddressHash> = Vec::new();
    let mut closed_link_ids: Vec<AddressHash> = Vec::new();
    let mut pending_packets: Vec<Packet> = Vec::new();
    let mut direct_messages: Vec<TxMessage> = Vec::new();
    let mut closed_pending_destinations: Vec<AddressHash> = Vec::new();
    let mut rediscovery_requests: Vec<AddressHash> = Vec::new();
    let now = std::time::Instant::now();

    // Clean up input links
    for link_entry in &handler.in_links {
        let mut link = link_entry.1.lock().await;
        if let Some(iface) = link.ingress_iface() {
            for packet in link.poll_channel_timeouts(now) {
                direct_messages.push(TxMessage { tx_type: TxMessageType::Direct(iface), packet });
            }
        }
        match link.status() {
            LinkStatus::Closed => {
                links_to_remove.push(*link_entry.0);
                closed_link_ids.push(*link.id());
            }
            LinkStatus::Pending | LinkStatus::Handshake => {
                if link.elapsed() > INTERVAL_INPUT_LINK_CLEANUP {
                    link.close();
                    links_to_remove.push(*link_entry.0);
                    closed_link_ids.push(*link.id());
                }
            }
            LinkStatus::Active | LinkStatus::Stale => {
                if let LinkWatchdogAction::SendTeardown(packet) = link.check_watchdog(false) {
                    if let Some(iface) = link.ingress_iface() {
                        direct_messages
                            .push(TxMessage { tx_type: TxMessageType::Direct(iface), packet });
                    }
                    links_to_remove.push(*link_entry.0);
                    closed_link_ids.push(*link.id());
                }
            }
        }
    }

    for addr in &links_to_remove {
        handler.in_links.remove(addr);
    }
    for link_id in &closed_link_ids {
        handler.resource_manager.remove_link_state(*link_id);
    }

    links_to_remove.clear();
    closed_link_ids.clear();

    for link_entry in &handler.out_links {
        let mut link = link_entry.1.lock().await;
        if let Some(iface) = link.ingress_iface() {
            for packet in link.poll_channel_timeouts(now) {
                direct_messages.push(TxMessage { tx_type: TxMessageType::Direct(iface), packet });
            }
        }
        match link.status() {
            LinkStatus::Closed => {
                let destination = link.destination().address_hash;
                let rediscover_closed_pending =
                    !handler.config.transport_enabled && !link.was_activated();
                links_to_remove.push(*link_entry.0);
                closed_link_ids.push(*link.id());
                if rediscover_closed_pending {
                    closed_pending_destinations.push(destination);
                }
            }
            LinkStatus::Active | LinkStatus::Stale => match link.check_watchdog(true) {
                LinkWatchdogAction::SendKeepAlive => {
                    if let Some(iface) = link.ingress_iface() {
                        direct_messages.push(TxMessage {
                            tx_type: TxMessageType::Direct(iface),
                            packet: link.keep_alive_packet(KEEP_ALIVE_REQUEST),
                        });
                    }
                }
                LinkWatchdogAction::SendTeardown(packet) => {
                    if let Some(iface) = link.ingress_iface() {
                        direct_messages
                            .push(TxMessage { tx_type: TxMessageType::Direct(iface), packet });
                    }
                    links_to_remove.push(*link_entry.0);
                    closed_link_ids.push(*link.id());
                }
                LinkWatchdogAction::None => {}
            },
            LinkStatus::Pending => {
                if link.elapsed() > INTERVAL_OUTPUT_LINK_REPEAT {
                    log::warn!("tp({}): repeat link request {}", handler.config.name, link.id());
                    pending_packets.push(link.request());
                }
            }
            LinkStatus::Handshake => {}
        }
    }

    for addr in &links_to_remove {
        handler.out_links.remove(addr);
    }
    for link_id in &closed_link_ids {
        handler.resource_manager.remove_link_state(*link_id);
    }
    closed_pending_destinations.sort();
    closed_pending_destinations.dedup();
    for destination in closed_pending_destinations {
        if handler.path_table.expire_path(&destination) {
            log::debug!(
                "tp({}): expired path to {} after pending link never activated",
                handler.config.name,
                destination
            );
        }
        if !handler.config.connected_to_shared_instance
            && !handler.path_requests.outgoing_request_recently_sent(
                &destination,
                now.into(),
                PATH_REQUEST_MI,
            )
        {
            rediscovery_requests.push(destination);
        }
    }

    for packet in pending_packets {
        handler.send_packet(packet).await;
    }
    rediscovery_requests.sort();
    rediscovery_requests.dedup();
    for destination in rediscovery_requests {
        log::debug!(
            "tp({}): trying to rediscover path for {} since a pending link never activated",
            handler.config.name,
            destination
        );
        handler.request_path(&destination, None, None).await;
    }
    for message in direct_messages {
        handler.send(message).await;
    }
}

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

    let _packet_task = {
        let handler_arc = handler_arc.clone();
        let cancel = cancel.clone();

        log::trace!("tp({}): start packet task", handler_arc.lock().await.config.name);

        tokio::spawn(async move {
            loop {
                let mut rx_receiver = rx_receiver.lock().await;

                if cancel.is_cancelled() {
                    break;
                }

                tokio::select! {
                    _ = cancel.cancelled() => {
                        break;
                    },
                    Some(message) = rx_receiver.recv() => {
                        if iface_messages_tx.send(message.clone()).is_err() {
                            log::trace!(
                                "[tp-diag] interface message has no active subscribers iface={}",
                                message.address
                            );
                        }

                        let packet = message.packet;

                        let mut handler = handler_arc.lock().await;

                        if PACKET_TRACE {
                            log::debug!("<< rx({}) = {} {}", message.address, packet, packet.hash());
                        }

                        log::info!(
                            "[tp-diag] inbound_packet node={} iface={} src={:?} dst={} type={:?} dest_type={:?} propagation={:?} ctx={:?} len={} hash={}",
                            handler.config.name,
                            message.address,
                            message.source,
                            packet.destination,
                            packet.header.packet_type,
                            packet.header.destination_type,
                            packet.header.propagation_type,
                            packet.context,
                            packet.data.len(),
                            packet.hash()
                        );

                        if handle_fixed_destinations(
                            &packet,
                            &mut handler,
                            message.address
                        ).await {
                            continue;
                        }

                        if !handler.filter_duplicate_packets(&packet).await {
                            log::debug!(
                                "tp({}): dropping duplicate packet: dst={}, ctx={:?}, type={:?}",
                                handler.config.name,
                                packet.destination,
                                packet.context,
                                packet.header.packet_type
                            );
                            continue;
                        }

                        match packet.header.packet_type {
                            PacketType::Announce => handle_announce(
                                &packet,
                                handler,
                                message.address,
                                message.source,
                            ).await,
                            // Link traffic: learn the sender's unicast route and
                            // bind the link to that virtual iface, so replies are
                            // unicast
                            PacketType::LinkRequest => {
                                let route_iface = handler
                                    .ingress_route_iface(&packet, message.address, message.source)
                                    .await;
                                handle_link_request(&packet, route_iface, handler).await
                            }
                            PacketType::Proof => {
                                let route_iface = handler
                                    .ingress_route_iface(&packet, message.address, message.source)
                                    .await;
                                drop(handler);
                                handle_proof(packet, handler_arc.clone(), route_iface).await;
                            }
                            PacketType::Data => {
                                let route_iface = handler
                                    .ingress_route_iface(&packet, message.address, message.source)
                                    .await;
                                handle_data(&packet, route_iface, handler).await
                            }
                        }
                    }
                };
            }
        })
    };

    {
        let handler = handler_arc.clone();
        let cancel = cancel.clone();

        tokio::spawn(async move {
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
        let handler = handler_arc.clone();
        let cancel = cancel.clone();

        tokio::spawn(async move {
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

        tokio::spawn(async move {
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

        tokio::spawn(async move {
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

        tokio::spawn(async move {
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
}

#[cfg(test)]
include!("jobs_parts/tests.rs");
