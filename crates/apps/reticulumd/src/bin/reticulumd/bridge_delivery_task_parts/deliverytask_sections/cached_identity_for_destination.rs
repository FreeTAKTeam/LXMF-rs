impl DeliveryTask {

    fn cached_identity_for_destination(&self, destination_hash: AddressHash) -> Option<Identity> {
        identity_resolver::cached_identity_for_destination(
            destination_hash,
            self.peer_identity,
            self.propagation_node_identity,
            &self.peer_crypto,
            &self.outbound_propagation_identities,
        )
    }

    pub(super) fn start_delivery_trace(&self) {
        log_delivery_trace(&self.message_id, &self.destination_hex, "start", "delivery requested");
    }

    #[cfg(test)]
    pub(super) async fn run(self) {
        self.start_delivery_trace();
        if self.abort_if_cancelled("start") {
            return;
        }
        let payload = match self.build_payload().await {
            Ok(payload) => payload,
            Err(err) => {
                if self.abort_if_cancelled("payload") {
                    return;
                }
                self.fail_payload_build(err);
                return;
            }
        };
        self.run_prepared(
            PreparedDeliveryPayload {
                lxmf_payload: payload,
                propagation: None,
            },
            Arc::new(tokio::sync::Semaphore::new(1)),
        )
        .await;
    }

    pub(super) async fn run_prepared(
        self,
        prepared: PreparedDeliveryPayload,
        stamp_limit: Arc<tokio::sync::Semaphore>,
    ) {
        if self.abort_if_cancelled("payload") {
            return;
        }
        match self.requested_method {
            RequestedDeliveryMethod::Direct => self.run_direct(prepared.lxmf_payload, stamp_limit).await,
            RequestedDeliveryMethod::Opportunistic => {
                self.run_opportunistic(prepared.lxmf_payload).await;
            }
            RequestedDeliveryMethod::Propagated => {
                if let Some(propagation) = prepared.propagation {
                    self.send_prepared_propagated(propagation).await;
                } else {
                    self.run_propagated(prepared.lxmf_payload, stamp_limit).await;
                }
            }
            RequestedDeliveryMethod::Paper => {}
        }
    }

    pub(super) fn fail_payload_build(&self, err: std::io::Error) {
        let _ = self.receipt_tx.try_send(ReceiptEvent {
            message_id: self.message_id.clone(),
            status: format!("failed: {err}"),
        });
    }

    async fn run_direct(self, payload: Vec<u8>, stamp_limit: Arc<tokio::sync::Semaphore>) {
        if self.abort_if_cancelled("link") {
            return;
        }
        let Some(identity) = self.resolve_destination_identity().await else {
            return;
        };
        if self.abort_if_cancelled("link") {
            return;
        }
        let destination_desc = DestinationDesc {
            identity,
            address_hash: self.destination_hash,
            name: DestinationName::new("lxmf", "delivery"),
        };

        match self
            .send_via_link_mode(
                "link",
                self.destination_hex.as_str(),
                destination_desc,
                &payload,
                LinkModeStatuses {
                    packet: "sent: link",
                    resource: "sending: link resource",
                    resource_sent: OUTBOUND_RESOURCE_SENT_STATUS,
                },
            )
            .await
        {
            Ok(()) => {}
            Err(err) if self.try_propagation_on_fail && self.propagation_node_hex.is_some() => {
                let detail = format!("direct failed err={err}; trying propagated");
                log_delivery_trace(&self.message_id, &self.destination_hex, "link", &detail);
                let _ = self.receipt_tx.try_send(ReceiptEvent {
                    message_id: self.message_id.clone(),
                    status: format!("link failed: {err}; trying propagated"),
                });
                self.run_propagated(payload, stamp_limit).await;
            }
            Err(err) => {
                let detail = format!("direct failed err={err}");
                log_delivery_trace(&self.message_id, &self.destination_hex, "link", &detail);
                let _ = self.receipt_tx.try_send(ReceiptEvent {
                    message_id: self.message_id,
                    status: format!("failed: {err}"),
                });
            }
        }
    }

    async fn run_propagated(self, payload: Vec<u8>, stamp_limit: Arc<tokio::sync::Semaphore>) {
        if self.abort_if_cancelled("propagation") {
            return;
        }
        let Some(context) = self.propagation_preparation_context().await else {
            return;
        };
        log_delivery_trace(
            &self.message_id,
            &self.destination_hex,
            "propagation",
            "building propagation payload",
        );
        self.record_propagation_stamp_work_metadata("queued", context.target_cost, None);
        let mut propagation_payload = None;
        for attempt in 1..=2u32 {
            let _stamp_permit = match stamp_limit.clone().acquire_owned().await {
                Ok(permit) => permit,
                Err(_) => {
                    let _ = self.receipt_tx.try_send(ReceiptEvent {
                        message_id: self.message_id,
                        status: "failed: stamp worker stopped".to_string(),
                    });
                    return;
                }
            };
            self.record_propagation_stamp_attempt_metadata(context.target_cost, attempt);
            if self.abort_if_cancelled("propagation") {
                self.record_propagation_stamp_work_metadata("cancelled", context.target_cost, None);
                return;
            }
            let result = propagation::build_propagation_payload_until_cancelled(
                &payload,
                &context.destination_identity,
                context.target_cost,
                || {
                    let status = self.daemon.message_receipt_status(&self.message_id).ok().flatten();
                    Self::is_cancelled_status(status.as_deref())
                },
            );
            drop(_stamp_permit);
            match result {
                Ok(payload) => {
                    propagation_payload = Some(payload);
                    break;
                }
                Err(err) => {
                    if self.abort_if_cancelled("propagation") {
                        self.record_propagation_stamp_work_metadata(
                            "cancelled",
                            context.target_cost,
                            None,
                        );
                        return;
                    }
                    if attempt < 2 {
                        self.record_propagation_stamp_retry_metadata(
                            context.target_cost,
                            attempt,
                            err.to_string(),
                        );
                        tokio::time::sleep(Duration::from_millis(100)).await;
                        continue;
                    }
                    self.record_propagation_stamp_work_metadata(
                        "failed",
                        context.target_cost,
                        Some(err.to_string()),
                    );
                    let _ = self.receipt_tx.try_send(ReceiptEvent {
                        message_id: self.message_id,
                        status: format!("failed: {err}"),
                    });
                    return;
                }
            }
        }
        let Some(propagation_payload) = propagation_payload else {
            return;
        };
        self.record_propagation_stamp_work_metadata(
            "ready",
            context.target_cost,
            Some(propagation_payload.stamp_value.to_string()),
        );
        self.record_propagation_payload_metadata(&propagation_payload, context.target_cost);
        self
            .send_prepared_propagated(PreparedPropagationPayload {
                propagation_node_hex: context.propagation_node_hex,
                propagation_hash: context.propagation_hash,
                target_cost: context.target_cost,
                payload: propagation_payload,
            })
            .await;
    }

    async fn send_prepared_propagated(self, prepared: PreparedPropagationPayload) {
        if self.selected_propagation_node_is_local(prepared.propagation_node_hex.as_str()) {
            match self.store_local_propagation_payload(
                prepared.propagation_node_hex.as_str(),
                &prepared.payload,
            ) {
                Ok(()) => {
                    let _ = self.receipt_tx.try_send(ReceiptEvent {
                        message_id: self.message_id,
                        status: "sent: propagated resource".to_string(),
                    });
                }
                Err(err) => {
                    let _ = self.receipt_tx.try_send(ReceiptEvent {
                        message_id: self.message_id,
                        status: format!("failed: {err}"),
                    });
                }
            }
            return;
        }
        let payload = prepared.payload.bytes;
        log_delivery_trace(
            &self.message_id,
            &self.destination_hex,
            "propagation",
            format!(
                "propagation payload ready bytes={} target_cost={}",
                payload.len(),
                prepared.target_cost
            )
            .as_str(),
        );
        if self.abort_if_cancelled("propagation") {
            return;
        }

        log_delivery_trace(
            &self.message_id,
            &self.destination_hex,
            "propagation",
            "resolving propagation link",
        );
        let propagation_link = match self
            .resolve_or_create_propagation_link(
                &prepared.propagation_node_hex,
                prepared.propagation_hash,
            )
            .await
        {
            Ok(link) => link,
            Err(err) => {
                let _ = self.receipt_tx.try_send(ReceiptEvent {
                    message_id: self.message_id,
                    status: format!("failed: {err}"),
                });
                return;
            }
        };
        let (link_id, link_status) = {
            let guard = propagation_link.lock().await;
            (*guard.id(), guard.status())
        };
        log_delivery_trace(
            &self.message_id,
            &self.destination_hex,
            "propagation",
            format!("propagation link ready link={link_id} status={link_status:?}").as_str(),
        );
        if self.abort_if_cancelled("propagation") {
            return;
        }

        if let Err(err) = self
            .send_via_existing_link_mode(
                "propagation",
                prepared.propagation_node_hex.as_str(),
                propagation_link,
                &payload,
                LinkModeStatuses {
                    packet: "sent: propagated",
                    resource: "sending: propagated resource",
                    resource_sent: "sent: propagated resource",
                },
            )
            .await
        {
            let detail = format!("propagated failed err={err}");
            log_delivery_trace(&self.message_id, &self.destination_hex, "propagation", &detail);
            let _ = self.receipt_tx.try_send(ReceiptEvent {
                message_id: self.message_id,
                status: format!("failed: {err}"),
            });
        }
    }

    async fn run_opportunistic(self, payload: Vec<u8>) {
        if self.abort_if_cancelled("opportunistic") {
            return;
        }
        let Some(identity) = self.resolve_destination_identity().await else {
            return;
        };
        // Opportunistic SINGLE packets must carry LXMF wire bytes
        // without the destination prefix. Receivers prepend the
        // packet destination hash before unpacking.
        let opportunistic_payload = opportunistic_payload(&payload, &self.destination);
        if opportunistic_payload.len() > rns_transport::packet::PACKET_MDU {
            log_delivery_trace(
                &self.message_id,
                &self.destination_hex,
                "opportunistic",
                "payload too large for single packet",
            );
            self.run_opportunistic_link_fallback(
                payload,
                identity,
                "payload too large for single packet",
            )
            .await;
            return;
        }
        let mut data = PacketDataBuffer::new();
        if data.write(opportunistic_payload).is_err() {
            log_delivery_trace(
                &self.message_id,
                &self.destination_hex,
                "opportunistic",
                "payload too large",
            );
            self.run_opportunistic_link_fallback(payload, identity, "payload too large").await;
            return;
        }

        let mut packet = Packet {
            header: Header {
                ifac_flag: IfacFlag::Open,
                header_type: HeaderType::Type1,
                context_flag: ContextFlag::Unset,
                propagation_type: PropagationType::Broadcast,
                destination_type: DestinationType::Single,
                packet_type: PacketType::Data,
                hops: 0,
            },
            ifac: None,
            destination: self.destination_hash,
            transport: None,
            context: PacketContext::None,
            data,
        };
        let ciphertext = match encrypt_for_public_key(
            &identity.public_key,
            identity.address_hash.as_slice(),
            packet.data.as_slice(),
            OsRng,
        ) {
            Ok(ciphertext) => ciphertext,
            Err(err) => {
                log_delivery_trace(
                    &self.message_id,
                    &self.destination_hex,
                    "opportunistic",
                    &format!("encrypt failed: {err:?}"),
                );
                let _ = self.receipt_tx.try_send(ReceiptEvent {
                    message_id: self.message_id,
                    status: "failed: opportunistic encrypt failed".to_string(),
                });
                return;
            }
        };
        let mut encrypted_data = PacketDataBuffer::new();
        if encrypted_data.write(ciphertext.as_slice()).is_err() {
            log_delivery_trace(
                &self.message_id,
                &self.destination_hex,
                "opportunistic",
                "ciphertext too large",
            );
            self.run_opportunistic_link_fallback(payload, identity, "ciphertext too large").await;
            return;
        }
        packet.data = encrypted_data;
        let packet_hash = hex::encode(packet.hash().to_bytes());
        track_receipt_mapping(&self.receipt_map, &packet_hash, &self.message_id);
        if diagnostics_enabled() {
            let detail = format!(
                "sending packet_hash={} payload_len={} payload_prefix={}",
                packet_hash,
                opportunistic_payload.len(),
                payload_preview(opportunistic_payload, 16)
            );
            log_delivery_trace(&self.message_id, &self.destination_hex, "opportunistic", &detail);
        } else {
            log_delivery_trace(&self.message_id, &self.destination_hex, "opportunistic", "sending");
        }
        let trace = self.transport.send_prepared_packet_broadcast_with_trace(packet).await;
        let trace_detail = send_trace_detail(trace);
        log_delivery_trace(&self.message_id, &self.destination_hex, "opportunistic", &trace_detail);
        let outcome = trace.outcome;
        if !send_outcome_is_sent(outcome) {
            if let Ok(mut map) = self.receipt_map.lock() {
                map.remove(&packet_hash);
            }
        }
        let _ = self.receipt_tx.try_send(ReceiptEvent {
            message_id: self.message_id,
            status: send_outcome_status("opportunistic", outcome),
        });
    }

    async fn run_opportunistic_link_fallback(
        self,
        payload: Vec<u8>,
        identity: Identity,
        reason: &str,
    ) {
        if self.abort_if_cancelled("opportunistic-link") {
            return;
        }
        log_delivery_trace(
            &self.message_id,
            &self.destination_hex,
            "opportunistic",
            &format!("{reason}; falling back to link delivery"),
        );
        let destination_desc = DestinationDesc {
            identity,
            address_hash: self.destination_hash,
            name: DestinationName::new("lxmf", "delivery"),
        };
        if let Err(err) = self
            .send_via_link_mode(
                "opportunistic-link",
                self.destination_hex.as_str(),
                destination_desc,
                &payload,
                LinkModeStatuses {
                    packet: "sent: opportunistic link",
                    resource: "sending: link resource",
                    resource_sent: OUTBOUND_RESOURCE_SENT_STATUS,
                },
            )
            .await
        {
            log_delivery_trace(
                &self.message_id,
                &self.destination_hex,
                "opportunistic-link",
                &format!("fallback failed err={err}"),
            );
            let _ = self.receipt_tx.try_send(ReceiptEvent {
                message_id: self.message_id,
                status: format!("failed: {err}"),
            });
        }
    }

    pub(super) async fn resolve_destination_identity(&self) -> Option<Identity> {
        let identity = self
            .resolve_identity(
                Some(self.destination_hex.as_str()),
                self.destination_hash,
                self.peer_identity,
                "identity",
                "failed: peer not announced",
            )
            .await?;

        if let Ok(mut peers) = self.peer_crypto.lock() {
            peers.insert(self.destination_hex.clone(), PeerCrypto { identity });
        }
        Some(identity)
    }
}
