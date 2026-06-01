# Migration to SDK Easy Mode

SDK easy mode is the app-facing `lxmf_sdk::app` surface. It replaces ad hoc
startup, polling, send, retry, and error-mapping logic with the profile-aware
contract in `docs/contracts/sdk-app-api-v1.md`.

## Migration Checklist

1. Replace direct backend construction in app code with `lxmf_sdk::app::Client`.
2. Start once with `client.runtime().start_async(Config::desktop_default())` or
   the platform profile that matches the app.
3. Subscribe to `client.events().subscribe(SubscriptionStart::Tail)` before
   sending user-visible work.
4. Replace raw `sdk_send_v2` calls with `client.messages().send_async(...)`.
5. Replace polling loops with typed events. Keep low-level `poll_events` only
   for explicit recovery, diagnostics, or embedded manual hosts.
6. Map app errors by `lxmf_sdk::app::ErrorCode`, not backend string parsing.
7. Keep product-specific commands as `custom_operations` catalogs on `Config`
   instead of hard-coding operation dispatch in the wrapper.

## Low-Level to Easy-Mode Mapping

| Low-level behavior | Easy-mode replacement |
| --- | --- |
| `sdk_negotiate_v2` plus `sdk_configure_v2` startup calls | `runtime().start_async(Config::desktop_default())` |
| Raw `sdk_send_v2` JSON | `messages().send_async(SendRequest::new(...))` |
| One-second app polling loop | `events().subscribe(SubscriptionStart::Tail)` |
| Manual cursor state in normal apps | Stream-gap event plus explicit recovery path |
| Backend error string matching | `app::ErrorCode` and typed categories |
| Product-local command registry | `Config::with_custom_operation(...)` |

## Conformance Scenarios

The migration is considered compatible when the app still satisfies the SDK app
v1 fixture scenarios in `docs/fixtures/sdk-app-v1/manifest.json`:

- `lifecycle.start_stop_restart`
- `events.delivery_ordering`
- `timeout.poll_timeout`
- `delivery.queue_pressure`
- `connectivity.reconnect_recovery`
- `errors.typed_mapping`
- `compatibility.unknown_additive`

## Golden Paths

- Rust managed app: `examples/sdk-easy/rust-managed`
- Kotlin mobile wrapper shape: `examples/sdk-easy/kotlin-mobile`
- First-party Kotlin wrapper source: `wrappers/kotlin-mobile`

Use these as the starting point for new app teams. Advanced integrations can
still use `docs/sdk/advanced-embedding.md`, but wrapper defaults should preserve
the easy-mode lifecycle, event, retry, and error semantics above.
