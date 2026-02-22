# iOS BLE Conformance Notes

iOS host adapters must implement `MobileBleHostAdapter` and satisfy event ordering,
timeout, and session lifecycle rules from `docs/contracts/mobile-ble-host-contract.md`.

Background restore and CoreBluetooth lifecycle handling must preserve monotonic event sequencing.

Required CI artifacts:

- event transcript JSON (`docs/fixtures/mobile-ble/ios/events.sample.json` format)
- capability JSON (`supports_background_restore`, queue limits, payload limits)
- pass/fail status for ordering and timeout checks
