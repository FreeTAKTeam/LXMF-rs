use super::*;
use serde_json::json;
fn context() -> Context<'static> {
    Context { owner: "authenticated-principal", scope: "project-A" }
}
fn route() -> Route {
    Route { topic: "observations".into(), subject: Some("asset-1".into()) }
}
fn event(id: &str) -> EventEnvelope {
    EventEnvelope {
        version: 1,
        occurred_at: 1234,
        origin: "publisher-reported-origin".into(),
        correlation_id: Some("request-7".into()),
        event_id: id.into(),
        event_type: "observed".into(),
        route: route(),
        payload: json!({"value":42}),
    }
}
fn request(id: &str) -> PublishRequest {
    PublishRequest { operation_id: id.into(), event: event(id), completion_reserve: 4096 }
}
fn resume(id: &str) -> ResumeRequest {
    ResumeRequest {
        consumer_id: ConsumerId(id.into()),
        identity: "untrusted-ignored".into(),
        journal_id: None,
        stored: EventPosition(0),
    }
}
fn fetch(id: &str) -> FetchRequest {
    FetchRequest {
        consumer_id: ConsumerId(id.into()),
        identity: "untrusted-ignored".into(),
        max_events: 128,
        max_bytes: MAX_BATCH_BYTES,
    }
}
fn ack(batch: &BrokerBatch) -> AckStoredRequest {
    AckStoredRequest {
        consumer_id: batch.consumer_id.clone(),
        identity: "untrusted-ignored".into(),
        journal_id: batch.journal_id.clone(),
        end: batch.end,
        receipt: batch.receipt.clone(),
    }
}
fn open() -> (tempfile::TempDir, Broker) {
    let dir = tempfile::tempdir().unwrap();
    let broker = Broker::open(dir.path().join("broker.sqlite"), Config::default()).unwrap();
    (dir, broker)
}
fn usage(broker: &Broker) -> i64 {
    broker.connection.query_row("SELECT used_bytes FROM broker_meta", [], |row| row.get(0)).unwrap()
}

#[test]
fn publication_is_immutable_idempotent_and_survives_reopen() {
    let (dir, mut broker) = open();
    let request = request("operation-1");
    let receipt = broker.publish(context(), &request).unwrap();
    let used = usage(&broker);
    assert_eq!(broker.publish(context(), &request).unwrap(), receipt);
    assert_eq!(usage(&broker), used);
    let mut changed = request.clone();
    changed.event.origin = "different-origin".into();
    assert!(broker
        .publish(context(), &changed)
        .unwrap_err()
        .to_string()
        .contains("OPERATION_CONFLICT"));
    changed = request.clone();
    changed.operation_id = "different-operation".into();
    assert!(broker
        .publish(context(), &changed)
        .unwrap_err()
        .to_string()
        .contains("EVENT_CONFLICT"));
    drop(broker);
    let mut broker = Broker::open(dir.path().join("broker.sqlite"), Config::default()).unwrap();
    assert_eq!(broker.publish(context(), &request).unwrap(), receipt);
    assert_eq!(broker.reconcile(context(), "operation-1").unwrap(), Some(receipt));
    assert!(broker
        .reconcile(Context { owner: "other", ..context() }, "operation-1")
        .unwrap()
        .is_none());
    assert!(broker
        .reconcile(Context { scope: "other", ..context() }, "operation-1")
        .unwrap()
        .is_none());
}

#[test]
fn independent_consumers_replay_and_authenticate_topic_subject_scope_and_owner() {
    let (dir, mut broker) = open();
    for id in ["consumer-A", "consumer-B"] {
        broker.resume(context(), &route(), resume(id)).unwrap();
    }
    let request = request("event-1");
    broker.publish(context(), &request).unwrap();
    let mut other = request.clone();
    other.operation_id = "other-subject".into();
    other.event.event_id = "other-subject".into();
    other.event.route.subject = Some("asset-2".into());
    broker.publish(context(), &other).unwrap();
    let batch = broker.fetch(context(), &route(), fetch("consumer-A")).unwrap();
    assert_eq!(batch.events.len(), 1);
    assert_eq!(batch.events[0].payload, serde_json::to_value(&request.event).unwrap());
    drop(broker);
    let mut broker = Broker::open(dir.path().join("broker.sqlite"), Config::default()).unwrap();
    assert_eq!(
        serde_json::to_value(broker.fetch(context(), &route(), fetch("consumer-A")).unwrap())
            .unwrap(),
        serde_json::to_value(&batch).unwrap()
    );
    assert!(broker.ack(Context { owner: "wrong", ..context() }, &route(), ack(&batch)).is_err());
    assert!(broker.ack(Context { scope: "wrong", ..context() }, &route(), ack(&batch)).is_err());
    let mut wrong_route = route();
    wrong_route.topic = "wrong".into();
    assert!(broker.ack(context(), &wrong_route, ack(&batch)).is_err());
    wrong_route = route();
    wrong_route.subject = None;
    assert!(broker.ack(context(), &wrong_route, ack(&batch)).is_err());
    let mut forged = ack(&batch);
    forged.receipt.0.push('0');
    assert!(broker.ack(context(), &route(), forged).is_err());
    broker.ack(context(), &route(), ack(&batch)).unwrap();
    broker.ack(context(), &route(), ack(&batch)).unwrap();
    let independent = broker.fetch(context(), &route(), fetch("consumer-B")).unwrap();
    assert_eq!(independent.events.len(), 1);
    // Custody does not complete business processing.
    assert_eq!(
        broker.reconcile(context(), "event-1").unwrap().unwrap().state,
        ProcessingState::Queued
    );
}

#[test]
fn publisher_and_application_state_rollback_together_on_event_failure() {
    let (_dir, mut broker) = open();
    broker.connection.execute_batch("CREATE TABLE application_state(value TEXT); CREATE TRIGGER reject_event BEFORE INSERT ON broker_events BEGIN SELECT RAISE(ABORT,'injected event failure'); END;").unwrap();
    let initial = usage(&broker);
    let result = broker.transaction(|session| {
        session.connection.execute("INSERT INTO application_state VALUES('must roll back')", [])?;
        session.publish(context(), &request("atomic"))
    });
    assert!(result.is_err());
    assert_eq!(usage(&broker), initial);
    assert!(broker.reconcile(context(), "atomic").unwrap().is_none());
    assert_eq!(
        broker
            .connection
            .query_row("SELECT COUNT(*) FROM application_state", [], |row| row.get::<_, i64>(0))
            .unwrap(),
        0
    );
}

#[test]
fn application_owned_transaction_can_atomically_embed_generic_publication() {
    let dir = tempfile::tempdir().unwrap();
    let mut conn = Connection::open(dir.path().join("embedded.sqlite")).unwrap();
    storage::configure_profile(&conn, Config::default().budget_bytes).unwrap();
    conn.execute_batch("CREATE TABLE app(value TEXT)").unwrap();
    {
        let tx = conn.transaction().unwrap();
        let mut session = Session::new(&tx, Config::default()).unwrap();
        session.initialize().unwrap();
        tx.execute("INSERT INTO app VALUES('rollback')", []).unwrap();
        session.publish(context(), &request("rolled-back")).unwrap();
        // No commit: schema and application/event state all roll back.
    }
    assert!(!storage::enabled(&conn).unwrap());
    assert_eq!(conn.query_row("SELECT COUNT(*) FROM app", [], |r| r.get::<_, i64>(0)).unwrap(), 0);
    let tx = conn.transaction().unwrap();
    let mut session = Session::new(&tx, Config::default()).unwrap();
    session.initialize().unwrap();
    session.publish(context(), &request("committed")).unwrap();
    tx.commit().unwrap();
}

#[test]
fn expired_claims_require_explicit_recovery_and_stale_tokens_cannot_finish() {
    let (dir, mut broker) = open();
    broker.publish(context(), &request("work")).unwrap();
    let claim = broker.transaction(|s| s.claim(context(), "work", 100, 10)).unwrap().unwrap();
    assert!(broker.transaction(|s| s.claim(context(), "work", 101, 10)).unwrap().is_none());
    let reserved = usage(&broker);
    drop(broker);
    let mut broker = Broker::open(dir.path().join("broker.sqlite"), Config::default()).unwrap();
    assert_eq!(
        broker.reconcile(context(), "work").unwrap().unwrap().state,
        ProcessingState::Claimed
    );
    assert_eq!(broker.transaction(|s| s.expire_claims(context(), 109)).unwrap(), 0);
    assert_eq!(broker.transaction(|s| s.expire_claims(context(), 110)).unwrap(), 1);
    assert_eq!(
        broker.reconcile(context(), "work").unwrap().unwrap().state,
        ProcessingState::Unknown
    );
    assert!(broker
        .transaction(|s| s.complete(context(), &claim, 110, &event("completed")))
        .is_err());
    assert_eq!(usage(&broker), reserved);
    broker.transaction(|s| s.recover(context(), "work", Recovery::Requeue)).unwrap();
    let replacement = broker.transaction(|s| s.claim(context(), "work", 111, 10)).unwrap().unwrap();
    assert_ne!(replacement.token, claim.token);
    assert!(broker
        .transaction(|s| s.complete(context(), &claim, 112, &event("completed")))
        .is_err());
    assert_eq!(usage(&broker), reserved);
    let complete = broker
        .transaction(|s| s.complete(context(), &replacement, 112, &event("completed")))
        .unwrap();
    assert_eq!(complete.state, ProcessingState::Completed);
    let completed_used = usage(&broker);
    assert_eq!(
        broker
            .transaction(|s| s.complete(context(), &replacement, 999, &event("completed")))
            .unwrap(),
        complete
    );
    assert_eq!(usage(&broker), completed_used);
    assert!(broker
        .transaction(|s| s.complete(
            Context { owner: "other", ..context() },
            &replacement,
            112,
            &event("completed")
        ))
        .is_err());
    assert!(broker
        .transaction(|s| s.complete(context(), &replacement, 112, &event("different")))
        .is_err());
}

#[test]
fn completion_bounds_failure_and_control_headroom_preserve_reservations() {
    let (_dir, mut broker) = open();
    broker.publish(context(), &request("reserved")).unwrap();
    let claim = broker.transaction(|s| s.claim(context(), "reserved", 10, 10)).unwrap().unwrap();
    let mut oversized = event("completion");
    oversized.payload = json!("x".repeat(4096));
    let initial = usage(&broker);
    assert!(broker
        .transaction(|s| s.complete(context(), &claim, 11, &oversized))
        .unwrap_err()
        .to_string()
        .contains("COMPLETION_TOO_LARGE"));
    assert_eq!(usage(&broker), initial);
    // Exhaust ordinary logical admission without exhausting this operation's reserved space.
    broker.connection.execute("UPDATE broker_meta SET used_bytes=budget-512*1024", []).unwrap();
    assert!(broker.publish(context(), &request("cannot-admit")).is_err());
    let before = usage(&broker);
    assert!(broker.reconcile(context(), "cannot-admit").unwrap().is_none());
    broker.transaction(|s| s.complete(context(), &claim, 11, &event("completion"))).unwrap();
    assert!(usage(&broker) < before);
}

#[test]
fn completion_append_failure_rolls_back_state_and_reservation() {
    let (_dir, mut broker) = open();
    broker.publish(context(), &request("work")).unwrap();
    let claim = broker.transaction(|s| s.claim(context(), "work", 10, 10)).unwrap().unwrap();
    broker.connection.execute_batch("CREATE TRIGGER reject_completion BEFORE INSERT ON broker_events BEGIN SELECT RAISE(ABORT,'injected completion failure'); END;").unwrap();
    let used = usage(&broker);
    assert!(broker
        .transaction(|s| s.complete(context(), &claim, 11, &event("completion")))
        .is_err());
    assert_eq!(usage(&broker), used);
    assert_eq!(
        broker.reconcile(context(), "work").unwrap().unwrap().state,
        ProcessingState::Claimed
    );
    broker
        .transaction(|s| {
            s.expire_claims(context(), 20)?;
            s.recover(context(), "work", Recovery::Fail)
        })
        .unwrap();
    assert_eq!(
        broker.reconcile(context(), "work").unwrap().unwrap().state,
        ProcessingState::Failed
    );
    assert_eq!(usage(&broker), used - 4096);
}

#[test]
fn configurable_generic_limits_validate_envelope_without_lxmf_message_policy() {
    let dir = tempfile::tempdir().unwrap();
    let config = Config {
        max_payload_bytes: 32,
        limits: Limits { max_batch_events: 2, max_batch_bytes: 8192 },
        max_completion_bytes: 2048,
        ..Config::default()
    };
    let mut broker = Broker::open(dir.path().join("small.sqlite"), config).unwrap();
    let mut request = request("bounded");
    request.completion_reserve = 1024;
    let initial = usage(&broker);
    request.event.payload = json!("x".repeat(33));
    assert!(broker.publish(context(), &request).is_err());
    request.event.payload = json!(null);
    request.event.version = 2;
    assert!(broker.publish(context(), &request).is_err());
    request.event.version = 1;
    request.event.occurred_at = -1;
    assert!(broker.publish(context(), &request).is_err());
    request.event.occurred_at = 1;
    request.event.origin.clear();
    assert!(broker.publish(context(), &request).is_err());
    request.event.origin = "origin".into();
    request.completion_reserve = 2049;
    assert!(broker.publish(context(), &request).is_err());
    assert_eq!(usage(&broker), initial);
    request.completion_reserve = 1024;
    broker.publish(context(), &request).unwrap();
    assert!(broker.transaction(|s| s.claim(context(), "bounded", 1, 0)).is_err());
    assert!(broker.transaction(|s| s.claim(context(), "bounded", 1, 3601)).is_err());
}

#[test]
fn incompatible_reopen_cannot_shrink_reserved_completion_or_batch_bounds() {
    let (dir, mut broker) = open();
    broker.publish(context(), &request("retained")).unwrap();
    drop(broker);
    for config in [
        Config { limits: Limits { max_batch_events: 1, ..Limits::default() }, ..Config::default() },
        Config { max_completion_bytes: 1024, ..Config::default() },
        Config { max_payload_bytes: 4096, ..Config::default() },
    ] {
        assert!(Broker::open(dir.path().join("broker.sqlite"), config)
            .err()
            .unwrap()
            .to_string()
            .contains("CONFIG_CONFLICT"));
    }
    let mut broker = Broker::open(dir.path().join("broker.sqlite"), Config::default()).unwrap();
    let claim = broker.transaction(|s| s.claim(context(), "retained", 1, 10)).unwrap().unwrap();
    broker.transaction(|s| s.complete(context(), &claim, 2, &event("completed"))).unwrap();
}

#[test]
fn completion_identity_cannot_alias_publication_or_another_completion() {
    let (_dir, mut broker) = open();
    for id in ["first", "second"] {
        broker.publish(context(), &request(id)).unwrap();
    }
    let first = broker.transaction(|s| s.claim(context(), "first", 1, 10)).unwrap().unwrap();
    let second = broker.transaction(|s| s.claim(context(), "second", 1, 10)).unwrap().unwrap();
    let used = usage(&broker);
    assert!(broker
        .transaction(|s| s.complete(context(), &first, 2, &event("second")))
        .unwrap_err()
        .to_string()
        .contains("EVENT_CONFLICT"));
    assert_eq!(usage(&broker), used);
    broker.transaction(|s| s.complete(context(), &first, 2, &event("completion"))).unwrap();
    let used = usage(&broker);
    assert!(broker
        .transaction(|s| s.complete(context(), &second, 2, &event("completion")))
        .unwrap_err()
        .to_string()
        .contains("EVENT_CONFLICT"));
    assert_eq!(usage(&broker), used);
}

#[test]
fn explicit_new_subscriptions_after_pruning_choose_retained_history_or_tail() {
    let (_dir, mut broker) = open();
    broker
        .subscribe(
            context(),
            &route(),
            ConsumerId("old".into()),
            SubscriptionStart::EarliestAvailable,
        )
        .unwrap();
    broker.publish(context(), &request("pruned")).unwrap();
    let batch = broker.fetch(context(), &route(), fetch("old")).unwrap();
    broker.ack(context(), &route(), ack(&batch)).unwrap();
    broker.publish(context(), &request("retained")).unwrap();
    // Legacy recovery/resume never silently skips a missing history prefix.
    assert!(broker.resume(context(), &route(), resume("implicit-new")).is_err());
    let earliest = broker
        .subscribe(
            context(),
            &route(),
            ConsumerId("earliest".into()),
            SubscriptionStart::EarliestAvailable,
        )
        .unwrap();
    let latest = broker
        .subscribe(context(), &route(), ConsumerId("latest".into()), SubscriptionStart::Latest)
        .unwrap();
    assert_eq!(earliest.stored, batch.end);
    assert_eq!(latest.stored, EventPosition(2));
    assert_eq!(earliest.journal_id, latest.journal_id);
    let visible = broker.fetch(context(), &route(), fetch("earliest")).unwrap();
    assert_eq!(visible.events.len(), 1);
    assert_eq!(visible.events[0].payload["event_id"], "retained");
    assert!(broker.fetch(context(), &route(), fetch("latest")).unwrap().events.is_empty());
    let used = usage(&broker);
    assert!(broker
        .subscribe(context(), &route(), ConsumerId("earliest".into()), SubscriptionStart::Latest)
        .is_err());
    assert!(broker
        .subscribe(
            Context { owner: "other", ..context() },
            &route(),
            ConsumerId("earliest".into()),
            SubscriptionStart::Latest
        )
        .is_err());
    assert_eq!(usage(&broker), used);
    let result: rusqlite::Result<()> = broker.transaction(|session| {
        session.subscribe(
            context(),
            &route(),
            ConsumerId("rolled-back".into()),
            SubscriptionStart::Latest,
        )?;
        Err(storage::error("INJECTED", "rollback subscription"))
    });
    assert!(result.is_err());
    assert_eq!(usage(&broker), used);
    broker
        .subscribe(
            context(),
            &route(),
            ConsumerId("rolled-back".into()),
            SubscriptionStart::EarliestAvailable,
        )
        .unwrap();
}

#[test]
fn embedded_sessions_reject_incompatible_bounds_before_mutation() {
    let (_dir, mut broker) = open();
    broker.publish(context(), &request("reserved")).unwrap();
    let used = usage(&broker);
    let tx = broker.connection.transaction().unwrap();
    let changed = Config { max_completion_bytes: 1024, ..Config::default() };
    assert!(Session::new(&tx, changed).err().unwrap().to_string().contains("CONFIG_CONFLICT"));
    tx.rollback().unwrap();
    assert_eq!(usage(&broker), used);
}
