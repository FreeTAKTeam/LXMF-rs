async fn task_with_active_link(
) -> (DeliveryTask, Arc<tokio::sync::Mutex<Link>>, tokio::sync::mpsc::Receiver<ReceiptEvent>) {
    let message_id = "link-dispatch-owner";
    let store = MessagesStore::in_memory().expect("store");
    store
        .insert_message(&MessageRecord {
            id: message_id.into(),
            source: "sender".into(),
            destination: "receiver".into(),
            title: String::new(),
            content: String::new(),
            timestamp: 0,
            direction: "out".into(),
            fields: None,
            receipt_status: Some("queued".into()),
        })
        .expect("message");
    let daemon = Arc::new(RpcDaemon::with_store(store, "dispatch-owner".into()));
    let mut task = delivery_task_for_propagation_cost_lookup(daemon);
    task.message_id = message_id.into();
    task.requested_method = RequestedDeliveryMethod::Direct;
    let remote = rns_transport::identity::PrivateIdentity::new_from_rand(OsRng);
    let identity = *remote.as_identity();
    let destination = DestinationDesc {
        identity,
        address_hash: identity.address_hash,
        name: DestinationName::new("lxmf", "delivery"),
    };
    task.peer_identity = Some(identity);
    task.destination.copy_from_slice(identity.address_hash.as_slice());
    task.destination_hash = identity.address_hash;
    task.destination_hex = hex::encode(task.destination);
    let (tx, receipts) = tokio::sync::mpsc::channel(16);
    task.receipt_tx = tx.into();
    let mut channel = task
        .transport
        .iface_manager()
        .lock()
        .await
        .new_channel_with_role(8, rns_transport::iface::IfaceRole::Unicast);
    let iface = *channel.address();
    let link = task.transport.link(destination).await;
    let request = channel.tx_channel.recv().await.expect("Link request");
    let (events, _) = tokio::sync::broadcast::channel(8);
    let mut inbound =
        Link::new_from_request(&request.packet, remote.sign_key().clone(), destination, events)
            .expect("Link request parses");
    assert!(matches!(
        link.lock().await.handle_packet(&inbound.prove(), iface),
        rns_transport::destination::link::LinkHandleResult::Activated
    ));
    // A real active Link whose bound interface can no longer accept output.
    drop(channel);
    (task, link, receipts)
}

fn dispatch_statuses() -> LinkModeStatuses {
    LinkModeStatuses {
        packet: "sent: link",
        resource: "sending: link resource",
        resource_sent: OUTBOUND_RESOURCE_SENT_STATUS,
    }
}

#[tokio::test]
async fn registered_resource_dispatch_failure_does_not_escape_as_retryable_error() {
    for stage in ["link", "propagation"] {
        let (task, link, mut receipts) = task_with_active_link().await;
        let mut resources = task.transport.resource_events();
        let result = task
            .send_via_existing_link_mode(
                stage,
                &task.destination_hex,
                link,
                &vec![42; 8192],
                dispatch_statuses(),
            )
            .await;
        assert_eq!(
            result.expect("registered Resource owns failure"),
            link_send::LinkAttemptOutcome::ResourceOwned
        );
        let event = tokio::time::timeout(Duration::from_secs(1), resources.recv())
            .await
            .expect("Resource event timeout")
            .expect("Resource event");
        assert!(matches!(event.kind, rns_transport::resource::ResourceEventKind::OutboundFailed));
        assert!(task
            .outbound_resource_map
            .lock()
            .expect("map")
            .contains_key(&hex::encode(event.hash.as_slice())));
        assert!(receipts.try_recv().is_err(), "no fabricated sent/sending receipt");
    }
}

#[tokio::test]
async fn direct_resource_dispatch_failure_has_one_terminal_consumer_and_no_fallback() {
    assert_direct_resource_failure_consumed(true).await;
}

#[tokio::test]
async fn recovered_resource_failure_is_retained_until_startup_consumer_runs() {
    assert_direct_resource_failure_consumed(false).await;
}

async fn assert_direct_resource_failure_consumed(start_before_send: bool) {
    let (mut task, _link, mut receipts) = task_with_active_link().await;
    task.try_propagation_on_fail = true;
    task.propagation_node_hex = Some(hex::encode([0x55; 16]));
    let daemon = task.daemon.clone();
    let receipt_map = task.receipt_map.clone();
    let resource_map = task.outbound_resource_map.clone();
    let message_id = task.message_id.clone();
    let transport = task.transport.clone();
    let resource_events = transport.resource_events();
    let worker_daemon = daemon.clone();
    let worker_map = resource_map.clone();
    let publisher = task.receipt_tx.clone();
    let start_consumer = move || {
        crate::inbound_worker::spawn_inbound_worker_with_resource_events(
            worker_daemon,
            transport,
            crate::bootstrap::PropagationControlContext {
                enabled: false,
                local_identity_hash: [0; 16],
                propagation_destination_hash_hex: None,
                control_destination_hash_hex: None,
                delivery_destination: None,
                allowed_control_identities: Vec::new(),
                validated_peer_links: Arc::new(Mutex::new(std::collections::HashSet::new())),
                identified_peer_links: Arc::new(Mutex::new(HashMap::new())),
            },
            None,
            publisher,
            worker_map,
            resource_events,
        )
    };
    let start_consumer = if start_before_send {
        start_consumer();
        None
    } else {
        Some(start_consumer)
    };
    // Deliberately send immediately after registration, before the worker is
    // polled. A synchronous dispatch failure must not fall through startup.
    tokio::time::timeout(
        Duration::from_secs(1),
        task.run_prepared(
            PreparedDeliveryPayload { lxmf_payload: vec![42; 8192], propagation: None },
            Arc::new(tokio::sync::Semaphore::new(1)),
        ),
    )
    .await
    .expect("no propagation fallback waiting for a different peer");
    if let Some(start_consumer) = start_consumer {
        start_consumer();
    }
    let receipt = tokio::time::timeout(Duration::from_secs(1), receipts.recv())
        .await
        .expect("terminal Resource receipt")
        .expect("receipt");
    assert_eq!(receipt.status, "failed: resource transfer failed");
    assert_eq!(receipt.delivery_kind.as_deref(), Some("resource-failed"));
    crate::receipt_events::persist_receipt_update(
        daemon.as_ref(),
        receipt,
        &receipt_map,
        &resource_map,
    )
    .expect("persist terminal result");
    assert_eq!(
        daemon.message_receipt_status(&message_id).expect("status").as_deref(),
        Some("failed: resource transfer failed")
    );
    assert!(resource_map.lock().expect("map").is_empty());
    assert!(receipt_map.lock().expect("map").is_empty());
    assert!(receipts.try_recv().is_err(), "no competing fallback or fabricated progress receipt");
}

#[tokio::test]
async fn cancelled_link_send_reports_stopped_without_dispatch() {
    let (task, link, mut receipts) = task_with_active_link().await;
    crate::receipt_events::persist_receipt_update(
        task.daemon.as_ref(),
        ReceiptEvent::new(task.message_id.clone(), "cancelled"),
        &task.receipt_map,
        &task.outbound_resource_map,
    )
    .expect("cancel message");
    let destination = *link.lock().await.destination();
    for result in [
        task.send_via_existing_link_mode(
            "link",
            &task.destination_hex,
            link,
            b"cancelled payload",
            dispatch_statuses(),
        )
        .await,
        task.send_via_link_mode(
            "link",
            &task.destination_hex,
            destination,
            b"cancelled payload",
            dispatch_statuses(),
        )
        .await,
    ] {
        assert_eq!(result.expect("stopped"), link_send::LinkAttemptOutcome::Stopped);
    }
    assert!(task.receipt_map.lock().expect("map").is_empty());
    assert!(task.outbound_resource_map.lock().expect("map").is_empty());
    assert!(receipts.try_recv().is_err());
}

#[tokio::test]
async fn dropped_link_packet_removes_only_its_receipt_mapping() {
    let (task, link, mut receipts) = task_with_active_link().await;
    task.receipt_map.lock().expect("map").insert("other-packet".into(), "other-message".into());
    assert!(task
        .send_via_existing_link_mode(
            "link",
            &task.destination_hex,
            link,
            b"small packet",
            dispatch_statuses(),
        )
        .await
        .is_err());
    let map = task.receipt_map.lock().expect("map");
    assert_eq!(map.len(), 1, "a packet proven dropped must not await a receipt");
    assert_eq!(map.get("other-packet").map(String::as_str), Some("other-message"));
    assert!(receipts.try_recv().is_err());
}
