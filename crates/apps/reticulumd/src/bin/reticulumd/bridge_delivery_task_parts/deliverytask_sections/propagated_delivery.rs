impl DeliveryTask {
    async fn run_propagated(
        &self,
        prepared: &mut PreparedDeliveryPayload,
        stamp_limit: &Arc<tokio::sync::Semaphore>,
    ) -> DeliveryAttemptOutcome {
        if self.abort_if_cancelled("propagation") {
            return DeliveryAttemptOutcome::Stopped;
        }
        // Both initial preparation and fallback publish this cache only after
        // the durable write succeeds. Reuse the exact envelope on later attempts.
        if let Some(propagation) = prepared.propagation.as_ref() {
            return self.send_prepared_propagated(propagation).await;
        }
        let durable = match self.daemon.durable_prepared_payload(&self.message_id) {
            Ok(Some(Some(value))) => {
                let stored = match crate::bridge::delivery_scheduler::decode_prepared(value) {
                    Ok(stored) => stored,
                    Err(error) => {
                        self.fail_payload_build(error);
                        return DeliveryAttemptOutcome::Terminal;
                    }
                };
                if stored.lxmf_payload != prepared.lxmf_payload {
                    self.fail_payload_build(std::io::Error::other(
                        "prepared LXMF bytes differ from durable message",
                    ));
                    return DeliveryAttemptOutcome::Terminal;
                }
                if let Some(propagation) = stored.propagation {
                    prepared.propagation = Some(propagation);
                    return self
                        .send_prepared_propagated(
                            prepared.propagation.as_ref().expect("restored envelope"),
                        )
                        .await;
                }
                true
            }
            Ok(Some(None)) => true,
            Ok(None) => false,
            Err(error) => {
                log::error!(
                    "durable propagation lookup failed message_id={}: {error}",
                    self.message_id
                );
                return DeliveryAttemptOutcome::DeferredStorage;
            }
        };
        let envelope =
            match self.build_propagation_envelope(&prepared.lxmf_payload, stamp_limit).await {
                Ok(envelope) => envelope,
                Err(outcome) => return outcome,
            };
        if durable {
            let encoded = crate::bridge::delivery_scheduler::encode_prepared_parts(
                &prepared.lxmf_payload,
                Some(&envelope),
            );
            if let Err(error) =
                self.daemon.persist_durable_prepared_payload(&self.message_id, encoded)
            {
                log::error!(
                    "propagation bytes were not committed; send suppressed message_id={}: {error}",
                    self.message_id
                );
                return DeliveryAttemptOutcome::DeferredStorage;
            }
        }
        prepared.propagation = Some(envelope);
        self.send_prepared_propagated(prepared.propagation.as_ref().expect("committed envelope"))
            .await
    }

    async fn build_propagation_envelope(
        &self,
        payload: &[u8],
        stamp_limit: &Arc<tokio::sync::Semaphore>,
    ) -> Result<PreparedPropagationPayload, DeliveryAttemptOutcome> {
        let context = self.propagation_preparation_context().await?;
        log_delivery_trace(
            &self.message_id,
            &self.destination_hex,
            "propagation",
            "building propagation payload",
        );
        self.record_propagation_stamp_work_metadata("queued", context.target_cost, None);
        for attempt in 1..=2u32 {
            let permit = match stamp_limit.clone().acquire_owned().await {
                Ok(permit) => permit,
                Err(_) => {
                    emit_receipt_event(
                        &self.receipt_tx,
                        ReceiptEvent::new(self.message_id.clone(), "failed: stamp worker stopped")
                            .with_method("propagation"),
                    );
                    return Err(DeliveryAttemptOutcome::Terminal);
                }
            };
            self.record_propagation_stamp_attempt_metadata(context.target_cost, attempt);
            if self.abort_if_cancelled("propagation") {
                self.record_propagation_stamp_work_metadata("cancelled", context.target_cost, None);
                return Err(DeliveryAttemptOutcome::Stopped);
            }
            let result = propagation::build_propagation_payload_until_cancelled(
                payload,
                &context.destination_identity,
                context.target_cost,
                || self.abort_if_cancelled("propagation-stamp"),
            );
            drop(permit);
            match result {
                Ok(payload) => {
                    self.record_propagation_stamp_work_metadata(
                        "ready",
                        context.target_cost,
                        Some(payload.stamp_value.to_string()),
                    );
                    self.record_propagation_payload_metadata(&payload, context.target_cost);
                    return Ok(PreparedPropagationPayload {
                        propagation_node_hex: context.propagation_node_hex,
                        propagation_hash: context.propagation_hash,
                        target_cost: context.target_cost,
                        payload,
                    });
                }
                Err(error) => {
                    if self.abort_if_cancelled("propagation") {
                        self.record_propagation_stamp_work_metadata(
                            "cancelled",
                            context.target_cost,
                            None,
                        );
                        return Err(DeliveryAttemptOutcome::Stopped);
                    }
                    if attempt < 2 {
                        self.record_propagation_stamp_retry_metadata(
                            context.target_cost,
                            attempt,
                            error.to_string(),
                        );
                        tokio::time::sleep(Duration::from_millis(100)).await;
                        continue;
                    }
                    self.record_propagation_stamp_work_metadata(
                        "failed",
                        context.target_cost,
                        Some(error.to_string()),
                    );
                    emit_receipt_event(
                        &self.receipt_tx,
                        ReceiptEvent::new(self.message_id.clone(), format!("failed: {error}"))
                            .with_method("propagation"),
                    );
                    return Err(DeliveryAttemptOutcome::Terminal);
                }
            }
        }
        self.fail_payload_build(std::io::Error::other("propagation stamp attempts exhausted"));
        Err(DeliveryAttemptOutcome::Terminal)
    }

    async fn send_prepared_propagated(
        &self,
        prepared: &PreparedPropagationPayload,
    ) -> DeliveryAttemptOutcome {
        if self.abort_if_cancelled("propagation") {
            return DeliveryAttemptOutcome::Stopped;
        }
        if self.selected_propagation_node_is_local(&prepared.propagation_node_hex) {
            return match self
                .store_local_propagation_payload(&prepared.propagation_node_hex, &prepared.payload)
            {
                Ok(()) => {
                    emit_receipt_event(
                        &self.receipt_tx,
                        ReceiptEvent::new(self.message_id.clone(), "sent: propagated resource")
                            .with_method("propagation")
                            .with_delivery_kind("local-propagation-store"),
                    );
                    DeliveryAttemptOutcome::CompletedLocal
                }
                Err(error) => {
                    emit_receipt_event(
                        &self.receipt_tx,
                        ReceiptEvent::new(self.message_id.clone(), format!("failed: {error}"))
                            .with_method("propagation"),
                    );
                    DeliveryAttemptOutcome::Terminal
                }
            };
        }
        let payload = prepared.payload.bytes.as_slice();
        log_delivery_trace(
            &self.message_id,
            &self.destination_hex,
            "propagation",
            &format!(
                "propagation payload ready bytes={} target_cost={}",
                payload.len(),
                prepared.target_cost
            ),
        );
        let link = match self
            .resolve_or_create_propagation_link(
                &prepared.propagation_node_hex,
                prepared.propagation_hash,
            )
            .await
        {
            Ok(link) => link,
            Err(outcome) => return outcome,
        };
        match self
            .send_via_existing_link_mode(
                "propagation",
                &prepared.propagation_node_hex,
                link,
                payload,
                LinkModeStatuses {
                    packet: "sent: propagated",
                    resource: "sending: propagated resource",
                    resource_sent: "sent: propagated resource",
                },
            )
            .await
        {
            Ok(outcome) => outcome.into(),
            Err(error) => {
                log_delivery_trace(
                    &self.message_id,
                    &self.destination_hex,
                    "propagation",
                    &format!("propagated failed err={error}"),
                );
                emit_receipt_event(
                    &self.receipt_tx,
                    ReceiptEvent::new(self.message_id.clone(), format!("failed: {error}"))
                        .with_method("propagation"),
                );
                DeliveryAttemptOutcome::Terminal
            }
        }
    }
}
