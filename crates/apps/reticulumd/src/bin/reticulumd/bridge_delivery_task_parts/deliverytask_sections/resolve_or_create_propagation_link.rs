impl DeliveryTask {
    async fn resolve_or_create_propagation_link(
        &self,
        propagation_node_hex: &str,
        propagation_hash: AddressHash,
    ) -> Result<Arc<tokio::sync::Mutex<Link>>, DeliveryAttemptOutcome> {
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
            });
        let propagation_identity = self
            .resolve_identity(
                Some(propagation_node_hex),
                propagation_hash,
                cached_identity,
                "propagation-node",
            )
            .await?;
        self.outbound_propagation_identities
            .lock()
            .map_err(|error| {
                self.fail_payload_build(std::io::Error::other(format!(
                    "propagation identity cache lock poisoned for {propagation_node_hex}: {error}"
                )));
                DeliveryAttemptOutcome::Terminal
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
    ) -> Result<Identity, DeliveryAttemptOutcome> {
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
            identity = self.transport.destination_identity(&destination_hash).await;
        }
        let Some(identity) = identity else {
            return Err(DeliveryAttemptOutcome::AwaitPath { destination: destination_hash });
        };

        let detail = destination_hex.unwrap_or(self.destination_hex.as_str());
        log_delivery_trace(&self.message_id, detail, stage, "resolved");
        Ok(identity)
    }

    pub(super) async fn discovery_ready(&self, destination: AddressHash) -> bool {
        self.cached_identity_for_destination(destination).is_some()
            || self.transport.destination_identity(&destination).await.is_some()
    }
}
