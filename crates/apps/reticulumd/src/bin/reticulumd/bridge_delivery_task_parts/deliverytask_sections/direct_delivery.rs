impl DeliveryTask {
    async fn run_direct(
        &self,
        prepared: &mut PreparedDeliveryPayload,
    ) -> DeliveryAttemptOutcome {
        let payload = prepared.lxmf_payload.as_slice();
        if self.abort_if_cancelled("link") {
            return DeliveryAttemptOutcome::Stopped;
        }
        let identity = match self.resolve_destination_identity().await {
            Ok(identity) => identity,
            Err(outcome) => return outcome,
        };
        if self.abort_if_cancelled("link") {
            return DeliveryAttemptOutcome::Stopped;
        }
        let result = if let Some(backchannel_link) = self
            .direct_backchannel_links
            .active_link(self.transport.as_ref(), &self.destination_hash)
            .await
        {
            log_delivery_trace(
                &self.message_id,
                &self.destination_hex,
                "link",
                "using direct backchannel link",
            );
            self.send_via_existing_link_mode(
                "link-backchannel",
                self.destination_hex.as_str(),
                backchannel_link,
                payload,
                LinkModeStatuses {
                    packet: "sent: link",
                    resource: "sending: link resource",
                    resource_sent: OUTBOUND_RESOURCE_SENT_STATUS,
                },
            )
            .await
        } else {
            let destination_desc = DestinationDesc {
                identity,
                address_hash: self.destination_hash,
                name: DestinationName::new("lxmf", "delivery"),
            };
            self.send_via_link_mode(
                "link",
                self.destination_hex.as_str(),
                destination_desc,
                payload,
                LinkModeStatuses {
                    packet: "sent: link",
                    resource: "sending: link resource",
                    resource_sent: OUTBOUND_RESOURCE_SENT_STATUS,
                },
            )
            .await
        };

        match result {
            Ok(outcome) => outcome.into(),
            Err(err) if self.try_propagation_on_fail && self.propagation_node_hex.is_some() => {
                self.direct_backchannel_links.remove_destination(&self.destination_hash);
                let detail = format!("direct failed err={err}; trying propagated");
                log_delivery_trace(&self.message_id, &self.destination_hex, "link", &detail);
                emit_receipt_event(
                    &self.receipt_tx,
                    ReceiptEvent::new(
                        self.message_id.clone(),
                        format!("link failed: {err}; trying propagated"),
                    )
                    .with_method("direct")
                    .with_delivery_kind("direct-fallback"),
                );
                DeliveryAttemptOutcome::FallbackPropagation
            }
            Err(err) => {
                self.direct_backchannel_links.remove_destination(&self.destination_hash);
                let detail = format!("direct failed err={err}");
                log_delivery_trace(&self.message_id, &self.destination_hex, "link", &detail);
                emit_receipt_event(
                    &self.receipt_tx,
                    ReceiptEvent::new(self.message_id.clone(), format!("failed: {err}"))
                        .with_method("direct"),
                );
                DeliveryAttemptOutcome::Terminal
            }
        }
    }
}
