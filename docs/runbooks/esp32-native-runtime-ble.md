# ESP32 Native Runtime BLE

This runbook covers the publishable host/device path for the embedded native runtime over BLE.

## Scope

- ESP32 firmware runs `rns-embedded-ffi` linked into the firmware bridge
- BLE carries wrapped native runtime frames on the existing service
- Host uses Rust `rnx` commands as the primary path
- Python/Bleak helper scripts remain fallback diagnostics

## BLE wrapper constants

- `0x21` `BLE_FRAME_NATIVE_ANNOUNCE_REQ`
  - bring-up helper
- `0x22` `BLE_FRAME_NATIVE_MESSAGE_TX_REQ`
  - bring-up helper
- `0x23` `BLE_FRAME_NATIVE_WIRE`
  - primary transport wrapper for encoded runtime packet frames

## Runtime frame kinds

- `0x11` native announce frame
- `0x31` native LXMF message frame
- `0x45` test ping frame
- `0x46` test pong frame

## Build host tool

```bash
cd /Users/tommy/Documents/TAK/LXMF-rs
cargo build -p rns-tools --bin rnx
```

## Build embedded Rust library

```bash
cd /Users/tommy/Documents/TAK/LXMF-rs
PATH="$HOME/.rustup/toolchains/esp/bin:$PATH" RUSTUP_TOOLCHAIN=esp \
cargo build -Z build-std=core,alloc -p rns-embedded-ffi --release \
  --target xtensa-esp32-espidf --no-default-features --features alloc
```

## Flash firmware

```bash
cd /Users/tommy/Documents/TAK/lxmf-esp32-cam-fw
LXMF_RUST_FFI_LIB=/Users/tommy/Documents/TAK/LXMF-rs/target/xtensa-esp32-espidf/release/librns_embedded_ffi.a \
LXMF_RUST_FFI_INCLUDE=/Users/tommy/Documents/TAK/LXMF-rs/crates/libs/rns-embedded-ffi/include \
pio run -t upload
pio device monitor
```

Expected boot line:

```text
[lxmf-native] init backend=rust-ffi status=0
```

## Raw native runtime ping

```bash
cd /Users/tommy/Documents/TAK/LXMF-rs
./target/debug/rnx ble-native-peer \
  --name-hint LXMF \
  --service-uuid 12345678-1234-1234-1234-1234567890ab \
  --write-char-uuid 12345678-1234-1234-1234-1234567890ac \
  --notify-char-uuid 12345678-1234-1234-1234-1234567890ad \
  --mode raw-ping \
  --payload ping \
  --timeout-secs 8
```

Expected reply:

```text
BLE_NATIVE_PEER frame kind=0x46 ... payload_hex=706f6e673a70696e67
```

## LXMF-style runtime ping

```bash
cd /Users/tommy/Documents/TAK/LXMF-rs
./target/debug/rnx ble-native-peer \
  --name-hint LXMF \
  --service-uuid 12345678-1234-1234-1234-1234567890ab \
  --write-char-uuid 12345678-1234-1234-1234-1234567890ac \
  --notify-char-uuid 12345678-1234-1234-1234-1234567890ad \
  --mode lxmf-ping \
  --source-hex 99999999999999999999999999999999 \
  --destination-hex 22222222222222222222222222222222 \
  --payload hello \
  --timeout-secs 8
```

Expected reply:

```text
BLE_NATIVE_PEER frame kind=0x31 ... body=pong:hello
```

`--runtime-seq` is optional. If omitted, `rnx` auto-generates a fresh sequence from current time.

## Bridge to reticulumd

Start daemon:

```bash
cd /Users/tommy/Documents/TAK/LXMF-rs
mkdir -p .tmp/ble-native-bridge
./target/debug/reticulumd --rpc 127.0.0.1:4243 --db .tmp/ble-native-bridge/reticulum.db
```

In another terminal:

```bash
cd /Users/tommy/Documents/TAK/LXMF-rs
./target/debug/rnx ble-native-bridge \
  --name-hint LXMF \
  --service-uuid 12345678-1234-1234-1234-1234567890ab \
  --write-char-uuid 12345678-1234-1234-1234-1234567890ac \
  --notify-char-uuid 12345678-1234-1234-1234-1234567890ad \
  --rpc 127.0.0.1:4243 \
  --source-hex 99999999999999999999999999999999 \
  --destination-hex 22222222222222222222222222222222 \
  --payload hello \
  --timeout-secs 8
```

Expected output:

```text
BLE_NATIVE_BRIDGE ok: ... body=pong:hello attachment_id=...
```

## Compatibility notes

- The embedded payloads now use real native packet framing and minimal LXMF envelopes.
- The BLE wrapper `0x23` is a project transport wrapper, not a standard off-the-shelf Reticulum client transport.
- A normal client is not directly compatible unless it also speaks this BLE wrapper and runtime packet framing.
- The host-side Rust peer in `rnx` is the reference client for this transport.
