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
            PreparedDeliveryPayload { lxmf_payload: payload, propagation: None },
            Arc::new(tokio::sync::Semaphore::new(1)),
        )
        .await;
    }

    #[cfg(test)]
    pub(super) async fn run_prepared(
        mut self,
        mut prepared: PreparedDeliveryPayload,
        stamp_limit: Arc<tokio::sync::Semaphore>,
    ) -> DeliveryAttemptOutcome {
        loop {
            if self.requested_method == RequestedDeliveryMethod::Propagated {
                if let Err(outcome) = super::delivery_scheduler::prepare_propagation(
                    &self, &mut prepared, &stamp_limit,
                    &Arc::new(super::delivery_scheduler::DeliverySchedulerMetrics::default()), &self.destination_hex,
                ).await { return outcome; }
            }
            let outcome = self.run_prepared_attempt(&mut prepared).await;
            if outcome != DeliveryAttemptOutcome::FallbackPropagation { return outcome; }
            self.requested_method = RequestedDeliveryMethod::Propagated;
        }
    }

    pub(super) async fn run_prepared_attempt(
        &self,
        prepared: &mut PreparedDeliveryPayload,
    ) -> DeliveryAttemptOutcome {
        if self.abort_if_cancelled("payload") {
            return DeliveryAttemptOutcome::Stopped;
        }
        match self.requested_method {
            RequestedDeliveryMethod::Direct => self.run_direct(prepared).await,
            RequestedDeliveryMethod::Opportunistic => {
                self.run_opportunistic(&prepared.lxmf_payload).await
            }
            RequestedDeliveryMethod::Propagated => self.run_propagated(prepared).await,
            RequestedDeliveryMethod::Paper => DeliveryAttemptOutcome::Stopped,
        }
    }

    pub(super) fn fail_payload_build(&self, err: std::io::Error) {
        emit_receipt_event(
            &self.receipt_tx,
            ReceiptEvent::new(self.message_id.clone(), format!("failed: {err}")),
        );
    }

    async fn run_opportunistic(&self, payload: &[u8]) -> DeliveryAttemptOutcome {
        if self.abort_if_cancelled("opportunistic") {
            return DeliveryAttemptOutcome::Stopped;
        }
        let identity = match self.resolve_destination_identity().await {
            Ok(identity) => identity,
            Err(outcome) => return outcome,
        };
        // Opportunistic SINGLE packets must carry LXMF wire bytes
        // without the destination prefix. Receivers prepend the
        // packet destination hash before unpacking.
        let opportunistic_payload = opportunistic_payload(payload, &self.destination);
        if opportunistic_payload.len() > lxmf::constants::ENCRYPTED_PACKET_MDU {
            log_delivery_trace(
                &self.message_id,
                &self.destination_hex,
                "opportunistic",
                "payload too large for single packet",
            );
            return self
                .run_opportunistic_link_fallback(
                    payload,
                    identity,
                    "payload too large for single packet",
                )
                .await;
        }
        let mut data = PacketDataBuffer::new();
        if data.write(opportunistic_payload).is_err() {
            log_delivery_trace(
                &self.message_id,
                &self.destination_hex,
                "opportunistic",
                "payload too large",
            );
            return self
                .run_opportunistic_link_fallback(payload, identity, "payload too large")
                .await;
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
                emit_receipt_event(
                    &self.receipt_tx,
                    ReceiptEvent::new(
                        self.message_id.clone(),
                        "failed: opportunistic encrypt failed",
                    )
                    .with_method("opportunistic"),
                );
                return DeliveryAttemptOutcome::Terminal;
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
            return self
                .run_opportunistic_link_fallback(payload, identity, "ciphertext too large")
                .await;
        }
        packet.data = encrypted_data;
        let packet_hash = hex::encode(packet.hash().to_bytes());
        if log::log_enabled!(log::Level::Trace) {
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
        let trace = match self.transport.send_prepared_single_packet_observed_with_trace(
            packet, identity, DestinationName::new("lxmf", "delivery"),
            |hash| track_receipt_mapping(&self.receipt_map, &hex::encode(hash.to_bytes()), &self.message_id),
        ).await {
            Ok(trace) => trace,
            Err(error) => {
                self.fail_payload_build(std::io::Error::other(format!(
                    "opportunistic recipient binding failed for {}: {error}", self.destination_hex
                )));
                return DeliveryAttemptOutcome::Terminal;
            }
        };
        let trace_detail = send_trace_detail(trace);
        log_delivery_trace(&self.message_id, &self.destination_hex, "opportunistic", &trace_detail);
        let outcome = trace.outcome;
        if !send_outcome_is_sent(outcome) {
            match self.receipt_map.lock() {
                Ok(mut map) => {
                    map.remove(&packet_hash);
                }
                Err(error) => {
                    log::error!(
                        "[daemon] receipt map lock poisoned while removing packet_hash={} message_id={}: {}",
                        packet_hash,
                        self.message_id,
                        error
                    );
                }
            }
        }
        emit_receipt_event(
            &self.receipt_tx,
            ReceiptEvent::new(
                self.message_id.clone(),
                send_outcome_status("opportunistic", outcome),
            )
            .with_packet_hash(packet_hash)
            .with_method("opportunistic")
            .with_delivery_kind("opportunistic-packet")
            .with_bytes(opportunistic_payload.len()),
        );
        if send_outcome_is_sent(outcome) {
            DeliveryAttemptOutcome::AwaitPacket
        } else {
            DeliveryAttemptOutcome::Terminal
        }
    }

    async fn run_opportunistic_link_fallback(
        &self,
        payload: &[u8],
        identity: Identity,
        reason: &str,
    ) -> DeliveryAttemptOutcome {
        if self.abort_if_cancelled("opportunistic-link") {
            return DeliveryAttemptOutcome::Stopped;
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
        match self
            .send_via_link_mode(
                "opportunistic-link",
                self.destination_hex.as_str(),
                destination_desc,
                payload,
                LinkModeStatuses {
                    packet: "sent: opportunistic link",
                    resource: "sending: link resource",
                    resource_sent: OUTBOUND_RESOURCE_SENT_STATUS,
                },
            )
            .await
        {
            Ok(outcome) => outcome.into(),
            Err(err) => {
                log_delivery_trace(
                    &self.message_id,
                    &self.destination_hex,
                    "opportunistic-link",
                    &format!("fallback failed err={err}"),
                );
                emit_receipt_event(
                    &self.receipt_tx,
                    ReceiptEvent::new(self.message_id.clone(), format!("failed: {err}"))
                        .with_method("opportunistic-link"),
                );
                DeliveryAttemptOutcome::Terminal
            }
        }
    }

    pub(super) async fn resolve_destination_identity(
        &self,
    ) -> Result<Identity, DeliveryAttemptOutcome> {
        let identity = self
            .resolve_identity(
                Some(self.destination_hex.as_str()),
                self.destination_hash,
                self.peer_identity,
                "identity",
            )
            .await?;

        match self.peer_crypto.lock() {
            Ok(mut peers) => {
                peers.insert(self.destination_hex.clone(), PeerCrypto { identity });
            }
            Err(error) => {
                log::error!(
                    "[daemon] peer identity cache lock poisoned for destination={} message_id={}: {}",
                    self.destination_hex,
                    self.message_id,
                    error
                );
            }
        }
        Ok(identity)
    }
}
