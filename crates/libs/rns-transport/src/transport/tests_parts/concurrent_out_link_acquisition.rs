use std::future::Future;

async fn acquisition_fixture() -> (Transport, DestinationDesc, crate::iface::InterfaceChannel) {
    let identity = PrivateIdentity::new_from_rand(OsRng);
    let transport = Transport::new(TransportConfig::new("acquisition", &identity, true));
    let channel = transport.iface_manager().lock().await.new_channel(16);
    let remote = PrivateIdentity::new_from_rand(OsRng);
    let destination = SingleInputDestination::new(remote, DestinationName::new("lxmf", "delivery"));
    assert!(transport.handler.lock().await.path_table.restore_tunnel_path(
        destination.desc.address_hash,
        destination.desc.address_hash,
        1,
        *channel.address(),
        Hash::new_from_slice(b"acquisition path"),
        std::time::Instant::now(),
    ));
    (transport, destination.desc, channel)
}

async fn concurrent_link_acquisition(replace_closed: bool) {
    let (transport, destination, mut channel) = acquisition_fixture().await;
    let old = if replace_closed {
        let old = transport.link(destination).await;
        channel.tx_channel.try_recv().expect("original request");
        old.lock().await.close();
        Some(old)
    } else {
        None
    };
    let manager = transport.iface_manager();
    let blocked = manager.lock().await;
    let mut first = Box::pin(transport.link(destination));
    let mut second = Box::pin(transport.link(destination));
    std::future::poll_fn(|cx| {
        assert!(first.as_mut().poll(cx).is_pending());
        assert!(second.as_mut().poll(cx).is_pending());
        std::task::Poll::Ready(())
    }).await;
    drop(blocked);
    let (first, second) = tokio::join!(first, second);
    assert!(Arc::ptr_eq(&first, &second), "concurrent callers must share one published Link");
    if let Some(old) = old {
        assert!(!Arc::ptr_eq(&first, &old), "Closed Link is replaced");
    }
    let handler = transport.handler.lock().await;
    assert_eq!(handler.out_links.len(), 1);
    assert!(Arc::ptr_eq(handler.out_links.get(&destination.address_hash).expect("published"), &first));
    let request = channel.tx_channel.try_recv().expect("winner sends request");
    assert_eq!(request.packet.header.packet_type, PacketType::LinkRequest);
    assert!(channel.tx_channel.try_recv().is_err(), "losing candidates must not dispatch");
}

#[tokio::test]
async fn concurrent_link_acquisition_from_absent_entry_publishes_once() {
    concurrent_link_acquisition(false).await;
}

#[tokio::test]
async fn concurrent_link_acquisition_from_closed_entry_publishes_once() {
    concurrent_link_acquisition(true).await;
}

#[tokio::test]
async fn concurrent_link_acquisition_preserves_restart_during_preparation() {
    let (transport, destination, mut channel) = acquisition_fixture().await;
    let original = transport.link(destination).await;
    channel.tx_channel.try_recv().expect("original request");
    original.lock().await.close();
    let manager = transport.iface_manager();
    let blocked = manager.lock().await;
    let mut acquire = Box::pin(transport.link(destination));
    std::future::poll_fn(|cx| {
        assert!(acquire.as_mut().poll(cx).is_pending());
        std::task::Poll::Ready(())
    }).await;
    original.lock().await.restart();
    drop(blocked);
    let acquired = acquire.await;
    assert!(Arc::ptr_eq(&original, &acquired), "restarted Link must not be replaced");
    assert!(channel.tx_channel.try_recv().is_err(), "discarded candidate must not dispatch");
}

#[tokio::test]
async fn concurrent_link_acquisition_does_not_reuse_removed_entry() {
    let (transport, destination, mut channel) = acquisition_fixture().await;
    let original = transport.link(destination).await;
    channel.tx_channel.try_recv().expect("original request");
    let blocked = original.lock().await;
    let mut acquire = Box::pin(transport.link(destination));
    let mut reset = Box::pin(transport.reset_out_link(&destination.address_hash));
    std::future::poll_fn(|cx| {
        assert!(acquire.as_mut().poll(cx).is_pending());
        assert!(reset.as_mut().poll(cx).is_pending());
        std::task::Poll::Ready(())
    }).await;
    assert!(!transport.handler.lock().await.out_links.contains_key(&destination.address_hash));
    drop(blocked);
    let (acquired, ()) = tokio::join!(biased; acquire, reset);
    assert!(!Arc::ptr_eq(&original, &acquired), "removed entry must not be reused");
    assert_eq!(original.lock().await.status(), LinkStatus::Closed);
    assert!(Arc::ptr_eq(transport.handler.lock().await.out_links.get(&destination.address_hash).expect("replacement"), &acquired));
    channel.tx_channel.try_recv().expect("replacement request");
    assert!(channel.tx_channel.try_recv().is_err());
}

#[tokio::test]
async fn concurrent_link_acquisition_waits_for_restart_without_holding_handler() {
    let (transport, destination, mut channel) = acquisition_fixture().await;
    let original = transport.link(destination).await;
    channel.tx_channel.try_recv().expect("original request");
    original.lock().await.close();
    let manager = transport.iface_manager();
    let blocked = manager.lock().await;
    let mut acquire = Box::pin(transport.link(destination));
    std::future::poll_fn(|cx| {
        assert!(acquire.as_mut().poll(cx).is_pending());
        std::task::Poll::Ready(())
    }).await;
    let mut restarting = original.lock().await;
    drop(blocked);
    std::future::poll_fn(|cx| {
        assert!(acquire.as_mut().poll(cx).is_pending());
        std::task::Poll::Ready(())
    }).await;
    assert!(transport.handler.try_lock().is_ok(), "a busy Link must not block unrelated handler work");
    restarting.restart();
    drop(restarting);
    let acquired = acquire.await;
    assert!(Arc::ptr_eq(&original, &acquired));
    assert!(channel.tx_channel.try_recv().is_err());
}
