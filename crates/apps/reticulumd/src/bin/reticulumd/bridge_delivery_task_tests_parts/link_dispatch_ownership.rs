pub(in crate::bridge) async fn task_with_active_link(
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
    let id = *link.lock().await.id();
    let mut link_events = task.transport.out_link_events();
    channel
        .rx_channel
        .send(rns_transport::iface::RxMessage {
            address: iface,
            packet: inbound.prove(),
            source: rns_transport::iface::IfaceSource::None,
        })
        .await
        .expect("proof enters native ingress");
    tokio::time::timeout(Duration::from_secs(1), async {
        loop {
            let event = link_events.recv().await.expect("native Link activation event");
            if event.id == id
                && matches!(event.event, rns_transport::destination::link::LinkEvent::Activated)
            {
                break;
            }
        }
    })
    .await
    .expect("proof validates and activates through native hop selection");
    // A real active Link whose bound interface can no longer accept output.
    drop(channel);
    (task, link, receipts)
}
