# `lxmf` Operator CLI

The Rust port now includes a production-oriented operator CLI binary:

```bash
cargo run --bin lxmf -- --help
```

The CLI targets `reticulumd` over framed msgpack RPC (`POST /rpc`) and event polling (`GET /events`).

## Global Flags

- `--profile <name>`: profile name (default `default`)
- `--rpc <host:port>`: override profile RPC endpoint
- `--json`: machine-readable output
- `--no-color`: disable colored tabular output
- `--quiet`: suppress non-error output
- `--verbose`: increase verbosity (`-v`, `-vv`, ...)

## Command Tree

- `lxmf profile init|list|show|select|import-identity|export-identity|delete`
- `lxmf daemon start|stop|restart|status|logs`
- `lxmf iface list|add|remove|enable|disable|apply`
- `lxmf peer list|show|watch|sync|unpeer|clear`
- `lxmf message send|list|show|watch|clear`
- `lxmf propagation status|enable|ingest|fetch|sync`
- `lxmf paper ingest-uri|show`
- `lxmf stamp target|get|set|generate-ticket|cache`
- `lxmf announce now`
- `lxmf events watch`
- `lxmf tui`

## Profiles and Runtime Files

Profiles are rooted at:

```text
~/.config/lxmf/profiles/<name>/
```

Files:

- `profile.toml`
- `reticulum.toml`
- `daemon.pid`
- `daemon.log`
- `identity`

`iface add/remove/enable/disable` edits profile `reticulum.toml`.
`iface apply` pushes interface state via RPC (`set_interfaces` + `reload_config` when available).

## Managed vs External Daemon

- Managed mode: `lxmf daemon start --managed` supervises `reticulumd` using the selected profile.
- External mode: point `--rpc` at an existing daemon; lifecycle commands are intended for managed profiles.

## Examples

Create and select a managed profile:

```bash
lxmf profile init ops --managed --rpc 127.0.0.1:4243
```

Start daemon and check status:

```bash
lxmf --profile ops daemon start --managed
lxmf --profile ops daemon status
```

Add an interface and apply:

```bash
lxmf --profile ops iface add uplink --type tcp_client --host 127.0.0.1 --port 4242
lxmf --profile ops iface apply
```

Send a message with `send_message_v2` semantics:

```bash
lxmf --profile ops message send \
  --source 00112233445566778899aabbccddeeff \
  --destination ffeeddccbbaa99887766554433221100 \
  --title "status" \
  --content "hello from lxmf"
```

## Compatibility

`lxmd` remains available for legacy flows and scripts.
