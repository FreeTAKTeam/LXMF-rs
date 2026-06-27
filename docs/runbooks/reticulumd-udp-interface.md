# `reticulumd` UDP Interface Runbook

This runbook covers the software-visible UDP interface path in `reticulumd`.
It is focused on daemon startup, Python-compatible config aliases, runtime
status, and repeatable local evidence. It does not claim production multicast
or multi-host network validation by itself.

## Config Shape

Python-style `UDPInterface` configs normalize to the daemon `udp` interface
kind. The parser accepts `listen_ip`/`listen_port` for the local bind address
and `forward_ip`/`forward_port` for the optional fixed peer target:

```toml
[[interfaces]]
type = "UDPInterface"
enabled = true
name = "udp-loopback"
listen_ip = "127.0.0.1"
listen_port = 4242
forward_ip = "127.0.0.1"
forward_port = 4243
```

The Rust-native aliases are `host`/`port` and `target_host`/`target_port`.
When only a bind address is configured, the interface is receive-only and the
runtime role is `listener`. When a forward address is configured, the runtime
role is `peer`. Device-based UDP configs can derive IPv4 broadcast bind and
forward addresses from the named host interface.

## Runtime Status

`reticulumd` records startup state under `settings._runtime.startup_status`.
When a UDP interface is spawned, live daemon/RPC status refreshes
`settings._runtime.udp.status` with:

- `link_state`
- `role`
- `bind_addr`
- `forward_addr`
- `iface`
- `peer_routes`
- `packets_rx` / `packets_tx`
- `bytes_rx` / `bytes_tx`
- `decode_errors`
- `rx_queue_errors`
- `socket_errors`
- `tx_errors`
- `dropped_direct`
- `last_error`

`rnstatus-rs` renders the same status in compact human output as
`udp state=... role=... bind=...`, followed by optional counters and
`err=...` when present.

## Software Loopback Smoke

The software loopback smoke validates strict daemon startup, Python-style alias
parsing, UDP bind status, `rnstatus-rs` JSON/human reporting, and receive-side
decode-error telemetry without external network services:

```bash
./tools/scripts/udp-loopback-smoke.sh
```

The script chooses two local UDP ports, writes a Python-style `UDPInterface`
config using `listen_ip`, `listen_port`, `forward_ip`, and `forward_port`,
starts `reticulumd` with `--strict-interface-startup`, waits for a bound
runtime status row, then sends a malformed loopback UDP datagram to the daemon
bind port. A passing run requires:

- `_runtime.startup_status = "spawned"`
- `_runtime.udp.status.link_state = "bound"`
- `_runtime.udp.status.role = "peer"`
- `_runtime.udp.status.bind_addr = "127.0.0.1:<listen_port>"`
- `_runtime.udp.status.forward_addr = "127.0.0.1:<forward_port>"`
- `_runtime.udp.status.bytes_rx` to increase by at least the malformed
  datagram size
- `_runtime.udp.status.decode_errors >= 1`
- `_runtime.udp.status.last_error = "couldn't decode packet"`
- human `rnstatus-rs` output to include the bound row, forward target,
  `decode_errors=1`, and the decode error string

The smoke writes structured evidence under `target/udp-loopback-smoke/`,
including `report.json`, daemon logs, `rnstatus-rs` JSON/human output, and the
loopback probe payload metadata. This proves local UDP bind/status and
receive-side decode telemetry through the real daemon path. It is local-only
evidence. It is not a substitute for multi-host multicast evidence or
production broadcast-domain peer evidence.
