# Kotlin Mobile Easy-Mode Example

This directory documents a small Kotlin app using the first-party wrapper shape
from `wrappers/kotlin-mobile`. Wrapper users start the runtime, subscribe to
typed events, send one message, and handle delivery or stream-gap outcomes
without owning a raw poll loop.

The names in `Main.kt` match the SDK app v1 contract and the first-party Kotlin
wrapper source:

- `Config.mobile_default()`
- `LxmfEasyClient.rpc(...)`
- `client.start(...)`
- `client.subscribeEvents(...)`
- `client.send(...)`
- typed event kinds such as `MessageDelivered`, `StreamGapDetected`, and
  `QueuePressureRaised`

Conformance anchors:

- `lifecycle.start_stop_restart`
- `events.delivery_ordering`
- `delivery.queue_pressure`
- `connectivity.reconnect_recovery`
- `errors.typed_mapping`

The executable wrapper test anchors live at
`wrappers/kotlin-mobile/src/test/kotlin/org/freetakteam/lxmf/easy/LxmfEasyConformanceTest.kt`.
