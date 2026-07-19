use crate::bootstrap::PropagationControlContext;
use rns_transport::destination::{DestinationDesc, DestinationName, SingleInputDestination};
use rns_transport::hash::AddressHash;
use rns_transport::packet::PacketContext;
use rns_transport::transport::{ReceivedData, ReceivedPayloadMode, Transport};
use std::sync::Arc;

use super::propagation;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum InboundLxmfDestination {
    Delivery([u8; 16]),
    Propagation,
}

pub(super) async fn resolve_resource_destination(
    transport: &Transport,
    link_id: &AddressHash,
    local_delivery_destination: Option<[u8; 16]>,
) -> Option<InboundLxmfDestination> {
    if let Some(link) = transport.find_in_link(link_id).await {
        let guard = link.lock().await;
        if let Some(destination) = lxmf_destination_from_desc(guard.destination()) {
            return Some(destination);
        }
    }
    if let Some(link) = transport.find_out_link(link_id).await {
        let guard = link.lock().await;
        return destination_for_outbound_link(guard.destination(), local_delivery_destination);
    }
    None
}

pub(super) async fn resolve_packet_destination(
    transport: &Transport,
    control: &PropagationControlContext,
    destination: &AddressHash,
    payload_mode: ReceivedPayloadMode,
    local_delivery_destination: Option<[u8; 16]>,
) -> Option<InboundLxmfDestination> {
    match payload_mode {
        ReceivedPayloadMode::DestinationStripped => {
            if let Some(resolved) = resolve_link_packet_destination(transport, destination).await {
                return Some(resolved);
            }
            if let Some(link) = transport.find_out_link(destination).await {
                let guard = link.lock().await;
                if let Some(resolved) =
                    destination_for_outbound_link(guard.destination(), local_delivery_destination)
                {
                    return Some(resolved);
                }
            }
            if propagation::is_lxmf_propagation_destination(destination, control) {
                Some(InboundLxmfDestination::Propagation)
            } else {
                Some(InboundLxmfDestination::Delivery(destination_hash(destination)))
            }
        }
        ReceivedPayloadMode::FullWire => {
            if let Some(resolved) = resolve_link_packet_destination(transport, destination).await {
                return Some(resolved);
            }
            if let Some(link) = transport.find_out_link(destination).await {
                let guard = link.lock().await;
                if let Some(resolved) =
                    destination_for_outbound_link(guard.destination(), local_delivery_destination)
                {
                    return Some(resolved);
                }
            }
            local_delivery_destination
                .filter(|local| local.as_slice() == destination.as_slice())
                .map(InboundLxmfDestination::Delivery)
        }
    }
}

fn destination_for_outbound_link(
    remote_destination: &DestinationDesc,
    local_delivery_destination: Option<[u8; 16]>,
) -> Option<InboundLxmfDestination> {
    if is_lxmf_delivery_destination(remote_destination) {
        return local_delivery_destination.map(InboundLxmfDestination::Delivery);
    }
    lxmf_destination_from_desc(remote_destination)
}

async fn resolve_link_packet_destination(
    transport: &Transport,
    link_id: &AddressHash,
) -> Option<InboundLxmfDestination> {
    let link = transport.find_in_link(link_id).await?;
    let guard = link.lock().await;
    lxmf_destination_from_desc(guard.destination())
}

pub(super) async fn local_delivery_destination_hash(
    destination: Option<&Arc<tokio::sync::Mutex<SingleInputDestination>>>,
) -> Option<[u8; 16]> {
    let destination = destination?;
    let guard = destination.lock().await;
    Some(destination_hash(&guard.desc.address_hash))
}

pub(super) fn should_skip_control_payload(
    event: &ReceivedData,
    control: &PropagationControlContext,
) -> bool {
    let Some(control_hash) = control.control_destination_hash_hex.as_deref() else {
        return false;
    };
    if hex::encode(event.destination.as_slice()) != control_hash {
        return false;
    }
    matches!(
        event.context,
        Some(PacketContext::Request | PacketContext::Response | PacketContext::LinkIdentify)
    )
}

pub(super) fn should_skip_resolved_control_payload(
    destination: InboundLxmfDestination,
    context: Option<PacketContext>,
) -> bool {
    matches!(destination, InboundLxmfDestination::Propagation)
        && matches!(
            context,
            Some(PacketContext::Request | PacketContext::Response | PacketContext::LinkIdentify)
        )
}

fn lxmf_destination_from_desc(destination: &DestinationDesc) -> Option<InboundLxmfDestination> {
    if is_lxmf_delivery_destination(destination) {
        return Some(InboundLxmfDestination::Delivery(destination_hash(&destination.address_hash)));
    }
    if is_lxmf_propagation_link_destination(destination) {
        return Some(InboundLxmfDestination::Propagation);
    }
    None
}

fn destination_hash(destination: &AddressHash) -> [u8; 16] {
    let mut hash = [0u8; 16];
    hash.copy_from_slice(destination.as_slice());
    hash
}

// `pub(super)` — the direct-backchannel `PeerIdentified` consumer in
// `module_core.rs` (included into this same `inbound_worker` module via
// `include!`) needs this to gate which links get cached as a peer's
// delivery backchannel (see its call site's doc comment for why).
pub(super) fn is_lxmf_delivery_destination(destination: &DestinationDesc) -> bool {
    destination.name.hash == DestinationName::new("lxmf", "delivery").hash
}

fn is_lxmf_propagation_link_destination(destination: &DestinationDesc) -> bool {
    destination.name.hash == DestinationName::new("lxmf", "propagation").hash
}

#[cfg(test)]
mod tests {
    use super::{
        destination_for_outbound_link, is_lxmf_delivery_destination,
        is_lxmf_propagation_link_destination, resolve_resource_destination,
        should_skip_resolved_control_payload, InboundLxmfDestination,
    };
    use rand_core::OsRng;
    use rns_transport::destination::{DestinationDesc, DestinationName};
    use rns_transport::identity::PrivateIdentity;
    use rns_transport::packet::PacketContext;

    #[test]
    fn lxmf_delivery_destination_is_accepted_for_resource_decode() {
        let signer = PrivateIdentity::new_from_rand(OsRng);
        let destination = DestinationDesc {
            identity: *signer.as_identity(),
            address_hash: *signer.address_hash(),
            name: DestinationName::new("lxmf", "delivery"),
        };

        assert!(is_lxmf_delivery_destination(&destination));
    }

    #[test]
    fn non_delivery_destination_is_rejected_for_resource_decode() {
        let signer = PrivateIdentity::new_from_rand(OsRng);
        let destination = DestinationDesc {
            identity: *signer.as_identity(),
            address_hash: *signer.address_hash(),
            name: DestinationName::new("lxmf", "propagation.control"),
        };

        assert!(!is_lxmf_delivery_destination(&destination));
    }

    #[test]
    fn propagation_destination_is_detected_for_resource_decode() {
        let signer = PrivateIdentity::new_from_rand(OsRng);
        let destination = DestinationDesc {
            identity: *signer.as_identity(),
            address_hash: *signer.address_hash(),
            name: DestinationName::new("lxmf", "propagation"),
        };

        assert!(is_lxmf_propagation_link_destination(&destination));
    }

    #[test]
    fn propagation_link_control_context_is_skipped_from_payload_ingest() {
        assert!(should_skip_resolved_control_payload(
            InboundLxmfDestination::Propagation,
            Some(PacketContext::LinkIdentify)
        ));
        assert!(should_skip_resolved_control_payload(
            InboundLxmfDestination::Propagation,
            Some(PacketContext::Request)
        ));
    }

    #[test]
    fn outbound_delivery_link_backchannel_resolves_to_local_delivery_destination() {
        let remote = PrivateIdentity::new_from_rand(OsRng);
        let remote_destination = DestinationDesc {
            identity: *remote.as_identity(),
            address_hash: *remote.address_hash(),
            name: DestinationName::new("lxmf", "delivery"),
        };
        let local_destination = [7_u8; 16];

        assert_eq!(
            destination_for_outbound_link(&remote_destination, Some(local_destination)),
            Some(InboundLxmfDestination::Delivery(local_destination))
        );
    }

    // Live-reproduction check for PR #482 / issue #481: does a genuine
    // plain (non-Request, non-Response) Resource transfer, completed over a
    // real Link the sender opened to our own registered `lxmf.delivery`
    // destination, actually resolve via THIS function the way
    // `spawn_inbound_worker`'s pre-existing `resource_events()` consumer
    // already relies on? No synthetic destination descriptors here — a
    // real inbound Link, built the same way `handle_link_request_as_destination`
    // builds one in production, carrying a real completed Resource transfer.
    #[tokio::test]
    async fn plain_resource_over_a_real_link_resolves_to_local_delivery_destination() {
        use rns_transport::iface::{IfaceSource, RxMessage};
        use rns_transport::resource::ResourceEventKind;
        use rns_transport::transport::{Transport, TransportConfig};
        use tokio::time::{timeout, Duration};

        let receiver_identity = PrivateIdentity::new_from_rand(OsRng);
        let mut receiver_transport =
            Transport::new(TransportConfig::new("receiver", &receiver_identity, true));
        let receiver_iface = receiver_transport.iface_manager().lock().await.new_channel(64);
        let own_destination = receiver_transport
            .add_destination(receiver_identity.clone(), DestinationName::new("lxmf", "delivery"))
            .await;
        let own_desc = own_destination.lock().await.desc;
        let mut destination_hash = [0u8; 16];
        destination_hash.copy_from_slice(own_desc.address_hash.as_slice());

        let sender_identity = PrivateIdentity::new_from_rand(OsRng);
        let sender_transport =
            Transport::new(TransportConfig::new("sender", &sender_identity, true));
        let sender_iface = sender_transport.iface_manager().lock().await.new_channel(64);

        // Autonomously relay whatever each side transmits to the other, so
        // the real Link handshake and Resource transfer run to completion
        // exactly as they would over a real network — no manual packet
        // stepping.
        let (mut receiver_tx, receiver_rx_channel, receiver_addr) =
            (receiver_iface.tx_channel, receiver_iface.rx_channel, receiver_iface.address);
        let (mut sender_tx, sender_rx_channel, sender_addr) =
            (sender_iface.tx_channel, sender_iface.rx_channel, sender_iface.address);
        tokio::spawn({
            let sender_rx_channel = sender_rx_channel.clone();
            async move {
                while let Some(msg) = receiver_tx.recv().await {
                    if sender_rx_channel
                        .send(RxMessage {
                            address: sender_addr,
                            packet: msg.packet,
                            source: IfaceSource::None,
                        })
                        .await
                        .is_err()
                    {
                        break;
                    }
                }
            }
        });
        tokio::spawn({
            let receiver_rx_channel = receiver_rx_channel.clone();
            async move {
                while let Some(msg) = sender_tx.recv().await {
                    if receiver_rx_channel
                        .send(RxMessage {
                            address: receiver_addr,
                            packet: msg.packet,
                            source: IfaceSource::None,
                        })
                        .await
                        .is_err()
                    {
                        break;
                    }
                }
            }
        });

        let out_link = sender_transport.link(own_desc).await;
        let link_id = *out_link.lock().await.id();
        for _ in 0..200 {
            if out_link.lock().await.status()
                == rns_transport::destination::link::LinkStatus::Active
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert_eq!(
            out_link.lock().await.status(),
            rns_transport::destination::link::LinkStatus::Active,
            "real Link handshake should complete over the relayed virtual ifaces"
        );

        // Large enough to force genuine multi-part chunking over the Link's
        // MTU, matching the motivating "large message" scenario in #481 —
        // not just a single-packet resource that might trivially round-trip
        // regardless of whether the real chunked-transfer machinery works.
        let payload: Vec<u8> = (0..20_000).map(|i| (i % 256) as u8).collect();
        let mut resource_events = receiver_transport.resource_events();
        sender_transport
            .send_resource(&link_id, payload.clone(), None)
            .await
            .expect("plain resource send should succeed over the active link");

        let event = timeout(Duration::from_secs(5), async {
            loop {
                let event = resource_events.recv().await.expect("resource event stream open");
                if let ResourceEventKind::Complete(_) = &event.kind {
                    return event;
                }
            }
        })
        .await
        .expect("receiver should observe a real ResourceEventKind::Complete");

        let ResourceEventKind::Complete(ref complete) = event.kind else { unreachable!() };
        assert_eq!(
            complete.data, payload,
            "the full, untruncated large payload should have reassembled correctly"
        );
        assert!(!complete.is_request && !complete.is_response, "this is a plain resource send");

        let resolved = resolve_resource_destination(
            &receiver_transport,
            &event.link_id,
            Some(destination_hash),
        )
        .await;

        assert_eq!(
            resolved,
            Some(InboundLxmfDestination::Delivery(destination_hash)),
            "a plain Resource completed over a real Link to our own registered lxmf.delivery \
             destination should resolve via resolve_resource_destination the same way \
             spawn_inbound_worker's pre-existing resource_events() consumer already depends on"
        );
    }
}
