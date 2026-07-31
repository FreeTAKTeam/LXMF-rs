use super::*;

#[allow(dead_code)]
pub(in crate::transport) async fn handle_check_links<'a>(
    mut handler: MutexGuard<'a, TransportHandler>,
) {
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
