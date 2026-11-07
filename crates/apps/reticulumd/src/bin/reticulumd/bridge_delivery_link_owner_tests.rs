use super::*;
use rns_transport::destination::link::{Link, LinkHandleResult, LinkStatus};
use rns_transport::iface::{IfaceRole, InterfaceChannel};
use rns_transport::packet::PacketContext;
use rns_transport::resource::ResourceEventKind;

#[path = "bridge_delivery_link_shutdown_tests.rs"]
mod shutdown_tests;

pub(super) async fn active_owner_link(
    task: &DeliveryTask,
    signer: &rns_transport::identity::PrivateIdentity,
    destination: rns_transport::destination::DestinationDesc,
) -> (Arc<tokio::sync::Mutex<Link>>, InterfaceChannel, Link) {
    let mut channel =
        task.transport.iface_manager().lock().await.new_channel_with_role(1, IfaceRole::Unicast);
    let link = task.transport.link(destination).await;
    let request = tokio::time::timeout(Duration::from_secs(2), channel.tx_channel.recv())
        .await
        .expect("request deadline")
        .expect("request")
        .packet;
    assert_eq!(request.header.packet_type, PacketType::LinkRequest);
    let (events, _) = tokio::sync::broadcast::channel(8);
    let mut peer = Link::new_from_request(&request, signer.sign_key().clone(), destination, events)
        .expect("peer native Link");
    channel
        .rx_channel
        .send(RxMessage {
            address: *channel.address(),
            packet: peer.prove(),
            source: IfaceSource::None,
        })
        .await
        .expect("proof ingress");
    let rtt = tokio::time::timeout(Duration::from_secs(2), channel.tx_channel.recv())
        .await
        .expect("RTT deadline")
        .expect("RTT")
        .packet;
    assert_eq!(rtt.context, PacketContext::LinkRTT);
    assert!(matches!(peer.handle_packet(&rtt, *channel.address()), LinkHandleResult::None));
    assert_eq!(peer.status(), LinkStatus::Active);
    assert_eq!(link.lock().await.status(), LinkStatus::Active);
    (link, channel, peer)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn owned_resource_shutdown_cleans_registered_handoff_with_full_interface_queue() {
    owned_resource_cleanup_case(false).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn owned_resource_receipt_cancel_after_peer_close_cleans_native_transfer() {
    owned_resource_cleanup_case(true).await;
}

async fn owned_resource_cleanup_case(peer_closes: bool) {
    let signer = rns_transport::identity::PrivateIdentity::new_from_rand(OsRng);
    let recipient =
        SingleInputDestination::new(signer.clone(), DestinationName::new("lxmf", "delivery"));
    let destination = recipient.desc.address_hash.as_slice().try_into().expect("destination");
    let content = "Resource cleanup production payload".repeat(2000);
    let (_directory, connection, mut task) = durable_task_for_content(destination, &content);
    connection.execute_batch("DROP TRIGGER fail_prepared;").expect("healthy storage");
    task.peer_identity = Some(*signer.as_identity());
    let (link, mut channel, peer_link) = active_owner_link(&task, &signer, recipient.desc).await;
    let id = task.message_id.clone();
    let peer = task.destination_hex.clone();
    let daemon = task.daemon.clone();
    let transport = task.transport.clone();
    let resources = task.outbound_resource_map.clone();
    let mut native = transport.resource_events();
    let (sender, receiver) = tokio::sync::mpsc::channel(4);
    let publisher = reticulum_daemon::receipt_bridge::ReceiptPublisher::from(sender);
    publisher.reserve_terminal(&id).expect("terminal reservation");
    task.receipt_tx = publisher.clone();
    let receipt_owner = crate::receipt_worker::spawn_receipt_worker(
        daemon.clone(),
        receiver,
        task.receipt_map.clone(),
        publisher.clone(),
    );
    let scheduler = DeliveryScheduler::spawn(DeliverySchedulerConfig::default());
    let global = Arc::new(Semaphore::new(1));
    let peer_limit = Arc::new(Semaphore::new(1));
    let stamp = Arc::new(Semaphore::new(1));
    prepare_payload(&task, &stamp, &scheduler.metrics, &peer)
        .await
        .expect("prepared Resource payload");
    let (stop, stopping) = tokio::sync::watch::channel(false);
    let owner = tokio::spawn(completion::complete_delivery(
        admitted(&scheduler, task),
        global.clone(),
        peer_limit.clone(),
        stamp,
        scheduler.metrics.clone(),
        peer,
        stopping,
    ));
    let hash = tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            if let Some((hash, tracking)) = resources
                .lock()
                .expect("tracking")
                .iter()
                .next()
                .map(|(hash, tracking)| (hash.clone(), tracking.clone()))
            {
                assert_eq!(tracking, id);
                break hash;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("native registration before blocked advertisement");
    assert!(scheduler.owns_delivery(&id));
    if peer_closes {
        let identify = channel.tx_channel.recv().await.expect("identify frame");
        assert_eq!(identify.packet.context, PacketContext::LinkIdentify);
        let advertisement = channel.tx_channel.recv().await.expect("Resource advertisement");
        assert_eq!(advertisement.packet.context, PacketContext::ResourceAdvrtisement);
        let mut plain = vec![0; 500];
        let advertisement = rns_transport::resource::ResourceAdvertisement::unpack(
            peer_link
                .decrypt(advertisement.packet.data.as_slice(), &mut plain)
                .expect("advertisement decryption"),
        )
        .expect("native advertisement");
        let request = rns_transport::resource::ResourceRequest {
            hashmap_exhausted: false,
            last_map_hash: None,
            resource_hash: advertisement.hash,
            requested_hashes: vec![advertisement.hashmap[..4].try_into().expect("first part hash")],
        };
        channel
            .rx_channel
            .send(RxMessage {
                address: *channel.address(),
                packet: rns_transport::resource::build_resource_request_packet(
                    &peer_link, &request,
                ),
                source: IfaceSource::None,
            })
            .await
            .expect("actual Resource request ingress");
        let part = tokio::time::timeout(Duration::from_secs(2), channel.tx_channel.recv())
            .await
            .expect("Resource part deadline")
            .expect("Resource part");
        assert_eq!(part.packet.context, PacketContext::Resource, "native transfer is active");
        let mut closed = link.lock().await;
        closed.close();
        assert_eq!(closed.status(), LinkStatus::Closed);
        publisher
            .try_send(reticulum_daemon::receipt_bridge::ReceiptEvent::new(&id, "cancelled"))
            .expect("receipt cancellation publication");
        drop(closed);
    } else {
        // Identify occupies the sole queue slot. Never drain it: shutdown must
        // drop blocked dispatch, clean native registration and release admission.
        assert_eq!(global.available_permits(), 0);
        stop.send_replace(true);
    }
    tokio::time::timeout(Duration::from_secs(3), owner)
        .await
        .expect("bounded native cleanup")
        .expect("owner");
    assert!(!scheduler.owns_delivery(&id));
    assert!(resources.lock().expect("tracking").is_empty());
    assert_eq!(global.available_permits(), 1);
    assert_eq!(peer_limit.available_permits(), 1);
    let terminal = tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let event = native.recv().await.expect("native cleanup event");
            if event.hash.to_string() == hash
                && matches!(
                    event.kind,
                    ResourceEventKind::OutboundCancelled | ResourceEventKind::OutboundFailed
                )
            {
                break event.kind;
            }
        }
    })
    .await
    .expect("native terminal result");
    if peer_closes {
        assert!(matches!(
            terminal,
            ResourceEventKind::OutboundCancelled | ResourceEventKind::OutboundFailed
        ));
        assert_eq!(
            daemon.message_receipt_status(&id).expect("persisted status").as_deref(),
            Some("cancelled")
        );
        assert_eq!(state(&connection, &id), "terminal");
    } else {
        assert!(matches!(terminal, ResourceEventKind::OutboundCancelled));
        assert_eq!(state(&connection, &id), "scheduled", "shutdown preserves restart recovery");
        assert_eq!(
            channel.tx_channel.try_recv().expect("still full").packet.context,
            PacketContext::LinkIdentify
        );
        assert!(channel.tx_channel.try_recv().is_err(), "no abandoned advertisement/retry");
    }
    receipt_owner.drain().await.expect("receipt worker drain");
    scheduler.shutdown();
}
