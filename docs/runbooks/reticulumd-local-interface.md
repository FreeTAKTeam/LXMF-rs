# `reticulumd` LocalInterface Runbook

## Purpose

This runbook documents the supported `LocalInterface` subset in `reticulumd`.
The current implementation provides Python-compatible TCP-loopback shared
instance listener/client-attach behavior plus Unix shared-instance listener and
client-attach behavior over the existing stream/HDLC runtime.

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

- `shared_instance_type` supports `tcp` and `unix`; the default is `tcp`.
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
  `bitrate`; the default is `1000000000`.

## Runtime Behavior

In TCP mode, when enabled without `--transport`, `local` is selected as the
active TCP listener and uses accepted per-client HDLC streams. If the configured
TCP shared-instance endpoint is already bound by another local process,
`reticulumd` attaches to it as a stream client and reports the interface as
attached. In Unix mode, `local` starts as its own configured listener and does
not consume the TCP bind selection. If the Unix endpoint is already bound,
`reticulumd` attaches to it as a local Unix client and retries the connection
after startup connect failures or later disconnects. TCP and Unix
shared-instance attach clients emit reconnect signals after a previously active
connection reappears; `reticulumd` responds by synthesizing the local Reticulum
tunnel packet again on that interface. The listener itself is reported as active
in `list_interfaces`; accepted client streams are handled by the shared stream
runtime.

When attached to an existing shared instance, outbound one-hop packets are
transport-wrapped before they are sent to the shared instance. This matches
Python Reticulum's local-client routing special case: destinations that would
normally be broadcast directly at one hop are injected into the shared
instance's transport path with Type 2 transport headers.

Expected startup log:

- `local enabled iface=<iface> bind=<host>:<port>`
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

## Verification Commands

```bash
cargo test -p reticulum-rs-transport shared_instance
cargo test -p reticulumd --test config local_interface
cargo test -p reticulumd --bin reticulumd local
cargo test -p reticulumd --test config
cargo test -p reticulumd --bin reticulumd
```

## Rollback

- Disable `local` interface entries and restart daemon.
- Confirm only intended remaining interfaces are active with `list_interfaces`.
