use super::*;
use crate::packet::PacketDataBuffer;

fn packet(seed: &[u8]) -> Packet {
    Packet { data: PacketDataBuffer::new_from_slice(seed), ..Default::default() }
}

#[test]
fn proof_context_expiry_does_not_forget_duplicate_membership() {
    let mut cache = PacketCache::new();
    let packet = packet(b"membership survives proof expiry");
    assert!(cache.update(&packet));
    cache.map.get_mut(&packet.hash()).expect("live proof context").time =
        Instant::now() - Duration::from_secs(181);
    cache.release(Duration::from_secs(180));
    assert!(cache
        .proof_context_for_destination(&AddressHash::new_from_hash(&packet.hash()))
        .is_none());
    assert!(
        cache.hashes().contains(&packet.hash()),
        "proof TTL must not expire duplicate membership"
    );
    assert!(!cache.update(&packet), "an expired proof context is not a new packet");
    assert!(
        cache.proof_context_for_destination(&AddressHash::new_from_hash(&packet.hash())).is_some(),
        "explicit packet tracking can recreate its actual destination binding"
    );
}

#[test]
fn restored_membership_does_not_invent_packet_or_proof_contexts() {
    let mut cache = PacketCache::new();
    let hash = packet(b"persisted membership only").hash();
    assert_eq!(cache.restore_hashes([hash, hash]), 1);
    assert_eq!(cache.hashes(), vec![hash]);
    assert!(cache.map.is_empty(), "restored hashes carry no destination/source proof context");
    assert!(cache.by_proof_destination.is_empty());
    cache.release(Duration::ZERO);
    assert_eq!(cache.hashes(), vec![hash]);
}

fn counter_hash(value: u64) -> Hash {
    let mut bytes = [0; crate::hash::HASH_SIZE];
    bytes[..8].copy_from_slice(&value.to_be_bytes());
    Hash::new(bytes)
}

#[test]
fn generations_rotate_only_above_half_budget_and_preserve_proof_context() {
    let mut cache = PacketCache::new();
    let packet = packet(b"retained proof context");
    assert!(cache.update(&packet));
    for value in 0..499_999 {
        cache.remember_hash(counter_hash(value));
    }
    assert_eq!(cache.hashes().len(), 500_000);
    assert!(!cache.maintain_duplicate_generations(), "exact half does not rotate");
    cache.remember_hash(counter_hash(499_999));
    assert_eq!(cache.hashes().len(), 500_001, "insertion never rotates implicitly");
    assert!(cache.maintain_duplicate_generations());
    assert!(cache.hashes().is_empty(), "only current membership is saved");
    assert!(cache.contains_hash(&packet.hash()), "previous generation filters duplicates");
    assert!(cache
        .proof_context_for_destination(&AddressHash::new_from_hash(&packet.hash()))
        .is_some());
    for value in 500_000..1_000_001 {
        cache.remember_hash(counter_hash(value));
    }
    assert!(cache.maintain_duplicate_generations());
    assert!(!cache.contains_hash(&packet.hash()), "second rotation displaces oldest membership");
    assert!(cache.contains_hash(&counter_hash(500_000)));
    assert!(
        cache.proof_context_for_destination(&AddressHash::new_from_hash(&packet.hash())).is_some(),
        "membership displacement must not expire independent proof context"
    );
}

#[test]
fn lookup_does_not_promote_previous_hash_but_remember_does_and_forget_clears_both() {
    let mut cache = PacketCache::new();
    let hash = counter_hash(7);
    cache.previous.insert(hash);
    assert!(cache.contains_hash(&hash));
    assert!(cache.hashes().is_empty());
    cache.remember_hash(hash);
    assert!(cache.previous.contains(&hash));
    assert_eq!(cache.hashes(), vec![hash]);
    cache.forget_hash(&hash);
    assert!(!cache.contains_hash(&hash));
}

#[test]
fn note_source_updates_context_without_remembering_membership() {
    let mut cache = PacketCache::new();
    let packet = packet(b"proof source independent from membership");
    let source = AddressHash::new_from_slice(&[5; crate::hash::ADDRESS_HASH_SIZE]);
    cache.note_source(&packet, source);
    assert!(cache.hashes().is_empty());
    assert_eq!(cache.source_iface_for_hash(&packet.hash()), Some(source));
}

#[tokio::test]
async fn current_generation_alone_survives_production_save_and_restore() {
    use crate::transport::{Transport, TransportConfig};
    let mut config = TransportConfig::default();
    config.set_transport_enabled(true);
    let transport = Transport::new(config);
    let packet = packet(b"current persisted membership");
    let previous = counter_hash(7);
    let cache = transport.handler.lock().await.packet_cache.clone();
    {
        let mut cache = cache.lock().await;
        cache.previous.insert(previous);
        cache.update(&packet);
        assert!(cache.contains_hash(&previous));
    }
    let temp = tempfile::tempdir().expect("owner storage");
    assert_eq!(transport.save_packet_hashlist(temp.path()).await.expect("save current"), 1);
    assert_eq!(
        std::fs::read(temp.path().join("packet_hashlist.raw")).expect("saved bytes"),
        packet.hash().as_slice()
    );
    let mut restored_config = TransportConfig::default();
    restored_config.set_transport_enabled(true);
    let restored = Transport::new(restored_config);
    assert_eq!(restored.restore_packet_hashlist(temp.path()).await.expect("restore current"), 1);
    let cache = restored.handler.lock().await.packet_cache.clone();
    let cache = cache.lock().await;
    assert!(cache.contains_hash(&packet.hash()));
    assert!(!cache.contains_hash(&previous));
    assert!(cache.map.is_empty());
    assert!(cache.by_proof_destination.is_empty());
    transport.cancel.cancel();
    restored.cancel.cancel();
}

#[tokio::test]
async fn native_membership_job_rotates_without_waiting_for_proof_ttl_cleanup() {
    use crate::transport::{Transport, TransportConfig};
    let transport = Transport::new(TransportConfig::default());
    let cache = transport.handler.lock().await.packet_cache.clone();
    {
        let mut cache = cache.lock().await;
        assert_eq!(cache.restore_hashes((0..500_001).map(counter_hash)), 500_001);
        assert_eq!(cache.hashes().len(), 500_001);
    }
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            if cache.lock().await.current.is_empty() {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("250ms native membership job runs independently of 90s context cleanup");
    assert!(cache.lock().await.contains_hash(&counter_hash(0)));
    transport.cancel.cancel();
}

#[tokio::test]
async fn rejected_outbound_dispatch_does_not_poison_duplicate_membership() {
    use crate::iface::{TxMessage, TxMessageType};
    use crate::transport::{Transport, TransportConfig};

    for recursive in [false, true] {
        for closed_queue in [false, true] {
            let transport = Transport::new(TransportConfig::default());
            if closed_queue {
                let channel = transport.iface_manager().lock().await.new_channel(1);
                drop(channel);
            }
            let packet = packet(b"failed outbound must remain eligible for ingress");
            let handler = transport.get_handler();
            let handler = handler.lock().await;
            let message =
                TxMessage { tx_type: TxMessageType::Broadcast(None), packet: packet.clone() };
            let dispatch = if recursive {
                handler.send_recursive_path_request_with_modes(message, None).await
            } else {
                handler.send(message).await
            };
            assert_eq!(dispatch.sent_ifaces, 0);
            assert_eq!(dispatch.queued_ifaces, 0);
            {
                let cache = handler.packet_cache.lock().await;
                assert!(
                    !cache.contains_hash(&packet.hash()),
                    "an outbound attempt rejected by every carrier cannot suppress later ingress; recursive={recursive}, closed_queue={closed_queue}"
                );
                assert!(
                    cache
                        .proof_context_for_destination(&AddressHash::new_from_hash(&packet.hash()))
                        .is_some(),
                    "destination-bound proof context remains independent of dispatch membership"
                );
            }
            assert!(
                handler.filter_duplicate_packets(&packet).await,
                "first real ingress must still be admitted after rejected outbound"
            );
            assert!(
                !handler.filter_duplicate_packets(&packet).await,
                "accepted ingress must continue to reject replays"
            );
            let retry = handler
                .send(TxMessage { tx_type: TxMessageType::Broadcast(None), packet: packet.clone() })
                .await;
            assert_eq!(retry.sent_ifaces + retry.queued_ifaces, 0);
            assert!(
                handler.packet_cache.lock().await.contains_hash(&packet.hash()),
                "a rejected retry cannot erase membership from an earlier accepted ingress"
            );
            transport.cancel.cancel();
        }
    }
}

#[tokio::test]
async fn rejected_prepared_dispatch_does_not_remember_membership() {
    use crate::destination::{DestinationName, SingleOutputDestination};
    use crate::identity::PrivateIdentity;
    use crate::transport::{SendPacketOutcome, Transport, TransportConfig};

    let transport = Transport::new(TransportConfig::default());
    let recipient = PrivateIdentity::new_from_name("unsent-prepared-recipient");
    let name = DestinationName::new("lxmf", "delivery");
    let destination =
        SingleOutputDestination::new(*recipient.as_identity(), name).desc.address_hash;
    let packet = Packet { destination, ..packet(b"unsent prepared ciphertext") };
    let trace = transport
        .send_prepared_single_packet_observed_with_trace(
            packet.clone(),
            *recipient.as_identity(),
            name,
            |_| {},
        )
        .await
        .expect("valid prepared packet");
    assert_eq!(trace.outcome, SendPacketOutcome::DroppedNoRoute);
    let cache = transport.get_handler().lock().await.packet_cache.clone();
    assert!(
        !cache.lock().await.contains_hash(&packet.hash()),
        "a prepared packet rejected by every carrier cannot suppress later ingress"
    );
    transport.cancel.cancel();
}

#[test]
fn reverse_route_authority_requires_forwarding_and_expires_with_source_context() {
    let mut cache = PacketCache::new();
    let packet = packet(b"reverse route lifetime");
    let hash = packet.hash();
    let destination = AddressHash::new_from_hash(&hash);
    let source = AddressHash::new_from_slice(&[3; 16]);
    let outbound = AddressHash::new_from_slice(&[4; 16]);
    cache.note_source(&packet, source);
    assert_eq!(
        cache.take_proof_source(&destination, outbound),
        None,
        "source observation alone cannot authorize a reverse route"
    );
    cache.note_forwarded_data(hash, outbound);
    assert_eq!(cache.tracked_count(), 3);
    assert_eq!(cache.take_proof_source(&destination, outbound), Some(source));
    assert_eq!(cache.take_proof_source(&destination, outbound), None);
    assert!(cache.reverse_routes.is_empty(), "consumption frees route metadata");
    assert_eq!(cache.tracked_count(), 2);
    cache.note_forwarded_data(hash, outbound);
    cache.map.get_mut(&hash).expect("source context").time =
        Instant::now() - Duration::from_secs(181);
    cache.release(Duration::from_secs(180));
    assert_eq!(cache.tracked_count(), 0);
    assert!(cache.reverse_routes.is_empty(), "pending reverse routes share source TTL");
}
