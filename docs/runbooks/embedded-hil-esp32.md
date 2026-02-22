# Embedded HIL ESP32 Smoke Runbook

Status: active nightly gate for constrained-device validation.

## Purpose

Validate real-device `embedded-alloc` lifecycle behavior against a lab ESP32 board:

1. negotiate/start
2. send
3. manual tick
4. poll events
5. snapshot
6. shutdown

## Required Environment

Set these environment variables for the nightly workflow runner:

- `HIL_SERIAL_PORT` (example: `/dev/ttyUSB0`)
- `HIL_RPC_ENDPOINT` (example: `127.0.0.1:4242`)
- `HIL_SEND_SOURCE` (identity/source hash used by smoke send)
- `HIL_SEND_DESTINATION` (destination hash reachable by lab setup)
- `HIL_TICK_MAX_WORK_ITEMS` (optional; default `32`)
- `HIL_TICK_MAX_DURATION_MS` (optional; default `25`)

## Local Dry Run

```bash
HIL_SERIAL_PORT=/dev/ttyUSB0 \
HIL_RPC_ENDPOINT=127.0.0.1:4242 \
HIL_SEND_SOURCE=<source_hash> \
HIL_SEND_DESTINATION=<destination_hash> \
cargo run -p xtask -- embedded-hil-check
```

## Artifacts

The smoke gate writes:

- `target/hil/esp32-smoke.log`
- `target/hil/esp32-smoke-report.json`

Nightly workflow uploads both artifacts for audit and regression triage.

## Failure Handling

1. Check serial port availability and board power state.
2. Verify `reticulumd` endpoint and auth mode alignment with the test harness.
3. Inspect `target/hil/esp32-smoke.log` for the first failed lifecycle command.
4. If failure is reproducible for two consecutive runs, open a P1 issue tagged `embedded-hil`.
