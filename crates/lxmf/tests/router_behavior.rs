use lxmf::message::{Payload, WireMessage};
use lxmf::reticulum::Adapter;
use lxmf::router::{OutboundStatus, TransferPhase};
use lxmf::ticket::Ticket;

fn make_message(destination: [u8; 16], source: [u8; 16]) -> WireMessage {
    let payload = Payload::new(1_700_000_000.0, Some(b"test".to_vec()), None, None, None);
    WireMessage::new(destination, source, payload)
}

#[test]
fn handle_outbound_enforces_auth_and_ignore_policy() {
    let adapter = Adapter::with_outbound_sender(|_message| Ok(()));
    let mut router = lxmf::router::Router::with_adapter(adapter);
    router.set_auth_required(true);

    let destination = [0xAA; 16];
    let source = [0xBB; 16];

    router.enqueue_outbound(make_message(destination, source));
    let rejected = router.handle_outbound(1).unwrap();
    assert_eq!(rejected.len(), 1);
    assert_eq!(rejected[0].status, OutboundStatus::RejectedAuth);
    assert_eq!(router.stats().outbound_rejected_auth_total, 1);

    router.allow_destination(destination);
    router.enqueue_outbound(make_message(destination, source));
    let sent = router.handle_outbound(1).unwrap();
    assert_eq!(sent[0].status, OutboundStatus::Sent);
    assert_eq!(router.stats().outbound_processed_total, 1);

    router.ignore_destination(destination);
    router.enqueue_outbound(make_message(destination, source));
    let ignored = router.handle_outbound(1).unwrap();
    assert_eq!(ignored[0].status, OutboundStatus::Ignored);
    assert_eq!(router.stats().outbound_ignored_total, 1);
}

#[test]
fn prioritised_destination_moves_to_queue_front() {
    let mut router = lxmf::router::Router::default();
    let low = make_message([0x10; 16], [0x01; 16]);
    let high = make_message([0x20; 16], [0x01; 16]);

    router.prioritise_destination(high.destination);
    router.enqueue_outbound(low);
    router.enqueue_outbound(high);

    let dequeued = router.dequeue_outbound().expect("priority message");
    assert_eq!(dequeued.destination, [0x20; 16]);
}

#[test]
fn cancel_outbound_removes_message() {
    let mut router = lxmf::router::Router::default();
    let msg = make_message([0x31; 16], [0x41; 16]);
    let message_id = msg.message_id().to_vec();
    router.enqueue_outbound(msg);
    assert_eq!(router.outbound_len(), 1);

    assert!(router.cancel_outbound(&message_id));
    assert_eq!(router.outbound_len(), 0);
}

#[test]
fn propagation_transfer_lifecycle_is_tracked_and_pruned() {
    let mut router = lxmf::router::Router::default();
    let transient_id = b"transfer-01".to_vec();

    let requested = router.request_propagation_transfer(transient_id.clone());
    assert_eq!(requested.phase, TransferPhase::Requested);

    assert!(router.update_propagation_transfer_progress(&transient_id, 33));
    let in_progress = router.propagation_transfer_state(&transient_id).unwrap();
    assert_eq!(in_progress.phase, TransferPhase::InProgress);
    assert_eq!(in_progress.progress, 33);

    assert!(router.complete_propagation_transfer(&transient_id));
    let completed = router.propagation_transfer_state(&transient_id).unwrap();
    assert_eq!(completed.phase, TransferPhase::Completed);
    assert_eq!(completed.progress, 100);

    let ttl = router.config().transfer_state_ttl_secs;
    router.jobs_at(completed.updated_at + ttl + 1);
    assert!(router.propagation_transfer_state(&transient_id).is_none());
}

#[test]
fn jobs_prune_stale_inflight_transfers() {
    let mut router = lxmf::router::Router::default();
    let transient_id = b"transfer-stale".to_vec();

    let requested = router.request_propagation_transfer(transient_id.clone());
    assert_eq!(requested.phase, TransferPhase::Requested);
    assert!(router.update_propagation_transfer_progress(&transient_id, 25));

    let in_progress = router.propagation_transfer_state(&transient_id).expect("in progress state");
    assert_eq!(in_progress.phase, TransferPhase::InProgress);

    let ttl = router.config().transfer_state_ttl_secs;
    router.jobs_at(in_progress.updated_at + ttl + 1);
    assert!(router.propagation_transfer_state(&transient_id).is_none());
}

#[test]
fn jobs_prune_expired_tickets() {
    let mut router = lxmf::router::Router::default();
    let destination = [0x77; 16];
    let ticket = Ticket::new(100.0, vec![0x55; 16]);
    router.cache_ticket(destination, ticket);
    assert!(router.ticket_for(&destination).is_some());

    router.jobs_at(1_000_000);
    assert!(router.ticket_for(&destination).is_none());
}
