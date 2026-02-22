# `reticulumd` Serial Interface Runbook

## Purpose

This runbook describes how to operate `serial` interfaces in `reticulumd`, including startup checks,
expected behavior, and incident response.

## Scope

- Interface kind: `serial`
- Startup lifecycle: daemon bootstrap only (not hot-applied by RPC)
- Runtime mutation policy: `set_interfaces`/`reload_config` with `serial` changes require restart

## Required Config Fields

```toml
interfaces = [
  {
    type = "serial",
    enabled = true,
    name = "tty-primary",
    device = "/dev/ttyUSB0",
    baud_rate = 115200,
    data_bits = 8,
    parity = "none",
    stop_bits = 1,
    flow_control = "none",
    mtu = 2048,
    reconnect_backoff_ms = 500,
    max_reconnect_backoff_ms = 5000
  }
]
```

## Validation Rules

- `device` is required when enabled.
- `baud_rate` is required when enabled.
- `data_bits` allowed: `5`, `6`, `7`, `8`.
- `stop_bits` allowed: `1`, `2`.
- `parity` allowed: `none`, `even`, `odd`.
- `flow_control` allowed: `none`, `software`, `hardware`.
- `mtu` allowed range: `256..=65535`.
- `reconnect_backoff_ms` must be `>= 50`.
- `max_reconnect_backoff_ms` must be `>= reconnect_backoff_ms`.

## Runtime Behavior

1. The daemon opens the configured serial device.
2. Transport payloads are framed with HDLC.
3. On open/read/write errors, the interface logs and retries with configured backoff.
4. Malformed HDLC frames are dropped without panicking.
5. Worker shutdown cancels cleanly when daemon shutdown is requested.

Startup policy controls:

- Default mode is best-effort (daemon continues in degraded mode when some interfaces fail).
- `--strict-interface-startup` makes startup/preflight failures fatal.

## Health Signals

Expected startup log examples:

- `serial enabled iface=<hash> name=<name> device=<path> baud_rate=<rate>`
- `serial: opened device=<path> baud_rate=<rate> ...`

Degraded/failure signals:

- `serial startup rejected ...`
- `serial: failed to open device=...`
- `serial: read error ...`
- `serial: write error ...`
- `interface startup degraded started=<n> failed=<m> strict=<bool>`

Runtime status visibility:

- `list_interfaces` includes `_runtime.startup_status`.
- Failed interfaces include `_runtime.startup_error`.

## Incident Response

1. Verify device path exists and permissions are correct.
2. Confirm line settings match peer device (baud/data/parity/stop/flow).
3. Check cabling and adapter health.
4. If repeated open failures persist, disable the interface in config and restart daemon.
5. Capture daemon logs and include last `serial:` warnings for triage.

## Verification Commands

```bash
cargo test -p rns-transport serial::tests
cargo test -p reticulumd --test config
cargo test -p reticulumd --bin reticulumd
```

## Rollback

- Remove or disable `serial` interface entries from config.
- Restart `reticulumd`.
- Confirm only legacy TCP interfaces are active via `list_interfaces`.
