# RNode probe examples

Two standalone tools for testing RNode hardware without running the full daemon.

- **`lora_serial_probe`** — connect over USB serial
- **`rnode_ble_probe`** — connect over Bluetooth LE

---

## Prerequisites

### Serial

Find the port your RNode appears on:

```bash
ls /dev/ttyUSB* /dev/ttyACM*
# or plug it in and watch:
dmesg | tail -5
```

Access serial ports without sudo — add yourself to `dialout`:

```bash
# permanent (takes effect after re-login):
sudo usermod -aG dialout $USER

# current session only (no re-login needed):
newgrp dialout
```

### BLE

BlueZ and DBus must be running (standard on most Linux desktops). Find your RNode's BLE name:

```bash
bluetoothctl scan on
# look for a device named something like "RNode ..." and note its name or address
```

---

## Building

Pre-built binaries (if available): `dist/lora_serial_probe`, `dist/rnode_ble_probe`

From source:

```bash
cargo build --release -p reticulum-rs-transport --example lora_serial_probe
cargo build --release -p reticulum-rs-transport --features rnode-ble --example rnode_ble_probe
# binaries land in target/release/examples/
```

---

## lora_serial_probe

Firmware/connectivity check only:

```bash
./dist/lora_serial_probe --port /dev/ttyUSB0
```

Full radio probe (configures frequency, verifies radio comes online):

```bash
./dist/lora_serial_probe --port /dev/ttyUSB0 --region EU868
```

All options:

```bash
./dist/lora_serial_probe --help
```

---

## rnode_ble_probe

Firmware/connectivity check only:

```bash
./dist/rnode_ble_probe --peripheral-id "RNode ABC123"
```

Full radio probe:

```bash
./dist/rnode_ble_probe --peripheral-id "RNode ABC123" --region EU868
```

All options:

```bash
./dist/rnode_ble_probe --help
```

---

## Sending a packet between two nodes

Both nodes must use the **same region** (or the same explicit frequency/bandwidth/SF/CR).

### Serial ↔ Serial

```bash
# terminal 1 — listen
./dist/lora_serial_probe --port /dev/ttyUSB0 --region EU868 --listen-secs 60

# terminal 2 — send
./dist/lora_serial_probe --port /dev/ttyUSB1 --region EU868 --send-hex deadbeef
```

### BLE ↔ BLE

```bash
# terminal 1 — listen
./dist/rnode_ble_probe --peripheral-id "RNode AAA" --region EU868 --listen-secs 60

# terminal 2 — send
./dist/rnode_ble_probe --peripheral-id "RNode BBB" --region EU868 --send-hex deadbeef
```

### Mixed: BLE listener + Serial sender

```bash
# terminal 1 — BLE listen
./dist/rnode_ble_probe --peripheral-id "RNode AAA" --region EU868 --listen-secs 60

# terminal 2 — serial send
./dist/lora_serial_probe --port /dev/ttyUSB0 --region EU868 --send-hex deadbeef
```
