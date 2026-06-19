impl DeliveryTask {

    async fn resolve_or_create_propagation_link(
        &self,
        propagation_node_hex: &str,
        propagation_hash: AddressHash,
    ) -> Result<Arc<tokio::sync::Mutex<Link>>, std::io::Error> {
        if let Some(link) = propagation::cached_propagation_link(
            &self.outbound_propagation_link,
            propagation_node_hex,
        )
        .await
        {
            return Ok(link);
        }

        let cached_identity = self
            .propagation_node_identity
            .or_else(|| {
                self.outbound_propagation_identities
                    .lock()
                    .ok()
                    .and_then(|guard| guard.get(propagation_node_hex).cloned())
            })
            .or_else(|| {
                resolve_destination_identity_blocking(
                    self.transport.clone(),
                    propagation_hash,
                    Duration::from_secs(12),
                )
            });
        let Some(propagation_identity) = self
            .resolve_identity(
                Some(propagation_node_hex),
                propagation_hash,
                cached_identity,
                "propagation-node",
                "failed: propagation node not announced",
            )
            .await
        else {
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "propagation node not announced",
            ));
        };
        if let Ok(mut guard) = self.outbound_propagation_identities.lock() {
            guard.insert(propagation_node_hex.to_string(), propagation_identity);
        }

        let propagation_destination = SingleOutputDestination::new(
            propagation_identity,
            DestinationName::new("lxmf", "propagation"),
        );

        Ok(propagation::propagation_link_for_node(
            self.transport.as_ref(),
            &self.outbound_propagation_link,
            propagation_node_hex,
            propagation_destination.desc,
        )
        .await)
    }

    async fn resolve_identity(
        &self,
        destination_hex: Option<&str>,
        destination_hash: AddressHash,
        cached: Option<Identity>,
        stage: &str,
        failure_status: &str,
    ) -> Option<Identity> {
        let mut identity =
            cached.or_else(|| self.cached_identity_for_destination(destination_hash));
        if identity.is_some() {
            let detail = destination_hex.unwrap_or(self.destination_hex.as_str());
            log_delivery_trace(
                &self.message_id,
                detail,
                stage,
                "resolved from cached peer identity",
            );
        }

        if identity.is_none() {
            self.transport.request_path(&destination_hash, None, None).await;
            log_delivery_trace(&self.message_id, &self.destination_hex, stage, "path-requested");
            let detail = destination_hex.unwrap_or(self.destination_hex.as_str());
            log_delivery_trace(&self.message_id, detail, stage, "waiting for announce");
            let deadline = tokio::time::Instant::now() + Duration::from_secs(12);
            while tokio::time::Instant::now() < deadline {
                if self.abort_if_cancelled(stage) {
                    return None;
                }
                if let Some(found) = self.transport.destination_identity(&destination_hash).await {
                    identity = Some(found);
                    break;
                }
                tokio::time::sleep(Duration::from_millis(250)).await;
            }
        }

        if identity.is_none() {
            identity = self.cached_identity_for_destination(destination_hash);
            if identity.is_some() {
                let detail = destination_hex.unwrap_or(self.destination_hex.as_str());
                log_delivery_trace(
                    &self.message_id,
                    detail,
                    stage,
                    "resolved from cached peer identity",
                );
            }
        }

        let Some(identity) = identity else {
            let detail = destination_hex.unwrap_or(self.destination_hex.as_str());
            log_delivery_trace(&self.message_id, detail, stage, "not found");
            emit_receipt_event(&self.receipt_tx, ReceiptEvent {
                message_id: self.message_id.clone(),
                status: failure_status.to_string(),
            });
            return None;
        };

        let detail = destination_hex.unwrap_or(self.destination_hex.as_str());
        log_delivery_trace(&self.message_id, detail, stage, "resolved");
        Some(identity)
    }
}
