# ESP32 Native Runtime TCP

This runbook covers the primary standalone-node path for the embedded native runtime over Wi-Fi/TCP.

## Scope

- ESP32 firmware runs `rns-embedded-ffi` linked into the firmware bridge
- Wi-Fi/TCP is the primary node transport
- BLE remains provisioning/recovery only
- Host uses Rust `rnx` commands as the primary path

## Transport contract

- TCP carries raw encoded runtime packet frames
- each TCP record is:
  - `u16` big-endian frame length
  - encoded packet frame bytes
- BLE wrapper bytes are not used on TCP

## Build host tool

```bash
cargo build -p rns-tools --bin rnx
```

## Start host listener

Passive listener:

```bash
./target/debug/rnx tcp-native-listener \
  --bind 0.0.0.0:7443 \
  --mode passive \
  --timeout-secs 15
```

Long-running service listener:

```bash
./target/debug/rnx tcp-native-listener \
  --bind 0.0.0.0:7443 \
  --mode passive \
  --serve \
  --timeout-secs 30
```

Or use the smoke wrapper:

```bash
./tools/scripts/esp32-tcp-native-smoke.sh
```

Active raw ping via smoke wrapper:

```bash
LISTENER_MODE=raw-ping ./tools/scripts/esp32-tcp-native-smoke.sh
```

Active LXMF ping via smoke wrapper:

```bash
LISTENER_MODE=lxmf-ping PAYLOAD=hello ./tools/scripts/esp32-tcp-native-smoke.sh
```

Capture via smoke wrapper:

```bash
LISTENER_MODE=capture CAPTURE_OUT=/tmp/lxmf-tcp-capture.jpg ./tools/scripts/esp32-tcp-native-smoke.sh
```

If `CAPTURE_OUT` is omitted, captures are written under `target/hil/captures/`.

Override the capture profile per request:

```bash
LISTENER_MODE=capture CAPTURE_PROFILE=very_high ./tools/scripts/esp32-tcp-native-smoke.sh
```

Supported request-time profiles:
- `default`
- `thumbnail`
- `balanced`
- `high`
- `very_high`

## Bridge to `reticulumd`

Start the daemon:

```bash
mkdir -p .tmp/tcp-native-bridge
./target/debug/reticulumd --rpc 127.0.0.1:4243 --db .tmp/tcp-native-bridge/reticulum.db
```

Bridge a capture into the daemon:

```bash
./tools/scripts/esp32-tcp-native-bridge.sh
```

If `CAPTURE_OUT` is omitted, bridge captures are written under `target/hil/captures/`.

Bridge an LXMF reply body into the daemon:

```bash
BRIDGE_MODE=lxmf-ping CONTENT_TYPE=text/plain PAYLOAD=hello \
  ./tools/scripts/esp32-tcp-native-bridge.sh
```

Long-running bridge:

```bash
./target/debug/rnx tcp-native-bridge \
  --bind 0.0.0.0:7443 \
  --mode capture \
  --rpc 127.0.0.1:4243 \
  --serve \
  --timeout-secs 30
```

## Soak testing

Repeated capture validation:

```bash
RUNS=5 ./tools/scripts/esp32-tcp-native-soak.sh
```

Repeated capture validation with per-request override:

```bash
RUNS=5 CAPTURE_PROFILE=balanced ./tools/scripts/esp32-tcp-native-soak.sh
```

Repeated LXMF ping validation:

```bash
MODE=lxmf-ping RUNS=5 PAYLOAD=hello ./tools/scripts/esp32-tcp-native-soak.sh
```

Active raw ping listener:

```bash
./target/debug/rnx tcp-native-listener \
  --bind 0.0.0.0:7443 \
  --mode raw-ping \
  --payload ping \
  --timeout-secs 15
```

Active LXMF ping listener:

```bash
./target/debug/rnx tcp-native-listener \
  --bind 0.0.0.0:7443 \
  --mode lxmf-ping \
  --source-hex 99999999999999999999999999999999 \
  --destination-hex 22222222222222222222222222222222 \
  --payload hello \
  --timeout-secs 15
```

## Connect to an ESP TCP endpoint

Raw ping:

```bash
./target/debug/rnx tcp-native-peer \
  --addr 192.168.1.50:7443 \
  --mode raw-ping \
  --payload ping \
  --timeout-secs 8
```

LXMF ping:

```bash
./target/debug/rnx tcp-native-peer \
  --addr 192.168.1.50:7443 \
  --mode lxmf-ping \
  --source-hex 99999999999999999999999999999999 \
  --destination-hex 22222222222222222222222222222222 \
  --payload hello \
  --timeout-secs 8
```

Expected reply:

```text
TCP_NATIVE_PEER frame kind=0x31 ... body=pong:hello
```

## Firmware client-mode defaults

The current firmware scaffold can dial a host listener using compile-time defaults.

Expected build flags:

```text
LXMF_NODE_MODE_TCP_CLIENT
LXMF_WIFI_SSID=<ssid>
LXMF_WIFI_PASSWORD=<password>
LXMF_TCP_HOST=<host-ip-or-name>
LXMF_TCP_PORT=7443
```

## Compatibility notes

- TCP is the primary public transport for the standalone node path.
- BLE is still supported, but only for provisioning/recovery and local diagnostics.
- The direct-compatible client for this transport is `rnx`.
- Bridged compatibility to `reticulumd` remains the path for existing host-side integrations.
