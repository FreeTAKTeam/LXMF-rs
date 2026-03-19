use super::announce::{handle_announce, release_held_announces, retransmit_announces};
use super::path::{handle_fixed_destinations, handle_link_request};
use super::wire::{handle_data, handle_proof};
use super::*;
use crate::destination::link::LinkWatchdogAction;

const MIN_LINKS_CHECK_DELAY: Duration = Duration::from_millis(10);

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

async fn next_link_check_delay(handler_arc: &Arc<Mutex<TransportHandler>>) -> Duration {
    let (in_links, out_links) = {
        let handler = handler_arc.lock().await;
        (
            handler.in_links.values().cloned().collect::<Vec<_>>(),
            handler.out_links.values().cloned().collect::<Vec<_>>(),
        )
    };

    let now = std::time::Instant::now();
    let mut earliest_retry = None;
    for link in in_links.into_iter().chain(out_links) {
        if let Some(deadline) = link.lock().await.next_channel_retry_at() {
            earliest_retry = Some(match earliest_retry {
                Some(current) => std::cmp::min(current, deadline),
                None => deadline,
            });
        }
    }

    link_check_delay_from_deadline(now, earliest_retry)
}

pub(super) async fn handle_check_links<'a>(mut handler: MutexGuard<'a, TransportHandler>) {
    let mut links_to_remove: Vec<AddressHash> = Vec::new();
    let mut pending_packets: Vec<Packet> = Vec::new();
    let mut direct_messages: Vec<TxMessage> = Vec::new();
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
            LinkStatus::Closed => links_to_remove.push(*link_entry.0),
            LinkStatus::Pending | LinkStatus::Handshake => {
                if link.elapsed() > INTERVAL_INPUT_LINK_CLEANUP {
                    link.close();
                    links_to_remove.push(*link_entry.0);
                }
            }
            LinkStatus::Active | LinkStatus::Stale => {
                if link.check_watchdog(false) == LinkWatchdogAction::Close {
                    links_to_remove.push(*link_entry.0);
                }
            }
        }
    }

    for addr in &links_to_remove {
        handler.in_links.remove(addr);
    }

    links_to_remove.clear();

    for link_entry in &handler.out_links {
        let mut link = link_entry.1.lock().await;
        if let Some(iface) = link.ingress_iface() {
            for packet in link.poll_channel_timeouts(now) {
                direct_messages.push(TxMessage { tx_type: TxMessageType::Direct(iface), packet });
            }
        }
        match link.status() {
            LinkStatus::Closed => links_to_remove.push(*link_entry.0),
            LinkStatus::Active | LinkStatus::Stale => match link.check_watchdog(true) {
                LinkWatchdogAction::SendKeepAlive => {
                    if let Some(iface) = link.ingress_iface() {
                        direct_messages.push(TxMessage {
                            tx_type: TxMessageType::Direct(iface),
                            packet: link.keep_alive_packet(KEEP_ALIVE_REQUEST),
                        });
                    }
                }
                LinkWatchdogAction::Close => {
                    links_to_remove.push(*link_entry.0);
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

    for packet in pending_packets {
        handler.send_packet(packet).await;
    }
    for message in direct_messages {
        handler.send(message).await;
    }
}

pub(super) async fn handle_cleanup<'a>(handler: MutexGuard<'a, TransportHandler>) {
    handler.iface_manager.lock().await.cleanup();
}

pub(super) async fn manage_transport(
    handler_arc: Arc<Mutex<TransportHandler>>,
    rx_receiver: Arc<Mutex<InterfaceRxReceiver>>,
    iface_messages_tx: broadcast::Sender<RxMessage>,
) {
    let cancel = handler_arc.lock().await.cancel.clone();
    let retransmit = handler_arc.lock().await.config.retransmit;

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
                        let _ = iface_messages_tx.send(message);

                        let packet = message.packet;

                        let mut handler = handler_arc.lock().await;

                        if PACKET_TRACE {
                            log::debug!("tp: << rx({}) = {} {}", message.address, packet, packet.hash());
                        }

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
                                message.address
                            ).await,
                            PacketType::LinkRequest => handle_link_request(
                                &packet,
                                message.address,
                                handler
                            ).await,
                            PacketType::Proof => {
                                drop(handler);
                                handle_proof(packet, handler_arc.clone(), message.address).await;
                            }
                            PacketType::Data => handle_data(&packet, message.address, handler).await,
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

                let retry_delay = next_link_check_delay(&handler).await;

                tokio::select! {
                    _ = cancel.cancelled() => {
                        break;
                    },
                    _ = time::sleep(retry_delay) => {
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
                        let mut handler = handler.lock().await;

                        handler
                            .packet_cache
                            .lock()
                            .await
                            .release(INTERVAL_KEEP_PACKET_CACHED);

                        handler.link_table.remove_stale();
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
                        if retransmit {
                            retransmit_announces(guard).await;
                        } else {
                            release_held_announces(guard).await;
                            continue;
                        }
                        release_held_announces(handler.lock().await).await;
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
                    }
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn link_check_delay_uses_retry_deadline_when_sooner_than_default_sweep() {
        let now = std::time::Instant::now();
        let deadline = now + Duration::from_millis(150);

        assert_eq!(link_check_delay_from_deadline(now, Some(deadline)), Duration::from_millis(150));
    }

    #[test]
    fn link_check_delay_clamps_overdue_retries_to_minimum_delay() {
        let now = std::time::Instant::now();
        let deadline = now - Duration::from_millis(5);

        assert_eq!(link_check_delay_from_deadline(now, Some(deadline)), MIN_LINKS_CHECK_DELAY);
    }

    #[test]
    fn link_check_delay_keeps_default_sweep_without_pending_retries() {
        let now = std::time::Instant::now();

        assert_eq!(link_check_delay_from_deadline(now, None), INTERVAL_LINKS_CHECK);
    }
}
