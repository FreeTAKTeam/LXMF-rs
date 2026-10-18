use super::*;
use std::future::Future;

#[tokio::test]
async fn native_error_before_packet_observation_is_not_silent_absence() {
    assert_registration_race(false).await;
}

#[tokio::test]
async fn unavailable_status_during_native_packet_registration_waits_without_failure() {
    assert_registration_race(true).await;
}

async fn assert_registration_race(status_failure: bool) {
    let (mut task, link, _receipts) =
        crate::bridge::delivery_task_tests::task_with_active_link().await;
    let directory = tempfile::tempdir().expect("database directory");
    let path = directory.path().join("registration.sqlite");
    let store = rns_rpc::MessagesStore::open(&path).expect("message store");
    store
        .insert_message(&rns_rpc::MessageRecord {
            id: task.message_id.clone(),
            source: "source".into(),
            destination: task.destination_hex.clone(),
            title: String::new(),
            content: String::new(),
            timestamp: 0,
            direction: "out".into(),
            fields: None,
            receipt_status: Some("queued".into()),
        })
        .expect("admitted message");
    task.daemon = Arc::new(rns_rpc::RpcDaemon::with_store(store, "registration race".into()));
    let connection = rusqlite::Connection::open(path).expect("fault connection");
    let mut channel = task
        .transport
        .iface_manager()
        .lock()
        .await
        .new_channel_with_role(1, rns_transport::iface::IfaceRole::Unicast);
    link.lock().await.set_ingress_iface(*channel.address());
    let first = link.lock().await.data_packet(b"first unrelated packet").expect("filler");
    task.transport.send_link_packet_on_bound_iface(&link, first).await;
    let second = link.lock().await.data_packet(b"second unrelated packet").expect("blocker");
    let transport = task.transport.clone();
    let blocked_link = link.clone();
    let mut blocker = Box::pin(transport.send_link_packet_on_bound_iface(&blocked_link, second));
    std::future::poll_fn(|cx| {
        assert!(blocker.as_mut().poll(cx).is_pending(), "native handler held at full interface");
        std::task::Poll::Ready(())
    })
    .await;
    let context = Context {
        link: link.clone(),
        peer: task.destination_hex.clone(),
        method: Method::Direct,
        identify: false,
        signal: None,
    };
    let mut packets = Vec::new();
    let (_stop, mut stopping) = watch::channel(false);
    let mut attempt =
        Box::pin(dispatch(&task, &context, b"owned packet", &mut packets, &mut stopping));
    std::future::poll_fn(|cx| {
        assert!(
            attempt.as_mut().poll(cx).is_pending(),
            "packet captured before native registration waits"
        );
        std::task::Poll::Ready(())
    })
    .await;
    if status_failure {
        connection
            .execute_batch(
                "ALTER TABLE messages RENAME COLUMN receipt_status TO unavailable_receipt_status",
            )
            .expect("receipt observation fault");
    } else {
        link.lock().await.close();
    }
    channel.tx_channel.recv().await.expect("release unrelated interface handoff");
    blocker.await;
    let result = attempt.await;
    if status_failure {
        assert!(
            matches!(result, Ok(None)),
            "status unavailability retains the owner instead of authorizing failure/fallback"
        );
    } else {
        assert!(result.is_err(), "native Closed-generation error must reach the retained owner");
    }
    assert!(packets.is_empty());
    assert!(!task.receipt_tx.terminal_pending(&task.message_id).expect("no competing publication"));
    assert!(task.receipt_map.lock().expect("correlations").is_empty());
    channel.tx_channel.recv().await.expect("second unrelated packet");
    assert!(channel.tx_channel.try_recv().is_err(), "the owned packet was never handed off");
    if status_failure {
        connection
            .execute_batch(
                "ALTER TABLE messages RENAME COLUMN unavailable_receipt_status TO receipt_status",
            )
            .expect("observation recovery");
        assert_eq!(
            task.daemon
                .message_receipt_status(&task.message_id)
                .expect("unchanged status")
                .as_deref(),
            Some("queued")
        );
        let resumed = dispatch(&task, &context, b"owned packet", &mut packets, &mut stopping)
            .await
            .expect("recovered handoff");
        assert!(matches!(resumed, Some(Transfer::Packet { .. })));
        assert_eq!(packets.len(), 1);
        channel.tx_channel.recv().await.expect("single resumed packet");
        assert!(channel.tx_channel.try_recv().is_err());
    }
}
