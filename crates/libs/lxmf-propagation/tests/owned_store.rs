use lxmf_core::{message::Payload, WireMessage};
use lxmf_propagation::{
    prepared::delivery_address, Error, MessageKind, OwnedStore, PreparedMessage, State, StoreLimits,
};
use rns_core::identity::PrivateIdentity;
use tempfile::TempDir;

fn identity(name: &str) -> PrivateIdentity {
    PrivateIdentity::new_from_name(name)
}
fn wire(sender: &PrivateIdentity, receiver: &PrivateIdentity, sequence: u8) -> WireMessage {
    let mut w = WireMessage::new(
        delivery_address(receiver.as_identity()),
        delivery_address(sender.as_identity()),
        Payload::new(sequence as f64, Some(vec![sequence; 32]), None, None, None),
    );
    w.sign(sender).unwrap();
    w
}
fn prepare(sender: &PrivateIdentity, receiver: &PrivateIdentity, sequence: u8) -> PreparedMessage {
    PreparedMessage::new(&wire(sender, receiver, sequence), sender, Some(receiver.as_identity()))
        .unwrap()
}
fn open(dir: &TempDir, sender: &PrivateIdentity, capacity: u64) -> OwnedStore {
    OwnedStore::open(
        dir.path().join("outbox.db"),
        sender.clone(),
        StoreLimits { capacity_bytes: capacity, retention_secs: 100 },
    )
    .unwrap()
}

#[test]
fn admits_only_signed_local_messages_with_validated_recipient() {
    let a = identity("a");
    let b = identity("b");
    let c = identity("c");
    let w = wire(&a, &b, 1);
    assert!(matches!(PreparedMessage::new(&w, &a, None), Err(Error::UnresolvedRecipient)));
    assert!(matches!(
        PreparedMessage::new(&w, &a, Some(c.as_identity())),
        Err(Error::UnresolvedRecipient)
    ));
    assert!(matches!(
        PreparedMessage::new(&w, &c, Some(b.as_identity())),
        Err(Error::ForeignMessage)
    ));
    let dir = TempDir::new().unwrap();
    let mut store = open(&dir, &c, 10000);
    assert!(matches!(
        store.deposit_own(prepare(&a, &b, 1), MessageKind::Chat, 10, None),
        Err(Error::ForeignMessage)
    ));
    assert!(store.status(10).unwrap().is_empty());
}

#[test]
fn duplicate_preparation_keeps_original_transient_and_deadline() {
    let a = identity("a");
    let b = identity("b");
    let dir = TempDir::new().unwrap();
    let mut store = open(&dir, &a, 10000);
    let first = store.deposit_own(prepare(&a, &b, 1), MessageKind::Chat, 10, None).unwrap();
    let duplicate = store.deposit_own(prepare(&a, &b, 1), MessageKind::Chat, 80, None).unwrap();
    assert_eq!(first, duplicate);
    assert_eq!(first.expires_at, 110);
    assert_eq!(store.status(110).unwrap()[0].state, State::Expired);
    assert_eq!(store.status(110).unwrap()[0].encrypted_bytes, 0);
}

#[test]
fn recovery_survives_restart_and_preserves_original_signed_wire() {
    let a = identity("a");
    let b = identity("b");
    let dir = TempDir::new().unwrap();
    let mut store = open(&dir, &a, 10000);
    let first = store.deposit_own(prepare(&a, &b, 1), MessageKind::Mission, 10, None).unwrap();
    let original = wire(&a, &b, 1).pack().unwrap();
    drop(store);
    let mut store = open(&dir, &a, 10000);
    assert!(store.recover(&first.transient_id, 11, 41, |_| false).unwrap().is_none());
    let recovery = store.recover(&first.transient_id, 11, 41, |_| true).unwrap().unwrap();
    assert_eq!(recovery.direct_wire, original);
    assert_eq!(&recovery.recipient_transient[..16], delivery_address(b.as_identity()));
    assert!(matches!(
        store.recover(&first.transient_id, 11, 41, |_| true),
        Err(Error::InvalidState(_))
    ));
    store.finish_recovery(&recovery.lease);
    assert!(store.recover(&first.transient_id, 12, 42, |_| true).unwrap().is_none());
    assert!(!store.delivered(&first.transient_id, "wrong", false, 12).unwrap());
    assert!(store.delivered(&first.transient_id, &first.message_id, false, 12).unwrap());
    let statuses = store.status(12).unwrap();
    assert_eq!(statuses[0].state, State::Delivered);
    assert_eq!(statuses[0].encrypted_bytes, 0);
}

#[test]
fn relay_upload_does_not_retire_and_is_not_repeated_after_restart() {
    let a = identity("a");
    let b = identity("b");
    let dir = TempDir::new().unwrap();
    let mut store = open(&dir, &a, 10000);
    let s = store.deposit_own(prepare(&a, &b, 1), MessageKind::Chat, 10, None).unwrap();
    let relay = "11".repeat(16);
    let recovery = store.recover(&s.transient_id, 11, 41, |_| true).unwrap().unwrap();
    store.begin_upload(&recovery.lease, &relay, 11).unwrap();
    store.uploaded_unverified(&recovery.lease, 12).unwrap();
    drop(store);
    let mut store = open(&dir, &a, 10000);
    let after = store.status(42).unwrap().remove(0);
    assert_eq!(after.state, State::UploadedUnverified);
    assert_eq!(after.encrypted_bytes, s.encrypted_bytes);
    assert_eq!(after.expires_at, s.expires_at);
    let recovery = store.recover(&s.transient_id, 42, 72, |_| true).unwrap().unwrap();
    assert!(matches!(store.begin_upload(&recovery.lease, &relay, 42), Err(Error::InvalidState(_))));
    assert!(store.delivered(&s.transient_id, &s.message_id, true, 43).unwrap());
    assert_eq!(store.status(43).unwrap()[0].state, State::Acknowledged);
}

#[test]
fn telemetry_supersession_is_per_recipient_and_never_accepts_stale_ack() {
    let a = identity("a");
    let b = identity("b");
    let c = identity("c");
    let dir = TempDir::new().unwrap();
    let mut store = open(&dir, &a, 10000);
    let kind = MessageKind::Telemetry { key: "latest-fix".into() };
    let old = store.deposit_own(prepare(&a, &b, 1), kind.clone(), 10, None).unwrap();
    let other = store.deposit_own(prepare(&a, &c, 2), kind.clone(), 11, None).unwrap();
    let latest = store.deposit_own(prepare(&a, &b, 3), kind, 12, Some(25)).unwrap();
    let statuses = store.status(12).unwrap();
    assert_eq!(statuses[0].state, State::Evicted);
    assert_eq!(statuses[1], other);
    assert!(!store.delivered(&old.transient_id, &old.message_id, true, 13).unwrap());
    assert!(!store.delivered(&latest.transient_id, &old.message_id, true, 13).unwrap());
    assert_eq!(store.status(25).unwrap().last().unwrap().state, State::Expired);
}

#[test]
fn overflow_prefers_telemetry_and_dominant_recipient_before_old_chat() {
    let a = identity("a");
    let b = identity("b");
    let c = identity("c");
    let d = identity("d");
    let dir = TempDir::new().unwrap();
    let size = prepare(&a, &b, 1).encrypted_bytes() as u64;
    let mut store = open(&dir, &a, size * 3);
    let old = store.deposit_own(prepare(&a, &b, 1), MessageKind::Chat, 10, None).unwrap();
    let dominant = store.deposit_own(prepare(&a, &c, 2), MessageKind::Chat, 11, None).unwrap();
    store.deposit_own(prepare(&a, &c, 3), MessageKind::Chat, 12, None).unwrap();
    store.deposit_own(prepare(&a, &d, 4), MessageKind::Chat, 13, None).unwrap();
    let statuses = store.status(13).unwrap();
    assert_eq!(statuses[0], old);
    assert_eq!(statuses[1].transient_id, dominant.transient_id);
    assert_eq!(statuses[1].state, State::Evicted);
    store
        .deposit_own(prepare(&a, &d, 5), MessageKind::Telemetry { key: "fix".into() }, 14, None)
        .unwrap();
    let chat = store.deposit_own(prepare(&a, &d, 6), MessageKind::Mission, 15, None).unwrap();
    let statuses = store.status(15).unwrap();
    assert!(statuses.iter().any(|s| s.admitted_at == 14 && s.state == State::Evicted));
    assert!(
        statuses.iter().filter(|s| !s.state.terminal()).map(|s| s.encrypted_bytes).sum::<u64>()
            <= size * 3
    );
    assert!(statuses.contains(&chat));
}

#[test]
fn oversized_and_expired_admissions_are_typed_and_do_not_evict() {
    let a = identity("a");
    let b = identity("b");
    let dir = TempDir::new().unwrap();
    let mut store = open(&dir, &a, 100);
    assert!(matches!(
        store.deposit_own(prepare(&a, &b, 1), MessageKind::Chat, 10, None),
        Err(Error::PayloadTooLarge)
    ));
    assert!(matches!(
        store.deposit_own(prepare(&a, &b, 2), MessageKind::Chat, 10, Some(10)),
        Err(Error::Expired)
    ));
    assert!(store.status(10).unwrap().is_empty());
}

#[test]
fn retirement_is_atomic_if_storage_rejects_after_transport_completion() {
    let a = identity("a");
    let b = identity("b");
    let dir = TempDir::new().unwrap();
    let mut store = open(&dir, &a, 10000);
    let first = store.deposit_own(prepare(&a, &b, 1), MessageKind::Chat, 10, None).unwrap();
    let conn = rusqlite::Connection::open(dir.path().join("outbox.db")).unwrap();
    conn.execute_batch(
        "CREATE TRIGGER reject_retirement BEFORE UPDATE OF recovery ON propagation_owned
        BEGIN SELECT RAISE(ABORT,'injected storage rejection'); END;",
    )
    .unwrap();
    assert!(store.delivered(&first.transient_id, &first.message_id, false, 11).is_err());
    assert_eq!(store.status(11).unwrap()[0], first);
    drop(conn);
    drop(store);
    let mut store = open(&dir, &a, 10000);
    assert!(store.recover(&first.transient_id, 12, 42, |_| true).unwrap().is_some());
}

#[test]
fn stopping_preserves_pending_and_bounds_database_and_journal() {
    let a = identity("a");
    let b = identity("b");
    let dir = TempDir::new().unwrap();
    let mut store = open(&dir, &a, 10000);
    let first = store.deposit_own(prepare(&a, &b, 1), MessageKind::Chat, 10, None).unwrap();
    store.stop();
    assert!(matches!(store.recover(&first.transient_id, 11, 41, |_| true), Err(Error::Stopped)));
    drop(store);
    let mut store = open(&dir, &a, 10000);
    assert_eq!(store.status(11).unwrap()[0], first);
    let conn = rusqlite::Connection::open(dir.path().join("outbox.db")).unwrap();
    let journal: String = conn.pragma_query_value(None, "journal_mode", |r| r.get(0)).unwrap();
    assert_eq!(journal, "delete");
    assert!(
        std::fs::metadata(dir.path().join("outbox.db")).unwrap().len()
            <= 10000 * 3 + 8 * 1024 * 1024
    );
    assert!(!dir.path().join("outbox.db-wal").exists());
    assert!(!dir.path().join("outbox.db-journal").exists());
    assert!(matches!(
        OwnedStore::open(dir.path().join("outbox.db"), b, StoreLimits::default()),
        Err(Error::ForeignMessage)
    ));
}

#[test]
fn interrupted_upload_is_uncertain_after_restart_and_keeps_payload() {
    let a = identity("a");
    let b = identity("b");
    let dir = TempDir::new().unwrap();
    let mut store = open(&dir, &a, 10000);
    let s = store.deposit_own(prepare(&a, &b, 1), MessageKind::Chat, 10, None).unwrap();
    let relay = "11".repeat(16);
    let recovery = store.recover(&s.transient_id, 11, 41, |_| true).unwrap().unwrap();
    store.begin_upload(&recovery.lease, &relay, 11).unwrap();
    drop(store);
    let mut store = open(&dir, &a, 10000);
    let after = store.status(42).unwrap().remove(0);
    assert_eq!(after.state, State::UploadedUnverified);
    assert_eq!(after.encrypted_bytes, s.encrypted_bytes);
    let next = store.recover(&s.transient_id, 42, 72, |_| true).unwrap().unwrap();
    assert!(store.begin_upload(&next.lease, &relay, 42).is_err());
    assert_eq!(next.direct_wire, wire(&a, &b, 1).pack().unwrap());
}

#[test]
fn terminal_projection_acknowledgement_keeps_deduplication_through_retention() {
    let a = identity("a");
    let b = identity("b");
    let dir = TempDir::new().unwrap();
    let mut store = open(&dir, &a, 10000);
    let s = store.deposit_own(prepare(&a, &b, 1), MessageKind::Chat, 10, None).unwrap();
    store.delivered(&s.transient_id, &s.message_id, true, 11).unwrap();
    store.acknowledge_status(&s.transient_id).unwrap();
    assert!(store.status(12).unwrap().is_empty());
    drop(store);
    let mut store = open(&dir, &a, 10000);
    let replay = store.deposit_own(prepare(&a, &b, 1), MessageKind::Chat, 50, None).unwrap();
    assert_eq!(replay.state, State::Acknowledged);
    assert_eq!(replay.transient_id, s.transient_id);
    assert_eq!(replay.expires_at, s.expires_at);
    assert!(store.recover(&s.transient_id, 51, 81, |_| true).unwrap().is_none());
}

#[test]
fn stale_completion_cannot_release_another_attempts_transfer_lease() {
    let a = identity("a");
    let b = identity("b");
    let dir = TempDir::new().unwrap();
    let mut store = open(&dir, &a, 10000);
    let s = store.deposit_own(prepare(&a, &b, 1), MessageKind::Chat, 10, None).unwrap();
    let first = store.recover(&s.transient_id, 11, 41, |_| true).unwrap().unwrap();
    store.begin_upload(&first.lease, &"11".repeat(16), 11).unwrap();
    store.finish_recovery(&first.lease);
    let second = store.recover(&s.transient_id, 42, 72, |_| true).unwrap().unwrap();
    assert!(store.uploaded_unverified(&first.lease, 43).is_err());
    store.finish_recovery(&first.lease);
    assert!(store.recover(&s.transient_id, 43, 73, |_| true).is_err());
    store.finish_recovery(&second.lease);
}

#[test]
fn late_duplicate_and_late_proof_expire_without_needing_a_status_poll() {
    for poll in [false, true] {
        let a = identity("a");
        let b = identity("b");
        let dir = TempDir::new().unwrap();
        let mut store = open(&dir, &a, 10000);
        let s = store.deposit_own(prepare(&a, &b, 1), MessageKind::Chat, 10, Some(20)).unwrap();
        if poll {
            store.status(21).unwrap();
        }
        assert!(!store.delivered(&s.transient_id, &s.message_id, true, 21).unwrap());
        assert_eq!(
            store.deposit_own(prepare(&a, &b, 1), MessageKind::Chat, 22, None).unwrap().state,
            State::Expired
        );
        assert_eq!(store.status(22).unwrap()[0].encrypted_bytes, 0);
    }
}

#[test]
fn later_application_ack_becomes_visible_after_transport_delivery_was_projected() {
    let a = identity("a");
    let b = identity("b");
    let dir = TempDir::new().unwrap();
    let mut store = open(&dir, &a, 10000);
    let s = store.deposit_own(prepare(&a, &b, 1), MessageKind::Chat, 10, None).unwrap();
    store.delivered(&s.transient_id, &s.message_id, false, 11).unwrap();
    store.acknowledge_status(&s.transient_id).unwrap();
    store.delivered(&s.transient_id, &s.message_id, false, 12).unwrap();
    assert!(store.status(12).unwrap().is_empty());
    store.delivered(&s.transient_id, &s.message_id, true, 13).unwrap();
    assert_eq!(store.status(13).unwrap()[0].state, State::Acknowledged);
}
