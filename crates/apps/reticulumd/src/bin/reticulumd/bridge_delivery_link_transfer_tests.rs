use super::*;
use reticulum_daemon::receipt_bridge::{ReceiptBridge, ReceiptPublisher};
use rns_transport::destination::DestinationName;
use rns_transport::iface::{IfaceRole, InterfaceChannel};
use rns_transport::packet::{Packet, PacketContext};
use rns_transport::resource::ResourceEventKind;
use rns_transport::transport::{Transport, TransportConfig};

async fn ingress(channel: &InterfaceChannel, packet: Packet) {
    channel
        .rx_channel
        .send(RxMessage { address: *channel.address(), packet, source: IfaceSource::None })
        .await
        .expect("native peer ingress");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn retained_link_transfers_commit_real_native_packet_and_resource_proofs() {
    for (method, resource) in [
        (RequestedDeliveryMethod::Direct, false),
        (RequestedDeliveryMethod::Direct, true),
        (RequestedDeliveryMethod::Opportunistic, true),
        (RequestedDeliveryMethod::Propagated, false),
        (RequestedDeliveryMethod::Propagated, true),
    ] {
        assert_native_transfer(method, resource, false, false).await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn creator_rejection_while_packet_custody_is_uncommitted_wins() {
    assert_native_transfer(RequestedDeliveryMethod::Propagated, false, true, false).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn creator_rejection_while_resource_custody_is_uncommitted_wins() {
    assert_native_transfer(RequestedDeliveryMethod::Propagated, true, true, false).await;
}

async fn assert_native_transfer(
    method: RequestedDeliveryMethod,
    resource: bool,
    reject: bool,
    late_signal: bool,
) {
    let propagated = method == RequestedDeliveryMethod::Propagated;
    let final_identity = rns_transport::identity::PrivateIdentity::new_from_rand(OsRng);
    let final_destination = SingleInputDestination::new(
        final_identity.clone(),
        DestinationName::new("lxmf", "delivery"),
    );
    let remote_identity = if propagated {
        rns_transport::identity::PrivateIdentity::new_from_rand(OsRng)
    } else {
        final_identity.clone()
    };
    let receiver =
        Transport::new(TransportConfig::new("retained-link-peer", &remote_identity, true));
    let remote_destination = receiver
        .add_destination(
            remote_identity.clone(),
            DestinationName::new("lxmf", if propagated { "propagation" } else { "delivery" }),
        )
        .await;
    let descriptor = remote_destination.lock().await.desc;
    let mut receiving =
        receiver.iface_manager().lock().await.new_channel_with_role(64, IfaceRole::Unicast);
    let content = if resource {
        "native Resource payload".repeat(2000)
    } else {
        "native packet payload".into()
    };
    let (_directory, connection, mut task) = durable_task_for_content(
        final_destination.desc.address_hash.as_slice().try_into().expect("recipient hash"),
        &content,
    );
    connection.execute_batch("DROP TRIGGER fail_prepared;").expect("healthy preparation storage");
    task.requested_method = method;
    task.peer_identity = Some(*final_identity.as_identity());
    if propagated {
        task.propagation_node_hex = Some(descriptor.address_hash.to_hex_string());
        task.propagation_node_identity = Some(*remote_identity.as_identity());
    }
    let sender = task.transport.clone();
    let mut sending =
        sender.iface_manager().lock().await.new_channel_with_role(64, IfaceRole::Unicast);
    ingress(
        &sending,
        remote_destination.lock().await.announce(OsRng, None).expect("signed announce"),
    )
    .await;
    tokio::time::timeout(Duration::from_secs(2), async {
        while !sender.has_path(&descriptor.address_hash).await {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("real peer path");
    let id = task.message_id.clone();
    let peer = task.destination_hex.clone();
    let daemon = task.daemon.clone();
    let resources = task.outbound_resource_map.clone();
    let scheduler = DeliveryScheduler::spawn(DeliverySchedulerConfig::default());
    let stamp = Arc::new(Semaphore::new(1));
    let mut prepared = prepare_payload(&task, &stamp, &scheduler.metrics, &peer)
        .await
        .expect("durable LXMF bytes");
    if propagated {
        // Stamp generation/cost discovery have independent reference coverage.
        // This fixture starts from a committed immutable remote envelope.
        prepared.propagation = Some(crate::bridge::delivery_task::PreparedPropagationPayload {
            propagation_node_hex: descriptor.address_hash.to_hex_string(),
            propagation_hash: descriptor.address_hash,
            target_cost: 1,
            payload: crate::bridge::propagation::build_propagation_payload_until_cancelled(
                &prepared.lxmf_payload,
                final_identity.as_identity(),
                1,
                || false,
            )
            .expect("propagation envelope"),
        });
        daemon
            .persist_durable_prepared_payload(&id, encode_prepared(&prepared))
            .expect("commit envelope");
    }
    let expected =
        prepared.propagation.as_ref().map_or(&prepared.lxmf_payload, |p| &p.payload.bytes).clone();
    connection.execute_batch("CREATE TRIGGER fail_native_receipt BEFORE UPDATE OF receipt_status ON messages BEGIN SELECT RAISE(ABORT,'SDK_STORAGE_BROKER_FULL injected'); END;").expect("block native proof persistence");
    let (tx, rx) = tokio::sync::mpsc::channel(4);
    let publisher = ReceiptPublisher::from(tx);
    publisher.reserve_completion(&id, propagated).expect("retained receipt capacity");
    task.receipt_tx = publisher.clone();
    sender
        .set_receipt_handler(Box::new(ReceiptBridge::new(
            task.receipt_map.clone(),
            publisher.clone(),
        )))
        .await;
    let receipt_owner = crate::receipt_worker::spawn_receipt_worker(
        daemon.clone(),
        rx,
        task.receipt_map.clone(),
        publisher.clone(),
    );
    let mut received_data = receiver.received_data_events();
    let mut received_resources = receiver.resource_events();
    let global = Arc::new(Semaphore::new(1));
    let peer_limit = Arc::new(Semaphore::new(1));
    let (_stop, stopping) = tokio::sync::watch::channel(false);
    let mut owner = tokio::spawn(completion::complete_delivery(
        admitted(&scheduler, task),
        global.clone(),
        peer_limit.clone(),
        stamp.clone(),
        scheduler.metrics.clone(),
        peer,
        stopping,
    ));
    let mut requests = 0;
    let mut advertisements = 0;
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            tokio::select! {
                frame = sending.tx_channel.recv() => {
                    let frame = frame.expect("sender frame").packet;
                    requests += usize::from(frame.header.packet_type == PacketType::LinkRequest);
                    advertisements += usize::from(frame.context == PacketContext::ResourceAdvrtisement);
                    ingress(&receiving, frame).await;
                },
                frame = receiving.tx_channel.recv() => {
                    let frame = frame.expect("peer frame").packet;
                    let completed = frame.header.packet_type == PacketType::Proof
                        && matches!(frame.context, PacketContext::None | PacketContext::LinkProof | PacketContext::ResourceProof);
                    ingress(&sending, frame).await;
                    if completed { break; }
                },
                result = &mut owner => panic!("owner retired before native proof: {result:?}"),
            }
        }
    }).await.expect("native peer completion proof");
    assert_eq!(requests, 1, "one native Link lifetime");
    assert_eq!(advertisements, usize::from(resource), "packet/Resource boundary for {method:?}");
    let received = if resource {
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                if let ResourceEventKind::Complete(value) =
                    received_resources.recv().await.expect("native Resource event").kind
                {
                    break value.data;
                }
            }
        })
        .await
        .expect("recipient Resource bytes")
    } else {
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                let value = received_data.recv().await.expect("native Link data");
                if value.context == Some(PacketContext::None) {
                    break value.data.as_slice().to_vec();
                }
            }
        })
        .await
        .expect("recipient packet bytes")
    };
    assert_eq!(received, expected, "peer reconstructs the exact committed bytes");
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(!owner.is_finished(), "native success retains admission across failed storage");
    assert!(scheduler.owns_delivery(&id));
    assert_eq!(state(&connection, &id), "scheduled");
    assert_eq!(global.available_permits(), 1);
    assert_eq!(peer_limit.available_permits(), 1);
    assert_eq!(stamp.available_permits(), 1);
    assert!(sending.tx_channel.try_recv().is_err(), "persistence retry cannot retransmit");
    if reject {
        send_creator_signal(&sender, &receiver, descriptor.address_hash, &sending, &mut receiving)
            .await;
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert!(!owner.is_finished(), "uncommitted custody still owns creator signal");
    }
    connection.execute_batch("DROP TRIGGER fail_native_receipt;").expect("restore receipt storage");
    tokio::time::timeout(Duration::from_secs(3), owner)
        .await
        .expect("commit releases admission")
        .expect("owner");
    if late_signal {
        send_creator_signal(&sender, &receiver, descriptor.address_hash, &sending, &mut receiving)
            .await;
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    receipt_owner.drain().await.expect("receipt worker drain");
    assert!(!scheduler.owns_delivery(&id));
    assert!(resources.lock().expect("Resource correlation").is_empty());
    assert_eq!(state(&connection, &id), if propagated && !reject { "custody" } else { "terminal" });
    assert_eq!(
        daemon.message_receipt_status(&id).expect("committed status").as_deref(),
        Some(if reject {
            "rejected"
        } else if propagated {
            if resource {
                "sent: propagated resource"
            } else {
                "sent: propagated"
            }
        } else {
            "delivered"
        })
    );
    assert_eq!(
        connection
            .query_row("SELECT COUNT(*) FROM broker_events WHERE event_type='receipt'", [], |row| {
                row.get::<_, i64>(0)
            })
            .expect("receipt journal"),
        if reject { 2 } else { 1 }
    );
    scheduler.shutdown();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn creator_rejection_after_acknowledged_custody_cannot_reopen_delivery() {
    for resource in [false, true] {
        assert_native_transfer(RequestedDeliveryMethod::Propagated, resource, false, true).await;
    }
}

async fn send_creator_signal(
    sender: &Transport,
    receiver: &Transport,
    target: rns_transport::hash::AddressHash,
    sending: &InterfaceChannel,
    receiving: &mut InterfaceChannel,
) {
    let link = sender.delivery_link(&target).await.expect("retained Link");
    let id = *link.lock().await.id();
    let remote_link = receiver.find_in_link(&id).await.expect("recipient native Link");
    let signal = remote_link
        .lock()
        .await
        .data_packet(&[0x91, 0xcc, 245])
        .expect("authenticated rejection signal");
    assert_eq!(
        receiver.send_link_packet_on_bound_iface(&remote_link, signal).await,
        rns_transport::transport::SendPacketOutcome::SentDirect
    );
    let frame = receiving.tx_channel.recv().await.expect("signal handoff").packet;
    ingress(sending, frame).await;
}
