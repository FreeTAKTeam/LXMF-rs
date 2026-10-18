use super::*;

fn custody_event(id: &str) -> ReceiptEvent {
    ReceiptEvent::propagation_custody(id, rns_rpc::broker::PropagationCustody::Resource)
}

#[tokio::test]
async fn paired_admission_rolls_back_and_custody_retirement_does_not_exhaust_queue() {
    let (sender, _receiver) = tokio::sync::mpsc::channel(1);
    let publisher = ReceiptPublisher::from(sender);
    assert!(publisher.reserve_completion("pair", true).is_err());
    assert_eq!(publisher.sender.capacity(), 1);
    assert!(publisher.reservations.lock().unwrap().is_empty());
    publisher.reserve_terminal("existing").unwrap();
    assert!(publisher.reserve_completion("existing", true).is_err());
    assert_eq!(publisher.sender.capacity(), 0, "failed upgrade preserves existing reservation");
    publisher.release_unsent("existing");
    assert_eq!(publisher.sender.capacity(), 1);

    let (sender, mut receiver) = tokio::sync::mpsc::channel(2);
    let publisher = ReceiptPublisher::from(sender);
    for sequence in 0..20 {
        let id = sequence.to_string();
        publisher.reserve_completion(&id, true).unwrap();
        publisher.reserve_completion(&id, true).unwrap();
        let mut completion = publisher.publish_custody(custody_event(&id)).unwrap();
        assert_eq!(completion.current(), None, "enqueue is not commit");
        receiver.recv().await.unwrap();
        publisher.complete_custody(&id, CustodyCommit::Stored);
        assert_eq!(completion.wait().await.unwrap(), CustodyCommit::Stored);
        assert_eq!(
            completion.wait().await.unwrap(),
            CustodyCommit::Stored,
            "final outcome retained"
        );
        assert_eq!(publisher.sender.capacity(), 2);
        assert!(publisher.reservations.lock().unwrap().is_empty());
    }
}

#[tokio::test]
async fn paired_outcomes_survive_saturation_duplicates_and_both_commit_orders() {
    for terminal_first in [false, true] {
        let (sender, mut receiver) = tokio::sync::mpsc::channel(3);
        let publisher = ReceiptPublisher::from(sender);
        publisher.reserve_completion("message", true).unwrap();
        publisher.try_send(ReceiptEvent::new("progress", "sending")).unwrap();
        assert!(publisher.try_send(ReceiptEvent::new("progress", "sending")).is_err());
        let mut completion = publisher.publish_custody(custody_event("message")).unwrap();
        let mut duplicate = publisher.publish_custody(custody_event("message")).unwrap();
        publisher.try_send(ReceiptEvent::new("message", "delivered")).unwrap();
        for _ in 0..3 {
            receiver.recv().await.unwrap();
        }
        assert!(receiver.try_recv().is_err());
        assert!(publisher.terminal_pending("message").unwrap());
        let expected = if terminal_first {
            CustodyCommit::SupersededByTerminal
        } else {
            CustodyCommit::Stored
        };
        if terminal_first {
            publisher.complete_terminal("message");
            assert_eq!(
                completion.current(),
                None,
                "terminal acknowledgement cannot erase pending custody"
            );
        }
        publisher.complete_custody("message", expected);
        assert_eq!(completion.wait().await.unwrap(), expected);
        assert_eq!(duplicate.wait().await.unwrap(), expected);
        if !terminal_first {
            assert!(
                publisher.terminal_pending("message").unwrap(),
                "custody cannot acknowledge accepted terminal work"
            );
            publisher.complete_terminal("message");
        }
        assert!(publisher.reservations.lock().unwrap().is_empty());
    }
}

#[tokio::test]
async fn custody_is_typed_and_late_terminal_uses_unreserved_admission() {
    let (sender, mut receiver) = tokio::sync::mpsc::channel(3);
    let publisher = ReceiptPublisher::from(sender);
    publisher.reserve_completion("message", true).unwrap();
    assert!(publisher
        .publish_custody(ReceiptEvent::new("message", "sent: propagated resource"))
        .is_err());
    let mut event = custody_event("message");
    event.status = "delivered".into();
    let mut completion = publisher.publish_custody(event).unwrap();
    assert_eq!(receiver.recv().await.unwrap().status, "sent: propagated resource");
    publisher.complete_custody("message", CustodyCommit::Stored);
    assert_eq!(completion.wait().await.unwrap(), CustodyCommit::Stored);
    publisher.try_send(ReceiptEvent::new("message", "delivered")).unwrap();
    assert!(publisher.terminal_pending("message").unwrap());
    receiver.recv().await.unwrap();
    publisher.complete_terminal("message");
    assert!(publisher.reservations.lock().unwrap().is_empty());
}

#[tokio::test]
async fn shutdown_preserves_accepted_custody_and_owner_failure_wakes_observers() {
    let (sender, mut receiver) = tokio::sync::mpsc::channel(4);
    let publisher = ReceiptPublisher::from(sender);
    publisher.reserve_completion("pending", true).unwrap();
    publisher.reserve_completion("unused", true).unwrap();
    let mut completion = publisher.publish_custody(custody_event("pending")).unwrap();
    publisher.stop_admission();
    publisher.release_unobserved();
    assert!(publisher.reserve_completion("new", true).is_err());
    assert!(publisher.publish_custody(custody_event("new")).is_err());
    receiver.recv().await.unwrap();
    assert_eq!(completion.current(), None);
    publisher.fail_owner();
    assert!(completion.wait().await.is_err());
    assert!(publisher.publish_custody(custody_event("pending")).is_err());
}
#[tokio::test]
async fn reserved_terminal_survives_full_progress_queue_and_duplicate_callback() {
    let (sender, mut receiver) = tokio::sync::mpsc::channel(2);
    let publisher = ReceiptPublisher::from(sender);
    publisher.reserve_terminal("durable").unwrap();
    publisher.try_send(ReceiptEvent::new("legacy", "sending")).unwrap();
    assert!(publisher.try_send(ReceiptEvent::new("legacy", "sending")).is_err());
    publisher.try_send(ReceiptEvent::new("durable", "delivered")).unwrap();
    publisher.try_send(ReceiptEvent::new("durable", "delivered")).unwrap();
    assert_eq!(receiver.recv().await.unwrap().message_id, "legacy");
    assert_eq!(receiver.recv().await.unwrap().message_id, "durable");
    assert!(receiver.try_recv().is_err());
    publisher.complete_terminal("durable");
    publisher.stop_admission();
    assert!(publisher.reserve_terminal("next").is_err());
}
#[tokio::test]
async fn terminal_pending_tracks_accepted_work_and_commit_races() {
    for reserved in [false, true] {
        let (sender, mut receiver) = tokio::sync::mpsc::channel(2);
        let publisher = ReceiptPublisher::from(sender);
        if reserved {
            publisher.reserve_terminal("message").unwrap();
        }
        assert!(!publisher.terminal_pending("message").unwrap());
        let mut completion = publisher.terminal_completion_events();
        publisher.try_send(ReceiptEvent::new("message", "delivered")).unwrap();
        assert!(publisher.terminal_pending("message").unwrap());
        publisher.try_send(ReceiptEvent::new("message", "failed: duplicate")).unwrap();
        receiver.recv().await.unwrap();
        assert!(receiver.try_recv().is_err());
        // Completion after query but before waiting remains visible.
        publisher.complete_terminal("message");
        assert!(completion.has_changed().unwrap());
        completion.changed().await.unwrap();
        assert!(!publisher.terminal_pending("message").unwrap());
        // Completion before subscribing requires no notification to finish.
        let _late = publisher.terminal_completion_events();
        assert!(!publisher.terminal_pending("message").unwrap());
    }
}
#[tokio::test]
async fn refused_legacy_terminal_does_not_create_pending_work() {
    let (sender, mut receiver) = tokio::sync::mpsc::channel(1);
    let publisher = ReceiptPublisher::from(sender);
    publisher.try_send(ReceiptEvent::new("progress", "sending")).unwrap();
    assert!(publisher.try_send(ReceiptEvent::new("legacy", "failed")).is_err());
    assert!(!publisher.terminal_pending("legacy").unwrap());
    receiver.recv().await.unwrap();
    publisher.try_send(ReceiptEvent::new("legacy", "failed")).unwrap();
    assert!(publisher.terminal_pending("legacy").unwrap());
}
