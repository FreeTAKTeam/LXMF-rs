use super::{store_error, transport_error, LocalPropagation};
use lxmf_propagation::Error;
use rns_transport::{
    destination::{DestinationDesc, DestinationName},
    hash::AddressHash,
    identity::Identity,
};

impl LocalPropagation {
    /// Only native transport can supply the authenticated public keys.
    pub async fn remember_recipient(
        &self,
        destination: AddressHash,
        now: i64,
    ) -> Result<(), Error> {
        let native = self
            .config
            .transport
            .destination_identity(&destination)
            .await
            .ok_or(Error::UnresolvedRecipient)?;
        let identity = lxmf_core::identity::Identity::try_new_from_slices(
            native.public_key_bytes(),
            native.verifying_key_bytes(),
        )
        .map_err(|_| Error::UnresolvedRecipient)?;
        self.lock()?.remember_recipient(
            destination.as_slice().try_into().map_err(|_| Error::UnresolvedRecipient)?,
            &identity,
            now,
        )
    }

    // Custody needs a verified encryption key, never a path discovery wait.
    pub(super) async fn offline_recipient(
        &self,
        destination: AddressHash,
    ) -> Result<DestinationDesc, lxmf_sdk::SdkError> {
        let identity = if let Some(native) =
            self.config.transport.destination_identity(&destination).await
        {
            native
        } else {
            let stored = self
                .lock()
                .map_err(store_error)?
                .recipient_identity(
                    destination
                        .as_slice()
                        .try_into()
                        .map_err(|_| store_error(Error::UnresolvedRecipient))?,
                )
                .map_err(store_error)?
                .ok_or_else(|| store_error(Error::UnresolvedRecipient))?;
            Identity::try_new_from_slices(stored.public_key_bytes(), stored.verifying_key_bytes())
                .map_err(|_| {
                transport_error("invalid persisted recipient")
                    .with_cause_code("UnresolvedRecipient")
            })?
        };
        Ok(DestinationDesc {
            identity,
            name: DestinationName::new("lxmf", "delivery"),
            address_hash: destination,
        })
    }
}
