use rns_transport::destination::link::{Link, LinkStatus};
use rns_transport::destination::DestinationName;
use rns_transport::hash::AddressHash;
use rns_transport::identity::Identity;
use rns_transport::transport::Transport;
use sha2::Digest;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};

#[derive(Clone, Default)]
pub(super) struct DirectBackchannelLinks {
    links: Arc<Mutex<HashMap<AddressHash, AddressHash>>>,
    outbound_local_sources: Arc<Mutex<HashMap<AddressHash, [u8; 16]>>>,
}

impl DirectBackchannelLinks {
    pub(super) fn new() -> Self {
        Self::default()
    }

    pub(super) fn record_identified_link(&self, identity: &Identity, link_id: AddressHash) {
        let destination = delivery_destination_hash_for_identity(identity);
        match self.links.lock() {
            Ok(mut guard) => {
                guard.insert(destination, link_id);
            }
            Err(error) => {
                log::error!(
                    "[daemon] direct backchannel cache lock poisoned while recording dst={} link={}: {}",
                    destination,
                    link_id,
                    error
                );
            }
        }
    }

    pub(super) fn remove_destination(&self, destination: &AddressHash) {
        match self.links.lock() {
            Ok(mut guard) => {
                guard.remove(destination);
            }
            Err(error) => {
                log::error!(
                    "[daemon] direct backchannel cache lock poisoned while removing dst={}: {}",
                    destination,
                    error
                );
            }
        }
    }

    pub(super) fn record_outbound_local_source(&self, link_id: AddressHash, source: [u8; 16]) {
        match self.outbound_local_sources.lock() {
            Ok(mut guard) => {
                guard.insert(link_id, source);
            }
            Err(error) => {
                log::error!(
                    "[daemon] outbound local-source cache lock poisoned link={link_id}: {error}"
                );
            }
        }
    }

    pub(super) fn outbound_local_source(&self, link_id: &AddressHash) -> Option<[u8; 16]> {
        match self.outbound_local_sources.lock() {
            Ok(guard) => guard.get(link_id).copied(),
            Err(error) => {
                log::error!(
                    "[daemon] outbound local-source cache lock poisoned link={link_id}: {error}"
                );
                None
            }
        }
    }

    pub(super) async fn active_link(
        &self,
        transport: &Transport,
        destination: &AddressHash,
    ) -> Option<Arc<tokio::sync::Mutex<Link>>> {
        let link_id = match self.links.lock() {
            Ok(guard) => guard.get(destination).copied()?,
            Err(err) => {
                log::warn!("[daemon] direct backchannel cache lock failed: {err}");
                return None;
            }
        };
        let link = match transport.find_in_link(&link_id).await {
            Some(link) => Some(link),
            None => transport.find_out_link(&link_id).await,
        };
        let Some(link) = link else {
            self.remove_destination(destination);
            return None;
        };
        let status = link.lock().await.status();
        if status == LinkStatus::Active {
            return Some(link);
        }
        if matches!(status, LinkStatus::Closed | LinkStatus::Stale) {
            self.remove_destination(destination);
        }
        None
    }
}

fn delivery_destination_hash_for_identity(identity: &Identity) -> AddressHash {
    let name = DestinationName::new("lxmf", "delivery");
    let hash = sha2::Sha256::new()
        .chain_update(name.as_name_hash_slice())
        .chain_update(identity.address_hash.as_slice())
        .finalize();
    let mut destination = [0u8; 16];
    destination.copy_from_slice(&hash[..16]);
    AddressHash::new(destination)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand_core::OsRng;
    use rns_transport::destination::{link::LinkHandleResult, DestinationDesc};
    use rns_transport::identity::PrivateIdentity;
    use rns_transport::iface::IfaceRole;
    use rns_transport::transport::TransportConfig;

    #[test]
    fn records_identified_link_by_lxmf_delivery_destination() {
        let identity = PrivateIdentity::new_from_rand(OsRng);
        let links = DirectBackchannelLinks::new();
        let link_id = AddressHash::new([3u8; 16]);
        let destination = delivery_destination_hash_for_identity(identity.as_identity());

        links.record_identified_link(identity.as_identity(), link_id);

        assert_eq!(links.link_id_for_test(&destination), Some(link_id));
    }

    #[tokio::test]
    async fn active_link_returns_cached_transport_link_for_delivery_destination() {
        let local_identity = PrivateIdentity::new_from_rand(OsRng);
        let transport =
            Transport::new(TransportConfig::new("direct-backchannel-test", &local_identity, true));
        let mut channel =
            transport.iface_manager().lock().await.new_channel_with_role(8, IfaceRole::Unicast);
        let iface = *channel.address();

        let remote_identity = PrivateIdentity::new_from_rand(OsRng);
        let remote_public = *remote_identity.as_identity();
        let destination = DestinationDesc {
            identity: remote_public,
            address_hash: delivery_destination_hash_for_identity(&remote_public),
            name: DestinationName::new("lxmf", "delivery"),
        };
        let outbound = transport.link(destination).await;
        let request =
            tokio::time::timeout(std::time::Duration::from_millis(200), channel.tx_channel.recv())
                .await
                .expect("link request")
                .expect("link request");
        let (tx, _) = tokio::sync::broadcast::channel(8);
        let mut inbound = Link::new_from_request(
            &request.packet,
            remote_identity.sign_key().clone(),
            destination,
            tx,
        )
        .expect("inbound link");
        let link_id = *outbound.lock().await.id();
        assert!(matches!(
            outbound.lock().await.handle_packet(&inbound.prove(), iface),
            LinkHandleResult::Activated
        ));

        let links = DirectBackchannelLinks::new();
        links.record_identified_link(&remote_public, link_id);

        let cached = links
            .active_link(&transport, &delivery_destination_hash_for_identity(&remote_public))
            .await
            .expect("active cached link");

        assert_eq!(*cached.lock().await.id(), link_id);
    }
}

impl DirectBackchannelLinks {
    #[cfg(test)]
    fn link_id_for_test(&self, destination: &AddressHash) -> Option<AddressHash> {
        match self.links.lock() {
            Ok(guard) => guard.get(destination).copied(),
            Err(_) => None,
        }
    }
}
