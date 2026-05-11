use super::announce::{
    handle_validated_announce_unlocked, release_held_announces_unlocked, retransmit_announces,
    validate_announce_on_worker,
};
use super::path::{handle_fixed_destinations_unlocked, handle_link_request_unlocked};
use super::wire::{
    handle_data, handle_link_resource_data, handle_local_single_destination_data, handle_proof,
    is_resource_data_packet,
};
use super::*;
use crate::destination::link::LinkWatchdogAction;
use crate::resource::ResourceRequest;

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

fn build_ready_resource_retry_packets(
    request_jobs: Vec<(Arc<Mutex<Link>>, ResourceRequest)>,
    advertisements: Vec<Packet>,
) -> Vec<Packet> {
    let mut packets = Vec::with_capacity(request_jobs.len() + advertisements.len());
    for (link, request) in request_jobs {
        let Ok(link_guard) = link.try_lock() else {
            log::debug!("resource: skipping retry request for busy link until next retry tick");
            continue;
        };
        packets.push(build_resource_request_packet(&link_guard, &request));
    }
    packets.extend(advertisements);
    packets
}

fn ready_link_check_deadline(
    in_links: Vec<Arc<Mutex<Link>>>,
    out_links: Vec<Arc<Mutex<Link>>>,
) -> Option<std::time::Instant> {
    let mut earliest_deadline = None;
    for link in in_links {
        let Ok(link) = link.try_lock() else {
            log::debug!("tp: skipping busy input link while scheduling link checks");
            continue;
        };
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
        let Ok(link) = link.try_lock() else {
            log::debug!("tp: skipping busy output link while scheduling link checks");
            continue;
        };
        for deadline in
            [link.next_channel_retry_at(), link.next_watchdog_deadline(true)].into_iter().flatten()
        {
            earliest_deadline = Some(match earliest_deadline {
                Some(current) => std::cmp::min(current, deadline),
                None => deadline,
            });
        }
    }
    earliest_deadline
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
    link_check_delay_from_deadline(now, ready_link_check_deadline(in_links, out_links))
}

pub(super) async fn handle_check_links(handler_arc: Arc<Mutex<TransportHandler>>) {
    let mut links_to_remove: Vec<AddressHash> = Vec::new();
    let mut closed_link_ids: Vec<AddressHash> = Vec::new();
    let mut pending_packets: Vec<Packet> = Vec::new();
    let mut direct_messages: Vec<TxMessage> = Vec::new();
    let now = std::time::Instant::now();
    let (config_name, in_links, out_links, resource_lane) = {
        let handler = handler_arc.lock().await;
        (
            handler.config.name.clone(),
            handler.in_links.iter().map(|(addr, link)| (*addr, link.clone())).collect::<Vec<_>>(),
            handler.out_links.iter().map(|(addr, link)| (*addr, link.clone())).collect::<Vec<_>>(),
            handler.resource_lane.clone(),
        )
    };

    // Clean up input links
    for (addr, link) in &in_links {
        let Ok(mut link) = link.try_lock() else {
            log::debug!("tp: skipping busy input link during link check sweep");
            continue;
        };
        if let Some(iface) = link.ingress_iface() {
            for packet in link.poll_channel_timeouts(now) {
                direct_messages.push(TxMessage { tx_type: TxMessageType::Direct(iface), packet });
            }
        }
        match link.status() {
            LinkStatus::Closed => {
                links_to_remove.push(*addr);
                closed_link_ids.push(*link.id());
            }
            LinkStatus::Pending | LinkStatus::Handshake => {
                if link.elapsed() > INTERVAL_INPUT_LINK_CLEANUP {
                    link.close();
                    links_to_remove.push(*addr);
                    closed_link_ids.push(*link.id());
                }
            }
            LinkStatus::Active | LinkStatus::Stale => {
                if let LinkWatchdogAction::SendTeardown(packet) = link.check_watchdog(false) {
                    if let Some(iface) = link.ingress_iface() {
                        direct_messages
                            .push(TxMessage { tx_type: TxMessageType::Direct(iface), packet });
                    }
                    links_to_remove.push(*addr);
                    closed_link_ids.push(*link.id());
                }
            }
        }
    }

    {
        let mut handler = handler_arc.lock().await;
        for addr in &links_to_remove {
            handler.in_links.remove(addr);
        }
    }
    resource_lane.remove_link_state(closed_link_ids.clone()).await;

    links_to_remove.clear();
    closed_link_ids.clear();

    for (addr, link) in &out_links {
        let Ok(mut link) = link.try_lock() else {
            log::debug!("tp: skipping busy output link during link check sweep");
            continue;
        };
        if let Some(iface) = link.ingress_iface() {
            for packet in link.poll_channel_timeouts(now) {
                direct_messages.push(TxMessage { tx_type: TxMessageType::Direct(iface), packet });
            }
        }
        match link.status() {
            LinkStatus::Closed => {
                links_to_remove.push(*addr);
                closed_link_ids.push(*link.id());
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
                    links_to_remove.push(*addr);
                    closed_link_ids.push(*link.id());
                }
                LinkWatchdogAction::None => {}
            },
            LinkStatus::Pending => {
                if link.elapsed() > INTERVAL_OUTPUT_LINK_REPEAT {
                    log::warn!("tp({}): repeat link request {}", config_name, link.id());
                    pending_packets.push(link.request());
                }
            }
            LinkStatus::Handshake => {}
        }
    }

    {
        let mut handler = handler_arc.lock().await;
        for addr in &links_to_remove {
            handler.out_links.remove(addr);
        }
    }
    resource_lane.remove_link_state(closed_link_ids.clone()).await;

    for packet in pending_packets {
        let _ =
            TransportHandler::send_packet_with_trace_unlocked(handler_arc.clone(), packet).await;
    }
    for message in direct_messages {
        let _ = TransportHandler::send_message_unlocked(handler_arc.clone(), message).await;
    }
}

pub(super) async fn handle_cleanup(handler_arc: Arc<Mutex<TransportHandler>>) {
    TransportHandler::gc_unicast_ifaces_unlocked(handler_arc.clone()).await;

    let iface_manager = { handler_arc.lock().await.iface_manager.clone() };
    cleanup_path_state_unlocked(handler_arc, iface_manager).await;
}

pub(super) async fn cleanup_path_state_unlocked(
    handler_arc: Arc<Mutex<TransportHandler>>,
    iface_manager: Arc<Mutex<InterfaceManager>>,
) {
    let iface_modes = {
        let mut iface_manager = iface_manager.lock().await;
        iface_manager.cleanup();
        iface_manager
            .path_metadata()
            .into_iter()
            .map(|(address, mode, _)| (address, mode))
            .collect::<HashMap<_, _>>()
    };
    let now = std::time::Instant::now();
    let mut handler = handler_arc.lock().await;
    handler.path_table.remove_stale(now, |iface| iface_modes.get(iface).copied());
    handler.tunnel_table.remove_stale(now);
}

async fn release_queued_announces_unlocked(handler: Arc<Mutex<TransportHandler>>) {
    let iface_manager = { handler.lock().await.iface_manager.clone() };
    let (trace, work) = { iface_manager.lock().await.plan_release_queued_announces() };
    let _ = InterfaceManager::dispatch_tx_work(trace, work).await;
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
                if cancel.is_cancelled() {
                    break;
                }

                let message = {
                    let mut rx_receiver = rx_receiver.lock().await;
                    tokio::select! {
                        _ = cancel.cancelled() => {
                            break;
                        },
                        message = rx_receiver.recv() => message,
                    }
                };

                let Some(message) = message else {
                    break;
                };

                let _ = iface_messages_tx.send(message);

                let packet = message.packet;

                if PACKET_TRACE {
                    log::debug!("tp: << rx({}) = {} {}", message.address, packet, packet.hash());
                }

                let (handled_fixed, fixed_message) = handle_fixed_destinations_unlocked(
                    &packet,
                    handler_arc.clone(),
                    message.address,
                )
                .await;
                if handled_fixed {
                    if let Some(message) = fixed_message {
                        let _ =
                            TransportHandler::send_message_unlocked(handler_arc.clone(), message)
                                .await;
                    }
                    continue;
                }

                let handler = handler_arc.lock().await;
                let config_name = handler.config.name.clone();
                drop(handler);

                if !TransportHandler::filter_duplicate_packets_unlocked(
                    handler_arc.clone(),
                    &packet,
                )
                .await
                {
                    log::debug!(
                        "tp({}): dropping duplicate packet: dst={}, ctx={:?}, type={:?}",
                        config_name,
                        packet.destination,
                        packet.context,
                        packet.header.packet_type
                    );
                    continue;
                }

                let handler = handler_arc.lock().await;
                match packet.header.packet_type {
                    PacketType::Announce => {
                        let announce_worker_backend = handler.announce_worker_backend.clone();
                        drop(handler);
                        let handler = handler_arc.clone();
                        let iface = message.address;
                        let source = message.source;
                        tokio::spawn(async move {
                            let announce =
                                match validate_announce_on_worker(packet, announce_worker_backend)
                                    .await
                                {
                                    Ok(result) => result,
                                    Err(err) => {
                                        log::trace!(
                                            "[transport] announce validate failed dst={} err={:?}",
                                            packet.destination,
                                            err
                                        );
                                        return;
                                    }
                                };
                            handle_validated_announce_unlocked(
                                &packet, handler, iface, source, announce,
                            )
                            .await;
                        });
                    }
                    PacketType::LinkRequest => {
                        drop(handler);
                        handle_link_request_unlocked(&packet, message.address, handler_arc.clone())
                            .await;
                    }
                    PacketType::Proof => {
                        drop(handler);
                        handle_proof(packet, handler_arc.clone(), message.address).await;
                    }
                    PacketType::Data if is_resource_data_packet(&packet) => {
                        drop(handler);
                        if !handle_link_resource_data(packet, handler_arc.clone()).await {
                            handle_data(
                                &packet,
                                message.address,
                                handler_arc.clone(),
                                handler_arc.lock().await,
                            )
                            .await;
                        }
                    }
                    PacketType::Data
                        if packet.header.destination_type == DestinationType::Single =>
                    {
                        let local_destination =
                            handler.single_in_destinations.get(&packet.destination).cloned();
                        if let Some(destination) = local_destination {
                            let received_data_tx = handler.received_data_tx.clone();
                            let config_name = handler.config.name.clone();
                            let single_destination_worker_backend =
                                handler.single_destination_worker_backend.clone();
                            drop(handler);
                            handle_local_single_destination_data(
                                &packet,
                                destination,
                                received_data_tx,
                                &config_name,
                                single_destination_worker_backend,
                            )
                            .await;
                            log::trace!(
                                "tp({}): handle data request for {} dst={:2x} ctx={:2x}",
                                config_name,
                                packet.destination,
                                packet.header.destination_type as u8,
                                packet.context as u8,
                            );
                        } else {
                            handle_data(&packet, message.address, handler_arc.clone(), handler)
                                .await;
                        }
                    }
                    PacketType::Data => {
                        handle_data(&packet, message.address, handler_arc.clone(), handler).await
                    }
                }
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
                        handle_check_links(handler.clone()).await;
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
                        handle_cleanup(handler.clone()).await;
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
                        let packet_cache = { handler.lock().await.packet_cache.clone() };
                        packet_cache.lock().await.release(INTERVAL_KEEP_PACKET_CACHED);

                        handler.lock().await.link_table.remove_stale();
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
                        if retransmit {
                            retransmit_announces(handler.clone()).await;
                        } else {
                            release_held_announces_unlocked(handler.clone()).await;
                            release_queued_announces_unlocked(handler.clone()).await;
                            continue;
                        }
                        release_held_announces_unlocked(handler.clone()).await;
                        release_queued_announces_unlocked(handler.clone()).await;
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
                        let now = Instant::now();
                        let (request_jobs, advertisements) = {
                            let (resource_lane, in_links, out_links) = {
                                let handler = handler.lock().await;
                                (
                                    handler.resource_lane.clone(),
                                    handler.in_links.clone(),
                                    handler.out_links.clone(),
                                )
                            };
                            let (requests, outgoing_packets) = resource_lane.retry_poll(now).await;
                            let request_jobs = requests
                                .into_iter()
                                .filter_map(|(link_id, request)| {
                                    in_links
                                        .get(&link_id)
                                        .cloned()
                                        .or_else(|| out_links.get(&link_id).cloned())
                                        .map(|link| (link, request))
                                })
                                .collect::<Vec<_>>();
                            let advertisements = outgoing_packets
                                .into_iter()
                                .map(|(_link_id, packet)| packet)
                                .collect::<Vec<_>>();
                            (request_jobs, advertisements)
                        };
                        let packets = build_ready_resource_retry_packets(request_jobs, advertisements);
                        for packet in packets {
                            let _ = TransportHandler::send_packet_with_trace_unlocked(
                                handler.clone(),
                                packet,
                            )
                            .await;
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
    use crate::destination::DestinationDesc;
    use crate::identity::PrivateIdentity;
    use crate::resource::MAPHASH_LEN;
    use rand_core::OsRng;
    use tokio::time::timeout;

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

    #[tokio::test]
    async fn resource_retry_packet_building_skips_busy_links_without_blocking_advertisements() {
        let identity = PrivateIdentity::new_from_rand(OsRng);
        let destination = DestinationDesc {
            identity: *identity.as_identity(),
            address_hash: identity.as_identity().address_hash,
            name: DestinationName::new("lxmf", "delivery"),
        };
        let (events, _) = tokio::sync::broadcast::channel(8);
        let link = Arc::new(Mutex::new(Link::new(destination, events)));
        let _busy_link = link.lock().await;
        let request = ResourceRequest {
            hashmap_exhausted: false,
            last_map_hash: None,
            resource_hash: Hash::new_from_slice(b"resource"),
            requested_hashes: vec![[0x42; MAPHASH_LEN]],
        };
        let advertisement = Packet {
            context: PacketContext::ResourceAdvrtisement,
            data: PacketDataBuffer::new_from_slice(b"advertisement"),
            ..Default::default()
        };

        let packets =
            build_ready_resource_retry_packets(vec![(link.clone(), request)], vec![advertisement]);

        assert_eq!(packets, vec![advertisement]);
    }

    #[tokio::test]
    async fn link_check_deadline_skips_busy_links_without_blocking_ready_links() {
        let signer = PrivateIdentity::new_from_rand(OsRng);
        let identity = *signer.as_identity();
        let destination = DestinationDesc {
            identity,
            address_hash: identity.address_hash,
            name: DestinationName::new("lxmf", "delivery"),
        };
        let (events, _) = tokio::sync::broadcast::channel(8);

        let busy_link = Arc::new(Mutex::new(Link::new(destination, events.clone())));
        let _busy_guard = busy_link.lock().await;

        let mut ready_link = Link::new(destination, events.clone());
        let request = ready_link.request();
        let mut inbound =
            Link::new_from_request(&request, signer.sign_key().clone(), destination, events)
                .expect("link request should parse");
        let iface = AddressHash::new_from_rand(OsRng);
        assert!(matches!(
            ready_link.handle_packet(&inbound.prove(), iface),
            LinkHandleResult::Activated
        ));
        ready_link.send_channel_message(0x5151, b"pending".to_vec()).expect("channel message");
        let expected = ready_link.next_channel_retry_at().expect("retry deadline");
        let ready_link = Arc::new(Mutex::new(ready_link));

        let deadline = ready_link_check_deadline(vec![busy_link.clone()], vec![ready_link])
            .expect("ready link deadline should be returned");

        assert_eq!(deadline, expected);
    }

    #[tokio::test]
    async fn link_check_sweep_skips_busy_links_without_blocking_ready_cleanup() {
        let local_identity = PrivateIdentity::new_from_rand(OsRng);
        let transport = Transport::new(TransportConfig::new("test", &local_identity, true));
        let handler = transport.get_handler();

        let busy_identity = PrivateIdentity::new_from_rand(OsRng);
        let busy_destination = DestinationDesc {
            identity: *busy_identity.as_identity(),
            address_hash: busy_identity.as_identity().address_hash,
            name: DestinationName::new("lxmf", "busy"),
        };
        let closed_identity = PrivateIdentity::new_from_rand(OsRng);
        let closed_destination = DestinationDesc {
            identity: *closed_identity.as_identity(),
            address_hash: closed_identity.as_identity().address_hash,
            name: DestinationName::new("lxmf", "closed"),
        };
        let (events, _) = tokio::sync::broadcast::channel(8);
        let busy_key = AddressHash::new_from_rand(OsRng);
        let closed_key = AddressHash::new_from_rand(OsRng);
        let busy_link = Arc::new(Mutex::new(Link::new(busy_destination, events.clone())));
        let closed_link = Arc::new(Mutex::new(Link::new(closed_destination, events)));
        closed_link.lock().await.close();
        {
            let mut handler = handler.lock().await;
            handler.out_links.insert(busy_key, busy_link.clone());
            handler.out_links.insert(closed_key, closed_link);
        }

        let _busy_guard = busy_link.lock().await;
        timeout(Duration::from_millis(200), handle_check_links(handler.clone()))
            .await
            .expect("link check sweep should not block on a busy unrelated link");

        let handler = handler.lock().await;
        assert!(handler.out_links.contains_key(&busy_key));
        assert!(!handler.out_links.contains_key(&closed_key));
    }
}
