impl DeliveryTask {
    async fn run_propagated(&self, prepared: &PreparedDeliveryPayload) -> DeliveryAttemptOutcome {
        let Some(propagation) = prepared.propagation.as_ref() else {
            self.fail_payload_build(std::io::Error::other("propagation payload was not prepared"));
            return DeliveryAttemptOutcome::Terminal;
        };
        self.send_prepared_propagated(propagation).await
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
                    DeliveryAttemptOutcome::PropagationCustody(Box::new(
                        ReceiptEvent::propagation_custody(self.message_id.clone(), rns_rpc::broker::PropagationCustody::Resource)
                            .with_method("propagation")
                            .with_delivery_kind("local-propagation-store"),
                    ))
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
