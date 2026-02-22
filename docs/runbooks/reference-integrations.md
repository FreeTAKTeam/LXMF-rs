# Reference Integrations

This runbook defines three production-oriented reference integration patterns for SDK v2.5.

## Service Host Integration (`reticulumd`)

Reference artifact:

- `crates/apps/reticulumd/examples/service-reference.toml`

Intent:

- run `reticulumd` as a long-lived service process
- expose local RPC for colocated SDK clients
- enforce hardened runtime defaults and redaction

Baseline smoke command:

```bash
cargo run -p reticulumd -- --help
```

## Desktop App Integration (`lxmf-cli`)

Reference artifact:

- `crates/apps/lxmf-cli/examples/desktop-reference.toml`

Intent:

- model desktop host behavior with SDK profile and event polling defaults
- demonstrate key-management backend selection and redaction defaults

Baseline smoke command:

```bash
cargo run -p lxmf-cli -- --help
```

## Gateway Integration (`rns-tools`)

Reference artifact:

- `crates/apps/rns-tools/examples/gateway-reference.toml`

Intent:

- model gateway bridge runtime with explicit backpressure policy
- keep auth mode explicit for remote integrations

Baseline smoke commands:

```bash
cargo run -p rns-tools --bin rnprobe -- --help
cargo run -p rns-tools --bin rnx -- --help
```

## Reference Integration Smoke Suite

Run:

```bash
cargo run -p xtask -- reference-integration-check
```

This gate verifies that reference artifacts exist and baseline command surfaces stay runnable.
