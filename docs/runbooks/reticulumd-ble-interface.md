# `reticulumd` BLE GATT Interface Runbook

## Purpose

This runbook documents configuration, startup semantics, and recovery for `ble_gatt` interfaces.

## Scope

- Interface kind: `ble_gatt`
- Backends: Linux, macOS, Windows
- Startup lifecycle: daemon bootstrap only
- Runtime mutation policy: `set_interfaces`/`reload_config` with `ble_gatt` changes require restart

## Required Config Fields

```toml
interfaces = [
  {
    type = "ble_gatt",
    enabled = true,
    name = "ble-main",
    adapter = "hci0",
    peripheral_id = "AA:BB:CC:DD:EE:FF",
    service_uuid = "12345678-1234-1234-1234-1234567890ab",
    write_char_uuid = "2A37",
    notify_char_uuid = "2A38",
    mtu = 247,
    scan_timeout_ms = 5000,
    connect_timeout_ms = 10000,
    reconnect_backoff_ms = 500,
    max_reconnect_backoff_ms = 5000
  }
]
```

## Validation Rules

- Required when enabled: `peripheral_id`, `service_uuid`, `write_char_uuid`, `notify_char_uuid`.
- UUID values must be 16-bit, 32-bit, or canonical 128-bit format.
- `scan_timeout_ms` and `connect_timeout_ms` must be > 0 when set.
- `mtu` allowed range: `23..=517`.
- `max_reconnect_backoff_ms` must be `>= reconnect_backoff_ms`.

## Runtime Behavior

1. Runtime settings are normalized at startup (timeouts/backoff defaults applied).
2. Backend dispatch is selected by target OS.
3. Startup emits a deterministic configuration line with adapter/peripheral/service/characteristic IDs.
4. Invalid runtime bounds are rejected before backend startup.

Startup policy controls:

- Default mode is best-effort (daemon continues in degraded mode when some interfaces fail).
- `--strict-interface-startup` makes startup/preflight failures fatal.

## Health Signals

Expected startup log examples:

- `ble_gatt configured (linux backend) ...`
- `ble_gatt configured (macos backend) ...`
- `ble_gatt configured (windows backend) ...`

Failure signals:

- `ble_gatt startup rejected name=<name> err=<reason>`
- `interface startup degraded started=<n> failed=<m> strict=<bool>`

Runtime status visibility:

- `list_interfaces` includes `_runtime.startup_status`.
- Failed interfaces include `_runtime.startup_error`.

## Incident Response

1. Verify UUIDs and peripheral identifier are correct.
2. Confirm platform BLE stack is enabled and permissions are granted.
3. Check adapter selection (`adapter`) matches host naming.
4. If startup rejects repeatedly, disable interface and restart daemon while preserving logs.
5. If rejection is due to bounds, fix config values and restart.

## Verification Commands

```bash
cargo test -p reticulumd --test config
cargo test -p reticulumd --bin reticulumd runtime_settings
cargo check -p reticulumd --all-targets
```

## Rollback

- Disable `ble_gatt` entries in config.
- Restart daemon.
- Validate active interface snapshot via `list_interfaces`.
