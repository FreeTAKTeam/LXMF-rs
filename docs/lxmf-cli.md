# `lxmf` Operator CLI

The Rust port now includes a production-oriented operator CLI binary:

```bash
cargo run --bin lxmf -- --help
```

The CLI targets `reticulumd` over framed msgpack RPC (`POST /rpc`) and event polling (`GET /events`).
The stable CLI RPC contract is documented in `docs/rpc-contract.md`.

## Global Flags

- `--profile <name>`: profile name (default `default`)
- `--rpc <host:port>`: override profile RPC endpoint
- `--json`: machine-readable output
- `--quiet`: suppress non-error output

## Command Tree

- `lxmf profile init|list|show|select|set|import-identity|export-identity|delete`
- `lxmf contact list|add|show|remove|import|export`
- `lxmf daemon start|stop|restart|status|logs`
- `lxmf iface list|add|remove|enable|disable|apply`
- `lxmf peer list|show|watch|sync|unpeer|clear`
- `lxmf message send|list|show|watch|clear`
- `lxmf propagation status|enable|ingest|fetch|sync`
- `lxmf paper ingest-uri|show`
- `lxmf stamp target|get|set|generate-ticket|cache`
- `lxmf announce now`
- `lxmf events watch`

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

Set or clear your local announce display name:

```bash
lxmf --profile ops profile set --display-name "Tommy Operator"
lxmf --profile ops profile set --clear-display-name
```

Search peers and inspect details:

```bash
lxmf --profile ops peer list --query alice --limit 10
lxmf --profile ops peer show alice
lxmf --profile ops peer show 6b33cafe --exact
```

Manage contacts and use aliases for sending:

```bash
lxmf --profile ops contact add alice 6b3362bd2c1dbf87b66a85f79a8d8c75 --notes "team relay"
lxmf --profile ops contact list --query ali
lxmf --profile ops message send --source my-self --destination @alice --content "hi"
lxmf --profile ops contact export ./contacts.json
```

When `--source` is omitted, `lxmf` uses daemon-reported `identity_hash` automatically.

Send a message with `send_message_v2` semantics:

```bash
lxmf --profile ops message send \
  --source 00112233445566778899aabbccddeeff \
  --destination ffeeddccbbaa99887766554433221100 \
  --title "status" \
  --content "hello from lxmf"
```
