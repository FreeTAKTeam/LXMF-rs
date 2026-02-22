# `reticulumd` LoRa Interface Runbook

## Purpose

This runbook documents `lora` startup policy, state persistence behavior, and fail-closed recovery steps.

## Scope

- Interface kind: `lora`
- Startup lifecycle: daemon bootstrap only
- Runtime mutation policy: `set_interfaces`/`reload_config` with `lora` changes require restart
- Compliance posture: fail-closed on uncertain duty-cycle state

## Required Config Fields

```toml
interfaces = [
  {
    type = "lora",
    enabled = true,
    name = "lora-main",
    region = "US915",
    state_path = "var/reticulumd/lora-state.json",
    spreading_factor = 9,
    coding_rate = "4/5",
    bandwidth_hz = 125000,
    max_payload_bytes = 220
  }
]
```

## Validation Rules

- `region` required when enabled.
- Supported regions: `EU868`, `US915`, `AU915`, `AS923`, `IN865`, `KR920`, `RU864`.
- `state_path` required and non-empty when enabled.
- `spreading_factor` allowed range: `5..=12`.
- `coding_rate` allowed: `4/5`, `4/6`, `4/7`, `4/8`.
- `bandwidth_hz` must be one of supported LoRa bandwidth presets.
- `max_payload_bytes` allowed range: `1..=255`.

## State Persistence and Fail-Closed Policy

`state_path` stores duty-cycle debt and uncertainty markers.

Persistence guarantees:

1. State writes use `*.tmp` + rename.
2. Temporary file is `fsync`'d before rename.
3. Parent directory is `fsync`'d after rename.

Fail-closed conditions:

1. State payload unreadable/invalid JSON.
2. Unsupported state schema version.
3. State marked `uncertain`.
4. Startup clock rollback beyond uncertainty threshold relative to persisted timestamp.

When a fail-closed condition is hit, startup rejects interface activation and logs the reason.

Startup policy controls:

- Default mode is best-effort (daemon continues in degraded mode when some interfaces fail).
- `--strict-interface-startup` makes startup/preflight failures fatal.

## Operator Recovery

1. Confirm host clock integrity (NTP/system clock).
2. Inspect the persisted state file and reason.
3. If state is unrecoverable or uncertain, archive and replace/reset the state file.
4. Restart daemon and verify startup log reports `uncertain=false`.

## Health Signals

Expected startup log:

- `lora configured name=<name> region=<region> state_path=<path> duty_cycle_debt_ms=<n> uncertain=false`

Failure log:

- `lora startup rejected name=<name> err=<fail-closed reason>`
- `interface startup degraded started=<n> failed=<m> strict=<bool>`

Runtime status visibility:

- `list_interfaces` includes `_runtime.startup_status`.
- Failed interfaces include `_runtime.startup_error`.

## Verification Commands

```bash
cargo test -p reticulumd --test config
cargo test -p reticulumd --bin reticulumd lora::tests
cargo check -p reticulumd --all-targets
```

## Rollback

- Disable `lora` interface entries and restart daemon.
- Keep state files for forensic review before deletion.
