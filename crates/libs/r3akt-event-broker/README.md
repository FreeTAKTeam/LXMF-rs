# r3akt-event-broker

An embedded disk SQLite event journal with authenticated custody receipts,
independent subscriptions, immutable publication IDs and explicit processing
recovery. This is a library, with no runtime, transport or domain dependencies.
The LXMF adapter and this standalone API use the same journal, receipt and
physical admission implementation.

`Broker::open(path, Config)` owns a connection with the validated WAL/FULL-sync
profile. Its methods commit atomically. `Broker::transaction` groups multiple
broker changes atomically; `Session::new(&Transaction, Config)` embeds the same
operations in an application's existing transaction. Configure the shared storage
profile before beginning an application-owned transaction, initialize once through
`Session::initialize`, and roll back that transaction if a session operation fails.
No lower-level storage primitive commits a caller's transaction.

```rust,no_run
use r3akt_event_broker::{Broker, Config, Context, EventEnvelope, PublishRequest, Route};
use serde_json::json;
# fn example() -> rusqlite::Result<()> {
let mut broker = Broker::open("events.sqlite", Config::default())?;
// Resolve authority in the application; request fields do not authenticate this context.
let context = Context { owner: "authenticated-service", scope: "tenant-a" };
let request = PublishRequest {
    operation_id: "request-42".into(),
    event: EventEnvelope {
        version: 1,
        occurred_at: 1720000000,
        origin: "sensor-gateway".into(), // claimed provenance, never authority
        correlation_id: Some("workflow-9".into()),
        event_id: "observation-42".into(),
        event_type: "observation.recorded".into(),
        route: Route { topic: "observations".into(), subject: Some("asset-a".into()) },
        payload: json!({"temperature": 21}),
    },
    completion_reserve: 4096,
};
let publication = broker.publish(context, &request)?;
// After a timeout/uncertain result, retry the SAME immutable request/operation ID.
assert_eq!(broker.publish(context, &request)?, publication);
# Ok(())
# }
```

Routing matches the exact `(scope, topic, subject)` tuple. Multiple consumer IDs
may subscribe independently for the same authenticated owner and route. New
subscriptions explicitly select `EarliestAvailable` (current retained floor) or
`Latest` (current tail); `resume` preserves strict recovery checks for restored
checkpoints. Subscription creation never resets an existing consumer. Applications
must authorize requested routes before calling the library.

Fetched batches replay with the same authenticated receipt until ACK. Persist the
batch in an application inbox before ACK, deduplicate by journal/position or event
identity, and expect at-least-once delivery after uncertain results. Receipts bind
the journal incarnation, consumer, owner, route and issued position range. ACK
advances durable custody only; it does not mean business processing completed.

Publishing charges retained operation/event identities and a bounded completion
reservation before accepting the event. Identity records remain retained after
journal pruning, so finite storage can eventually refuse new admissions rather
than forget idempotency. Configured batch, payload, completion and lease bounds
are persisted and incompatible reopen/embedded session configurations fail
explicitly. The existing journal budget survives reopen; configuration never
shrinks admitted custody. Ordinary admission preserves physical/control headroom;
completions use their prepaid reservation without requiring ordinary admission.
The completion reserve bounds full encoded event accounting, including route and
journal overhead, rather than just the business payload.

Processing methods are available in `Broker::transaction`/`Session`: `claim`
creates a bounded lease capability, `complete` records its immutable completion,
and `expire_claims` changes expired claims to `Unknown`. Reopening preserves live
claims until their lease deadline; it does not cancel another active handle's
worker. After a crash or expired lease, inspect external business effects and use
explicit `recover(Requeue)` only when replay is safe, or `recover(Fail)` to stop.
Expired/stale claims cannot complete or consume another claim's reservation.
Identical completed claims may be retried even after their deadline to reconcile
an uncertain commit. Application time arguments must come from trusted clock
policy. The broker never promises exactly-once external business effects.
