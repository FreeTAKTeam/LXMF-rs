use super::delivery_attempt::DeliveryAttemptOutcome;
use super::*;

impl DeliveryTask {
    pub(super) async fn resolve_identity(
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
