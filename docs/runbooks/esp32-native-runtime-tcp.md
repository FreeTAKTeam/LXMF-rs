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

Or use the smoke wrapper:

```bash
./tools/scripts/esp32-tcp-native-smoke.sh
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
