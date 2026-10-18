use super::*;

#[tokio::test(start_paused = true)]
async fn blocked_path_request_gets_full_fresh_window_after_handoff() {
    blocked_path_request_window(false).await;
}

#[tokio::test(start_paused = true)]
async fn blocked_retry_request_gets_full_fresh_window_after_handoff() {
    blocked_path_request_window(true).await;
}

async fn blocked_path_request_window(after_packet: bool) {
    use std::future::{poll_fn, Future};
    use std::task::Poll;
    let recipient = SingleInputDestination::new(
        rns_transport::identity::PrivateIdentity::new_from_rand(OsRng),
        DestinationName::new("lxmf", "delivery"),
    );
    let destination = recipient.desc.address_hash;
    let (_directory, connection, mut task) =
        durable_task_for_destination(destination.as_slice().try_into().expect("hash"));
    connection.execute_batch("DROP TRIGGER fail_prepared").expect("healthy storage");
    task.requested_method = RequestedDeliveryMethod::Opportunistic;
    task.peer_identity = Some(*recipient.identity.as_identity());
    let payload = task.build_payload().await.expect("payload");
    let transport = task.transport.clone();
    let manager = transport.iface_manager();
    let mut channel = manager.lock().await.new_channel(32);
    assert!(manager.lock().await.set_announce_pacing(*channel.address(), 100_000, 100_000));
    let metrics = DeliverySchedulerMetrics::default();
    metrics.record_admitted_for_peer(&task.destination_hex);
    let (stop, mut stopping) = tokio::sync::watch::channel(false);
    task.stopping = Some(stopping.clone());
    let owner = opportunistic::run(
        &task,
        &payload,
        Arc::new(Semaphore::new(1)),
        Arc::new(Semaphore::new(1)),
        &metrics,
        &task.destination_hex,
        &mut stopping,
    );
    tokio::pin!(owner);
    if after_packet {
        tokio::select! {
            _ = &mut owner => panic!("owner must retain delivery"),
            frame = channel.tx_channel.recv() => {
                assert_ne!(frame.expect("initial request").packet.destination, destination);
            },
        }
        poll_fn(|cx| {
            assert!(owner.as_mut().poll(cx).is_pending());
            Poll::Ready(())
        })
        .await;
        tokio::time::advance(Duration::from_secs(7) + Duration::from_nanos(1)).await;
        tokio::select! {
            _ = &mut owner => panic!("owner must retain delivery"),
            frame = channel.tx_channel.recv() => {
                assert_eq!(frame.expect("initial packet").packet.destination, destination);
            },
            _ = tokio::time::sleep(Duration::from_millis(10)) => panic!("initial send due"),
        }
        poll_fn(|cx| {
            assert!(owner.as_mut().poll(cx).is_pending());
            Poll::Ready(())
        })
        .await;
        tokio::time::advance(Duration::from_secs(10) + Duration::from_nanos(1)).await;
        // Finish newly ready maintenance before staging the owner's lock reads.
        for _ in 0..64 {
            tokio::task::yield_now().await;
        }
    }
    // FIFO mutex waiters separate the two measurement reads from the actual
    // handoff without a production hook or replacing the transport operation.
    let first = manager.lock().await;
    poll_fn(|cx| {
        assert!(owner.as_mut().poll(cx).is_pending());
        Poll::Ready(())
    })
    .await;
    let second = manager.lock();
    tokio::pin!(second);
    poll_fn(|cx| {
        assert!(second.as_mut().poll(cx).is_pending());
        Poll::Ready(())
    })
    .await;
    drop(first);
    poll_fn(|cx| {
        assert!(owner.as_mut().poll(cx).is_pending());
        Poll::Ready(())
    })
    .await;
    let second = second.await;
    let third = manager.lock();
    tokio::pin!(third);
    poll_fn(|cx| {
        assert!(third.as_mut().poll(cx).is_pending());
        Poll::Ready(())
    })
    .await;
    drop(second);
    poll_fn(|cx| {
        assert!(owner.as_mut().poll(cx).is_pending());
        Poll::Ready(())
    })
    .await;
    let mut third = third.await;
    assert!(
        task.path_requests
            .pending(
                destination,
                lxmf::message::DeliveryTiming::from_round_trip(0.0, None, 0.0).expect("timing")
            )
            .expect("claim"),
        "request must be claimed before the blocked handoff"
    );
    tokio::time::advance(Duration::from_secs(10)).await;
    assert!(channel.tx_channel.try_recv().is_err(), "handoff is still blocked");
    assert!(third.set_announce_pacing(*channel.address(), 100, 100));
    drop(third);
    tokio::select! {
        _ = &mut owner => panic!("owner must retain delivery"),
        frame = channel.tx_channel.recv() => {
            assert_ne!(frame.expect("request").packet.destination, destination);
        },
    }
    // Let transport maintenance finish any handler-lock work made ready by
    // advance(), then finish the owner's post-handoff timing read at this time.
    let handoff_time = tokio::time::Instant::now();
    for _ in 0..64 {
        poll_fn(|cx| {
            assert!(owner.as_mut().poll(cx).is_pending());
            Poll::Ready(())
        })
        .await;
        tokio::task::yield_now().await;
    }
    assert_eq!(tokio::time::Instant::now(), handoff_time);
    tokio::time::advance(Duration::from_secs(85)).await;
    for _ in 0..64 {
        poll_fn(|cx| {
            assert!(owner.as_mut().poll(cx).is_pending());
            Poll::Ready(())
        })
        .await;
        tokio::task::yield_now().await;
    }
    assert!(channel.tx_channel.try_recv().is_err(), "blocked time must not consume discovery wait");
    tokio::time::advance(Duration::from_secs(1)).await;
    for _ in 0..64 {
        poll_fn(|cx| {
            assert!(owner.as_mut().poll(cx).is_pending());
            Poll::Ready(())
        })
        .await;
        tokio::task::yield_now().await;
    }
    assert!(channel.tx_channel.try_recv().is_err(), "deadline equality is not due");
    tokio::time::advance(Duration::from_nanos(1)).await;
    tokio::select! {
        _ = &mut owner => panic!("packet handoff must retain delivery"),
        frame = channel.tx_channel.recv() => {
            assert_eq!(frame.expect("dispatch after full wait").packet.destination == destination, !after_packet);
        },
        _ = tokio::time::sleep(Duration::from_millis(10)) => panic!("first attempt due"),
    }
    if after_packet {
        // A premature retry can be suppressed by request debounce while still
        // consuming an attempt. Observe the next allowed request as well: the
        // old pre-handoff deadline exhausted the owner one window too early.
        for _ in 0..64 {
            poll_fn(|cx| {
                assert!(owner.as_mut().poll(cx).is_pending());
                Poll::Ready(())
            })
            .await;
            tokio::task::yield_now().await;
        }
        tokio::time::advance(Duration::from_secs(86) + Duration::from_nanos(1)).await;
        tokio::select! {
            _ = &mut owner => panic!("blocked handoff must not consume an extra attempt"),
            frame = channel.tx_channel.recv() => {
                assert_ne!(frame.expect("last allowed request").packet.destination, destination);
            },
            _ = tokio::time::sleep(Duration::from_millis(10)) => panic!("last request due"),
        }
    }
    stop.send_replace(true);
    owner.await;
}
