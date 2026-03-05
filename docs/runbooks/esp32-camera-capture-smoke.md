# ESP32-CAM BLE Capture Smoke Runbook

## Purpose

Validate end-to-end remote capture from ESP32-CAM over BLE and attachment commit into `reticulumd`.

## Prerequisites

- ESP32-CAM firmware implements BLE camera wire contract in `docs/contracts/ble-camera-wire-v1.md`.
- Host has BLE enabled and can discover the ESP32 peripheral.
- Rust workspace builds on host.

## Required BLE Values

Collect from firmware config:

- `PERIPHERAL_ID`
- `SERVICE_UUID`
- `WRITE_CHAR_UUID`
- `NOTIFY_CHAR_UUID`

## One-Command Smoke

```bash
PERIPHERAL_ID="AA:BB:CC:DD:EE:FF" \
SERVICE_UUID="12345678-1234-1234-1234-1234567890ab" \
WRITE_CHAR_UUID="2A37" \
NOTIFY_CHAR_UUID="2A38" \
./tools/scripts/esp32-camera-capture-smoke.sh
```

## Optional Overrides

- `RPC_ADDR` default `127.0.0.1:4243`
- `TIMEOUT_SECS` default `25`
- `CHUNK_SIZE` default `8192`
- `CONTENT_TYPE` default `image/jpeg`
- `KEEP_DAEMON=1` to leave `reticulumd` running after script exit

## Expected Output

- `CAMERA_CAPTURE_UPLOAD ok: bytes=<n> attachment_id=<id>` from `rnx`
- `[smoke] attachment_count=<n>` from script
- Log files:
  - `target/hil/esp32-camera-smoke-reticulumd.log`
  - `target/hil/esp32-camera-smoke-rnx.log`

## First Failure Checks

1. Peripheral not found:
- verify `PERIPHERAL_ID` normalization and BLE visibility.
- confirm host BLE permissions.

2. Characteristic mismatch:
- verify service/write/notify UUIDs match firmware.

3. Capture timeout:
- increase `TIMEOUT_SECS`.
- verify firmware sends `CAPTURE_ACK` then chunk notifications and `DONE`.

4. Upload commit fails:
- inspect `rnx` log and daemon RPC error payload.
- confirm chunk order and payload integrity on firmware side.
