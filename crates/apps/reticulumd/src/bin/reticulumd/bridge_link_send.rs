use super::*;
use crate::outbound_resources;

impl DeliveryTask {
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
        if diagnostics_enabled() {
            log_delivery_trace(
                &self.message_id,
                &self.destination_hex,
                trace_stage,
                "opening or reusing link",
            );
        }
        let result = send_via_link(
            self.transport.as_ref(),
            destination_desc,
            payload,
            Duration::from_secs(20),
        )
        .await;
        if diagnostics_enabled() {
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
                self.daemon.record_outbound_peer_activity(activity_peer, payload.len(), true);
                let packet_hash = hex::encode(packet.hash().to_bytes());
                track_receipt_mapping(&self.receipt_map, &packet_hash, &self.message_id);
                let detail = if diagnostics_enabled() {
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
                let _ = self.receipt_tx.try_send(ReceiptEvent {
                    message_id: self.message_id.clone(),
                    status: statuses.packet.to_string(),
                });
                Ok(())
            }
            Ok(LinkSendResult::Resource(resource_hash)) => {
                let resource_hash_hex = hex::encode(resource_hash.as_slice());
                track_outbound_resource(
                    &self.outbound_resource_map,
                    resource_hash_hex.clone(),
                    OutboundResourceTracking {
                        message_id: self.message_id.clone(),
                        peer: activity_peer.to_string(),
                        bytes: payload.len(),
                        sent_status: statuses.resource_sent.to_string(),
                    },
                );
                let detail = format!("resource_hash={resource_hash_hex}");
                log_delivery_trace(&self.message_id, &self.destination_hex, trace_stage, &detail);
                let _ = self.receipt_tx.try_send(ReceiptEvent {
                    message_id: self.message_id.clone(),
                    status: statuses.resource.to_string(),
                });
                Ok(())
            }
            Err(err) => {
                self.daemon.record_outbound_peer_activity(activity_peer, payload.len(), false);
                Err(err)
            }
        }
    }

    pub(super) async fn send_via_existing_link_mode(
        &self,
        trace_stage: &str,
        activity_peer: &str,
        link: Arc<tokio::sync::Mutex<Link>>,
        payload: &[u8],
        statuses: LinkModeStatuses,
    ) -> Result<(), std::io::Error> {
        await_link_activation(self.transport.as_ref(), &link, Duration::from_secs(20)).await?;
        if self.abort_if_cancelled(trace_stage) {
            return Ok(());
        }
        let destination_desc = *link.lock().await.destination();
        let link_id = *link.lock().await.id();
        if trace_stage == "propagation" {
            let propagation_signal_rx = self.transport.received_data_events();
            let resource_hash =
                self.transport.send_resource(&link_id, payload.to_vec(), None).await.map_err(
                    |err| std::io::Error::other(format!("link resource not sent: {err:?}")),
                )?;
            let resource_hash_hex = hex::encode(resource_hash.to_bytes());
            track_outbound_resource(
                &self.outbound_resource_map,
                resource_hash_hex.clone(),
                OutboundResourceTracking {
                    message_id: self.message_id.clone(),
                    peer: activity_peer.to_string(),
                    bytes: payload.len(),
                    sent_status: statuses.resource_sent.to_string(),
                },
            );
            let detail = format!(
                "resource_hash={} bytes={} peer={} destination={}",
                resource_hash_hex,
                payload.len(),
                activity_peer,
                destination_desc.address_hash
            );
            log_delivery_trace(&self.message_id, &self.destination_hex, trace_stage, &detail);
            let _ = self.receipt_tx.try_send(ReceiptEvent {
                message_id: self.message_id.clone(),
                status: statuses.resource.to_string(),
            });
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
        let result = send_on_link(self.transport.as_ref(), &link, payload).await;
        match result {
            Ok(LinkSendResult::Packet(packet)) => {
                let packet_hash = hex::encode(packet.hash().to_bytes());
                track_receipt_mapping(&self.receipt_map, &packet_hash, &self.message_id);
                let detail = format!("packet_hash={packet_hash}");
                log_delivery_trace(&self.message_id, &self.destination_hex, trace_stage, &detail);
                if let Some(ref mut signal_rx) = propagation_signal_rx {
                    if let Some(signal) = propagation::wait_for_propagation_signal(
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
                self.daemon.record_outbound_peer_activity(activity_peer, payload.len(), true);
                let _ = self.receipt_tx.try_send(ReceiptEvent {
                    message_id: self.message_id.clone(),
                    status: statuses.packet.to_string(),
                });
                Ok(())
            }
            Ok(LinkSendResult::Resource(resource_hash)) => {
                let resource_hash_hex = hex::encode(resource_hash.to_bytes());
                track_outbound_resource(
                    &self.outbound_resource_map,
                    resource_hash_hex.clone(),
                    OutboundResourceTracking {
                        message_id: self.message_id.clone(),
                        peer: activity_peer.to_string(),
                        bytes: payload.len(),
                        sent_status: statuses.resource_sent.to_string(),
                    },
                );
                let detail = format!(
                    "resource_hash={} bytes={} peer={} destination={}",
                    resource_hash_hex,
                    payload.len(),
                    activity_peer,
                    destination_desc.address_hash
                );
                log_delivery_trace(&self.message_id, &self.destination_hex, trace_stage, &detail);
                let _ = self.receipt_tx.try_send(ReceiptEvent {
                    message_id: self.message_id.clone(),
                    status: statuses.resource.to_string(),
                });
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
        let Some(signal) = propagation::wait_for_propagation_signal(
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
        let _ = receipt_tx.try_send(ReceiptEvent {
            message_id,
            status: "failed: propagation node rejected message: invalid stamp".to_string(),
        });
    });
}
