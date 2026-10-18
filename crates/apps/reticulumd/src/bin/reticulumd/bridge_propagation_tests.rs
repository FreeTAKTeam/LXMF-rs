use super::*;
use rns_transport::destination::DestinationName;
use rns_transport::identity::PrivateIdentity;
use rns_transport::iface::{IfaceRole, IfaceSource, InterfaceChannel, RxMessage};
use rns_transport::packet::PacketType;
use rns_transport::resource::{ResourceEventKind, ResourceOutcome};
use rns_transport::transport::TransportConfig;
use std::collections::HashMap;
use tokio::sync::Mutex;
use tokio::time::timeout;

fn test_transport(name: &str) -> Transport {
    let identity = PrivateIdentity::new_from_rand(OsRng);
    Transport::new(TransportConfig::new(name, &identity, true))
}

async fn forward(channel: &InterfaceChannel, packet: rns_transport::packet::Packet) {
    channel
        .rx_channel
        .send(RxMessage { address: *channel.address(), packet, source: IfaceSource::None })
        .await
        .expect("transport ingress");
}

#[tokio::test]
async fn propagation_cache_miss_preserves_live_resources() {
    for mismatch in [false, true] {
        let sender = test_transport("cache-sender");
        let receiver = test_transport("cache-relay");
        let remote = PrivateIdentity::new_from_rand(OsRng);
        let destination =
            receiver.add_destination(remote, DestinationName::new("lxmf", "propagation")).await;
        let destination = destination.lock().await.desc;
        let node = hex::encode(destination.address_hash.as_slice());
        let cache = Arc::new(Mutex::new(None));
        let mut sending =
            sender.iface_manager().lock().await.new_channel_with_role(64, IfaceRole::Unicast);
        let mut receiving =
            receiver.iface_manager().lock().await.new_channel_with_role(64, IfaceRole::Unicast);
        let link = propagation_link_for_node(&sender, &cache, &node, destination).await;
        let request = timeout(Duration::from_secs(2), sending.tx_channel.recv())
            .await
            .expect("request timeout")
            .expect("Link request");
        forward(&receiving, request.packet).await;
        let proof = timeout(Duration::from_secs(2), receiving.tx_channel.recv())
            .await
            .expect("proof timeout")
            .expect("Link proof");
        forward(&sending, proof.packet).await;
        timeout(Duration::from_secs(2), async {
            while link.lock().await.status() != LinkStatus::Active {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("Link activation");
        let link_id = *link.lock().await.id();
        let mut outbound = sender.resource_events();
        let mut lagging = sender.resource_events();
        let mut inbound = receiver.resource_events();
        let first_bytes = vec![0x31; 8192];
        let mut first_completion = None;
        let first = sender
            .send_resource_on_link_observed_with_completion(
                &link,
                first_bytes.clone(),
                None,
                |_, result| {
                    assert_eq!(result.current(), None, "register before native handoff");
                    first_completion = Some(result);
                },
            )
            .await
            .expect("first Resource");
        if mismatch {
            cache.lock().await.as_mut().expect("cache entry").node_hex =
                "another selected node".into();
        } else {
            *cache.lock().await = None;
        }
        let reused = propagation_link_for_node(&sender, &cache, &node, destination).await;
        assert!(
            Arc::ptr_eq(&link, &reused),
            "a cache miss must not replace an active transport Link"
        );
        assert_eq!(link.lock().await.status(), LinkStatus::Active);
        let second_bytes = vec![0x72; 4096];
        let second = sender
            .send_resource(&link_id, second_bytes.clone(), None)
            .await
            .expect("second Resource");
        let mut expected = HashMap::from([(first, first_bytes), (second, second_bytes)]);
        let mut completed = std::collections::HashSet::new();
        timeout(Duration::from_secs(5), async {
            while !expected.is_empty() || completed.len() != 2 {
                tokio::select! {
                    message = sending.tx_channel.recv() => {
                        let message = message.expect("sender channel");
                        assert_ne!(message.packet.header.packet_type, PacketType::LinkRequest, "cache lookup must not request another Link");
                        forward(&receiving, message.packet).await;
                    }
                    message = receiving.tx_channel.recv() => forward(&sending, message.expect("receiver channel").packet).await,
                    event = outbound.recv() => {
                        let event = event.expect("outbound Resource event");
                        match event.kind {
                            ResourceEventKind::OutboundComplete => {
                                assert_eq!(event.link_id, link_id);
                                assert!(event.hash == first || event.hash == second);
                                assert!(completed.insert(event.hash));
                            }
                            ResourceEventKind::OutboundFailed | ResourceEventKind::OutboundCancelled | ResourceEventKind::OutboundRejected => panic!("cache reacquisition interrupted Resource: {:?}", event.kind),
                            _ => {}
                        }
                    }
                    event = inbound.recv() => {
                        let event = event.expect("inbound Resource event");
                        match event.kind {
                            ResourceEventKind::Complete(resource) => assert_eq!(Some(resource.data), expected.remove(&event.hash)),
                            ResourceEventKind::InboundFailed(error) => panic!("recipient Resource failed: {error:?}"),
                            _ => {}
                        }
                    }
                }
            }
        }).await.expect("both Resources complete with recipient bytes and sender proofs");
        assert_eq!(link.lock().await.status(), LinkStatus::Active);
        // Overflow an unread legacy subscription with real dispatch failures.
        // The first Resource's result must still describe its recipient proof.
        drop(sending);
        for _ in 0..2048 {
            assert!(sender.send_resource(&link_id, vec![0x43], None).await.is_err());
        }
        assert!(matches!(
            lagging.try_recv(),
            Err(tokio::sync::broadcast::error::TryRecvError::Lagged(_))
        ));
        let mut completion = first_completion.expect("registered first Resource result");
        assert_eq!(completion.current(), Some(ResourceOutcome::Complete));
        assert_eq!(
            completion.wait().await.expect("retained recipient proof"),
            ResourceOutcome::Complete
        );
    }
}

#[tokio::test]
async fn propagation_cache_waits_for_native_closure_beyond_former_grace() {
    let transport = test_transport("slow-cache");
    let remote = PrivateIdentity::new_from_rand(OsRng);
    let destination = rns_transport::destination::SingleOutputDestination::new(
        *remote.as_identity(),
        DestinationName::new("lxmf", "propagation"),
    )
    .desc;
    let node = hex::encode(destination.address_hash.as_slice());
    let cache = Arc::new(Mutex::new(None));
    let _channel = transport.iface_manager().lock().await.new_channel(64);
    let original = propagation_link_for_node(&transport, &cache, &node, destination).await;
    original.lock().await.set_establishment_timeout(Duration::from_secs(90));
    // The displaced cache used std::time::Instant; virtual Tokio time would
    // not exercise its 25-second expiration. Native establishment remains live.
    tokio::time::sleep(Duration::from_secs(26)).await;
    assert_eq!(original.lock().await.status(), LinkStatus::Pending);
    let cached = cached_propagation_link(&cache, &node)
        .await
        .expect("native Pending Link remains cached beyond 25 seconds");
    assert!(Arc::ptr_eq(&original, &cached));
    let reused = propagation_link_for_node(&transport, &cache, &node, destination).await;
    assert!(Arc::ptr_eq(&original, &reused), "cache age must not override native establishment");
    original.lock().await.close();
    let replacement = propagation_link_for_node(&transport, &cache, &node, destination).await;
    assert!(!Arc::ptr_eq(&original, &replacement));
    assert_ne!(replacement.lock().await.status(), LinkStatus::Closed);
}
