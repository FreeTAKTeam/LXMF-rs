use crate::direct_backchannel::DirectBackchannelLinks;
use rns_transport::destination::link::LinkEvent;
use rns_transport::hash::AddressHash;
use rns_transport::transport::Transport;
use std::sync::Arc;

/// Start consumers for verified peer-identification events on both link
/// directions. Only delivery links can become direct delivery backchannels;
/// propagation and control links fail closed at the destination check.
pub(super) fn spawn_identified_peer_workers(
    transport: Arc<Transport>,
    backchannel_links: DirectBackchannelLinks,
) {
    for mut rx in [transport.in_link_events(), transport.out_link_events()] {
        let backchannel_links = backchannel_links.clone();
        let link_transport = transport.clone();
        tokio::spawn(async move {
            loop {
                match rx.recv().await {
                    Ok(event) => {
                        if let LinkEvent::PeerIdentified(identity) = event.event {
                            if !is_delivery_backchannel_link(&link_transport, &event.id).await {
                                log::debug!(
                                    "[daemon-rx] skipping non-delivery backchannel identify destination={} link={}",
                                    identity.address_hash,
                                    event.id
                                );
                                continue;
                            }
                            backchannel_links.record_identified_link(&identity, event.id);
                            log::debug!(
                                "[daemon-rx] direct backchannel available destination={} link={}",
                                identity.address_hash,
                                event.id
                            );
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                }
            }
        });
    }
}

async fn is_delivery_backchannel_link(transport: &Transport, link_id: &AddressHash) -> bool {
    let link = match transport.find_in_link(link_id).await {
        Some(link) => Some(link),
        None => transport.find_out_link(link_id).await,
    };
    match link {
        Some(link) => super::routing::is_lxmf_delivery_destination(link.lock().await.destination()),
        None => false,
    }
}

#[cfg(test)]
mod tests {
    use super::is_delivery_backchannel_link;
    use rand_core::OsRng;
    use rns_transport::destination::{DestinationDesc, DestinationName};
    use rns_transport::hash::AddressHash;
    use rns_transport::identity::PrivateIdentity;
    use rns_transport::transport::{Transport, TransportConfig};

    #[tokio::test]
    async fn only_a_delivery_aspect_link_is_treated_as_a_delivery_backchannel() {
        let local_identity = PrivateIdentity::new_from_rand(OsRng);
        let transport = Transport::new(TransportConfig::new(
            "backchannel-classification-test",
            &local_identity,
            true,
        ));

        let delivery_peer = PrivateIdentity::new_from_rand(OsRng);
        let delivery_destination = DestinationDesc {
            identity: *delivery_peer.as_identity(),
            address_hash: *delivery_peer.address_hash(),
            name: DestinationName::new("lxmf", "delivery"),
        };
        let delivery_link = transport.link(delivery_destination).await;
        let delivery_link_id = *delivery_link.lock().await.id();

        let control_peer = PrivateIdentity::new_from_rand(OsRng);
        let control_destination = DestinationDesc {
            identity: *control_peer.as_identity(),
            address_hash: *control_peer.address_hash(),
            name: DestinationName::new("lxmf", "propagation.control"),
        };
        let control_link = transport.link(control_destination).await;
        let control_link_id = *control_link.lock().await.id();

        assert!(is_delivery_backchannel_link(&transport, &delivery_link_id).await);
        assert!(!is_delivery_backchannel_link(&transport, &control_link_id).await);
    }

    #[tokio::test]
    async fn an_unresolvable_link_id_is_not_treated_as_a_delivery_backchannel() {
        let local_identity = PrivateIdentity::new_from_rand(OsRng);
        let transport = Transport::new(TransportConfig::new(
            "backchannel-classification-unresolvable-test",
            &local_identity,
            true,
        ));
        let bogus_link_id = AddressHash::new([7u8; 16]);

        assert!(!is_delivery_backchannel_link(&transport, &bogus_link_id).await);
    }
}
