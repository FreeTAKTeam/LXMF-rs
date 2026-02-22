# Android BLE Conformance Notes

Android host adapters must implement `MobileBleHostAdapter` and satisfy event ordering,
timeout, and session lifecycle rules from `docs/contracts/mobile-ble-host-contract.md`.

Required CI artifacts:

- event transcript JSON (`docs/fixtures/mobile-ble/android/events.sample.json` format)
- capability JSON (`supports_background_restore`, `max_notification_queue`, payload limits)
- pass/fail status for ordering and timeout checks
