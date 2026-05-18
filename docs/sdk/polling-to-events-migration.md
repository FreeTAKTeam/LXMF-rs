# Polling to Events Migration

This guide moves app integrations from periodic `poll_events` loops to the
native event stream exposed by `lxmf-sdk`.

## Migration Target

The target steady-state shape is:

1. start the runtime once
2. subscribe to typed events
3. send messages through typed domain handles
4. handle delivery and runtime updates from the stream
5. use cursor polling only for recovery or diagnostics

Applications should not run a one-second polling loop for normal delivery state
tracking. Polling remains supported as a fallback when a stream gap or cursor
recovery path requires explicit reconciliation.

## Before: Periodic Polling

Legacy integrations usually keep a mutable cursor and sleep between polls:

```rust
let mut cursor = None;
loop {
    let batch = client.events().poll(cursor.as_deref(), 256)?;
    for event in batch.events {
        handle_event(event)?;
    }
    cursor = batch.next_cursor;
    std::thread::sleep(std::time::Duration::from_secs(1));
}
```

This works for low-level recovery, but it is not the default application model:

- delivery latency is bounded by the sleep interval
- slow consumers can hide stream gaps until the next poll
- shutdown and cancellation require extra loop coordination
- app code tends to handle raw cursor state instead of typed stream outcomes

## After: Native Event Stream

Use `events().subscribe(...)` and process typed events from the stream:

```rust
use lxmf_sdk::app::{Client, Config, EventKind, SendRequest, SubscriptionStart};
use serde_json::json;
use tokio_stream::StreamExt;

let client = Client::rpc("unix:/tmp/lxmf-rpc.sock");
let handle = client.runtime().start_async(Config::desktop_default()).await?;
let mut events = client.events().subscribe(SubscriptionStart::Tail)?;

let receipt = client
    .messages()
    .send_async(
        SendRequest::new(
            "example.service",
            "example.peer",
            json!({"title": "hello", "content": "event-driven"}),
        )
        .with_correlation_id("send-1"),
    )
    .await?;

while let Some(event) = events.next().await.transpose()? {
    match event.kind {
        EventKind::MessageDelivered
            if event.metadata.message_id.as_deref() == Some(receipt.message_id.as_str()) =>
        {
            break;
        }
        EventKind::StreamGapDetected(_) => {
            let status = client.runtime().status_async().await?;
            trigger_reconciliation(status).await?;
        }
        _ => {}
    }
}
```

The SDK tracks cursor progress internally for the native stream and reconnects
with the latest cursor when the backend supports it. Replayed events are
deduplicated by sequence number.

## Recovery Fallback

Use the low-level `LxmfSdk::poll_events` API deliberately when a stream gap,
expired cursor, or diagnostic workflow requires an explicit catch-up loop:

```rust
use lxmf_sdk::LxmfSdk;

let sdk = build_low_level_sdk_client();
let mut cursor = persisted_cursor();
loop {
    let batch = LxmfSdk::poll_events(&sdk, cursor.clone(), 256)?;
    for event in batch.events {
        handle_event(event)?;
    }
    persist_cursor(batch.next_cursor.as_ref())?;
    cursor = batch.next_cursor.clone();
    if batch.dropped_count.unwrap_or(0) > 0 {
        let snapshot = LxmfSdk::snapshot(&sdk)?;
        reconcile_from_snapshot(snapshot)?;
        break;
    }
}
```

Keep recovery loops bounded and observable. If the cursor is invalid or expired,
reset from a snapshot rather than silently starting from the tail.

## Delivery State Changes

Do not infer delivery state from a successful `send_async` return alone.
`send_async` means the daemon accepted the message. Terminal state is reported
through typed events or an explicit status lookup:

- `MessageAccepted` or `MessageQueued` means the daemon accepted work
- `MessageDelivered` means the backend observed delivery
- retry or queue-pressure errors are typed SDK errors
- stream gaps require reconciliation before presenting final state to users

Use the returned `message_id` and optional `correlation_id` to join send
receipts, delivery events, status calls, and logs.

## Shutdown Changes

Event-stream consumers should stop by dropping the stream or by coordinating
with the application cancellation token, then call runtime stop/shutdown:

```rust
drop(events);
client.runtime().stop_async(lxmf_sdk::ShutdownMode::Graceful).await?;
```

Avoid leaving background polling tasks alive after runtime shutdown. They can
race with restart and reprocess stale cursor state.

## Checklist

- Replace steady-state `poll_events + sleep` loops with `events().subscribe(...)`.
- Treat `send_async` as acceptance, not delivery.
- Handle `StreamGapDetected` with snapshot or bounded cursor recovery.
- Persist cursor state only for recovery workflows that need it.
- Preserve `message_id`, `correlation_id`, and `trace_ref` in host logs.
- Keep raw JSON handling out of normal app workflows; use typed domain handles.
