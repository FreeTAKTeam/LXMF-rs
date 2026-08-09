# `reticulumd` LocalInterface Runbook

## Purpose

This runbook documents the supported `LocalInterface` subset in `reticulumd`.
The current implementation provides Python-compatible TCP-loopback shared
instance listener/client-attach behavior plus Unix shared-instance listener and
client-attach behavior over the existing stream/HDLC runtime. Python-style
global `[reticulum] share_instance` config can also synthesize the shared local
interface when no explicit `LocalInterface` or `LocalClientInterface` entry is
configured.

## Scope

- Interface kind: `local`
- Reticulum type alias: `LocalInterface`
- Active transports: TCP loopback listener/client attach, Unix abstract
  shared-instance listener/client attach on Linux/Android, or explicit Unix
  filesystem socket listener with HDLC-framed accepted clients
- Runtime mutation policy: `set_interfaces`/`reload_config` changes require
  restart

## Config

Python-style LocalInterface:

```toml
interfaces = [
  {
    type = "LocalInterface",
    enabled = true,
    name = "local-main",
    shared_instance_type = "tcp",
    shared_instance_port = 37428
  }
]
```

Python-style global shared instance:

```toml
[reticulum]
share_instance = true
shared_instance_type = "tcp"
shared_instance_port = 37428
instance_name = "default"
force_shared_instance_bitrate = 1000000
```

Native `reticulumd` form:

```toml
interfaces = [
  {
    type = "local",
    enabled = true,
    host = "127.0.0.1",
    port = 37428,
    mtu = 262144
  }
]
```

Unix filesystem socket:

```toml
interfaces = [
  {
    type = "LocalInterface",
    enabled = true,
    name = "local-unix",
    shared_instance_type = "unix",
    socket_path = "/tmp/rns-default.sock",
    mtu = 262144
  }
]
```

Unix shared instance compatible with Python Reticulum on Linux/Android:

```toml
interfaces = [
  {
    type = "LocalInterface",
    enabled = true,
    name = "local-unix",
    shared_instance_type = "unix",
    instance_name = "default",
    mtu = 262144
  }
]
```

## Validation Rules

- `shared_instance_type` supports `tcp` and `unix`; explicit interface entries
  default to `tcp`. Global `[reticulum] share_instance` follows Python's shared
  instance default: Unix on AF_UNIX-capable platforms unless
  `shared_instance_type = "tcp"` is configured, otherwise TCP.
- `[reticulum] share_instance = false` disables the implicit shared local
  interface. Without a `[reticulum]` section, `reticulumd` preserves native
  behavior and does not synthesize an implicit local interface.
- TCP mode: `host` defaults to `127.0.0.1` and must be loopback:
  `127.0.0.1`, `::1`, or `localhost`.
- TCP mode: `port`, `listen_port`, or `shared_instance_port` select the
  listener port. The Python default is `37428`.
- Unix mode: explicit `socket_path` selects a filesystem socket. If omitted,
  `instance_name` derives the Python-compatible abstract address
  `@rns/<instance_name>` on Linux/Android, or a temp-dir filesystem socket on
  other Unix platforms.
- `mtu` defaults to Python's local MTU, `262144`.
- `fixed_mtu` is accepted as a compatibility alias for `mtu`.
- `force_shared_instance_bitrate` is accepted as a compatibility alias for
  `bitrate`; the default is `1000000000`. When configured, shared-instance TCP
  and Unix client streams pace outbound packet writes with Python's
  `len(packet) * 8 / bitrate` delay before HDLC framing.

## Runtime Behavior

In TCP mode, an explicit `local` listener enabled without `--transport` is
selected as the active TCP listener and uses accepted per-client HDLC streams
when no other TCP listener is selected. If the configured TCP shared-instance
endpoint is already bound by another local process, `reticulumd` attaches to it
as a stream client and reports the interface as attached. In Unix mode, `local`
starts as its own configured listener and does not consume the TCP bind
selection. If the Unix endpoint is already bound, `reticulumd` attaches to it as
a local Unix client and retries the connection after startup connect failures or
later disconnects. TCP and Unix shared-instance attach clients emit reconnect
signals after a previously active connection reappears; `reticulumd` responds by
synthesizing the local Reticulum tunnel packet again on that interface. The
listener itself is reported as active in `list_interfaces`; accepted client
streams are handled by the shared stream runtime.

When a Python-style `[reticulum]` section enables sharing and no explicit local
shared-instance interface is configured, config loading creates the equivalent
enabled `local` interface named `shared-instance`. That synthetic entry then
uses the same listener-or-attach startup path and reports normal
`list_interfaces` runtime status. If another configured `TCPServerInterface` or
`BackboneInterface` is selected as the primary daemon TCP listener, the
synthetic shared local TCP listener starts as a sidecar listener instead of
being treated as a conflicting explicit multi-listener configuration. If that
synthetic endpoint is already bound by another local shared instance, the
sidecar path attaches as a local client and reports `attached`. Explicit
multi-listener TCP configurations still use the primary single-bind selector and
remain rejected unless `--transport` supplies the active override.

When attached to an existing shared instance, outbound one-hop packets are
transport-wrapped before they are sent to the shared instance. This matches
Python Reticulum's local-client routing special case: destinations that would
normally be broadcast directly at one hop are injected into the shared
instance's transport path with Type 2 transport headers.

Expected startup log:

- `local enabled iface=<iface> bind=<host>:<port>`
- `synthetic local tcp sidecar enabled iface=<iface> name=<name> bind=<host>:<port>`
- `local attached iface=<iface> name=<name> endpoint=<host>:<port>`
- `local unix enabled iface=<iface> name=<name> socket_path=<path>`
- `local unix attached iface=<iface> name=<name> socket_path=<path>`

Runtime status visibility:

- `list_interfaces` includes `_runtime.startup_status = "active"` for local
  listeners or `"attached"` for TCP/Unix client attach.
- `list_interfaces` includes `_runtime.iface` with the active listener
  interface hash.
- Attached TCP/Unix local clients re-synthesize tunnel state after reconnects so
  peer shared-instance state can be refreshed without restarting `reticulumd`.
- Attached TCP/Unix local clients transport-wrap one-hop outbound packets before
  handing them to the shared instance.

## Software TCP Shared-Instance Smoke

```bash
./tools/scripts/local-interface-smoke.sh
```

The smoke creates one `LocalInterface` TCP loopback listener and one
`LocalClientInterface` TCP attach entry with
`shared_instance_type = "tcp"`, `fixed_mtu = 262144`, and
`force_shared_instance_bitrate = 1000000`. It starts a fake shared instance on
loopback, runs `reticulumd` with `--strict-interface-startup`, and records
`rnstatus-rs` JSON/human output plus fake-peer state under
`target/local-interface-smoke/`.

Passing evidence requires:

- `_runtime.startup_status = "active"` for the loopback listener.
- `_runtime.startup_status = "attached"` for the attach client.
- The configured loopback host/ports, local MTU, and bitrate alias to be visible
  through RPC status.
- The fake shared instance accepting the attach connection.
- Human `rnstatus-rs` output containing both local interface rows.

This is software-only coverage for local shared-instance startup/status
plumbing. It is not a substitute for multi-process Python shared-instance
interop or Unix-domain socket production validation.

## Software Unix Shared-Instance Smoke

```bash
./tools/scripts/local-interface-unix-smoke.sh
```

The Unix smoke creates two `LocalInterface` listener entries and one
`LocalClientInterface` attach entry with `shared_instance_type = "unix"`,
`fixed_mtu = 262144`, and `force_shared_instance_bitrate = 1000000`. It proves
a filesystem Unix listener, a Linux abstract Unix listener using
`@rns/<instance_name>`, and a Linux abstract Unix client attach to a fake shared
instance. The smoke runs `reticulumd` with `--strict-interface-startup` and
records `rnstatus-rs` JSON/human output plus fake-peer state under
`target/local-interface-unix-smoke/`.

Passing evidence requires:

- `_runtime.startup_status = "active"` for the filesystem Unix listener.
- `_runtime.startup_status = "active"` for the abstract Unix listener.
- `_runtime.startup_status = "attached"` for the abstract Unix attach client.
- The configured `socket_path`, local MTU, and bitrate alias to be visible
  through RPC status for each Unix row.
- The fake abstract shared instance accepting the attach connection.
- Human `rnstatus-rs` output containing all three Unix local interface rows.

The report records `evidence_scope = "software_unix_shared_instance_local"`
plus a `product_boundary` note: not multi-process Python shared-instance interop evidence.

## Pinned Python Shared-Instance Smoke

```bash
RETICULUM_PY_REPO=.tmp/python-refs/Reticulum \
./tools/scripts/local-interface-python-shared-smoke.sh
```

The Python shared-instance smoke starts real pinned Python Reticulum shared instances
from `RETICULUM_PY_REPO`: one TCP shared instance and one Linux abstract Unix shared instance.
It then starts `reticulumd` with two
`LocalClientInterface` attach entries using `fixed_mtu = 262144` and
`force_shared_instance_bitrate = 1000000`. After both Rust attach clients are
visible, the smoke starts one additional pinned Python traffic client per
shared instance and sends three `destination.announce` calls through each
shared instance. Artifacts are written under
`target/local-interface-python-shared-smoke/`.

Passing evidence requires:

- `_runtime.startup_status = "attached"` for the Rust TCP attach client.
- `_runtime.startup_status = "attached"` for the Rust abstract Unix attach
  client.
- The Python TCP shared instance to report `local_client_count >= 2`.
- The Python Unix shared instance to report `local_client_count >= 2`.
- Each Python traffic client to report `announced_count >= 3`.
- Each Python shared instance to report `local_client_rxb_total > 0`.
- Each Python shared instance to report `local_client_txb_total > 0`.
- `rnstatus-rs` JSON/human output containing both Rust local client rows.
- The report to include `python_rns_revision` for the pinned reference checkout.

The report records
`evidence_scope = "python_shared_instance_tcp_unix_attach_and_announce_forward"` plus a
`product_boundary` note: this proves attach interop with real pinned Python
Reticulum shared instances and Python-origin announce fanout through those
shared instances, but does not prove broad application-level shared-instance traffic parity.

## Reticulum Interface Parity Audit

```bash
./tools/scripts/reticulum-interface-parity-audit.sh
```

The audit reads the LocalInterface #384 reports plus the RNode BLE #385 reports
and writes `target/reticulum-interface-parity-audit/report.json` with
`evidence_scope = "reticulum_interfaces_384_385_parity_audit"`. For the
LocalInterface side it requires the TCP software smoke, Unix software smoke,
and pinned Python shared-instance smoke to pass, including the pinned
`python_rns_revision`, attached TCP/Unix rows, Python-origin
`announced_count`, and shared-instance `local_client_rxb_total` /
`local_client_txb_total` counters.

Use `./tools/scripts/reticulum-interface-parity-audit.sh --require-full` as the
strict gate. That mode exits non-zero while `missing_full_parity` contains
unmet evidence, including any missing serial, TCP/Wi-Fi, and BLE prepared-host
RNode hardware reports.

When the RNode hardware endpoints are available, collect the strict #385 matrix
and immediately rerun the audit with:

```bash
RIF_RNODE_SERIAL_PORT=/dev/ttyACM0 \
RIF_RNODE_TCP_PORT=tcp://192.0.2.10:8001 \
RIF_RNODE_BLE_PORT='ble://RNode 1234' \
./tools/scripts/reticulum-interface-hil-matrix.sh
```

The matrix runner writes
`target/reticulum-interface-hil-matrix/report.json` with
`evidence_scope = "reticulum_interfaces_384_385_hil_matrix"` and copies the
bearer reports to `target/rnode-hil/matrix/{serial,tcp,ble}.report.json`. It
also writes `target/reticulum-interface-hil-matrix/artifact-manifest.json` with
SHA-256 digests for the matrix report, strict audit report, and each bearer
report.
To re-run the strict audit against archived HIL artifacts with digest
verification, set
`RNODE_HIL_ARTIFACT_MANIFEST=target/reticulum-interface-hil-matrix/artifact-manifest.json`
or the `RIF_HIL_ARTIFACT_MANIFEST` / `RIF_ARTIFACT_MANIFEST` aliases; the audit
requires `schema = "reticulum_interface_hil_matrix_artifacts.v1"` and verifies
the `rnode_serial_report`, `rnode_tcp_report`, and `rnode_ble_report` SHA-256
digests before using those reports.
Strict matrix runs refresh the LocalInterface #384 smoke evidence by default
before the final `--require-full` audit, so clean HIL runners collect the full
evidence set in one pass. Use `--audit-existing` to re-check archived matrix
reports without touching devices or refreshing smokes; partial mode skips local
smokes unless `--run-local-smokes` or `RIF_RUN_LOCAL_SMOKES=true` is set.

Nightly HIL exposes this path through the `reticulum-interface-matrix` profile.
Configure
`HIL_RNODE_MATRIX_SERIAL_PORT`, `HIL_RNODE_MATRIX_TCP_PORT`,
`HIL_RNODE_MATRIX_BLE_PORT`, and optionally
`HIL_RNODE_MATRIX_RUN_LOCAL_SMOKES=true`; use bearer-specific overrides such as
`HIL_RNODE_MATRIX_SERIAL_FREQUENCY`, `HIL_RNODE_MATRIX_TCP_FREQUENCY`,
`HIL_RNODE_MATRIX_BLE_FREQUENCY`, and `HIL_RNODE_MATRIX_BLE_ADAPTER` when the
three prepared RNodes do not share the same radio or BLE settings. Artifacts
are uploaded as `reticulum-interface-hil-matrix-artifacts`, including the
artifact manifest.

## Verification Commands

```bash
cargo test -p reticulum-rs-transport shared_instance
cargo test -p reticulumd --test config local_interface
cargo test -p reticulumd --test local_interface_smoke_contract --quiet
cargo test -p reticulumd --test reticulum_interface_parity_audit_contract --quiet
TIMEOUT_SECS=45 ./tools/scripts/local-interface-smoke.sh
TIMEOUT_SECS=45 ./tools/scripts/local-interface-unix-smoke.sh
TIMEOUT_SECS=45 ./tools/scripts/local-interface-python-shared-smoke.sh
./tools/scripts/reticulum-interface-parity-audit.sh
cargo test -p reticulumd --bin reticulumd local
cargo test -p reticulumd --test config
cargo test -p reticulumd --bin reticulumd
```

## Rollback

- Disable `local` interface entries and restart daemon.
- Confirm only intended remaining interfaces are active with `list_interfaces`.
