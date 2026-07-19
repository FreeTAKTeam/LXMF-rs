impl DeliveryTask {
    pub(super) const DEFERRED_PEER_IDENTITY_STATUS: &'static str = "queued: waiting for announce";

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
                match self.outbound_propagation_identities.lock() {
                    Ok(guard) => guard.get(propagation_node_hex).cloned(),
                    Err(err) => {
                        log::warn!(
                            "[daemon] failed to read propagation identity cache for {propagation_node_hex}: {err}"
                        );
                        None
                    }
                }
            })
            .or_else(|| {
                match resolve_destination_identity_blocking(
                    self.transport.clone(),
                    propagation_hash,
                    Duration::from_secs(12),
                ) {
                    Ok(identity) => identity,
                    Err(err) => {
                        log::warn!("[daemon] identity resolver for propagation node: {err}");
                        None
                    }
                }
            });
        let propagation_identity = match self
            .resolve_identity(
                Some(propagation_node_hex),
                propagation_hash,
                cached_identity,
                "propagation-node",
                "failed: propagation node not announced",
            )
            .await
        {
            Ok(Some(identity)) => identity,
            Ok(None) => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::Interrupted,
                    "delivery cancelled",
                ))
            }
            Err(msg) => {
                return Err(std::io::Error::new(std::io::ErrorKind::NotFound, msg))
            }
        };
        self.outbound_propagation_identities
            .lock()
            .map_err(|error| {
                std::io::Error::other(format!(
                    "propagation identity cache lock poisoned for {propagation_node_hex}: {error}"
                ))
            })?
            .insert(propagation_node_hex.to_string(), propagation_identity);

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
        failure_status: &'static str,
    ) -> Result<Option<Identity>, &'static str> {
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
                    return Ok(None);
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
            if self.abort_if_cancelled(stage) {
                return Ok(None);
            }
            let status = self.identity_miss_status(failure_status);
            let detail = destination_hex.unwrap_or(self.destination_hex.as_str());
            log_delivery_trace(&self.message_id, detail, stage, "not found");
            emit_receipt_event(
                &self.receipt_tx,
                ReceiptEvent::new(self.message_id.clone(), status).with_method(stage),
            );
            if status == Self::DEFERRED_PEER_IDENTITY_STATUS {
                return Ok(None);
            }
            return Err(status);
        };

        let detail = destination_hex.unwrap_or(self.destination_hex.as_str());
        log_delivery_trace(&self.message_id, detail, stage, "resolved");
        Ok(Some(identity))
    }

    pub(super) fn identity_miss_status(&self, failure_status: &'static str) -> &'static str {
        if failure_status == "failed: peer not announced"
            && matches!(
                self.requested_method,
                RequestedDeliveryMethod::Direct | RequestedDeliveryMethod::Opportunistic
            )
        {
            Self::DEFERRED_PEER_IDENTITY_STATUS
        } else {
            failure_status
        }
    }
}
