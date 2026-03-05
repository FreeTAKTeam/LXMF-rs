# Native Embedded Lab Profile v1

## Scope

This document defines the reproducible lab profile used for release-gated
embedded-native tests and reports.

## Hardware

- Target board: ESP32-CAM with OV2640 camera
- Target runtime: `xtensa-esp32-espidf`
- Firmware build profile: release
- Host platform: developer workstation running the host-side Rust tools

## Network Profiles

### LAN profile

- ESP and host are on the same 2.4 GHz Wi-Fi network
- Expected RSSI is better than `-65 dBm`
- No intentional packet shaping or impairment
- TCP server mode and TCP client mode are both exercised

### Internet-shaped profile

- ESP runs in TCP client mode
- Host endpoint is reachable on a stable address/port
- Public internet TCP server-mode reachability is documented but not release-gated

## Measurement Rules

- Each acceptance scenario runs 10 times
- LAN release gate: 10/10 successes required
- Internet-shaped client-mode release gate: 9/10 successes required
- Capture latency and throughput are recorded in reports but are not hard
  release blockers in this profile version

## Acceptance References

- `docs/contracts/native-embedded-interop-profile-v1.md`
- `docs/contracts/native-embedded-lockfile.toml`
