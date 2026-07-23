use super::*;
use crate::outbound_resources;

pub(super) async fn cancel_tracked_resource_if_message_cancelled(
    daemon: &RpcDaemon,
    transport: &Transport,
    outbound_resource_map: &OutboundResourceMap,
    message_id: &str,
    link_id: AddressHash,
    resource_hash: rns_transport::hash::Hash,
) -> Result<bool, std::io::Error> {
    let status = daemon.message_receipt_status(message_id).map_err(std::io::Error::other)?;
    if !DeliveryTask::is_cancelled_status(status.as_deref()) {
        return Ok(false);
    }

    let cancelled = transport
        .cancel_resource(&link_id, resource_hash)
        .await
        .map_err(|err| std::io::Error::other(format!("resource cancel not sent: {err:?}")))?;
    if cancelled {
        outbound_resources::prune_outbound_resource_mappings_for_message(
            outbound_resource_map,
            message_id,
        );
    }
    Ok(cancelled)
}

pub(super) struct ResourceCancelMonitor {
    pub(super) daemon: Arc<RpcDaemon>,
    pub(super) transport: Arc<Transport>,
    pub(super) outbound_resource_map: OutboundResourceMap,
    pub(super) message_id: String,
    pub(super) destination_hex: String,
    pub(super) trace_stage: String,
    pub(super) link_id: AddressHash,
    pub(super) resource_hash: rns_transport::hash::Hash,
}

pub(super) fn spawn_tracked_resource_cancel_monitor(monitor: ResourceCancelMonitor) {
    tokio::spawn(async move {
        let resource_hash_hex = hex::encode(monitor.resource_hash.as_slice());
        let mut interval = tokio::time::interval(Duration::from_millis(250));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        for _ in 0..(30 * 60 * 4) {
            interval.tick().await;
            let still_tracked = match monitor.outbound_resource_map.lock() {
                Ok(guard) => guard.contains_key(&resource_hash_hex),
                Err(err) => {
                    log::warn!(
                        "[daemon] failed to read outbound resource map for cancel monitor message_id={} hash={resource_hash_hex}: {err}",
                        monitor.message_id
                    );
                    false
                }
            };
            if !still_tracked {
                return;
            }
            match cancel_tracked_resource_if_message_cancelled(
                monitor.daemon.as_ref(),
                monitor.transport.as_ref(),
                &monitor.outbound_resource_map,
                &monitor.message_id,
                monitor.link_id,
                monitor.resource_hash,
            )
            .await
            {
                Ok(true) => {
                    log_delivery_trace(
                        &monitor.message_id,
                        &monitor.destination_hex,
                        monitor.trace_stage.as_str(),
                        format!("resource_cancelled hash={resource_hash_hex}").as_str(),
                    );
                    return;
                }
                Ok(false) => {}
                Err(err) => {
                    log_delivery_trace(
                        &monitor.message_id,
                        &monitor.destination_hex,
                        monitor.trace_stage.as_str(),
                        format!("resource_cancel_failed hash={resource_hash_hex} err={err}")
                            .as_str(),
                    );
                    return;
                }
            }
        }
    });
}

impl DeliveryTask {
    fn track_link_packet_before_send(&self, packet: &Packet) {
        let packet_hash = hex::encode(packet.hash().to_bytes());
        track_receipt_mapping(&self.receipt_map, &packet_hash, &self.message_id);
    }

    fn track_resource_before_send(
        &self,
        trace_stage: &str,
        activity_peer: &str,
        payload_len: usize,
        sent_status: &str,
        link_id: AddressHash,
        resource_hash: rns_transport::hash::Hash,
    ) {
        let resource_hash_hex = hex::encode(resource_hash.as_slice());
        track_outbound_resource(
            &self.outbound_resource_map,
            resource_hash_hex.clone(),
            OutboundResourceTracking {
                message_id: self.message_id.clone(),
                peer: activity_peer.to_string(),
                bytes: payload_len,
                sent_status: sent_status.to_string(),
            },
        );
        spawn_tracked_resource_cancel_monitor(ResourceCancelMonitor {
            daemon: self.daemon.clone(),
            transport: self.transport.clone(),
            outbound_resource_map: self.outbound_resource_map.clone(),
            message_id: self.message_id.clone(),
            destination_hex: self.destination_hex.clone(),
            trace_stage: trace_stage.to_string(),
            link_id,
            resource_hash,
        });
    }

    pub(super) async fn send_via_link_mode(
        &self,
        trace_stage: &str,
        activity_peer: &str,
        destination_desc: DestinationDesc,
        payload: &[u8],
        statuses: LinkModeStatuses,
    ) -> Result<(), std::io::Error> {
        if self.abort_if_cancelled(trace_stage) {
            return Ok(());
        }
        log_delivery_trace(
            &self.message_id,
            &self.destination_hex,
            trace_stage,
            "opening or reusing link",
        );
        let link = self.transport.link(destination_desc).await;
        let link_id = *link.lock().await.id();
        self.direct_backchannel_links.record_outbound_local_source(link_id, self.source_hash);
        let result =
            match await_link_activation(self.transport.as_ref(), &link, Duration::from_secs(20))
                .await
            {
                Ok(()) => {
                    if let Err(err) = self.identify_link_for_backchannel(&link).await {
                        log::warn!(
                            "[daemon] {} direct link backchannel identify failed link={} err={}",
                            self.message_id,
                            link_id,
                            err
                        );
                    }
                    send_on_link_observed(
                        self.transport.as_ref(),
                        &link,
                        payload,
                        |packet| {
                            self.track_link_packet_before_send(packet);
                        },
                        |resource_hash| {
                            self.track_resource_before_send(
                                trace_stage,
                                activity_peer,
                                payload.len(),
                                statuses.resource_sent,
                                link_id,
                                resource_hash,
                            );
                        },
                    )
                    .await
                }
                Err(err) => Err(err),
            };
        if log::log_enabled!(log::Level::Trace) {
            let payload_starts_with_dst =
                payload.len() >= 16 && payload[..16] == self.destination[..];
            let detail = format!(
                "payload_len={} payload_prefix={} starts_with_dst={}",
                payload.len(),
                payload_preview(payload, 16),
                payload_starts_with_dst
            );
            log_delivery_trace(&self.message_id, &self.destination_hex, "payload", &detail);
        }

        match result {
            Ok(LinkSendResult::Packet(packet)) => {
                self.daemon.record_outbound_peer_sent(activity_peer, payload.len());
                let packet_hash = hex::encode(packet.hash().to_bytes());
                let detail = if log::log_enabled!(log::Level::Trace) {
                    format!(
                        "packet_hash={} packet_data_len={} packet_data_prefix={}",
                        packet_hash,
                        packet.data.len(),
                        payload_preview(packet.data.as_slice(), 16)
                    )
                } else {
                    format!("packet_hash={packet_hash}")
                };
                log_delivery_trace(&self.message_id, &self.destination_hex, trace_stage, &detail);
                emit_receipt_event(
                    &self.receipt_tx,
                    ReceiptEvent::new(self.message_id.clone(), statuses.packet)
                        .with_packet_hash(packet_hash)
                        .with_peer(activity_peer)
                        .with_method(trace_stage)
                        .with_delivery_kind("link-packet")
                        .with_bytes(payload.len())
                        .with_link_id(link_id.to_string()),
                );
                Ok(())
            }
            Ok(LinkSendResult::Resource(resource_hash)) => {
                let resource_hash_hex = hex::encode(resource_hash.as_slice());
                let detail = format!("resource_hash={resource_hash_hex}");
                log_delivery_trace(&self.message_id, &self.destination_hex, trace_stage, &detail);
                emit_receipt_event(
                    &self.receipt_tx,
                    ReceiptEvent::new(self.message_id.clone(), statuses.resource)
                        .with_resource_hash(resource_hash_hex)
                        .with_peer(activity_peer)
                        .with_method(trace_stage)
                        .with_delivery_kind("link-resource")
                        .with_bytes(payload.len())
                        .with_link_id(link_id.to_string()),
                );
                Ok(())
            }
            Err(err) => {
                self.daemon.record_outbound_peer_activity(activity_peer, payload.len(), false);
                Err(err)
            }
        }
    }

    async fn identify_link_for_backchannel(
        &self,
        link: &Arc<tokio::sync::Mutex<Link>>,
    ) -> Result<(), std::io::Error> {
        let link_id = *link.lock().await.id();
        let identify_payload =
            remote_control_link::build_link_identify_payload(&self.signer, &link_id);
        remote_control_link::send_link_context_packet(
            self.transport.as_ref(),
            link,
            PacketContext::LinkIdentify,
            identify_payload.as_slice(),
        )
        .await?;
        log_delivery_trace(
            &self.message_id,
            &self.destination_hex,
            "link",
            &format!("identified local delivery destination on link={link_id}"),
        );
        Ok(())
    }

    pub(super) async fn send_via_existing_link_mode(
        &self,
        trace_stage: &str,
        activity_peer: &str,
        link: Arc<tokio::sync::Mutex<Link>>,
        payload: &[u8],
        statuses: LinkModeStatuses,
    ) -> Result<(), std::io::Error> {
        let (activation_link_id, activation_status) = {
            let guard = link.lock().await;
            (*guard.id(), guard.status())
        };
        log_delivery_trace(
            &self.message_id,
            &self.destination_hex,
            trace_stage,
            format!(
                "waiting for link activation link={activation_link_id} status={activation_status:?}"
            )
            .as_str(),
        );
        if let Err(err) =
            await_link_activation(self.transport.as_ref(), &link, Duration::from_secs(20)).await
        {
            log_delivery_trace(
                &self.message_id,
                &self.destination_hex,
                trace_stage,
                format!("link activation wait failed link={activation_link_id} err={err}").as_str(),
            );
            return Err(err);
        }
        let active_status = link.lock().await.status();
        log_delivery_trace(
            &self.message_id,
            &self.destination_hex,
            trace_stage,
            format!("link activation ready link={activation_link_id} status={active_status:?}")
                .as_str(),
        );
        if self.abort_if_cancelled(trace_stage) {
            return Ok(());
        }
        let destination_desc = *link.lock().await.destination();
        let link_id = *link.lock().await.id();
        if trace_stage == "propagation" {
            let propagation_signal_rx = self.transport.received_data_events();
            let resource_hash = self
                .transport
                .send_resource_observed(&link_id, payload.to_vec(), None, |resource_hash| {
                    self.track_resource_before_send(
                        trace_stage,
                        activity_peer,
                        payload.len(),
                        statuses.resource_sent,
                        link_id,
                        resource_hash,
                    );
                })
                .await
                .map_err(|err| std::io::Error::other(format!("link resource not sent: {err:?}")))?;
            let resource_hash_hex = hex::encode(resource_hash.to_bytes());
            let detail = format!(
                "resource_hash={} bytes={} peer={} destination={}",
                resource_hash_hex,
                payload.len(),
                activity_peer,
                destination_desc.address_hash
            );
            log_delivery_trace(&self.message_id, &self.destination_hex, trace_stage, &detail);
            emit_receipt_event(
                &self.receipt_tx,
                ReceiptEvent::new(self.message_id.clone(), statuses.resource)
                    .with_resource_hash(resource_hash_hex)
                    .with_peer(activity_peer)
                    .with_method(trace_stage)
                    .with_delivery_kind("propagation-resource")
                    .with_bytes(payload.len())
                    .with_link_id(link_id.to_string()),
            );
            spawn_propagation_resource_signal_monitor(
                propagation_signal_rx,
                link_id,
                self.message_id.clone(),
                self.destination_hex.clone(),
                self.outbound_resource_map.clone(),
                self.receipt_tx.clone(),
            );
            return Ok(());
        }

        let mut propagation_signal_rx =
            (trace_stage == "propagation").then(|| self.transport.received_data_events());
        let result = send_on_link_observed(
            self.transport.as_ref(),
            &link,
            payload,
            |packet| {
                self.track_link_packet_before_send(packet);
            },
            |resource_hash| {
                self.track_resource_before_send(
                    trace_stage,
                    activity_peer,
                    payload.len(),
                    statuses.resource_sent,
                    link_id,
                    resource_hash,
                );
            },
        )
        .await;
        match result {
            Ok(LinkSendResult::Packet(packet)) => {
                let packet_hash = hex::encode(packet.hash().to_bytes());
                let detail = format!("packet_hash={packet_hash}");
                log_delivery_trace(&self.message_id, &self.destination_hex, trace_stage, &detail);
                if let Some(ref mut signal_rx) = propagation_signal_rx {
                    if let Ok(signal) = propagation::wait_for_propagation_signal(
                        signal_rx,
                        link_id,
                        Duration::from_millis(1500),
                    )
                    .await
                    {
                        if signal == propagation::PROPAGATION_INVALID_STAMP_SIGNAL {
                            return Err(std::io::Error::other(
                                "propagation node rejected message: invalid stamp",
                            ));
                        }
                        let detail = format!("signal=0x{signal:02x}");
                        log_delivery_trace(
                            &self.message_id,
                            &self.destination_hex,
                            "propagation",
                            &detail,
                        );
                    }
                }
                self.daemon.record_outbound_peer_sent(activity_peer, payload.len());
                emit_receipt_event(
                    &self.receipt_tx,
                    ReceiptEvent::new(self.message_id.clone(), statuses.packet)
                        .with_packet_hash(packet_hash)
                        .with_peer(activity_peer)
                        .with_method(trace_stage)
                        .with_delivery_kind("existing-link-packet")
                        .with_bytes(payload.len())
                        .with_link_id(link_id.to_string()),
                );
                Ok(())
            }
            Ok(LinkSendResult::Resource(resource_hash)) => {
                let resource_hash_hex = hex::encode(resource_hash.to_bytes());
                let detail = format!(
                    "resource_hash={} bytes={} peer={} destination={}",
                    resource_hash_hex,
                    payload.len(),
                    activity_peer,
                    destination_desc.address_hash
                );
                log_delivery_trace(&self.message_id, &self.destination_hex, trace_stage, &detail);
                emit_receipt_event(
                    &self.receipt_tx,
                    ReceiptEvent::new(self.message_id.clone(), statuses.resource)
                        .with_resource_hash(resource_hash_hex)
                        .with_peer(activity_peer)
                        .with_method(trace_stage)
                        .with_delivery_kind("existing-link-resource")
                        .with_bytes(payload.len())
                        .with_link_id(link_id.to_string()),
                );
                Ok(())
            }
            Err(err) => Err(err),
        }
    }
}

fn spawn_propagation_resource_signal_monitor(
    mut signal_rx: tokio::sync::broadcast::Receiver<rns_transport::transport::ReceivedData>,
    link_id: AddressHash,
    message_id: String,
    destination_hex: String,
    outbound_resource_map: OutboundResourceMap,
    receipt_tx: tokio::sync::mpsc::Sender<ReceiptEvent>,
) {
    tokio::spawn(async move {
        let Ok(signal) = propagation::wait_for_propagation_signal(
            &mut signal_rx,
            link_id,
            Duration::from_secs(30),
        )
        .await
        else {
            return;
        };
        let detail = format!("resource_signal=0x{signal:02x}");
        log_delivery_trace(&message_id, &destination_hex, "propagation", &detail);
        if signal != propagation::PROPAGATION_INVALID_STAMP_SIGNAL {
            return;
        }
        outbound_resources::prune_outbound_resource_mappings_for_message(
            &outbound_resource_map,
            &message_id,
        );
        emit_receipt_event(
            &receipt_tx,
            ReceiptEvent::new(
                message_id,
                "failed: propagation node rejected message: invalid stamp",
            )
            .with_delivery_kind("propagation-resource")
            .with_link_id(link_id.to_string()),
        );
    });
}
