use super::announce::{
    handle_announce, handle_validated_announce_unlocked, release_held_announces,
    validate_announce_on_worker, ValidatedAnnounce,
};
use super::announce_limits::{AnnounceLimits, AnnounceRateLimit};
use super::path::{
    handle_link_request_as_intermediate, handle_link_request_unlocked, handle_path_request_unlocked,
};
use super::wire::{
    collect_ready_link_activation_rtts, collect_ready_outbound_link_proofs,
    complete_link_resource_on_worker, find_ready_outbound_link_candidate, handle_data,
    handle_link_resource_data, handle_local_single_destination_data, handle_proof,
    validated_receipt_hash_unlocked,
};
use super::*;

use crate::channel::{
    ChannelError, MessageState as ChannelMessageState, SystemMessageTypes, TypedMessage,
};
use crate::destination::link::{Link, LinkEvent, LinkEventData, LinkPayload};
use crate::destination::{DestinationName, SingleInputDestination, SingleOutputDestination};
use crate::error::RnsError;
use crate::identity::PrivateIdentity;
use crate::packet::{Header, HeaderType, PacketContext};
use crate::resource::{ResourceCompletionJob, ResourceEventKind, ResourceManager};
use crate::transport::worker_boundary::{
    WorkerBackend, WorkerError, WorkerJob, WorkerJobFuture, WorkerJobKind, WorkerResult,
    WorkerResultKind,
};
use rand_core::OsRng;
use serde_bytes::ByteBuf;
use std::sync::Mutex as StdMutex;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use tokio::time::{timeout, Duration};

static RESOURCE_PREPARE_TEST_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
static WIRE_WORKER_PERMIT_TEST_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[tokio::test]
async fn link_in_payload_is_forwarded_to_received_data() {
    let identity = PrivateIdentity::new_from_rand(OsRng);
    let config = TransportConfig::new("test", &identity, true);
    let transport = Transport::new(config);

    let mut rx = transport.received_data_events();

    let link_id = AddressHash::new_from_rand(OsRng);
    let address_hash = AddressHash::new_from_rand(OsRng);
    let payload = LinkPayload::new_from_slice(b"hello");

    let _ = transport.link_in_event_tx.send(LinkEventData {
        id: link_id,
        address_hash,
        event: LinkEvent::Data(Box::new(payload)),
    });

    let received = timeout(Duration::from_millis(200), rx.recv())
        .await
        .expect("expected forwarded payload")
        .expect("broadcast receive");

    assert_eq!(received.destination, link_id);
    assert_eq!(received.data.as_slice(), b"hello");
    assert_eq!(received.payload_mode, ReceivedPayloadMode::FullWire);
}

#[tokio::test]
async fn link_out_payload_is_forwarded_to_received_data() {
    let identity = PrivateIdentity::new_from_rand(OsRng);
    let config = TransportConfig::new("test", &identity, true);
    let transport = Transport::new(config);

    let mut rx = transport.received_data_events();

    let link_id = AddressHash::new_from_rand(OsRng);
    let address_hash = AddressHash::new_from_rand(OsRng);
    let payload = LinkPayload::new_from_slice(b"outbound");

    let _ = transport.link_out_event_tx.send(LinkEventData {
        id: link_id,
        address_hash,
        event: LinkEvent::Data(Box::new(payload)),
    });

    let received = timeout(Duration::from_millis(200), rx.recv())
        .await
        .expect("expected forwarded payload")
        .expect("broadcast receive");

    assert_eq!(received.destination, link_id);
    assert_eq!(received.data.as_slice(), b"outbound");
    assert_eq!(received.payload_mode, ReceivedPayloadMode::FullWire);
}

#[tokio::test]
async fn drop_duplicates() {
    let mut config: TransportConfig = Default::default();
    config.set_retransmit(true);

    let transport = Transport::new(config);
    let handler = transport.get_handler();

    let _source1 = AddressHash::new_from_slice(&[1u8; 32]);
    let _source2 = AddressHash::new_from_slice(&[2u8; 32]);
    let next_hop_iface = AddressHash::new_from_slice(&[3u8; 32]);
    let destination = AddressHash::new_from_slice(&[4u8; 32]);

    let mut announce: Packet = Default::default();
    announce.header.header_type = HeaderType::Type2;
    announce.header.packet_type = PacketType::Announce;
    announce.header.hops = 3;
    announce.transport = Some(destination);

    assert!(handler.lock().await.filter_duplicate_packets(&announce).await);

    handle_announce(
        &announce,
        handler.lock().await,
        next_hop_iface,
        crate::iface::IfaceSource::None,
    )
    .await;

    let data_packet: Packet = Packet {
        data: PacketDataBuffer::new_from_slice(b"foo"),
        destination,
        ..Default::default()
    };
    let duplicate: Packet = data_packet;

    let mut different_packet = data_packet;
    different_packet.data = PacketDataBuffer::new_from_slice(b"bar");

    assert!(handler.lock().await.filter_duplicate_packets(&data_packet).await);
    assert!(!handler.lock().await.filter_duplicate_packets(&duplicate).await);
    assert!(handler.lock().await.filter_duplicate_packets(&different_packet).await);

    tokio::time::sleep(Duration::from_secs(2)).await;
    handler.lock().await.packet_cache.lock().await.release(Duration::from_secs(1));

    // Packet should have been removed from cache (stale)
    assert!(handler.lock().await.filter_duplicate_packets(&duplicate).await);
}

#[tokio::test]
async fn duplicate_filter_skips_busy_inbound_link_proof_status_check() {
    let identity = PrivateIdentity::new_from_rand(OsRng);
    let transport = Transport::new(TransportConfig::new("test", &identity, true));
    let handler = transport.get_handler();

    let destination = crate::destination::DestinationDesc {
        identity: *identity.as_identity(),
        address_hash: identity.as_identity().address_hash,
        name: DestinationName::new("lxmf", "delivery"),
    };
    let (events, _) = tokio::sync::broadcast::channel(8);
    let link_id = AddressHash::new_from_rand(OsRng);
    let link = Arc::new(Mutex::new(Link::new(destination, events)));
    handler.lock().await.in_links.insert(link_id, link.clone());
    let _busy_guard = link.lock().await;

    let packet = Packet {
        header: Header { packet_type: PacketType::Proof, ..Default::default() },
        context: PacketContext::LinkRequestProof,
        destination: link_id,
        data: PacketDataBuffer::new_from_slice(b"proof"),
        ..Default::default()
    };

    let accepted = timeout(
        Duration::from_millis(200),
        TransportHandler::filter_duplicate_packets_unlocked(handler, &packet),
    )
    .await
    .expect("duplicate filter should not wait for a busy inbound link");

    assert!(accepted);
}

#[tokio::test]
async fn link_data_handling_skips_busy_inbound_link() {
    let identity = PrivateIdentity::new_from_rand(OsRng);
    let transport = Transport::new(TransportConfig::new("test", &identity, true));
    let handler = transport.get_handler();

    let destination = crate::destination::DestinationDesc {
        identity: *identity.as_identity(),
        address_hash: identity.as_identity().address_hash,
        name: DestinationName::new("lxmf", "delivery"),
    };
    let (events, _) = tokio::sync::broadcast::channel(8);
    let link_id = AddressHash::new_from_rand(OsRng);
    let link = Arc::new(Mutex::new(Link::new(destination, events)));
    handler.lock().await.in_links.insert(link_id, link.clone());
    let _busy_guard = link.lock().await;

    let packet = Packet {
        header: Header { destination_type: DestinationType::Link, ..Default::default() },
        destination: link_id,
        data: PacketDataBuffer::new_from_slice(b"link-data"),
        ..Default::default()
    };

    timeout(
        Duration::from_millis(200),
        handle_data(
            &packet,
            AddressHash::new_from_rand(OsRng),
            handler.clone(),
            handler.lock().await,
        ),
    )
    .await
    .expect("link data handling should not wait for a busy inbound link");
}

#[tokio::test]
async fn unlocked_dispatch_does_not_hold_shared_locks_while_iface_queue_waits() {
    let identity = PrivateIdentity::new_from_rand(OsRng);
    let config = TransportConfig::new("test", &identity, false);
    let transport = Transport::new(config);
    let handler = transport.get_handler();
    let channel = transport.iface_manager().lock().await.new_channel(1);
    let iface = *channel.address();

    let first = TxMessage {
        tx_type: TxMessageType::Direct(iface),
        packet: Packet { data: PacketDataBuffer::new_from_slice(b"first"), ..Default::default() },
    };
    let second = TxMessage {
        tx_type: TxMessageType::Direct(iface),
        packet: Packet { data: PacketDataBuffer::new_from_slice(b"second"), ..Default::default() },
    };

    let first_trace = TransportHandler::send_message_unlocked(handler.clone(), first).await;
    assert_eq!(first_trace.sent_ifaces, 1);

    let pending_send =
        tokio::spawn(TransportHandler::send_message_unlocked(handler.clone(), second));
    tokio::time::sleep(Duration::from_millis(10)).await;

    let guard = timeout(Duration::from_millis(20), handler.lock())
        .await
        .expect("slow interface dispatch must not hold transport handler lock");
    drop(guard);

    let iface_manager = transport.iface_manager();
    let iface_guard = timeout(Duration::from_millis(20), iface_manager.lock())
        .await
        .expect("slow interface dispatch must not hold interface manager lock");
    drop(iface_guard);

    let second_trace = pending_send.await.expect("send task");
    assert_eq!(second_trace.failed_ifaces, 1);
}

#[tokio::test]
async fn cleanup_does_not_hold_iface_manager_while_waiting_for_handler() {
    let identity = PrivateIdentity::new_from_rand(OsRng);
    let config = TransportConfig::new("test", &identity, true);
    let transport = Transport::new(config);
    let handler = transport.get_handler();
    let iface_manager = transport.iface_manager();
    let handler_guard = handler.lock().await;

    let cleanup = tokio::spawn(super::jobs::cleanup_path_state_unlocked(
        handler.clone(),
        iface_manager.clone(),
    ));
    tokio::task::yield_now().await;

    let iface_guard = timeout(Duration::from_millis(20), iface_manager.lock())
        .await
        .expect("cleanup must not hold interface manager while waiting for transport handler");
    drop(iface_guard);

    drop(handler_guard);
    timeout(Duration::from_millis(200), cleanup)
        .await
        .expect("cleanup should finish after handler is released")
        .expect("cleanup task should not panic");
}

#[tokio::test]
async fn link_fanout_does_not_hold_handler_while_waiting_for_link() {
    let identity = PrivateIdentity::new_from_rand(OsRng);
    let config = TransportConfig::new("test", &identity, true);
    let transport = Arc::new(Transport::new(config));
    let handler = transport.get_handler();

    let destination = crate::destination::DestinationDesc {
        identity: *identity.as_identity(),
        address_hash: identity.as_identity().address_hash,
        name: DestinationName::new("lxmf", "delivery"),
    };
    let (tx, _) = tokio::sync::broadcast::channel(8);
    let link = Arc::new(Mutex::new(Link::new(destination, tx)));
    handler.lock().await.out_links.insert(destination.address_hash, link.clone());

    let link_guard = link.lock().await;
    let fanout = tokio::spawn({
        let transport = Arc::clone(&transport);
        async move {
            transport.send_to_all_out_links(b"blocked-link").await;
        }
    });
    tokio::task::yield_now().await;

    let handler_guard = timeout(Duration::from_millis(20), handler.lock())
        .await
        .expect("link fanout must not hold transport handler while waiting for a link lock");
    drop(handler_guard);

    drop(link_guard);
    timeout(Duration::from_millis(200), fanout)
        .await
        .expect("fanout should finish after link is released")
        .expect("fanout task should not panic");
}

#[tokio::test]
async fn link_fanout_skips_busy_links_instead_of_waiting() {
    let identity = PrivateIdentity::new_from_rand(OsRng);
    let config = TransportConfig::new("test", &identity, true);
    let transport = Arc::new(Transport::new(config));
    let handler = transport.get_handler();

    let destination = crate::destination::DestinationDesc {
        identity: *identity.as_identity(),
        address_hash: identity.as_identity().address_hash,
        name: DestinationName::new("lxmf", "delivery"),
    };
    let (tx, _) = tokio::sync::broadcast::channel(8);
    let link = Arc::new(Mutex::new(Link::new(destination, tx)));
    handler.lock().await.out_links.insert(destination.address_hash, link.clone());

    let _link_guard = link.lock().await;
    timeout(Duration::from_millis(200), transport.send_to_all_out_links(b"busy-link"))
        .await
        .expect("public link fanout should skip busy links instead of waiting");
}

#[tokio::test]
async fn outbound_encryption_skips_busy_destination() {
    let identity = PrivateIdentity::new_from_rand(OsRng);
    let config = TransportConfig::new("test", &identity, false);
    let transport = Arc::new(Transport::new(config));
    let handler = transport.get_handler();

    let remote_identity = PrivateIdentity::new_from_rand(OsRng);
    let destination = Arc::new(Mutex::new(SingleOutputDestination::new(
        *remote_identity.as_identity(),
        DestinationName::new("lxmf", "delivery"),
    )));
    let destination_hash = destination.lock().await.desc.address_hash;
    handler.lock().await.single_out_destinations.insert(destination_hash, destination.clone());

    let _destination_guard = destination.lock().await;
    let send = transport.send_packet_with_outcome(Packet {
        header: Header {
            destination_type: DestinationType::Single,
            packet_type: PacketType::Data,
            ..Default::default()
        },
        destination: destination_hash,
        data: PacketDataBuffer::new_from_slice(b"encrypted"),
        ..Default::default()
    });

    let handler_guard = timeout(Duration::from_millis(20), handler.lock())
        .await
        .expect("outbound encryption must not hold handler before checking destination lock");
    drop(handler_guard);

    let outcome = timeout(Duration::from_millis(200), send)
        .await
        .expect("outbound encryption should not wait for a busy destination");
    assert_eq!(outcome, SendPacketOutcome::DroppedEncryptFailed);
}

#[tokio::test]
async fn outbound_encryption_saturation_returns_without_waiting() {
    let identity = PrivateIdentity::new_from_rand(OsRng);
    let config = TransportConfig::new("test", &identity, false);
    let transport = Arc::new(Transport::new(config));
    let handler = transport.get_handler();

    let remote_identity = PrivateIdentity::new_from_rand(OsRng);
    let destination = Arc::new(Mutex::new(SingleOutputDestination::new(
        *remote_identity.as_identity(),
        DestinationName::new("lxmf", "delivery"),
    )));
    let destination_hash = destination.lock().await.desc.address_hash;
    handler.lock().await.single_out_destinations.insert(destination_hash, destination);

    let permits = super::handler::outbound_encryption_permits();
    let _held_permits = permits
        .acquire_many_owned(super::handler::MAX_OUTBOUND_ENCRYPTION_WORKERS as u32)
        .await
        .expect("outbound encryption semaphore is open");

    let outcome = timeout(
        Duration::from_millis(50),
        transport.send_packet_with_outcome(Packet {
            header: Header {
                destination_type: DestinationType::Single,
                packet_type: PacketType::Data,
                ..Default::default()
            },
            destination: destination_hash,
            data: PacketDataBuffer::new_from_slice(b"encrypted"),
            ..Default::default()
        }),
    )
    .await
    .expect("saturated outbound encryption lane should return immediately");

    assert_eq!(outcome, SendPacketOutcome::DroppedEncryptFailed);
}

#[tokio::test]
async fn announce_lookup_key_uses_destination_hash() {
    let local_identity = PrivateIdentity::new_from_rand(OsRng);
    let mut config = TransportConfig::new("test", &local_identity, true);
    config.set_retransmit(true);
    let transport = Transport::new(config);
    let handler = transport.get_handler();

    let remote_identity = PrivateIdentity::new_from_rand(OsRng);
    let mut remote_destination =
        SingleInputDestination::new(remote_identity, DestinationName::new("lxmf", "delivery"));
    let announce = remote_destination.announce(OsRng, None).expect("valid announce packet");

    let announced_destination = announce.destination;
    let announced_identity = *remote_destination.identity.address_hash();
    assert_ne!(
        announced_destination, announced_identity,
        "destination hash must differ from identity hash for named destinations"
    );

    let iface = AddressHash::new_from_rand(OsRng);
    handle_announce(&announce, handler.lock().await, iface, crate::iface::IfaceSource::None).await;

    let guard = handler.lock().await;
    let keyed_by_destination = guard.announce_table.packet_for_destination(&announced_destination);
    assert!(keyed_by_destination.is_some(), "announce lookup should be keyed by destination hash");
    let keyed_by_identity = guard.announce_table.packet_for_destination(&announced_identity);
    assert!(keyed_by_identity.is_none(), "identity hash must not be used as announce lookup key");
}

#[test]
fn validated_announce_rebuilds_from_worker_result() {
    let remote_identity = PrivateIdentity::new_from_rand(OsRng);
    let destination = SingleOutputDestination::new(
        *remote_identity.as_identity(),
        DestinationName::new("lxmf", "delivery"),
    );
    let name_hash = {
        let mut bytes = [0u8; crate::destination::NAME_HASH_LENGTH];
        bytes.copy_from_slice(destination.desc.name.as_name_hash_slice());
        bytes
    };
    let result = WorkerResultKind::AnnounceValidated {
        destination: {
            let mut bytes = [0u8; crate::hash::ADDRESS_HASH_SIZE];
            bytes.copy_from_slice(destination.desc.address_hash.as_slice());
            bytes
        },
        public_key: *destination.desc.identity.public_key.as_bytes(),
        verifying_key: *destination.desc.identity.verifying_key.as_bytes(),
        name_hash,
        app_data: ByteBuf::from(b"announce app data".to_vec()),
        ratchet: None,
    };

    let announce = ValidatedAnnounce::from_worker_result(result).expect("worker announce result");

    assert_eq!(announce.destination.desc.address_hash, destination.desc.address_hash);
    assert_eq!(announce.destination.desc.identity.public_key, destination.desc.identity.public_key);
    assert_eq!(
        announce.destination.desc.identity.verifying_key,
        destination.desc.identity.verifying_key
    );
    assert_eq!(announce.destination.desc.name.as_name_hash_slice(), name_hash);
    assert_eq!(announce.app_data.as_slice(), b"announce app data");
    assert!(announce.ratchet.is_none());
}

#[test]
fn validated_announce_rejects_worker_result_hash_mismatch() {
    let remote_identity = PrivateIdentity::new_from_rand(OsRng);
    let destination = SingleOutputDestination::new(
        *remote_identity.as_identity(),
        DestinationName::new("lxmf", "delivery"),
    );
    let mut name_hash = [0u8; crate::destination::NAME_HASH_LENGTH];
    name_hash.copy_from_slice(destination.desc.name.as_name_hash_slice());
    let result = WorkerResultKind::AnnounceValidated {
        destination: [0x55; crate::hash::ADDRESS_HASH_SIZE],
        public_key: *destination.desc.identity.public_key.as_bytes(),
        verifying_key: *destination.desc.identity.verifying_key.as_bytes(),
        name_hash,
        app_data: ByteBuf::new(),
        ratchet: None,
    };

    let Err(err) = ValidatedAnnounce::from_worker_result(result) else {
        panic!("hash mismatch must fail");
    };

    assert!(matches!(err, WorkerError::InvalidJob { .. }));
}

struct ValidatingAnnounceBackend {
    calls: Arc<AtomicUsize>,
}

impl WorkerBackend for ValidatingAnnounceBackend {
    fn submit(&self, job: WorkerJob) -> WorkerJobFuture<'_> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Box::pin(async move {
            let WorkerJobKind::ValidateAnnounce { packet_wire } = job.kind else {
                return Err(WorkerError::InvalidJob {
                    message: "expected validate announce job".to_string(),
                });
            };
            let packet = Packet::from_bytes(&packet_wire).map_err(|err| WorkerError::Packet {
                message: format!("packet decode failed: {err:?}"),
            })?;
            let announce = super::announce::validate_announce(&packet).map_err(|err| {
                WorkerError::Packet { message: format!("announce validate failed: {err:?}") }
            })?;
            let mut destination = [0u8; crate::hash::ADDRESS_HASH_SIZE];
            destination.copy_from_slice(announce.destination.desc.address_hash.as_slice());
            let mut name_hash = [0u8; crate::destination::NAME_HASH_LENGTH];
            name_hash.copy_from_slice(announce.destination.desc.name.as_name_hash_slice());
            Ok(WorkerResult {
                id: job.id,
                kind: WorkerResultKind::AnnounceValidated {
                    destination,
                    public_key: *announce.destination.desc.identity.public_key.as_bytes(),
                    verifying_key: *announce.destination.desc.identity.verifying_key.as_bytes(),
                    name_hash,
                    app_data: ByteBuf::from(announce.app_data.as_slice().to_vec()),
                    ratchet: announce.ratchet.map(|ratchet| ByteBuf::from(ratchet.to_vec())),
                },
            })
        })
    }
}

struct FailingAnnounceBackend {
    calls: Arc<AtomicUsize>,
}

impl WorkerBackend for FailingAnnounceBackend {
    fn submit(&self, _job: WorkerJob) -> WorkerJobFuture<'_> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Box::pin(async {
            Err(WorkerError::BackendUnavailable {
                message: "worker process unavailable".to_string(),
            })
        })
    }
}

struct BlockingAnnounceBackend {
    started: Arc<tokio::sync::Notify>,
    release: Arc<tokio::sync::Notify>,
}

impl WorkerBackend for BlockingAnnounceBackend {
    fn submit(&self, job: WorkerJob) -> WorkerJobFuture<'_> {
        let started = self.started.clone();
        let release = self.release.clone();
        Box::pin(async move {
            let WorkerJobKind::ValidateAnnounce { .. } = job.kind else {
                return Err(WorkerError::InvalidJob {
                    message: "expected validate announce job".to_string(),
                });
            };
            started.notify_one();
            release.notified().await;
            Err(WorkerError::BackendUnavailable {
                message: "announce worker intentionally blocked".to_string(),
            })
        })
    }
}

struct OutboundEncryptBackend {
    calls: Arc<AtomicUsize>,
}

impl WorkerBackend for OutboundEncryptBackend {
    fn submit(&self, job: WorkerJob) -> WorkerJobFuture<'_> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Box::pin(async move {
            let (packet_wires, batch) = match job.kind {
                WorkerJobKind::OutboundEncrypt { packet_wire, .. } => (vec![packet_wire], false),
                WorkerJobKind::OutboundEncryptBatch { items } => {
                    (items.into_iter().map(|item| item.packet_wire).collect(), true)
                }
                _ => {
                    return Err(WorkerError::InvalidJob {
                        message: "expected outbound encrypt job".to_string(),
                    });
                }
            };
            let mut encrypted = Vec::with_capacity(packet_wires.len());
            for packet_wire in packet_wires {
                let mut packet = Packet::from_bytes(&packet_wire).map_err(|err| {
                    WorkerError::Packet { message: format!("packet decode failed: {err:?}") }
                })?;
                packet.data = PacketDataBuffer::new_from_slice(b"remote encrypted");
                let packet_wire = packet.to_bytes().map_err(|err| WorkerError::Packet {
                    message: format!("packet encode failed: {err:?}"),
                })?;
                encrypted.push(super::worker_boundary::PacketWireBatchItem { packet_wire });
            }
            if !batch && encrypted.len() == 1 {
                let packet_wire = encrypted.remove(0).packet_wire;
                Ok(WorkerResult { id: job.id, kind: WorkerResultKind::PacketWire { packet_wire } })
            } else {
                Ok(WorkerResult {
                    id: job.id,
                    kind: WorkerResultKind::PacketWireBatch { items: encrypted },
                })
            }
        })
    }
}

struct SingleDestinationDecryptBackend {
    calls: Arc<AtomicUsize>,
}

impl WorkerBackend for SingleDestinationDecryptBackend {
    fn submit(&self, job: WorkerJob) -> WorkerJobFuture<'_> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Box::pin(async move {
            match job.kind {
                WorkerJobKind::SingleDestinationDecrypt { .. } => Ok(WorkerResult {
                    id: job.id,
                    kind: WorkerResultKind::DestinationPayload {
                        payload: ByteBuf::from(b"remote decrypted".to_vec()),
                        ratchet_used: false,
                    },
                }),
                WorkerJobKind::SingleDestinationDecryptBatch { items } => Ok(WorkerResult {
                    id: job.id,
                    kind: WorkerResultKind::DestinationPayloadBatch {
                        items: items
                            .into_iter()
                            .map(|_| super::worker_boundary::DestinationPayloadBatchItem {
                                payload: ByteBuf::from(b"remote decrypted".to_vec()),
                                ratchet_used: false,
                            })
                            .collect(),
                    },
                }),
                _ => Err(WorkerError::InvalidJob {
                    message: "expected single destination decrypt job".to_string(),
                }),
            }
        })
    }
}

struct ResourceCompleteBackend {
    calls: Arc<AtomicUsize>,
}

impl WorkerBackend for ResourceCompleteBackend {
    fn submit(&self, job: WorkerJob) -> WorkerJobFuture<'_> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Box::pin(async move {
            let WorkerJobKind::ResourceComplete { resource_hash, .. } = job.kind else {
                return Err(WorkerError::InvalidJob {
                    message: "expected resource complete job".to_string(),
                });
            };
            Ok(WorkerResult {
                id: job.id,
                kind: WorkerResultKind::ResourceCompleted {
                    resource_hash,
                    proof: [0x77; crate::hash::HASH_SIZE],
                    data: ByteBuf::from(b"remote completed".to_vec()),
                    metadata: None,
                    request_id: None,
                    is_request: false,
                    is_response: false,
                },
            })
        })
    }
}

#[tokio::test]
async fn announce_validation_uses_configured_worker_backend() {
    let remote_identity = PrivateIdentity::new_from_rand(OsRng);
    let mut remote_destination =
        SingleInputDestination::new(remote_identity, DestinationName::new("lxmf", "delivery"));
    let packet =
        remote_destination.announce(OsRng, Some(b"worker-backed announce")).expect("announce");
    let calls = Arc::new(AtomicUsize::new(0));
    let backend = Arc::new(ValidatingAnnounceBackend { calls: calls.clone() });

    let announce = validate_announce_on_worker(packet, Some(backend)).await.expect("announce");

    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(announce.app_data.as_slice(), b"worker-backed announce");
    assert_eq!(announce.destination.desc.address_hash, remote_destination.desc.address_hash);
}

#[tokio::test]
async fn outbound_encryption_uses_configured_worker_backend() {
    let local_identity = PrivateIdentity::new_from_rand(OsRng);
    let calls = Arc::new(AtomicUsize::new(0));
    let mut config = TransportConfig::new("test", &local_identity, false);
    config.set_outbound_worker_backend(Arc::new(OutboundEncryptBackend { calls: calls.clone() }));
    let transport = Transport::new(config);
    let handler = transport.get_handler();

    let remote_identity = PrivateIdentity::new_from_rand(OsRng);
    let destination = Arc::new(Mutex::new(SingleOutputDestination::new(
        *remote_identity.as_identity(),
        DestinationName::new("lxmf", "delivery"),
    )));
    let destination_hash = destination.lock().await.desc.address_hash;
    handler.lock().await.single_out_destinations.insert(destination_hash, destination);

    let trace = TransportHandler::send_packet_with_trace_unlocked(
        handler,
        Packet {
            header: Header {
                destination_type: DestinationType::Single,
                packet_type: PacketType::Data,
                ..Default::default()
            },
            destination: destination_hash,
            data: PacketDataBuffer::new_from_slice(b"plain"),
            ..Default::default()
        },
    )
    .await;

    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(trace.outcome, SendPacketOutcome::DroppedNoRoute);
}

#[tokio::test]
async fn single_destination_decrypt_uses_configured_worker_backend() {
    let local_identity = PrivateIdentity::new_from_rand(OsRng);
    let destination = Arc::new(Mutex::new(SingleInputDestination::new(
        local_identity,
        DestinationName::new("lxmf", "delivery"),
    )));
    let destination_hash = destination.lock().await.desc.address_hash;
    let packet = Packet {
        header: Header {
            destination_type: DestinationType::Single,
            packet_type: PacketType::Data,
            ..Default::default()
        },
        destination: destination_hash,
        data: PacketDataBuffer::new_from_slice(b"ciphertext"),
        ..Default::default()
    };
    let (received_tx, mut received_rx) = tokio::sync::broadcast::channel(4);
    let calls = Arc::new(AtomicUsize::new(0));
    let backend = Arc::new(SingleDestinationDecryptBackend { calls: calls.clone() });
    let batch_lane = super::crypto_batch_lane::InboundCryptoBatchLane::spawn(backend);

    assert!(
        handle_local_single_destination_data(
            &packet,
            destination,
            received_tx,
            "test",
            Some(batch_lane),
        )
        .await
    );

    let received = received_rx.recv().await.expect("received data");
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(received.data.as_slice(), b"remote decrypted");
}

#[tokio::test]
async fn single_destination_decrypt_falls_back_when_worker_backend_fails() {
    let _worker_permit_guard = WIRE_WORKER_PERMIT_TEST_LOCK.lock().await;
    let local_identity = PrivateIdentity::new_from_rand(OsRng);
    let destination = Arc::new(Mutex::new(SingleInputDestination::new(
        local_identity,
        DestinationName::new("lxmf", "delivery"),
    )));
    let destination_hash = destination.lock().await.desc.address_hash;
    let public_key = *destination.lock().await.desc.identity.public_key.as_bytes();
    let salt = {
        let destination = destination.lock().await;
        let mut salt = [0u8; crate::hash::ADDRESS_HASH_SIZE];
        salt.copy_from_slice(destination.identity.as_identity().address_hash.as_slice());
        salt
    };
    let ciphertext = crate::ratchets::encrypt_for_public_key_bytes(
        &public_key,
        &salt,
        b"local decrypted",
        OsRng,
    )
    .expect("encrypt payload");
    let packet = Packet {
        header: Header {
            destination_type: DestinationType::Single,
            packet_type: PacketType::Data,
            ..Default::default()
        },
        destination: destination_hash,
        data: PacketDataBuffer::new_from_slice(&ciphertext),
        ..Default::default()
    };
    let (received_tx, mut received_rx) = tokio::sync::broadcast::channel(4);
    let calls = Arc::new(AtomicUsize::new(0));
    let backend = Arc::new(FailingAnnounceBackend { calls: calls.clone() });
    let batch_lane = super::crypto_batch_lane::InboundCryptoBatchLane::spawn(backend);

    assert!(
        handle_local_single_destination_data(
            &packet,
            destination,
            received_tx,
            "test",
            Some(batch_lane),
        )
        .await
    );

    let received = received_rx.recv().await.expect("received data");
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(received.data.as_slice(), b"local decrypted");
}

#[tokio::test]
async fn outbound_encryption_falls_back_when_worker_backend_fails() {
    let local_identity = PrivateIdentity::new_from_rand(OsRng);
    let calls = Arc::new(AtomicUsize::new(0));
    let mut config = TransportConfig::new("test", &local_identity, false);
    config.set_outbound_worker_backend(Arc::new(FailingAnnounceBackend { calls: calls.clone() }));
    let transport = Transport::new(config);
    let handler = transport.get_handler();

    let remote_identity = PrivateIdentity::new_from_rand(OsRng);
    let destination = Arc::new(Mutex::new(SingleOutputDestination::new(
        *remote_identity.as_identity(),
        DestinationName::new("lxmf", "delivery"),
    )));
    let destination_hash = destination.lock().await.desc.address_hash;
    handler.lock().await.single_out_destinations.insert(destination_hash, destination);

    let trace = TransportHandler::send_packet_with_trace_unlocked(
        handler,
        Packet {
            header: Header {
                destination_type: DestinationType::Single,
                packet_type: PacketType::Data,
                ..Default::default()
            },
            destination: destination_hash,
            data: PacketDataBuffer::new_from_slice(b"plain"),
            ..Default::default()
        },
    )
    .await;

    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(trace.outcome, SendPacketOutcome::DroppedNoRoute);
}

#[tokio::test]
async fn announce_validation_falls_back_when_worker_backend_fails() {
    let remote_identity = PrivateIdentity::new_from_rand(OsRng);
    let mut remote_destination =
        SingleInputDestination::new(remote_identity, DestinationName::new("lxmf", "delivery"));
    let packet =
        remote_destination.announce(OsRng, Some(b"local fallback announce")).expect("announce");
    let calls = Arc::new(AtomicUsize::new(0));
    let backend = Arc::new(FailingAnnounceBackend { calls: calls.clone() });

    let announce = validate_announce_on_worker(packet, Some(backend)).await.expect("announce");

    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(announce.app_data.as_slice(), b"local fallback announce");
    assert_eq!(announce.destination.desc.address_hash, remote_destination.desc.address_hash);
}

#[tokio::test]
async fn announce_processing_skips_busy_existing_destination() {
    let local_identity = PrivateIdentity::new_from_rand(OsRng);
    let transport = Transport::new(TransportConfig::new("test", &local_identity, true));
    let handler = transport.get_handler();

    let remote_identity = PrivateIdentity::new_from_rand(OsRng);
    let existing = Arc::new(Mutex::new(SingleOutputDestination::new(
        *remote_identity.as_identity(),
        DestinationName::new("lxmf", "delivery"),
    )));
    let destination_hash = existing.lock().await.desc.address_hash;
    handler.lock().await.single_out_destinations.insert(destination_hash, existing.clone());
    let _busy_guard = existing.lock().await;

    let announce = ValidatedAnnounce {
        destination: SingleOutputDestination::new(
            *remote_identity.as_identity(),
            DestinationName::new("lxmf", "delivery"),
        ),
        app_data: PacketDataBuffer::new(),
        ratchet: None,
    };
    let packet = Packet {
        header: Header { packet_type: PacketType::Announce, ..Default::default() },
        destination: destination_hash,
        data: PacketDataBuffer::new_from_slice(b"announce"),
        ..Default::default()
    };

    timeout(
        Duration::from_millis(200),
        handle_validated_announce_unlocked(
            &packet,
            handler,
            AddressHash::new_from_rand(OsRng),
            crate::iface::IfaceSource::None,
            announce,
        ),
    )
    .await
    .expect("announce processing should not wait for a busy existing destination");
}

#[tokio::test]
async fn reticulum_path_table_persistence_restores_route_and_identity_from_cached_announce() {
    let temp = tempfile::tempdir().expect("tempdir");
    let local_identity = PrivateIdentity::new_from_rand(OsRng);
    let mut config = TransportConfig::new("test", &local_identity, true);
    config.set_retransmit(true);
    let transport = Transport::new(config);
    let iface = *transport.iface_manager().lock().await.new_channel(16).address();

    let remote_identity = PrivateIdentity::new_from_rand(OsRng);
    let mut remote_destination =
        SingleInputDestination::new(remote_identity, DestinationName::new("lxmf", "delivery"));
    let announce = remote_destination.announce(OsRng, None).expect("valid announce packet");
    let destination = announce.destination;
    let expected_identity = *remote_destination.identity.as_identity();

    handle_announce(
        &announce,
        transport.get_handler().lock().await,
        iface,
        crate::iface::IfaceSource::None,
    )
    .await;

    assert_eq!(transport.save_reticulum_path_table(temp.path()).await.expect("save"), 1);
    assert!(temp.path().join("destination_table").exists());
    let destination_table = std::fs::read(temp.path().join("destination_table")).expect("read");
    let value: rmpv::Value =
        rmpv::decode::read_value(&mut std::io::Cursor::new(destination_table)).expect("msgpack");
    let rmpv::Value::Array(entries) = value else {
        panic!("destination_table must be an array");
    };
    let rmpv::Value::Array(fields) = &entries[0] else {
        panic!("destination_table entry must be an array");
    };
    let rmpv::Value::Binary(interface_hash) = &fields[6] else {
        panic!("interface hash must be binary");
    };
    assert_eq!(interface_hash.len(), crate::hash::HASH_SIZE);
    assert!(temp
        .path()
        .join("cache")
        .join("announces")
        .join(hex::encode(announce.hash().as_slice()))
        .exists());

    let mut restored_config = TransportConfig::new("test", &local_identity, true);
    restored_config.set_retransmit(true);
    let restored = Transport::new(restored_config);
    let restored_iface = *restored.iface_manager().lock().await.new_channel(16).address();
    assert_eq!(restored_iface, iface, "test relies on deterministic iface hashes");
    assert_eq!(restored.restore_reticulum_path_table(temp.path()).await.expect("restore"), 1);
    let restored_identity = restored.destination_identity(&destination).await.expect("identity");
    assert_eq!(restored_identity.public_key_bytes(), expected_identity.public_key_bytes());
    assert_eq!(restored_identity.verifying_key_bytes(), expected_identity.verifying_key_bytes());
    assert!(restored.has_path(&destination).await, "path table entry should be restored");
}

#[tokio::test]
async fn reticulum_tunnel_table_persistence_restores_tunnel_paths_after_reappearance() {
    let temp = tempfile::tempdir().expect("tempdir");
    let local_identity = PrivateIdentity::new_from_rand(OsRng);
    let mut config = TransportConfig::new("test", &local_identity, true);
    config.set_retransmit(true);
    let transport = Transport::new(config);
    let iface = *transport.iface_manager().lock().await.new_channel(16).address();
    let iface_hash = transport.iface_manager().lock().await.full_hash(&iface).expect("iface hash");

    let tunnel_identity = PrivateIdentity::new_from_rand(OsRng);
    let tunnel_synth = super::tunnels::synthesize_tunnel_packet(&tunnel_identity, iface_hash);
    {
        let handler = transport.get_handler();
        let mut handler = handler.lock().await;
        super::tunnels::handle_tunnel_synthesize_packet(&tunnel_synth, &mut handler, iface);
    }

    let remote_identity = PrivateIdentity::new_from_rand(OsRng);
    let mut remote_destination =
        SingleInputDestination::new(remote_identity, DestinationName::new("lxmf", "delivery"));
    let announce = remote_destination.announce(OsRng, None).expect("valid announce packet");
    let destination = announce.destination;
    handle_announce(
        &announce,
        transport.get_handler().lock().await,
        iface,
        crate::iface::IfaceSource::None,
    )
    .await;

    assert_eq!(transport.save_reticulum_path_table(temp.path()).await.expect("save"), 1);
    assert!(temp.path().join("tunnels").exists());
    std::fs::remove_file(temp.path().join("destination_table")).expect("remove active path table");

    let mut restored_config = TransportConfig::new("test", &local_identity, true);
    restored_config.set_retransmit(true);
    let restored = Transport::new(restored_config);
    let restored_iface = *restored.iface_manager().lock().await.new_channel(16).address();
    let restored_iface_hash =
        restored.iface_manager().lock().await.full_hash(&restored_iface).expect("iface hash");
    assert_eq!(restored_iface_hash, iface_hash, "test relies on deterministic iface hashes");

    assert_eq!(restored.restore_reticulum_path_table(temp.path()).await.expect("restore"), 0);
    assert!(
        !restored.has_path(&destination).await,
        "tunnel table load should not restore active path before tunnel reappears"
    );

    let tunnel_synth =
        super::tunnels::synthesize_tunnel_packet(&tunnel_identity, restored_iface_hash);
    {
        let handler = restored.get_handler();
        let mut handler = handler.lock().await;
        super::tunnels::handle_tunnel_synthesize_packet(
            &tunnel_synth,
            &mut handler,
            restored_iface,
        );
    }

    assert!(
        restored.has_path(&destination).await,
        "tunnel reappearance should restore the persisted tunnel path"
    );
    assert!(restored.destination_identity(&destination).await.is_some());
}

#[tokio::test]
async fn tunnel_synthesis_does_not_hold_handler_while_waiting_for_iface_manager() {
    let identity = PrivateIdentity::new_from_rand(OsRng);
    let config = TransportConfig::new("test", &identity, true);
    let transport = Transport::new(config);
    let handler = transport.get_handler();
    let iface_manager = transport.iface_manager();
    let iface = {
        let mut manager = iface_manager.lock().await;
        *manager.new_channel(16).address()
    };

    let iface_guard = iface_manager.lock().await;
    let synthesis =
        tokio::spawn(async move { transport.synthesize_tunnel_on_interface(iface).await });
    tokio::task::yield_now().await;

    let handler_guard = timeout(Duration::from_millis(50), handler.lock())
        .await
        .expect("tunnel synthesis should not hold the handler while blocked on iface manager");
    drop(handler_guard);
    drop(iface_guard);

    assert!(
        synthesis.await.expect("synthesis task should not panic"),
        "registered interface should synthesize a tunnel packet"
    );
}

#[tokio::test]
async fn packet_receive_drops_iface_receiver_before_transport_processing() {
    let identity = PrivateIdentity::new_from_rand(OsRng);
    let config = TransportConfig::new("test", &identity, true);
    let transport = Transport::new(config);
    let handler = transport.get_handler();
    let iface_manager = transport.iface_manager();
    let channel = iface_manager.lock().await.new_channel(16);
    let receiver = iface_manager.lock().await.receiver();
    let mut received = transport.iface_rx();
    tokio::task::yield_now().await;

    let handler_guard = handler.lock().await;
    channel
        .rx_channel
        .send(RxMessage {
            address: *channel.address(),
            packet: Packet {
                data: PacketDataBuffer::new_from_slice(b"blocked"),
                ..Default::default()
            },
            source: crate::iface::IfaceSource::None,
        })
        .await
        .expect("rx channel should accept packet");

    timeout(Duration::from_millis(200), received.recv())
        .await
        .expect("packet task should receive and fan out packet")
        .expect("iface rx broadcast should remain open");

    let receiver_guard = timeout(Duration::from_millis(50), receiver.lock())
        .await
        .expect("packet processing must not hold iface receiver while waiting for handler");
    drop(receiver_guard);
    drop(handler_guard);
}

#[tokio::test]
async fn packet_receive_continues_while_announce_worker_is_stalled() {
    let identity = PrivateIdentity::new_from_rand(OsRng);
    let started = Arc::new(tokio::sync::Notify::new());
    let release = Arc::new(tokio::sync::Notify::new());
    let mut config = TransportConfig::new("test", &identity, true);
    config.set_announce_worker_backend(Arc::new(BlockingAnnounceBackend {
        started: started.clone(),
        release: release.clone(),
    }));
    let transport = Transport::new(config);
    let iface_manager = transport.iface_manager();
    let channel = iface_manager.lock().await.new_channel(16);
    tokio::task::yield_now().await;

    let remote_identity = PrivateIdentity::new_from_rand(OsRng);
    let mut remote_destination =
        SingleInputDestination::new(remote_identity, DestinationName::new("lxmf", "delivery"));
    let announce = remote_destination.announce(OsRng, Some(b"blocked announce")).expect("announce");
    channel
        .rx_channel
        .send(RxMessage {
            address: *channel.address(),
            packet: announce,
            source: crate::iface::IfaceSource::None,
        })
        .await
        .expect("rx channel should accept announce");

    timeout(Duration::from_millis(200), started.notified())
        .await
        .expect("announce worker should receive the first packet");

    let mut received = transport.iface_rx();
    let second_packet =
        Packet { data: PacketDataBuffer::new_from_slice(b"second packet"), ..Default::default() };
    channel
        .rx_channel
        .send(RxMessage {
            address: *channel.address(),
            packet: second_packet,
            source: crate::iface::IfaceSource::None,
        })
        .await
        .expect("rx channel should accept second packet");

    let received = timeout(Duration::from_millis(200), received.recv())
        .await
        .expect("packet task should keep receiving while announce validation is stalled")
        .expect("iface rx broadcast should remain open");

    assert_eq!(received.packet.data.as_slice(), b"second packet");
    release.notify_waiters();
}

#[tokio::test]
async fn unknown_announces_are_held_per_interface_and_released_by_lowest_hops() {
    let local_identity = PrivateIdentity::new_from_rand(OsRng);
    let config = TransportConfig::new("test", &local_identity, true);
    let transport = Transport::new(config);
    let handler = transport.get_handler();
    let mut announce_rx = transport.recv_announces().await;

    handler.lock().await.announce_limits = AnnounceLimits::with_rate_limit(AnnounceRateLimit {
        incoming_freq_samples: 3,
        max_held_announces: 8,
        new_time: Duration::from_secs(3600),
        burst_freq_new: 100.0,
        burst_freq: 100.0,
        burst_hold: Duration::from_millis(20),
        burst_penalty: Duration::from_millis(20),
        held_release_interval: Duration::from_millis(10),
    });

    let iface = AddressHash::new_from_rand(OsRng);

    let mut first_destination = SingleInputDestination::new(
        PrivateIdentity::new_from_rand(OsRng),
        DestinationName::new("lxmf", "delivery"),
    );
    let mut first_announce = first_destination.announce(OsRng, None).expect("announce");
    first_announce.header.hops = 4;
    handle_announce(&first_announce, handler.lock().await, iface, crate::iface::IfaceSource::None)
        .await;
    let first_event = timeout(Duration::from_millis(200), announce_rx.recv())
        .await
        .expect("first announce should emit")
        .expect("broadcast receive");
    assert_eq!(first_event.hops, 4);
    tokio::time::sleep(Duration::from_millis(1)).await;

    let mut higher_hop_destination = SingleInputDestination::new(
        PrivateIdentity::new_from_rand(OsRng),
        DestinationName::new("lxmf", "delivery"),
    );
    let mut higher_hop_announce = higher_hop_destination.announce(OsRng, None).expect("announce");
    higher_hop_announce.header.hops = 3;
    handle_announce(
        &higher_hop_announce,
        handler.lock().await,
        iface,
        crate::iface::IfaceSource::None,
    )
    .await;
    tokio::time::sleep(Duration::from_millis(1)).await;

    let mut lower_hop_destination = SingleInputDestination::new(
        PrivateIdentity::new_from_rand(OsRng),
        DestinationName::new("lxmf", "delivery"),
    );
    let mut lower_hop_announce = lower_hop_destination.announce(OsRng, None).expect("announce");
    lower_hop_announce.header.hops = 1;
    handle_announce(
        &lower_hop_announce,
        handler.lock().await,
        iface,
        crate::iface::IfaceSource::None,
    )
    .await;

    let mut immediate_hops = Vec::new();
    while let Ok(event) = announce_rx.try_recv() {
        immediate_hops.push(event.hops);
    }
    assert!(
        immediate_hops.iter().all(|hops| matches!(*hops, 1 | 3)),
        "unexpected immediate announce release sequence {immediate_hops:?}"
    );
    if let Some(hops) = immediate_hops.first().copied() {
        assert_eq!(hops, 3);
    }

    tokio::time::sleep(Duration::from_millis(80)).await;
    if immediate_hops.contains(&1) {
        release_held_announces(handler.lock().await).await;
        assert!(matches!(
            announce_rx.try_recv(),
            Err(tokio::sync::broadcast::error::TryRecvError::Empty)
        ));
    } else {
        let mut released_lowest = None;
        for _ in 0..4 {
            release_held_announces(handler.lock().await).await;
            if let Ok(event) = timeout(Duration::from_millis(120), announce_rx.recv()).await {
                released_lowest = Some(event.expect("broadcast receive"));
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        let released_lowest = released_lowest.expect("lowest-hop held announce should emit");
        assert_eq!(released_lowest.hops, 1);
    }

    tokio::time::sleep(Duration::from_millis(50)).await;
    release_held_announces(handler.lock().await).await;

    if immediate_hops.contains(&3) {
        assert!(matches!(
            announce_rx.try_recv(),
            Err(tokio::sync::broadcast::error::TryRecvError::Empty)
        ));
    } else {
        tokio::time::sleep(Duration::from_millis(25)).await;
        release_held_announces(handler.lock().await).await;
        let released_next = timeout(Duration::from_millis(200), announce_rx.recv())
            .await
            .expect("next held announce should emit")
            .expect("broadcast receive");
        assert_eq!(released_next.hops, 3);
    }
}

#[tokio::test]
async fn learned_announces_are_not_held_after_route_is_known() {
    let local_identity = PrivateIdentity::new_from_rand(OsRng);
    let config = TransportConfig::new("test", &local_identity, true);
    let transport = Transport::new(config);
    let handler = transport.get_handler();
    let mut announce_rx = transport.recv_announces().await;

    handler.lock().await.announce_limits = AnnounceLimits::with_rate_limit(AnnounceRateLimit {
        incoming_freq_samples: 3,
        max_held_announces: 8,
        new_time: Duration::from_secs(3600),
        burst_freq_new: 100.0,
        burst_freq: 100.0,
        burst_hold: Duration::from_millis(20),
        burst_penalty: Duration::from_millis(20),
        held_release_interval: Duration::from_millis(10),
    });

    let iface = AddressHash::new_from_rand(OsRng);
    let mut destination = SingleInputDestination::new(
        PrivateIdentity::new_from_rand(OsRng),
        DestinationName::new("lxmf", "delivery"),
    );
    let announce = destination.announce(OsRng, None).expect("announce");

    handle_announce(&announce, handler.lock().await, iface, crate::iface::IfaceSource::None).await;
    timeout(Duration::from_millis(200), announce_rx.recv())
        .await
        .expect("first announce should emit")
        .expect("broadcast receive");

    tokio::time::sleep(Duration::from_millis(5)).await;
    handle_announce(&announce, handler.lock().await, iface, crate::iface::IfaceSource::None).await;

    let repeated = timeout(Duration::from_millis(200), announce_rx.recv())
        .await
        .expect("known announce should bypass ingress hold")
        .expect("broadcast receive");
    assert_eq!(repeated.hops, announce.header.hops);
}

#[tokio::test]
async fn path_response_announces_are_not_held_by_rate_limits() {
    let local_identity = PrivateIdentity::new_from_rand(OsRng);
    let config = TransportConfig::new("test", &local_identity, true);
    let transport = Transport::new(config);
    let handler = transport.get_handler();
    let mut announce_rx = transport.recv_announces().await;

    handler.lock().await.announce_limits = AnnounceLimits::with_rate_limit(AnnounceRateLimit {
        incoming_freq_samples: 1,
        max_held_announces: 8,
        new_time: Duration::from_secs(3600),
        burst_freq_new: 0.0,
        burst_freq: 0.0,
        burst_hold: Duration::from_secs(60),
        burst_penalty: Duration::from_secs(60),
        held_release_interval: Duration::from_secs(60),
    });

    let iface = AddressHash::new_from_rand(OsRng);
    let mut destination = SingleInputDestination::new(
        PrivateIdentity::new_from_rand(OsRng),
        DestinationName::new("lxmf", "propagation"),
    );
    let mut announce = destination.announce(OsRng, None).expect("announce");
    announce.context = PacketContext::PathResponse;

    handle_announce(&announce, handler.lock().await, iface, crate::iface::IfaceSource::None).await;

    let received = timeout(Duration::from_millis(200), announce_rx.recv())
        .await
        .expect("path response announce should emit immediately")
        .expect("broadcast receive");
    assert_eq!(received.destination.lock().await.desc.address_hash, announce.destination);
}

#[tokio::test]
async fn path_request_skips_busy_local_destination_response() {
    let local_identity = PrivateIdentity::new_from_rand(OsRng);
    let mut transport = Transport::new(TransportConfig::new("test", &local_identity, true));
    let handler = transport.get_handler();

    let remote_identity = PrivateIdentity::new_from_rand(OsRng);
    let destination =
        transport.add_destination(remote_identity, DestinationName::new("lxmf", "delivery")).await;
    let destination_hash = destination.lock().await.desc.address_hash;
    let request = {
        let mut handler = handler.lock().await;
        handler.path_requests.generate(&destination_hash, None)
    };

    let _busy_guard = destination.lock().await;
    let response = timeout(
        Duration::from_millis(200),
        handle_path_request_unlocked(&request, handler, AddressHash::new_from_rand(OsRng)),
    )
    .await
    .expect("path request handling should not wait for a busy local destination");

    assert!(response.is_none());
}

#[tokio::test]
async fn send_packet_with_outcome_reports_missing_identity() {
    let identity = PrivateIdentity::new_from_rand(OsRng);
    let config = TransportConfig::new("test", &identity, true);
    let transport = Transport::new(config);

    let packet = Packet { destination: AddressHash::new_from_rand(OsRng), ..Default::default() };
    let outcome = transport.send_packet_with_outcome(packet).await;

    assert_eq!(outcome, SendPacketOutcome::DroppedMissingDestinationIdentity);
}

#[tokio::test]
async fn send_packet_with_outcome_reports_no_route() {
    let identity = PrivateIdentity::new_from_rand(OsRng);
    let config = TransportConfig::new("test", &identity, false);
    let transport = Transport::new(config);

    let packet = Packet {
        header: Header { packet_type: PacketType::Data, ..Default::default() },
        context: PacketContext::KeepAlive,
        data: PacketDataBuffer::new_from_slice(&[KEEP_ALIVE_REQUEST]),
        destination: AddressHash::new_from_rand(OsRng),
        ..Default::default()
    };
    let outcome = transport.send_packet_with_outcome(packet).await;

    assert_eq!(outcome, SendPacketOutcome::DroppedNoRoute);
}

#[tokio::test]
async fn send_packet_with_outcome_drops_announce_without_route() {
    let identity = PrivateIdentity::new_from_rand(OsRng);
    let config = TransportConfig::new("test", &identity, false);
    let transport = Transport::new(config);

    let packet = Packet {
        header: Header { packet_type: PacketType::Announce, ..Default::default() },
        destination: AddressHash::new_from_rand(OsRng),
        ..Default::default()
    };
    let outcome = transport.send_packet_with_outcome(packet).await;

    assert_eq!(outcome, SendPacketOutcome::DroppedNoRoute);
}

struct CountingReceiptHandler {
    count: Arc<AtomicUsize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TestTypedMessage {
    value: Vec<u8>,
}

impl TypedMessage for TestTypedMessage {
    const MSG_TYPE: u16 = 0x7777;

    fn encode(&self) -> Vec<u8> {
        self.value.clone()
    }

    fn decode(payload: &[u8]) -> Result<Self, crate::channel::ChannelError> {
        Ok(Self { value: payload.to_vec() })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ReservedTypedMessage;

impl TypedMessage for ReservedTypedMessage {
    const MSG_TYPE: u16 = SystemMessageTypes::StreamData as u16;

    fn encode(&self) -> Vec<u8> {
        Vec::new()
    }

    fn decode(_payload: &[u8]) -> Result<Self, crate::channel::ChannelError> {
        Ok(Self)
    }
}

impl ReceiptHandler for CountingReceiptHandler {
    fn on_receipt(&self, _receipt: &DeliveryReceipt) {
        self.count.fetch_add(1, Ordering::SeqCst);
    }
}

#[tokio::test]
async fn handle_inbound_for_test_rejects_forged_destination_proof() {
    let local_identity = PrivateIdentity::new_from_rand(OsRng);
    let config = TransportConfig::new("test", &local_identity, true);
    let mut transport = Transport::new(config);
    let handler = transport.get_handler();

    let remote_identity = PrivateIdentity::new_from_rand(OsRng);
    let mut remote_destination =
        SingleInputDestination::new(remote_identity, DestinationName::new("lxmf", "delivery"));
    let announce = remote_destination.announce(OsRng, None).expect("valid announce packet");
    handle_announce(
        &announce,
        handler.lock().await,
        AddressHash::new_from_rand(OsRng),
        crate::iface::IfaceSource::None,
    )
    .await;

    let count = Arc::new(AtomicUsize::new(0));
    transport.set_receipt_handler(Box::new(CountingReceiptHandler { count: count.clone() })).await;

    let packet_hash = [0x44u8; HASH_SIZE];
    let mut data = PacketDataBuffer::new();
    data.safe_write(&packet_hash);
    data.safe_write(&[0xAA; ed25519_dalek::SIGNATURE_LENGTH]);
    let packet = Packet {
        header: Header { packet_type: PacketType::Proof, ..Default::default() },
        destination: announce.destination,
        context: PacketContext::None,
        data,
        ..Default::default()
    };

    transport.handle_inbound_for_test(packet).await;

    assert_eq!(count.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn handle_inbound_for_test_accepts_valid_destination_proof() {
    let local_identity = PrivateIdentity::new_from_rand(OsRng);
    let config = TransportConfig::new("test", &local_identity, true);
    let mut transport = Transport::new(config);
    let handler = transport.get_handler();

    let remote_identity = PrivateIdentity::new_from_rand(OsRng);
    let mut remote_destination =
        SingleInputDestination::new(remote_identity, DestinationName::new("lxmf", "delivery"));
    let announce = remote_destination.announce(OsRng, None).expect("valid announce packet");
    handle_announce(
        &announce,
        handler.lock().await,
        AddressHash::new_from_rand(OsRng),
        crate::iface::IfaceSource::None,
    )
    .await;

    let count = Arc::new(AtomicUsize::new(0));
    transport.set_receipt_handler(Box::new(CountingReceiptHandler { count: count.clone() })).await;

    let packet_hash = [0x55u8; HASH_SIZE];
    let signature = remote_destination.identity.sign(&packet_hash).to_bytes();
    let mut data = PacketDataBuffer::new();
    data.safe_write(&packet_hash);
    data.safe_write(&signature);
    let packet = Packet {
        header: Header { packet_type: PacketType::Proof, ..Default::default() },
        destination: announce.destination,
        context: PacketContext::None,
        data,
        ..Default::default()
    };

    transport.handle_inbound_for_test(packet).await;

    assert_eq!(count.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn routed_link_request_proof_requires_matching_iface_and_signature() {
    let local_identity = PrivateIdentity::new_from_rand(OsRng);
    let mut config = TransportConfig::new("test", &local_identity, true);
    config.set_retransmit(true);
    let transport = Transport::new(config);
    let handler = transport.get_handler();

    let remote_identity = PrivateIdentity::new_from_rand(OsRng);
    let mut remote_destination =
        SingleInputDestination::new(remote_identity, DestinationName::new("lxmf", "delivery"));
    let announce = remote_destination.announce(OsRng, None).expect("valid announce packet");
    handle_announce(
        &announce,
        handler.lock().await,
        AddressHash::new_from_rand(OsRng),
        crate::iface::IfaceSource::None,
    )
    .await;

    let received_from = AddressHash::new_from_slice(&[1u8; 16]);
    let next_hop = AddressHash::new_from_slice(&[2u8; 16]);
    let next_hop_iface = AddressHash::new_from_slice(&[3u8; 16]);

    let (tx, _) = tokio::sync::broadcast::channel(4);
    let mut outbound_link =
        crate::destination::link::Link::new(remote_destination.desc, tx.clone());
    let request = outbound_link.request();
    handle_link_request_as_intermediate(
        received_from,
        next_hop,
        next_hop_iface,
        &request,
        handler.clone(),
        handler.lock().await,
    )
    .await;

    let mut inbound_link = crate::destination::link::Link::new_from_request(
        &request,
        remote_destination.sign_key().clone(),
        remote_destination.desc,
        tx,
    )
    .expect("link from request");

    let valid_proof = inbound_link.prove();
    handle_proof(valid_proof, handler.clone(), AddressHash::new_from_slice(&[9u8; 16])).await;
    {
        let guard = handler.lock().await;
        assert!(
            guard.link_table.original_destination(outbound_link.id()).is_none(),
            "proof from wrong interface must not validate"
        );
    }

    let mut bad_signature_proof = inbound_link.prove();
    bad_signature_proof.data.as_mut_slice()[0] ^= 0x01;
    handle_proof(bad_signature_proof, handler.clone(), next_hop_iface).await;
    {
        let guard = handler.lock().await;
        assert!(
            guard.link_table.original_destination(outbound_link.id()).is_none(),
            "invalid proof signature must not validate"
        );
    }

    let valid_proof = inbound_link.prove();
    handle_proof(valid_proof, handler.clone(), next_hop_iface).await;
    {
        let guard = handler.lock().await;
        assert_eq!(
            guard.link_table.original_destination(outbound_link.id()),
            Some(request.destination)
        );
    }
}

#[tokio::test]
async fn link_request_proof_forwarding_skips_busy_destination_validation() {
    let local_identity = PrivateIdentity::new_from_rand(OsRng);
    let mut config = TransportConfig::new("test", &local_identity, true);
    config.set_retransmit(true);
    let transport = Transport::new(config);
    let handler = transport.get_handler();

    let remote_identity = PrivateIdentity::new_from_rand(OsRng);
    let mut remote_destination =
        SingleInputDestination::new(remote_identity, DestinationName::new("lxmf", "delivery"));
    let announce = remote_destination.announce(OsRng, None).expect("valid announce packet");
    handle_announce(
        &announce,
        handler.lock().await,
        AddressHash::new_from_rand(OsRng),
        crate::iface::IfaceSource::None,
    )
    .await;

    let received_from = AddressHash::new_from_slice(&[1u8; 16]);
    let next_hop = AddressHash::new_from_slice(&[2u8; 16]);
    let next_hop_iface = AddressHash::new_from_slice(&[3u8; 16]);

    let (tx, _) = tokio::sync::broadcast::channel(4);
    let mut outbound_link = Link::new(remote_destination.desc, tx.clone());
    let request = outbound_link.request();
    handle_link_request_as_intermediate(
        received_from,
        next_hop,
        next_hop_iface,
        &request,
        handler.clone(),
        handler.lock().await,
    )
    .await;

    let destination = {
        let handler = handler.lock().await;
        handler
            .single_out_destinations
            .get(&request.destination)
            .cloned()
            .expect("learned destination")
    };
    let _destination_guard = destination.lock().await;

    let mut inbound = Link::new_from_request(
        &request,
        remote_destination.sign_key().clone(),
        remote_destination.desc,
        tx,
    )
    .expect("link request should parse");
    let proof = inbound.prove();

    timeout(Duration::from_millis(200), handle_proof(proof, handler.clone(), next_hop_iface))
        .await
        .expect("proof forwarding should not wait for a busy destination");

    let handler = handler.lock().await;
    assert!(
        handler.link_table.original_destination(outbound_link.id()).is_none(),
        "busy destination validation should skip proof forwarding"
    );
}

#[tokio::test]
async fn receipt_proof_validation_skips_busy_link() {
    let identity = PrivateIdentity::new_from_rand(OsRng);
    let transport = Transport::new(TransportConfig::new("test", &identity, true));
    let handler = transport.get_handler();

    let destination = crate::destination::DestinationDesc {
        identity: *identity.as_identity(),
        address_hash: identity.as_identity().address_hash,
        name: DestinationName::new("lxmf", "delivery"),
    };
    let (events, _) = tokio::sync::broadcast::channel(8);
    let link_id = AddressHash::new_from_rand(OsRng);
    let link = Arc::new(Mutex::new(Link::new(destination, events)));
    handler.lock().await.in_links.insert(link_id, link.clone());
    let _busy_guard = link.lock().await;

    let packet = Packet {
        header: Header {
            destination_type: DestinationType::Link,
            packet_type: PacketType::Proof,
            ..Default::default()
        },
        context: PacketContext::LinkProof,
        destination: link_id,
        data: PacketDataBuffer::new_from_slice(b"proof"),
        ..Default::default()
    };

    let result =
        timeout(Duration::from_millis(200), validated_receipt_hash_unlocked(handler, &packet))
            .await
            .expect("receipt proof validation should not wait for a busy link");

    assert!(result.is_none());
}

#[tokio::test]
async fn receipt_proof_validation_skips_busy_output_destination() {
    let identity = PrivateIdentity::new_from_rand(OsRng);
    let transport = Transport::new(TransportConfig::new("test", &identity, true));
    let handler = transport.get_handler();

    let remote_identity = PrivateIdentity::new_from_rand(OsRng);
    let destination = Arc::new(Mutex::new(SingleOutputDestination::new(
        *remote_identity.as_identity(),
        DestinationName::new("lxmf", "delivery"),
    )));
    let destination_hash = destination.lock().await.desc.address_hash;
    handler.lock().await.single_out_destinations.insert(destination_hash, destination.clone());
    let _busy_guard = destination.lock().await;

    let packet = Packet {
        header: Header {
            destination_type: DestinationType::Single,
            packet_type: PacketType::Proof,
            ..Default::default()
        },
        destination: destination_hash,
        data: PacketDataBuffer::new_from_slice(b"proof"),
        ..Default::default()
    };

    let result =
        timeout(Duration::from_millis(200), validated_receipt_hash_unlocked(handler, &packet))
            .await
            .expect("receipt proof validation should not wait for a busy output destination");

    assert!(result.is_none());
}

#[tokio::test]
async fn receipt_proof_validation_skips_busy_input_destination() {
    let identity = PrivateIdentity::new_from_rand(OsRng);
    let transport = Transport::new(TransportConfig::new("test", &identity, true));
    let handler = transport.get_handler();

    let remote_identity = PrivateIdentity::new_from_rand(OsRng);
    let destination = Arc::new(Mutex::new(SingleInputDestination::new(
        remote_identity,
        DestinationName::new("lxmf", "delivery"),
    )));
    let destination_hash = destination.lock().await.desc.address_hash;
    handler.lock().await.single_in_destinations.insert(destination_hash, destination.clone());
    let _busy_guard = destination.lock().await;

    let packet = Packet {
        header: Header {
            destination_type: DestinationType::Single,
            packet_type: PacketType::Proof,
            ..Default::default()
        },
        destination: destination_hash,
        data: PacketDataBuffer::new_from_slice(b"proof"),
        ..Default::default()
    };

    let result =
        timeout(Duration::from_millis(200), validated_receipt_hash_unlocked(handler, &packet))
            .await
            .expect("receipt proof validation should not wait for a busy input destination");

    assert!(result.is_none());
}

#[tokio::test]
async fn local_single_destination_decrypt_skips_busy_destination() {
    let _worker_permit_guard = WIRE_WORKER_PERMIT_TEST_LOCK.lock().await;
    let identity = PrivateIdentity::new_from_rand(OsRng);
    let destination = Arc::new(Mutex::new(SingleInputDestination::new(
        identity,
        DestinationName::new("lxmf", "delivery"),
    )));
    let destination_hash = destination.lock().await.desc.address_hash;
    let _busy_guard = destination.lock().await;
    let (received_tx, mut received_rx) = tokio::sync::broadcast::channel(8);

    let packet = Packet {
        header: Header {
            packet_type: PacketType::Data,
            destination_type: DestinationType::Single,
            ..Default::default()
        },
        destination: destination_hash,
        context: PacketContext::None,
        data: PacketDataBuffer::new_from_slice(b"ciphertext"),
        ..Default::default()
    };

    let handled = timeout(
        Duration::from_millis(200),
        handle_local_single_destination_data(
            &packet,
            destination.clone(),
            received_tx,
            "test",
            None,
        ),
    )
    .await
    .expect("single-destination decrypt should not wait for a busy destination");

    assert!(handled);
    assert!(
        received_rx.try_recv().is_err(),
        "busy destination decrypt should not emit received data"
    );
}

#[tokio::test]
async fn local_single_destination_decrypt_returns_when_workers_are_saturated() {
    let _worker_permit_guard = WIRE_WORKER_PERMIT_TEST_LOCK.lock().await;
    let identity = PrivateIdentity::new_from_rand(OsRng);
    let destination = Arc::new(Mutex::new(SingleInputDestination::new(
        identity,
        DestinationName::new("lxmf", "delivery"),
    )));
    let destination_hash = destination.lock().await.desc.address_hash;
    let (received_tx, mut received_rx) = tokio::sync::broadcast::channel(8);
    let permits = super::wire::single_destination_decrypt_permits();
    let _held_permits = (0..super::wire::MAX_SINGLE_DESTINATION_DECRYPT_WORKERS)
        .map(|_| permits.clone().try_acquire_owned().expect("permit available"))
        .collect::<Vec<_>>();

    let packet = Packet {
        header: Header {
            packet_type: PacketType::Data,
            destination_type: DestinationType::Single,
            ..Default::default()
        },
        destination: destination_hash,
        context: PacketContext::None,
        data: PacketDataBuffer::new_from_slice(b"ciphertext"),
        ..Default::default()
    };

    let handled = timeout(
        Duration::from_millis(50),
        handle_local_single_destination_data(&packet, destination, received_tx, "test", None),
    )
    .await
    .expect("saturated single-destination decrypt workers should not stall handling");

    assert!(handled);
    assert!(
        received_rx.try_recv().is_err(),
        "saturated destination decrypt should not emit received data"
    );
}

#[tokio::test]
async fn link_request_skips_busy_local_destination() {
    let local_identity = PrivateIdentity::new_from_rand(OsRng);
    let mut transport = Transport::new(TransportConfig::new("test", &local_identity, true));
    let handler = transport.get_handler();

    let remote_identity = PrivateIdentity::new_from_rand(OsRng);
    let destination =
        transport.add_destination(remote_identity, DestinationName::new("lxmf", "delivery")).await;
    let destination_hash = destination.lock().await.desc.address_hash;

    let signer = PrivateIdentity::new_from_rand(OsRng);
    let destination_desc = crate::destination::DestinationDesc {
        identity: *signer.as_identity(),
        address_hash: destination_hash,
        name: DestinationName::new("lxmf", "delivery"),
    };
    let (tx, _) = tokio::sync::broadcast::channel(4);
    let mut outbound = Link::new(destination_desc, tx);
    let request = outbound.request();

    let _busy_guard = destination.lock().await;
    timeout(
        Duration::from_millis(200),
        handle_link_request_unlocked(&request, AddressHash::new_from_rand(OsRng), handler.clone()),
    )
    .await
    .expect("link request handling should not wait for a busy local destination");

    assert!(handler.lock().await.in_links.is_empty());
}

#[test]
fn link_request_proof_starts_with_zero_hops() {
    let signer = PrivateIdentity::new_from_rand(OsRng);
    let identity = *signer.as_identity();
    let destination = crate::destination::DestinationDesc {
        identity,
        address_hash: identity.address_hash,
        name: DestinationName::new("lxmf", "delivery"),
    };
    let (tx, _) = tokio::sync::broadcast::channel(4);
    let mut outbound = Link::new(destination, tx.clone());
    let mut request = outbound.request();
    request.header.hops = 2;

    let mut inbound = Link::new_from_request(&request, signer.sign_key().clone(), destination, tx)
        .expect("link request should parse");
    let proof = inbound.prove();

    assert_eq!(proof.context, PacketContext::LinkRequestProof);
    assert_eq!(proof.header.hops, 0);
}

#[tokio::test]
async fn routed_link_request_proof_preserves_wire_shape_when_forwarded_backwards() {
    let remote_identity = PrivateIdentity::new_from_rand(OsRng);
    let remote_destination =
        SingleInputDestination::new(remote_identity, DestinationName::new("lxmf", "delivery"));

    let received_from = AddressHash::new_from_slice(&[1u8; 16]);
    let next_hop = AddressHash::new_from_slice(&[2u8; 16]);
    let next_hop_iface = AddressHash::new_from_slice(&[3u8; 16]);

    let mut link_table = LinkTable::new(Duration::from_secs(5), Duration::from_secs(30));
    let (tx, _) = tokio::sync::broadcast::channel(4);
    let mut outbound_link = Link::new(remote_destination.desc, tx.clone());
    let mut request = outbound_link.request();
    request.header.hops = 1;
    link_table.add(&request, request.destination, received_from, next_hop, next_hop_iface);

    let mut inbound = Link::new_from_request(
        &request,
        remote_destination.sign_key().clone(),
        remote_destination.desc,
        tx,
    )
    .expect("link from request");
    let proof = inbound.prove();
    let (forwarded, target) = link_table.handle_proof(&proof).expect("forwarded proof");

    assert_eq!(target, received_from);
    assert_eq!(forwarded.context, PacketContext::LinkRequestProof);
    assert_eq!(forwarded.header.header_type, HeaderType::Type1);
    assert_eq!(forwarded.transport, None);
    assert_eq!(forwarded.destination, proof.destination);
    assert_eq!(forwarded.header.hops, proof.header.hops);
}

#[tokio::test]
async fn transport_register_channel_handler_dispatches_inbound_channel_message() {
    let local_identity = PrivateIdentity::new_from_rand(OsRng);
    let config = TransportConfig::new("test", &local_identity, true);
    let transport = Transport::new(config);
    let handler = transport.get_handler();

    let signer = PrivateIdentity::new_from_rand(OsRng);
    let identity = *signer.as_identity();
    let destination = crate::destination::DestinationDesc {
        identity,
        address_hash: identity.address_hash,
        name: DestinationName::new("lxmf", "delivery"),
    };
    let (tx, _) = tokio::sync::broadcast::channel(8);
    let mut outbound = Link::new(destination, tx.clone());
    let request = outbound.request();
    let mut inbound = Link::new_from_request(&request, signer.sign_key().clone(), destination, tx)
        .expect("link request should parse");
    let iface = AddressHash::new_from_rand(OsRng);
    assert!(matches!(
        outbound.handle_packet(&inbound.prove(), iface),
        crate::destination::link::LinkHandleResult::Activated
    ));

    let link_id = *outbound.id();
    handler.lock().await.out_links.insert(destination.address_hash, Arc::new(Mutex::new(outbound)));

    let seen = Arc::new(StdMutex::new(Vec::new()));
    let seen_clone = seen.clone();
    transport
        .register_channel_handler(&link_id, 0x4444, move |envelope| {
            seen_clone.lock().expect("lock").push(envelope);
            true
        })
        .await
        .expect("register handler");

    let (_sequence, packet) = inbound
        .send_channel_message(0x4444, b"transport-channel".to_vec())
        .expect("channel message");
    handle_data(&packet, iface, handler.clone(), handler.lock().await).await;

    let seen = seen.lock().expect("lock");
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].msg_type, 0x4444);
    assert_eq!(seen[0].payload, b"transport-channel");
}

#[tokio::test]
async fn transport_channel_message_state_tracks_delivery() {
    let local_identity = PrivateIdentity::new_from_rand(OsRng);
    let config = TransportConfig::new("test", &local_identity, true);
    let transport = Transport::new(config);
    let handler = transport.get_handler();

    let signer = PrivateIdentity::new_from_rand(OsRng);
    let identity = *signer.as_identity();
    let destination = crate::destination::DestinationDesc {
        identity,
        address_hash: identity.address_hash,
        name: DestinationName::new("lxmf", "delivery"),
    };
    let (tx, _) = tokio::sync::broadcast::channel(8);
    let mut outbound = Link::new(destination, tx.clone());
    let request = outbound.request();
    let mut inbound = Link::new_from_request(&request, signer.sign_key().clone(), destination, tx)
        .expect("link request should parse");
    let iface = AddressHash::new_from_rand(OsRng);
    assert!(matches!(
        outbound.handle_packet(&inbound.prove(), iface),
        crate::destination::link::LinkHandleResult::Activated
    ));

    let link_id = *outbound.id();
    let outbound = Arc::new(Mutex::new(outbound));
    handler.lock().await.out_links.insert(destination.address_hash, outbound.clone());
    inbound.register_channel_handler(0x55AA, |_| true);

    let (sequence, packet) = {
        let mut outbound = outbound.lock().await;
        outbound.send_channel_message(0x55AA, b"tracked".to_vec()).expect("channel message")
    };
    assert_eq!(
        transport.channel_message_state(&link_id, sequence).await.expect("state"),
        ChannelMessageState::Sent
    );

    let proof = match inbound.handle_packet(&packet, iface) {
        crate::destination::link::LinkHandleResult::Proof(proof) => proof,
        _ => panic!("channel packet should generate proof"),
    };
    {
        let mut outbound = outbound.lock().await;
        assert!(matches!(
            outbound.handle_packet(&proof, iface),
            crate::destination::link::LinkHandleResult::None
        ));
    }
    assert_eq!(
        transport.channel_message_state(&link_id, sequence).await.expect("state"),
        ChannelMessageState::Delivered
    );
}

#[tokio::test]
async fn transport_channel_handle_reports_missing_link() {
    let local_identity = PrivateIdentity::new_from_rand(OsRng);
    let config = TransportConfig::new("test", &local_identity, true);
    let transport = Transport::new(config);

    let link_id = AddressHash::new_from_rand(OsRng);
    let channel = transport.channel(link_id);

    assert_eq!(channel.link_id(), link_id);
    assert!(matches!(
        channel.message_state(0).await,
        Err(crate::channel::ChannelError::LinkNotReady)
    ));
}

#[tokio::test]
async fn transport_channel_handle_supports_typed_messages() {
    let local_identity = PrivateIdentity::new_from_rand(OsRng);
    let config = TransportConfig::new("test", &local_identity, true);
    let transport = Transport::new(config);
    let handler = transport.get_handler();

    let signer = PrivateIdentity::new_from_rand(OsRng);
    let identity = *signer.as_identity();
    let destination = crate::destination::DestinationDesc {
        identity,
        address_hash: identity.address_hash,
        name: DestinationName::new("lxmf", "delivery"),
    };
    let (tx, _) = tokio::sync::broadcast::channel(8);
    let mut outbound = Link::new(destination, tx.clone());
    let request = outbound.request();
    let mut inbound = Link::new_from_request(&request, signer.sign_key().clone(), destination, tx)
        .expect("link request should parse");
    let iface = AddressHash::new_from_rand(OsRng);
    assert!(matches!(
        outbound.handle_packet(&inbound.prove(), iface),
        crate::destination::link::LinkHandleResult::Activated
    ));

    let link_id = *outbound.id();
    handler.lock().await.out_links.insert(destination.address_hash, Arc::new(Mutex::new(outbound)));
    let channel = transport.channel(link_id);

    let seen = Arc::new(StdMutex::new(Vec::new()));
    let seen_clone = seen.clone();
    channel
        .register_typed_handler::<TestTypedMessage, _>(move |message| {
            seen_clone.lock().expect("lock").push(message);
            true
        })
        .await
        .expect("typed handler");

    let message = TestTypedMessage { value: b"typed-payload".to_vec() };
    let (_sequence, packet) = inbound
        .send_channel_message(TestTypedMessage::MSG_TYPE, message.encode())
        .expect("typed channel packet");
    handle_data(&packet, iface, handler.clone(), handler.lock().await).await;

    let seen = seen.lock().expect("lock");
    assert_eq!(seen.as_slice(), &[message]);
}

#[tokio::test]
async fn transport_channel_handle_can_remove_handlers() {
    let local_identity = PrivateIdentity::new_from_rand(OsRng);
    let config = TransportConfig::new("test", &local_identity, true);
    let transport = Transport::new(config);
    let handler = transport.get_handler();

    let signer = PrivateIdentity::new_from_rand(OsRng);
    let identity = *signer.as_identity();
    let destination = crate::destination::DestinationDesc {
        identity,
        address_hash: identity.address_hash,
        name: DestinationName::new("lxmf", "delivery"),
    };
    let (tx, _) = tokio::sync::broadcast::channel(8);
    let mut outbound = Link::new(destination, tx.clone());
    let request = outbound.request();
    let mut inbound = Link::new_from_request(&request, signer.sign_key().clone(), destination, tx)
        .expect("link request should parse");
    let iface = AddressHash::new_from_rand(OsRng);
    assert!(matches!(
        outbound.handle_packet(&inbound.prove(), iface),
        crate::destination::link::LinkHandleResult::Activated
    ));

    let link_id = *outbound.id();
    handler.lock().await.out_links.insert(destination.address_hash, Arc::new(Mutex::new(outbound)));
    let channel = transport.channel(link_id);

    let seen = Arc::new(StdMutex::new(Vec::new()));
    let seen_clone = seen.clone();
    let handler_id = channel
        .register_handler(0x7777, move |envelope| {
            seen_clone.lock().expect("lock").push(envelope);
            true
        })
        .await
        .expect("register handler");
    assert!(channel.remove_handler(handler_id).await.expect("remove handler"));
    assert!(!channel.remove_handler(handler_id).await.expect("remove handler twice"));

    let (_sequence, packet) =
        inbound.send_channel_message(0x7777, b"removed".to_vec()).expect("channel message");
    handle_data(&packet, iface, handler.clone(), handler.lock().await).await;

    assert!(seen.lock().expect("lock").is_empty());
}

#[tokio::test]
async fn transport_channel_handle_rejects_reserved_typed_messages_by_default() {
    let local_identity = PrivateIdentity::new_from_rand(OsRng);
    let config = TransportConfig::new("test", &local_identity, true);
    let transport = Transport::new(config);

    let link_id = AddressHash::new_from_rand(OsRng);
    let channel = transport.channel(link_id);

    assert!(matches!(
        channel.register_typed_handler::<ReservedTypedMessage, _>(|_message| true).await,
        Err(ChannelError::InvalidMessageType)
    ));
}

#[tokio::test]
async fn transport_channel_handle_can_open_channel_without_handlers() {
    let local_identity = PrivateIdentity::new_from_rand(OsRng);
    let config = TransportConfig::new("test", &local_identity, true);
    let transport = Transport::new(config);
    let handler = transport.get_handler();

    let signer = PrivateIdentity::new_from_rand(OsRng);
    let identity = *signer.as_identity();
    let destination = crate::destination::DestinationDesc {
        identity,
        address_hash: identity.address_hash,
        name: DestinationName::new("lxmf", "delivery"),
    };
    let (tx, _) = tokio::sync::broadcast::channel(8);
    let mut outbound = Link::new(destination, tx.clone());
    let request = outbound.request();
    let mut inbound = Link::new_from_request(&request, signer.sign_key().clone(), destination, tx)
        .expect("link request should parse");
    let iface = AddressHash::new_from_rand(OsRng);
    assert!(matches!(
        outbound.handle_packet(&inbound.prove(), iface),
        crate::destination::link::LinkHandleResult::Activated
    ));

    let link_id = *outbound.id();
    let outbound = Arc::new(Mutex::new(outbound));
    handler.lock().await.out_links.insert(destination.address_hash, outbound.clone());
    let channel = transport.channel(link_id);
    channel.open().await.expect("open channel");

    let (_sequence, packet) =
        inbound.send_channel_message(0xEEEE, b"open-no-handler".to_vec()).expect("channel message");
    let result = outbound.lock().await.handle_packet(&packet, iface);
    assert!(matches!(result, crate::destination::link::LinkHandleResult::Proof(_)));
}

#[tokio::test]
async fn send_resource_returns_error_when_advertisement_dispatch_drops() {
    let _resource_prepare_guard = RESOURCE_PREPARE_TEST_LOCK.lock().await;
    let local_identity = PrivateIdentity::new_from_rand(OsRng);
    let config = TransportConfig::new("test", &local_identity, true);
    let transport = Transport::new(config);
    let handler = transport.get_handler();

    let signer = PrivateIdentity::new_from_rand(OsRng);
    let identity = *signer.as_identity();
    let destination = crate::destination::DestinationDesc {
        identity,
        address_hash: identity.address_hash,
        name: DestinationName::new("lxmf", "delivery"),
    };
    let (tx, _) = tokio::sync::broadcast::channel(8);
    let mut outbound = Link::new(destination, tx.clone());
    let request = outbound.request();
    let mut inbound = Link::new_from_request(&request, signer.sign_key().clone(), destination, tx)
        .expect("link request should parse");
    let iface = AddressHash::new_from_rand(OsRng);
    assert!(matches!(
        outbound.handle_packet(&inbound.prove(), iface),
        crate::destination::link::LinkHandleResult::Activated
    ));

    let link_id = *outbound.id();
    handler.lock().await.out_links.insert(destination.address_hash, Arc::new(Mutex::new(outbound)));

    let result = transport.send_resource(&link_id, b"resource".to_vec(), None).await;
    assert!(matches!(result, Err(RnsError::ConnectionError)));

    let resource_manager = { handler.lock().await.resource_lane.manager_handle() };
    assert!(resource_manager.lock().await.has_no_outbound_state());
}

#[tokio::test]
async fn send_resource_skips_busy_link_preparation() {
    let _resource_prepare_guard = RESOURCE_PREPARE_TEST_LOCK.lock().await;
    let local_identity = PrivateIdentity::new_from_rand(OsRng);
    let config = TransportConfig::new("test", &local_identity, true);
    let transport = Transport::new(config);
    let handler = transport.get_handler();

    let signer = PrivateIdentity::new_from_rand(OsRng);
    let identity = *signer.as_identity();
    let destination = crate::destination::DestinationDesc {
        identity,
        address_hash: identity.address_hash,
        name: DestinationName::new("lxmf", "delivery"),
    };
    let (tx, _) = tokio::sync::broadcast::channel(8);
    let mut outbound = Link::new(destination, tx.clone());
    let request = outbound.request();
    let mut inbound = Link::new_from_request(&request, signer.sign_key().clone(), destination, tx)
        .expect("link request should parse");
    let iface = AddressHash::new_from_rand(OsRng);
    assert!(matches!(
        outbound.handle_packet(&inbound.prove(), iface),
        crate::destination::link::LinkHandleResult::Activated
    ));

    let link_id = *outbound.id();
    let outbound = Arc::new(Mutex::new(outbound));
    handler.lock().await.in_links.insert(link_id, outbound.clone());
    let _busy_guard = outbound.lock().await;

    let result = timeout(
        Duration::from_millis(200),
        transport.send_resource(&link_id, b"resource".to_vec(), None),
    )
    .await
    .expect("resource send should not wait for a busy link during preparation");
    assert!(matches!(result, Err(RnsError::ConnectionError)));

    let resource_manager = { handler.lock().await.resource_lane.manager_handle() };
    assert!(resource_manager.lock().await.has_no_outbound_state());
}

#[tokio::test]
async fn send_resource_returns_when_prepare_workers_are_saturated() {
    let _resource_prepare_guard = RESOURCE_PREPARE_TEST_LOCK.lock().await;
    let local_identity = PrivateIdentity::new_from_rand(OsRng);
    let config = TransportConfig::new("test", &local_identity, true);
    let transport = Transport::new(config);
    let handler = transport.get_handler();

    let signer = PrivateIdentity::new_from_rand(OsRng);
    let identity = *signer.as_identity();
    let destination = crate::destination::DestinationDesc {
        identity,
        address_hash: identity.address_hash,
        name: DestinationName::new("lxmf", "delivery"),
    };
    let (tx, _) = tokio::sync::broadcast::channel(8);
    let mut outbound = Link::new(destination, tx.clone());
    let request = outbound.request();
    let mut inbound = Link::new_from_request(&request, signer.sign_key().clone(), destination, tx)
        .expect("link request should parse");
    let iface = AddressHash::new_from_rand(OsRng);
    assert!(matches!(
        outbound.handle_packet(&inbound.prove(), iface),
        crate::destination::link::LinkHandleResult::Activated
    ));

    let link_id = *outbound.id();
    handler.lock().await.in_links.insert(link_id, Arc::new(Mutex::new(outbound)));

    let permits = super::links::resource_prepare_permits();
    let _held_permits = (0..super::links::MAX_RESOURCE_PREPARE_WORKERS)
        .map(|_| permits.clone().try_acquire_owned().expect("permit available"))
        .collect::<Vec<_>>();

    let result = timeout(
        Duration::from_millis(50),
        transport.send_resource(&link_id, b"resource".to_vec(), None),
    )
    .await
    .expect("saturated resource prepare workers should not stall send_resource");
    assert!(matches!(result, Err(RnsError::ConnectionError)));

    let resource_manager = { handler.lock().await.resource_lane.manager_handle() };
    assert!(resource_manager.lock().await.has_no_outbound_state());
}

fn decrypt_resource_packet_for_test(link: &Link, packet: &Packet) -> Packet {
    let mut plain_packet = *packet;
    let mut buffer = PacketDataBuffer::new();
    let plain_len = {
        let plaintext = link
            .decrypt(packet.data.as_slice(), buffer.accuire_buf_max())
            .expect("decrypt should succeed");
        plaintext.len()
    };
    buffer.resize(plain_len);
    plain_packet.data = buffer;
    plain_packet
}

fn active_resource_completion_link_for_test() -> Arc<Mutex<Link>> {
    let signer = PrivateIdentity::new_from_rand(OsRng);
    let identity = *signer.as_identity();
    let destination = crate::destination::DestinationDesc {
        identity,
        address_hash: identity.address_hash,
        name: DestinationName::new("lxmf", "delivery"),
    };
    let (tx, _) = tokio::sync::broadcast::channel(8);
    let mut sender_link = Link::new(destination, tx.clone());
    let request = sender_link.request();
    let mut receiver_link =
        Link::new_from_request(&request, signer.sign_key().clone(), destination, tx)
            .expect("link request should parse");
    let iface = AddressHash::new_from_rand(OsRng);
    assert!(matches!(
        sender_link.handle_packet(&receiver_link.prove(), iface),
        crate::destination::link::LinkHandleResult::Activated
    ));
    Arc::new(Mutex::new(receiver_link))
}

#[tokio::test]
async fn inbound_resource_completion_runs_through_transport_worker() {
    let local_identity = PrivateIdentity::new_from_rand(OsRng);
    let config = TransportConfig::new("test", &local_identity, true);
    let transport = Transport::new(config);
    let handler = transport.get_handler();
    let mut resource_rx = transport.resource_events();

    let signer = PrivateIdentity::new_from_rand(OsRng);
    let identity = *signer.as_identity();
    let destination = crate::destination::DestinationDesc {
        identity,
        address_hash: identity.address_hash,
        name: DestinationName::new("lxmf", "delivery"),
    };
    let (tx, _) = tokio::sync::broadcast::channel(8);
    let mut sender_link = Link::new(destination, tx.clone());
    let request = sender_link.request();
    let mut receiver_link =
        Link::new_from_request(&request, signer.sign_key().clone(), destination, tx)
            .expect("link request should parse");
    let iface = AddressHash::new_from_rand(OsRng);
    assert!(matches!(
        sender_link.handle_packet(&receiver_link.prove(), iface),
        crate::destination::link::LinkHandleResult::Activated
    ));
    let link_id = *sender_link.id();
    let receiver_link = Arc::new(Mutex::new(receiver_link));
    handler.lock().await.in_links.insert(link_id, receiver_link.clone());

    let mut sender_manager = ResourceManager::new();
    let payload = vec![0x5a; crate::packet::PACKET_MDU * 2 + 17];
    let (resource_hash, advertisement) =
        sender_manager.start_send(&sender_link, payload.clone(), None).expect("start send");
    sender_manager.confirm_outbound_dispatch(resource_hash, true);

    let plain_advertisement = {
        let receiver = receiver_link.lock().await;
        decrypt_resource_packet_for_test(&receiver, &advertisement)
    };
    let request_packet = {
        let mut receiver = receiver_link.lock().await;
        let resource_manager = { handler.lock().await.resource_lane.manager_handle() };
        let mut resource_manager = resource_manager.lock().await;
        let mut responses = Vec::new();
        resource_manager.handle_packet_into(&plain_advertisement, &mut receiver, &mut responses);
        responses.pop().expect("resource request")
    };
    let plain_request = decrypt_resource_packet_for_test(&sender_link, &request_packet);
    let resource_parts = {
        let mut responses = Vec::new();
        sender_manager.handle_packet_into(&plain_request, &mut sender_link, &mut responses);
        responses
    };

    for packet in resource_parts {
        assert!(handle_link_resource_data(packet, handler.clone()).await);
    }

    for _ in 0..4 {
        let event = timeout(Duration::from_secs(1), resource_rx.recv())
            .await
            .expect("resource completion event")
            .expect("broadcast receive");
        if let ResourceEventKind::Complete(complete) = event.kind {
            assert_eq!(event.hash, resource_hash);
            assert_eq!(complete.data, payload);
            return;
        }
    }
    panic!("expected resource completion event");
}

#[tokio::test]
async fn resource_completion_uses_configured_worker_backend() {
    let link = active_resource_completion_link_for_test();
    let link_id = *link.lock().await.id();
    let completion_job = ResourceCompletionJob::unencrypted_for_test(link_id, b"local payload");
    let calls = Arc::new(AtomicUsize::new(0));
    let backend = Arc::new(ResourceCompleteBackend { calls: calls.clone() });

    let completion = complete_link_resource_on_worker(completion_job, link, Some(backend))
        .await
        .expect("resource completion");

    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(completion.payload.data, b"remote completed");
}

#[tokio::test]
async fn resource_completion_falls_back_when_worker_backend_fails() {
    let _worker_permit_guard = WIRE_WORKER_PERMIT_TEST_LOCK.lock().await;
    let link = active_resource_completion_link_for_test();
    let link_id = *link.lock().await.id();
    let completion_job = ResourceCompletionJob::unencrypted_for_test(link_id, b"local completed");
    let calls = Arc::new(AtomicUsize::new(0));
    let backend = Arc::new(FailingAnnounceBackend { calls: calls.clone() });

    let completion = complete_link_resource_on_worker(completion_job, link, Some(backend))
        .await
        .expect("resource completion fallback");

    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(completion.payload.data, b"local completed");
}

#[tokio::test]
async fn resource_completion_worker_skips_busy_link() {
    let _worker_permit_guard = WIRE_WORKER_PERMIT_TEST_LOCK.lock().await;
    let identity = PrivateIdentity::new_from_rand(OsRng);
    let destination = crate::destination::DestinationDesc {
        identity: *identity.as_identity(),
        address_hash: identity.as_identity().address_hash,
        name: DestinationName::new("lxmf", "delivery"),
    };
    let (tx, _) = tokio::sync::broadcast::channel(8);
    let link = Arc::new(Mutex::new(Link::new(destination, tx)));
    let link_id = *link.lock().await.id();
    let completion_job = ResourceCompletionJob::unencrypted_for_test(link_id, b"complete payload");
    let _busy_guard = link.lock().await;

    let result = timeout(
        Duration::from_millis(200),
        complete_link_resource_on_worker(completion_job, link.clone(), None),
    )
    .await
    .expect("resource completion should not wait for a busy link");

    assert!(matches!(result, Err(RnsError::ConnectionError)));
    assert_eq!(link_id, *_busy_guard.id());
}

#[tokio::test]
async fn resource_completion_returns_when_workers_are_saturated() {
    let _worker_permit_guard = WIRE_WORKER_PERMIT_TEST_LOCK.lock().await;
    let link = active_resource_completion_link_for_test();
    let link_id = *link.lock().await.id();
    let completion_job = ResourceCompletionJob::unencrypted_for_test(link_id, b"complete payload");
    let permits = super::wire::resource_completion_permits();
    let _held_permits = (0..super::wire::MAX_RESOURCE_COMPLETION_WORKERS)
        .map(|_| permits.clone().try_acquire_owned().expect("permit available"))
        .collect::<Vec<_>>();

    let result = timeout(
        Duration::from_millis(50),
        complete_link_resource_on_worker(completion_job, link, None),
    )
    .await
    .expect("saturated resource completion workers should not stall completion");

    assert!(matches!(result, Err(RnsError::ConnectionError)));
}

#[tokio::test]
async fn link_resource_decrypt_skips_busy_link() {
    let _worker_permit_guard = WIRE_WORKER_PERMIT_TEST_LOCK.lock().await;
    let identity = PrivateIdentity::new_from_rand(OsRng);
    let config = TransportConfig::new("test", &identity, true);
    let transport = Transport::new(config);
    let handler = transport.get_handler();

    let destination = crate::destination::DestinationDesc {
        identity: *identity.as_identity(),
        address_hash: identity.as_identity().address_hash,
        name: DestinationName::new("lxmf", "delivery"),
    };
    let (tx, _) = tokio::sync::broadcast::channel(8);
    let link = Arc::new(Mutex::new(Link::new(destination, tx)));
    let link_id = *link.lock().await.id();
    handler.lock().await.in_links.insert(link_id, link.clone());
    let _busy_guard = link.lock().await;

    let packet = Packet {
        header: Header {
            packet_type: PacketType::Data,
            destination_type: DestinationType::Link,
            ..Default::default()
        },
        context: PacketContext::ResourceRequest,
        destination: link_id,
        data: PacketDataBuffer::new_from_slice(b"ciphertext"),
        ..Default::default()
    };

    let handled =
        timeout(Duration::from_millis(200), handle_link_resource_data(packet, handler.clone()))
            .await
            .expect("resource decrypt should not wait for a busy link");

    assert!(handled);
    let resource_manager = { handler.lock().await.resource_lane.manager_handle() };
    assert!(resource_manager.lock().await.has_no_receiver_state());
}

#[tokio::test]
async fn link_resource_decrypt_returns_when_workers_are_saturated() {
    let _worker_permit_guard = WIRE_WORKER_PERMIT_TEST_LOCK.lock().await;
    let identity = PrivateIdentity::new_from_rand(OsRng);
    let config = TransportConfig::new("test", &identity, true);
    let transport = Transport::new(config);
    let handler = transport.get_handler();

    let destination = crate::destination::DestinationDesc {
        identity: *identity.as_identity(),
        address_hash: identity.as_identity().address_hash,
        name: DestinationName::new("lxmf", "delivery"),
    };
    let (tx, _) = tokio::sync::broadcast::channel(8);
    let link = Arc::new(Mutex::new(Link::new(destination, tx)));
    let link_id = *link.lock().await.id();
    handler.lock().await.in_links.insert(link_id, link);
    let permits = super::wire::resource_decrypt_permits();
    let _held_permits = (0..super::wire::MAX_RESOURCE_DECRYPT_WORKERS)
        .map(|_| permits.clone().try_acquire_owned().expect("permit available"))
        .collect::<Vec<_>>();

    let packet = Packet {
        header: Header {
            packet_type: PacketType::Data,
            destination_type: DestinationType::Link,
            ..Default::default()
        },
        context: PacketContext::ResourceRequest,
        destination: link_id,
        data: PacketDataBuffer::new_from_slice(b"ciphertext"),
        ..Default::default()
    };

    let handled =
        timeout(Duration::from_millis(50), handle_link_resource_data(packet, handler.clone()))
            .await
            .expect("saturated resource decrypt workers should not stall packet handling");

    assert!(handled);
    let resource_manager = { handler.lock().await.resource_lane.manager_handle() };
    assert!(resource_manager.lock().await.has_no_receiver_state());
}

#[tokio::test]
async fn proof_activation_skips_busy_outbound_links_without_blocking_ready_links() {
    let signer = PrivateIdentity::new_from_rand(OsRng);
    let identity = *signer.as_identity();
    let destination = crate::destination::DestinationDesc {
        identity,
        address_hash: identity.address_hash,
        name: DestinationName::new("lxmf", "delivery"),
    };
    let (tx, _) = tokio::sync::broadcast::channel(8);
    let busy_link = Arc::new(Mutex::new(Link::new(destination, tx.clone())));
    let busy_guard = busy_link.lock().await;

    let mut ready_link = Link::new(destination, tx.clone());
    let request = ready_link.request();
    let mut inbound_link =
        Link::new_from_request(&request, signer.sign_key().clone(), destination, tx)
            .expect("link request should parse");
    let proof = inbound_link.prove();
    let iface = AddressHash::new_from_rand(OsRng);
    let ready_link = Arc::new(Mutex::new(ready_link));

    let rtt_messages =
        collect_ready_link_activation_rtts(&proof, iface, vec![busy_link.clone(), ready_link]);

    assert_eq!(rtt_messages.len(), 1);
    assert!(matches!(rtt_messages[0].tx_type, TxMessageType::Direct(target) if target == iface));
    assert_eq!(rtt_messages[0].packet.context, PacketContext::LinkRTT);
    drop(busy_guard);
}

#[tokio::test]
async fn link_data_proof_generation_skips_busy_outbound_links_without_blocking_ready_links() {
    let signer = PrivateIdentity::new_from_rand(OsRng);
    let identity = *signer.as_identity();
    let destination = crate::destination::DestinationDesc {
        identity,
        address_hash: identity.address_hash,
        name: DestinationName::new("lxmf", "delivery"),
    };
    let (tx, _) = tokio::sync::broadcast::channel(8);
    let busy_link = Arc::new(Mutex::new(Link::new(destination, tx.clone())));
    let busy_guard = busy_link.lock().await;

    let mut ready_link = Link::new(destination, tx.clone());
    let request = ready_link.request();
    let mut inbound_link =
        Link::new_from_request(&request, signer.sign_key().clone(), destination, tx)
            .expect("link request should parse");
    let iface = AddressHash::new_from_rand(OsRng);
    assert!(matches!(
        ready_link.handle_packet(&inbound_link.prove(), iface),
        crate::destination::link::LinkHandleResult::Activated
    ));
    let packet = inbound_link.data_packet(b"ready").expect("data packet");
    let ready_link = Arc::new(Mutex::new(ready_link));

    let proof_packets =
        collect_ready_outbound_link_proofs(&packet, iface, vec![busy_link.clone(), ready_link]);

    assert_eq!(proof_packets.len(), 1);
    assert_eq!(proof_packets[0].context, PacketContext::LinkProof);
    drop(busy_guard);
}

#[tokio::test]
async fn outbound_link_destination_lookup_skips_busy_nonmatching_candidates() {
    let signer = PrivateIdentity::new_from_rand(OsRng);
    let identity = *signer.as_identity();
    let destination = crate::destination::DestinationDesc {
        identity,
        address_hash: identity.address_hash,
        name: DestinationName::new("lxmf", "delivery"),
    };
    let (tx, _) = tokio::sync::broadcast::channel(8);

    let mut busy = Link::new(destination, tx.clone());
    let _busy_request = busy.request();
    let busy = Arc::new(Mutex::new(busy));
    let busy_guard = busy.lock().await;

    let mut ready = Link::new(destination, tx);
    let _ready_request = ready.request();
    let ready_id = *ready.id();
    let ready = Arc::new(Mutex::new(ready));

    let found = find_ready_outbound_link_candidate(vec![busy.clone(), ready.clone()], &ready_id)
        .expect("ready candidate should still be found");

    assert!(Arc::ptr_eq(&found, &ready));
    drop(busy_guard);
}

#[tokio::test]
async fn resource_lane_waiting_for_link_context_does_not_block_other_commands() {
    let identity = PrivateIdentity::new_from_rand(OsRng);
    let config = TransportConfig::new("test", &identity, true);
    let transport = Transport::new(config);
    let handler = transport.get_handler();
    let resource_lane = { handler.lock().await.resource_lane.clone() };

    let destination = crate::destination::DestinationDesc {
        identity: *identity.as_identity(),
        address_hash: identity.as_identity().address_hash,
        name: DestinationName::new("lxmf", "delivery"),
    };
    let (tx, _) = tokio::sync::broadcast::channel(8);
    let link = Arc::new(Mutex::new(Link::new(destination, tx)));
    let link_guard = link.lock().await;

    let blocked_lane = resource_lane.clone();
    let blocked_link = link.clone();
    let packet = Packet {
        context: PacketContext::ResourceProof,
        destination: *link_guard.id(),
        ..Default::default()
    };
    let blocked = tokio::spawn(async move {
        let _ = blocked_lane.handle_link_packet(packet, blocked_link).await;
    });

    tokio::task::yield_now().await;
    timeout(
        Duration::from_millis(200),
        resource_lane.remove_link_state(vec![AddressHash::new_from_slice(&[0x7a; 32])]),
    )
    .await
    .expect("resource lane command should not wait behind blocked link context");

    drop(link_guard);
    timeout(Duration::from_millis(200), blocked)
        .await
        .expect("blocked resource packet should finish after link is released")
        .expect("blocked task should not panic");
}

#[tokio::test]
async fn resource_lane_skips_busy_link_context() {
    let identity = PrivateIdentity::new_from_rand(OsRng);
    let config = TransportConfig::new("test", &identity, true);
    let transport = Transport::new(config);
    let handler = transport.get_handler();
    let resource_lane = { handler.lock().await.resource_lane.clone() };

    let destination = crate::destination::DestinationDesc {
        identity: *identity.as_identity(),
        address_hash: identity.as_identity().address_hash,
        name: DestinationName::new("lxmf", "delivery"),
    };
    let (tx, _) = tokio::sync::broadcast::channel(8);
    let link = Arc::new(Mutex::new(Link::new(destination, tx)));
    let _busy_guard = link.lock().await;

    let packet = Packet {
        header: Header { destination_type: DestinationType::Link, ..Default::default() },
        context: PacketContext::ResourceRequest,
        destination: AddressHash::new_from_rand(OsRng),
        data: PacketDataBuffer::new_from_slice(b"resource"),
        ..Default::default()
    };

    let result =
        timeout(Duration::from_millis(200), resource_lane.handle_link_packet(packet, link.clone()))
            .await
            .expect("resource lane should not wait for a busy link context");

    assert!(result.completion_job.is_none());
    assert!(result.responses.is_empty());
    assert!(result.events.is_empty());
}

#[tokio::test]
async fn resource_lane_skips_packet_when_manager_queue_is_full() {
    let identity = PrivateIdentity::new_from_rand(OsRng);
    let manager = Arc::new(Mutex::new(ResourceManager::new()));
    let resource_lane =
        super::resource_lane::ResourceManagerLane::spawn_with_capacity(manager.clone(), 1);
    let manager_guard = manager.lock().await;

    let destination = crate::destination::DestinationDesc {
        identity: *identity.as_identity(),
        address_hash: identity.as_identity().address_hash,
        name: DestinationName::new("lxmf", "delivery"),
    };
    let (tx, _) = tokio::sync::broadcast::channel(8);
    let link = Arc::new(Mutex::new(Link::new(destination, tx)));
    let packet = Packet {
        header: Header { destination_type: DestinationType::Link, ..Default::default() },
        context: PacketContext::ResourceRequest,
        destination: AddressHash::new_from_rand(OsRng),
        data: PacketDataBuffer::new_from_slice(b"resource"),
        ..Default::default()
    };

    let _first = resource_lane
        .try_enqueue_link_packet_for_test(packet.clone(), link.clone())
        .expect("first resource packet should enqueue");
    let _second = resource_lane.try_enqueue_link_packet_for_test(packet.clone(), link.clone());

    let result =
        timeout(Duration::from_millis(50), resource_lane.handle_link_packet(packet, link.clone()))
            .await
            .expect("full resource manager queue should not stall packet handling");

    assert!(result.completion_job.is_none());
    assert!(result.responses.is_empty());
    assert!(result.events.is_empty());

    drop(manager_guard);
}

#[tokio::test]
async fn resource_lane_retry_poll_skips_when_manager_queue_is_full() {
    let identity = PrivateIdentity::new_from_rand(OsRng);
    let manager = Arc::new(Mutex::new(ResourceManager::new()));
    let resource_lane =
        super::resource_lane::ResourceManagerLane::spawn_with_capacity(manager.clone(), 1);
    let manager_guard = manager.lock().await;

    let destination = crate::destination::DestinationDesc {
        identity: *identity.as_identity(),
        address_hash: identity.as_identity().address_hash,
        name: DestinationName::new("lxmf", "delivery"),
    };
    let (tx, _) = tokio::sync::broadcast::channel(8);
    let link = Arc::new(Mutex::new(Link::new(destination, tx)));
    let packet = Packet {
        header: Header { destination_type: DestinationType::Link, ..Default::default() },
        context: PacketContext::ResourceRequest,
        destination: AddressHash::new_from_rand(OsRng),
        data: PacketDataBuffer::new_from_slice(b"resource"),
        ..Default::default()
    };

    let _first = resource_lane
        .try_enqueue_link_packet_for_test(packet.clone(), link.clone())
        .expect("first resource packet should enqueue");
    let _second = resource_lane.try_enqueue_link_packet_for_test(packet, link);

    let (requests, advertisements) =
        timeout(Duration::from_millis(50), resource_lane.retry_poll(Instant::now()))
            .await
            .expect("full resource manager queue should not stall retry polling");

    assert!(requests.is_empty());
    assert!(advertisements.is_empty());

    drop(manager_guard);
}

#[tokio::test]
async fn resource_lane_remove_links_defers_when_manager_queue_is_full() {
    let identity = PrivateIdentity::new_from_rand(OsRng);
    let manager = Arc::new(Mutex::new(ResourceManager::new()));
    let resource_lane =
        super::resource_lane::ResourceManagerLane::spawn_with_capacity(manager.clone(), 1);
    let manager_guard = manager.lock().await;

    let destination = crate::destination::DestinationDesc {
        identity: *identity.as_identity(),
        address_hash: identity.as_identity().address_hash,
        name: DestinationName::new("lxmf", "delivery"),
    };
    let (tx, _) = tokio::sync::broadcast::channel(8);
    let link = Arc::new(Mutex::new(Link::new(destination, tx)));
    let packet = Packet {
        header: Header { destination_type: DestinationType::Link, ..Default::default() },
        context: PacketContext::ResourceRequest,
        destination: AddressHash::new_from_rand(OsRng),
        data: PacketDataBuffer::new_from_slice(b"resource"),
        ..Default::default()
    };

    let _first = resource_lane
        .try_enqueue_link_packet_for_test(packet.clone(), link.clone())
        .expect("first resource packet should enqueue");
    let _second = resource_lane.try_enqueue_link_packet_for_test(packet, link);

    timeout(
        Duration::from_millis(50),
        resource_lane.remove_link_state(vec![AddressHash::new_from_slice(&[0x7a; 32])]),
    )
    .await
    .expect("full resource manager queue should not stall link cleanup");

    drop(manager_guard);
}

#[tokio::test]
async fn resource_lane_commit_prepared_send_fails_when_manager_queue_is_full() {
    let identity = PrivateIdentity::new_from_rand(OsRng);
    let manager = Arc::new(Mutex::new(ResourceManager::new()));
    let resource_lane =
        super::resource_lane::ResourceManagerLane::spawn_with_capacity(manager.clone(), 1);
    let manager_guard = manager.lock().await;

    let destination = crate::destination::DestinationDesc {
        identity: *identity.as_identity(),
        address_hash: identity.as_identity().address_hash,
        name: DestinationName::new("lxmf", "delivery"),
    };
    let (tx, _) = tokio::sync::broadcast::channel(8);
    let link = Arc::new(Mutex::new(Link::new(destination, tx)));
    let packet = Packet {
        header: Header { destination_type: DestinationType::Link, ..Default::default() },
        context: PacketContext::ResourceRequest,
        destination: AddressHash::new_from_rand(OsRng),
        data: PacketDataBuffer::new_from_slice(b"resource"),
        ..Default::default()
    };
    let prepared = {
        let link = link.lock().await;
        let context = link.packet_context();
        ResourceManager::prepare_send_for(&context, b"resource-payload".to_vec(), None)
            .expect("prepared resource send")
    };

    let _first = resource_lane
        .try_enqueue_link_packet_for_test(packet.clone(), link.clone())
        .expect("first resource packet should enqueue");
    let _second = resource_lane.try_enqueue_link_packet_for_test(packet, link);

    let result = timeout(Duration::from_millis(50), resource_lane.commit_prepared_send(prepared))
        .await
        .expect("full resource manager queue should not stall prepared-send commit");
    assert!(matches!(result, Err(RnsError::ConnectionError)));

    drop(manager_guard);
}

#[tokio::test]
async fn resource_lane_confirm_dispatch_returns_when_manager_queue_is_full() {
    let identity = PrivateIdentity::new_from_rand(OsRng);
    let manager = Arc::new(Mutex::new(ResourceManager::new()));
    let resource_lane =
        super::resource_lane::ResourceManagerLane::spawn_with_capacity(manager.clone(), 1);
    let manager_guard = manager.lock().await;

    let destination = crate::destination::DestinationDesc {
        identity: *identity.as_identity(),
        address_hash: identity.as_identity().address_hash,
        name: DestinationName::new("lxmf", "delivery"),
    };
    let (tx, _) = tokio::sync::broadcast::channel(8);
    let link = Arc::new(Mutex::new(Link::new(destination, tx)));
    let packet = Packet {
        header: Header { destination_type: DestinationType::Link, ..Default::default() },
        context: PacketContext::ResourceRequest,
        destination: AddressHash::new_from_rand(OsRng),
        data: PacketDataBuffer::new_from_slice(b"resource"),
        ..Default::default()
    };

    let _first = resource_lane
        .try_enqueue_link_packet_for_test(packet.clone(), link.clone())
        .expect("first resource packet should enqueue");
    let _second = resource_lane.try_enqueue_link_packet_for_test(packet, link);

    timeout(
        Duration::from_millis(50),
        resource_lane.confirm_outbound_dispatch(Hash::new_from_slice(b"resource-hash"), true),
    )
    .await
    .expect("full resource manager queue should not stall dispatch confirmation");

    drop(manager_guard);
}

// ---------------------------------------------------------------------
// Per-peer virtual unicast iface registration
// (see TransportHandler::unicast_iface_for_source)
// ---------------------------------------------------------------------
//
// On receiving an announce from a UDP peer over a multicast iface, the
// transport registers a *virtual* iface pinned to that peer's
// SocketAddr in the iface's PeerRouting map. The virtual iface shares
// its tx channel with the host multicast iface; the host's tx task
// resolves the virtual hash to a unicast send on the same socket.
// This is what stops the 22 Mb/s LAN flood without creating separate
// per-peer sockets (which would bind to ephemeral ports and confuse
// ingress attribution).

fn peer_addr(port: u16) -> std::net::SocketAddr {
    use std::net::{IpAddr, Ipv4Addr};
    std::net::SocketAddr::new(IpAddr::V4(Ipv4Addr::new(192, 168, 1, 112)), port)
}

/// Register a fake multicast iface (role-tagged only — no real socket)
/// plus a shared `PeerRouting` map, and hand the routing map to the
/// handler so `unicast_iface_for_source` can use it. Returns the
/// iface's `AddressHash`.
///
/// Mirrors what `Transport::add_multicast_udp_interface` would do,
/// but without spawning the real UdpInterface task (which needs real
/// sockets). Tests can still exercise the handler's registration /
/// cache / GC logic in isolation this way.
async fn register_fake_multicast_iface(transport: &Transport) -> AddressHash {
    let routing = Arc::new(Mutex::new(crate::iface::udp::PeerRouting::new()));
    let iface_hash = {
        let mgr = transport.iface_manager();
        let mut mgr = mgr.lock().await;
        let channel = mgr.new_channel_with_role(16, crate::iface::IfaceRole::Multicast);
        *channel.address()
    };
    transport.get_handler().lock().await.register_multicast_peer_routing(iface_hash, routing);
    iface_hash
}

async fn new_unicast_iface_in(transport: &Transport) -> AddressHash {
    let mgr = transport.iface_manager();
    let mut mgr = mgr.lock().await;
    let channel = mgr.new_channel(16);
    *channel.address()
}

#[tokio::test]
async fn unicast_iface_for_source_returns_none_for_non_multicast_iface() {
    let identity = PrivateIdentity::new_from_rand(OsRng);
    let transport = Transport::new(TransportConfig::new("test", &identity, true));
    let unicast_iface = new_unicast_iface_in(&transport).await;
    let handler = transport.get_handler();

    let result = handler
        .lock()
        .await
        .unicast_iface_for_source(unicast_iface, crate::iface::IfaceSource::Udp(peer_addr(4242)))
        .await;

    assert_eq!(result, None, "non-multicast iface must not trigger auto-unicast");
}

#[tokio::test]
async fn unicast_iface_for_source_returns_none_when_source_is_not_udp() {
    let identity = PrivateIdentity::new_from_rand(OsRng);
    let transport = Transport::new(TransportConfig::new("test", &identity, true));
    let mc_iface = register_fake_multicast_iface(&transport).await;
    let handler = transport.get_handler();

    let result = handler
        .lock()
        .await
        .unicast_iface_for_source(mc_iface, crate::iface::IfaceSource::None)
        .await;

    assert_eq!(result, None, "no source addr means no auto-unicast");
}

#[tokio::test]
async fn unicast_iface_for_source_returns_none_when_no_peer_routing_registered() {
    let identity = PrivateIdentity::new_from_rand(OsRng);
    let transport = Transport::new(TransportConfig::new("test", &identity, true));
    // Register a Multicast-tagged iface *without* a PeerRouting map.
    let mc_iface = {
        let mgr = transport.iface_manager();
        let mut mgr = mgr.lock().await;
        let channel = mgr.new_channel_with_role(16, crate::iface::IfaceRole::Multicast);
        *channel.address()
    };
    let handler = transport.get_handler();

    let result = handler
        .lock()
        .await
        .unicast_iface_for_source(mc_iface, crate::iface::IfaceSource::Udp(peer_addr(4242)))
        .await;

    assert_eq!(
        result, None,
        "missing PeerRouting means we can't register — bail rather than silently misroute"
    );
}

#[tokio::test]
async fn unicast_iface_for_source_registers_virtual_iface_and_peer_routing() {
    let identity = PrivateIdentity::new_from_rand(OsRng);
    let transport = Transport::new(TransportConfig::new("test", &identity, true));
    let mc_iface = register_fake_multicast_iface(&transport).await;
    let handler = transport.get_handler();

    let iface_count_before = { transport.iface_manager().lock().await.iface_count() };

    let peer = peer_addr(4242);
    let virtual_hash = handler
        .lock()
        .await
        .unicast_iface_for_source(mc_iface, crate::iface::IfaceSource::Udp(peer))
        .await
        .expect("should register a virtual iface");

    assert_ne!(
        virtual_hash, mc_iface,
        "virtual iface hash is distinct from the host multicast iface"
    );

    // A single LocalInterface entry was added (the virtual one).
    let iface_count_after = { transport.iface_manager().lock().await.iface_count() };
    assert_eq!(iface_count_after, iface_count_before + 1);

    // Role is VirtualUnicast so InterfaceManager::send skips it on Broadcast tx.
    let role = { transport.iface_manager().lock().await.role(&virtual_hash) };
    assert_eq!(role, Some(crate::iface::IfaceRole::VirtualUnicast));

    // Handler tracks it.
    let guard = handler.lock().await;
    assert_eq!(guard.unicast_udp_ifaces.len(), 1);
    assert_eq!(guard.unicast_udp_ifaces.get(&peer).map(|(h, _)| *h), Some(virtual_hash),);

    // And the PeerRouting map has the forward + reverse entries.
    let routing = guard.multicast_peer_routings.get(&mc_iface).expect("routing");
    let routing = routing.lock().await;
    assert_eq!(routing.hash_for_addr(&peer), Some(virtual_hash));
    assert_eq!(routing.addr_for_hash(&virtual_hash), Some(peer));
}

#[tokio::test]
async fn unicast_iface_for_source_reuses_existing_virtual_iface_for_same_peer() {
    let identity = PrivateIdentity::new_from_rand(OsRng);
    let transport = Transport::new(TransportConfig::new("test", &identity, true));
    let mc_iface = register_fake_multicast_iface(&transport).await;
    let handler = transport.get_handler();

    let peer = peer_addr(4242);
    let first = handler
        .lock()
        .await
        .unicast_iface_for_source(mc_iface, crate::iface::IfaceSource::Udp(peer))
        .await
        .expect("first");
    let second = handler
        .lock()
        .await
        .unicast_iface_for_source(mc_iface, crate::iface::IfaceSource::Udp(peer))
        .await
        .expect("second");

    assert_eq!(first, second, "same peer reuses the same virtual iface hash");

    let guard = handler.lock().await;
    assert_eq!(guard.unicast_udp_ifaces.len(), 1);
}

#[tokio::test]
async fn unicast_iface_for_source_registers_distinct_virtual_ifaces_for_distinct_peers() {
    let identity = PrivateIdentity::new_from_rand(OsRng);
    let transport = Transport::new(TransportConfig::new("test", &identity, true));
    let mc_iface = register_fake_multicast_iface(&transport).await;
    let handler = transport.get_handler();

    let peer_a = peer_addr(4242);
    let peer_b = peer_addr(5252);

    let iface_a = handler
        .lock()
        .await
        .unicast_iface_for_source(mc_iface, crate::iface::IfaceSource::Udp(peer_a))
        .await
        .expect("peer a");
    let iface_b = handler
        .lock()
        .await
        .unicast_iface_for_source(mc_iface, crate::iface::IfaceSource::Udp(peer_b))
        .await
        .expect("peer b");

    assert_ne!(iface_a, iface_b);

    let guard = handler.lock().await;
    assert_eq!(guard.unicast_udp_ifaces.len(), 2);
    let routing = guard.multicast_peer_routings.get(&mc_iface).expect("routing").lock().await;
    assert_eq!(routing.hash_for_addr(&peer_a), Some(iface_a));
    assert_eq!(routing.hash_for_addr(&peer_b), Some(iface_b));
}

#[tokio::test]
async fn unicast_iface_for_source_refreshes_last_seen_on_repeat_call() {
    let identity = PrivateIdentity::new_from_rand(OsRng);
    let transport = Transport::new(TransportConfig::new("test", &identity, true));
    let mc_iface = register_fake_multicast_iface(&transport).await;
    let handler = transport.get_handler();

    let peer = peer_addr(4242);
    handler
        .lock()
        .await
        .unicast_iface_for_source(mc_iface, crate::iface::IfaceSource::Udp(peer))
        .await
        .expect("register");

    {
        let mut guard = handler.lock().await;
        let entry = guard.unicast_udp_ifaces.get_mut(&peer).expect("cached");
        entry.1 = tokio::time::Instant::now() - Duration::from_secs(600);
    }

    handler
        .lock()
        .await
        .unicast_iface_for_source(mc_iface, crate::iface::IfaceSource::Udp(peer))
        .await
        .expect("refresh");

    let guard = handler.lock().await;
    let (_, last_seen) = guard.unicast_udp_ifaces.get(&peer).expect("cached");
    let age = tokio::time::Instant::now().saturating_duration_since(*last_seen);
    assert!(age < Duration::from_secs(1), "last_seen must be refreshed; got age {:?}", age,);
}

#[tokio::test]
async fn gc_unicast_ifaces_removes_stale_entries_from_routing_and_manager() {
    let identity = PrivateIdentity::new_from_rand(OsRng);
    let transport = Transport::new(TransportConfig::new("test", &identity, true));
    let mc_iface = register_fake_multicast_iface(&transport).await;
    let handler = transport.get_handler();

    let stale_peer = peer_addr(4242);
    let fresh_peer = peer_addr(5252);

    let stale_iface = handler
        .lock()
        .await
        .unicast_iface_for_source(mc_iface, crate::iface::IfaceSource::Udp(stale_peer))
        .await
        .expect("stale");
    let fresh_iface = handler
        .lock()
        .await
        .unicast_iface_for_source(mc_iface, crate::iface::IfaceSource::Udp(fresh_peer))
        .await
        .expect("fresh");

    {
        let mut guard = handler.lock().await;
        let entry = guard.unicast_udp_ifaces.get_mut(&stale_peer).expect("cached");
        entry.1 = tokio::time::Instant::now() - Duration::from_secs(3600);
    }

    handler.lock().await.gc_unicast_ifaces().await;

    let guard = handler.lock().await;
    assert!(!guard.unicast_udp_ifaces.contains_key(&stale_peer));
    assert!(guard.unicast_udp_ifaces.contains_key(&fresh_peer));

    // PeerRouting map no longer contains the stale peer.
    let routing = guard.multicast_peer_routings.get(&mc_iface).expect("routing").lock().await;
    assert_eq!(routing.hash_for_addr(&stale_peer), None);
    assert_eq!(routing.hash_for_addr(&fresh_peer), Some(fresh_iface));
    assert_eq!(routing.addr_for_hash(&stale_iface), None);
    drop(routing);

    // InterfaceManager stopped the stale virtual iface (role lookup now None).
    let mgr = transport.iface_manager();
    let mgr = mgr.lock().await;
    assert_eq!(mgr.role(&stale_iface), None);
    assert_eq!(mgr.role(&fresh_iface), Some(crate::iface::IfaceRole::VirtualUnicast));
}

#[tokio::test]
async fn gc_unicast_ifaces_is_noop_when_no_entries_are_stale() {
    let identity = PrivateIdentity::new_from_rand(OsRng);
    let transport = Transport::new(TransportConfig::new("test", &identity, true));
    let mc_iface = register_fake_multicast_iface(&transport).await;
    let handler = transport.get_handler();

    let peer = peer_addr(4242);
    let iface = handler
        .lock()
        .await
        .unicast_iface_for_source(mc_iface, crate::iface::IfaceSource::Udp(peer))
        .await
        .expect("register");

    handler.lock().await.gc_unicast_ifaces().await;

    let guard = handler.lock().await;
    assert!(guard.unicast_udp_ifaces.contains_key(&peer));
    let routing = guard.multicast_peer_routings.get(&mc_iface).expect("routing").lock().await;
    assert_eq!(routing.hash_for_addr(&peer), Some(iface));
}
