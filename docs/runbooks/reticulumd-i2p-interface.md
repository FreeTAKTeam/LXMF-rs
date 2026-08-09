# `reticulumd` I2P Interface Runbook

## Purpose

This runbook documents the in-progress `I2PInterface` slice. It supports
configured outbound I2P peers and transient connectable accept sessions through
an I2P SAM bridge, with HDLC-framed Reticulum packets over each resulting
stream. It is not yet a production-complete I2P parity claim.

## Scope

- Reticulum type alias: `I2PInterface`
- SAM default endpoint: `I2P_SAM_ADDRESS` (`host:port`) when set and valid,
  otherwise `127.0.0.1:7656`
- Default MTU: `1064`
- Default bitrate: `256000`
- Runtime role: multicast parent with virtual unicast peer children
- Supported modes: outbound configured peers, transient connectable accept
  sessions
- Runtime status: startup metadata exposes an initial
  `_runtime.i2p.tunnel_status` schema with SAM endpoint, connectable state,
  accept-loop state, and per-peer tunnel rows.

## Configuration

```toml
interfaces = [
  {
    type = "I2PInterface",
    enabled = true,
    name = "i2p-main",
    peers = [
      "exampledestination.b32.i2p"
    ],
    sam_host = "127.0.0.1",
    sam_port = 7656,
    connectable = true
  }
]
```

The parser accepts Python-style `peers` as either a comma-separated string or a
string array. It also accepts `sam_ip` as an alias for `sam_host`,
`storagepath` as an alias for `state_path`, and `configured_bitrate` as the
interface bitrate used by announce pacing. When no explicit `sam_host`,
`sam_ip`, or `sam_port` is supplied, the default SAM endpoint follows Python's
`I2P_SAM_ADDRESS` environment variable if it contains `host:port`; explicit
config fields win over the environment. Python I2P-local `ifac_netname` and
`ifac_netkey` are accepted as aliases for the shared IFAC `network_name` and
`passphrase` fields; canonical shared fields win if both forms are supplied. If
no explicit `state_path` or `storagepath` is supplied, daemon startup injects
the Reticulum storage root for I2P destination-key persistence, matching
Python's `I2PInterface` setup.

## Runtime Behavior

Startup creates one multicast/controller interface and one virtual unicast
child per configured peer. Each child keeps a transient SAM stream session
open, resolves `.i2p` names through `NAMING LOOKUP`, and issues `STREAM
CONNECT` on a separate data socket for its peer destination.

With `connectable = true`, startup also creates a SAM stream session for
incoming peers and loops on `STREAM ACCEPT`. The generated private I2P
destination key is stored under the explicit `state_path`/`storagepath`, or
under the daemon Reticulum storage root when omitted, and reused on later
starts. During startup,
`_runtime.i2p.reachable_endpoint` reports the derived `.b32.i2p` address for
both already-persisted keys and keys generated in the same run. Each accepted
stream strips the remote-destination line that SAM prepends, registers a
virtual unicast child, and then hands the stream to the same HDLC packet
runtime.
SAM stream session IDs include the transport identity hash when available, so
multiple local daemons can share one SAM router without colliding on the same
interface-derived session ID. If a SAM bridge reports that an accept-loop ID is
no longer a stream session, the accept loop recreates the connectable session
instead of retrying a dead ID indefinitely.

Once the SAM stream is open, packets use the same HDLC framing as Reticulum's
stream interfaces. Direct outbound sends to a peer child use that peer's SAM
stream. Broadcast sends fan out to all configured peer children.

The I2P stream runtime uses the shared HDLC stream watchdog in opt-in mode:
idle streams emit empty HDLC keepalive frames, become `stale` when no reads are
observed past the Python-style probe window, and request a reconnect after the
read-timeout window. These events update the transport-side I2P tunnel status
model for configured peers and accepted incoming streams. Local fake-SAM tests
exercise the outbound peer loop through session creation, name lookup, stream
connect, HDLC writes, and refreshed byte counters, plus the connectable accept
loop through incoming `STREAM ACCEPT`, virtual child registration, HDLC ingress,
direct outbound egress over the accepted peer stream, runtime byte counters, and
cleanup without requiring a prepared I2P router.
`reticulumd`
periodically refreshes that model into the cached interface records returned by
`daemon_status_ex` and `list_interfaces` as
`_runtime.i2p.tunnel_status`.

Python `TCPClientInterface` and `TCPServerInterface` configs can also mark
plain TCP streams as carried through an external I2P tunnel with
`i2p_tunneled = true`. That flag does not create a SAM session; it applies
Reticulum's slower I2P TCP socket profile to outbound TCP clients and to
server-side accepted client streams: `TCP_NODELAY`, keepalive enabled, and on
Linux/Android a 45-second TCP user timeout, 10-second keepalive idle,
9-second keepalive interval, and 5 keepalive probes.

In normal startup mode, SAM failures are retried in the background. In strict
startup mode, the daemon preflights the SAM bridge and marks the interface
failed if the bridge is unreachable or does not complete `HELLO`.

## Status Schema

Runtime records include `_runtime.i2p.tunnel_status` with:

- `sam_endpoint`, `connectable`, and `configured_peer_count`
- `accept_state`, `accept_reconnect_attempts`, and `last_accept_error`
- `peers`, one row per configured or accepted peer, including `peer`,
  `direction`, `iface`, `state`, `reconnect_attempts`, `last_error`,
  `bytes_rx`, `bytes_tx`, and `keepalives_sent`

The initial startup snapshot is refreshed while the daemon is running, so
status consumers can observe peer connection, stale, reconnecting, timeout,
and counter changes without restarting the daemon.
Closed incoming peer rows are retained only as a bounded recent history, so
repeated transient inbound sessions cannot grow runtime status indefinitely.
When the parent I2P interface stops, configured and accepted virtual peer
children are removed from the interface manager, their route entries are
cleared, and runtime peer rows are marked `closed`.
`rnstatus-rs` human output summarizes the same tunnel status with the SAM
endpoint, accept-loop state, peer count, connected/stale/reconnecting peer
counts, outbound/incoming/closed row counts, aggregate tunnel bytes, and the
latest accept-loop error when present.

Current states are `configured`, `connecting`, `connected`, `listening`,
`reconnecting`, `stale`, and `closed`.

## Known Gaps

- Prepared-host runs without `I2P_PEERS` are scoped to SAM/connectable runtime,
  destination persistence, and status refresh. They do not prove outbound peer
  production parity.
- Full outbound peer production evidence requires a prepared-host run with
  reachable `I2P_PEERS` and connected outbound peer rows for every configured
  destination.
- The pair smoke below proves a single real-SAM router can connect two local
  daemon destinations. It does not replace broader public I2P peer-set or
  long-running production evidence.
- The pair smoke can optionally run a bounded soak with
  `I2P_PAIR_SOAK_SECS`; this proves the single-router connected rows remained
  stable for that requested local duration, not broad public peer-set parity.

## Software Fake-SAM Smoke

Use the software fake-SAM smoke to exercise the daemon and status tooling
without a real I2P router:

```bash
I2P_PEERS=peer-one.b32.i2p \
TIMEOUT_SECS=60 \
./tools/scripts/i2p-fake-sam-smoke.sh
```

The script starts a local fake SAM bridge on `127.0.0.1:0`, validates the SAM
`HELLO`, `DEST GENERATE`, `SESSION CREATE`, `NAMING LOOKUP`, `STREAM CONNECT`,
and `STREAM ACCEPT` command paths, then starts `reticulumd` with
`--strict-interface-startup` and polls both `rnstatus-rs --json` and human
`rnstatus-rs` output. The fake SAM bridge intentionally fails the first
`NAMING LOOKUP` for each configured outbound peer, then succeeds on retry; the
generated config sets `reconnect_backoff_ms = 100` so this recovery evidence is
fast and deterministic. A passing run requires:

- `_runtime.startup_status = "spawned"`
- `_runtime.i2p.reachable_endpoint` ending in `.b32.i2p`
- `_runtime.i2p.private_key_persisted = true`
- `_runtime.i2p.tunnel_status.accept_state = "listening"`
- `_runtime.i2p.tunnel_status.configured_peer_count` matching `I2P_PEERS`
- connected outbound peer rows with `direction = "outbound"` and non-empty
  `iface`
- recovered outbound peer rows with `reconnect_attempts >= 1` and
  `last_error = null`
- a connected incoming peer row for the fake `STREAM ACCEPT` remote
  destination, with `direction = "incoming"` and non-empty `iface`
- human `rnstatus-rs` output containing the I2P tunnel summary
  (`outbound=<I2P_PEERS count>` and `incoming=1`)

Evidence is written under `target/i2p-fake-sam-smoke/`, including
`report.json`, fake-SAM logs, daemon logs, generated config, captured JSON
status, and captured human status. The report records
`evidence_scope = "software_fake_sam_i2p_runtime"` plus a `product_boundary`
note. This smoke is software-only evidence for daemon integration, connectable
incoming-peer visibility, outbound retry recovery, and status refresh; it does
not replace the prepared-host router evidence below.

## Prepared-Host Smoke

Use the opt-in smoke harness on a host with a real local I2P router and SAM
enabled:

```bash
SAM_HOST=127.0.0.1 SAM_PORT=7656 ./tools/scripts/i2p-prepared-host-smoke.sh
```

To also collect outbound configured-peer evidence, provide one or more
comma-separated destinations that the prepared I2P router can connect to:

```bash
SAM_HOST=127.0.0.1 \
SAM_PORT=7656 \
I2P_PEERS=peer-one.b32.i2p \
./tools/scripts/i2p-prepared-host-smoke.sh
```

The harness starts `reticulumd` with `strict_interface_startup = true`
semantics via `--strict-interface-startup`, configures a connectable
`I2PInterface`, verifies the SAM `HELLO`, polls `rnstatus-rs --json`, and
requires `_runtime.i2p.reachable_endpoint`,
`_runtime.i2p.tunnel_status.accept_state = "listening"`, and persisted private
destination key metadata before passing. When `I2P_PEERS` is set, the harness
also requires `_runtime.i2p.tunnel_status.configured_peer_count` to match and
requires connected outbound peer rows for every configured destination.
Evidence is written under
`target/i2p-hil/`, including `report.json`, daemon logs, the generated config,
and the captured `rnstatus-rs` JSON. The report includes the expected outbound
peers, connected outbound peers, configured peer count, raw peer rows, and an
`evidence_scope` value. A no-peer run records
`evidence_scope = "sam_connectable_only"` plus a `product_boundary` note that
it is not outbound peer production parity. A run with reachable peers records
`evidence_scope = "sam_connectable_with_outbound_peers"` after all configured
outbound peer rows reach `connected`.

## Prepared-Host Pair Smoke

Use the pair smoke when a real local I2P router is available but no external
prepared peer set is ready:

```bash
SAM_HOST=127.0.0.1 \
SAM_PORT=7656 \
./tools/scripts/i2p-prepared-host-pair-smoke.sh
```

To keep the connected rows under observation for a bounded local soak, set
`I2P_PAIR_SOAK_SECS`:

```bash
SAM_HOST=127.0.0.1 \
SAM_PORT=7656 \
I2P_PAIR_SOAK_SECS=300 \
./tools/scripts/i2p-prepared-host-pair-smoke.sh
```

The harness starts two `reticulumd` instances against the same SAM endpoint.
The acceptor starts a connectable `I2PInterface` and publishes its
`_runtime.i2p.reachable_endpoint`; the dialer then starts a second connectable
`I2PInterface` with that live endpoint configured as its only peer. A passing
run requires the acceptor to show a connected incoming peer row and the dialer
to show a connected outbound peer row for the acceptor's `.b32.i2p`
destination. Evidence is written under `target/i2p-hil-pair/`, including both
daemon logs, both generated configs, both `rnstatus-rs` JSON snapshots, and a
combined `report.json`. The report records
`evidence_scope = "sam_connectable_with_outbound_peers_real_pair"` and a
`product_boundary` note that the run proves single-router prepared-host
connectivity, not broader public peer-set parity. When `I2P_PAIR_SOAK_SECS` is
greater than zero, the harness periodically refreshes both `rnstatus-rs` JSON
snapshots, requires the acceptor incoming row and dialer outbound row to remain
connected for the requested duration, writes `soak_samples` and
`soak_samples_jsonl`, and records
`evidence_scope = "sam_connectable_with_outbound_peers_real_pair_soak"`.
This is stronger local prepared-host stability evidence, but it still does not
replace broader public I2P peer-set or long-running production evidence.

The nightly HIL matrix runs the same harness through the `i2p` profile. Set
`HIL_I2P_SAM_HOST`, `HIL_I2P_SAM_PORT`, and
`HIL_I2P_TIMEOUT_SECS` as needed for the prepared runner; set `HIL_I2P_PEERS`
to enable configured outbound-peer proof. Unset host/port values fall back to
the local SAM default.

Nightly HIL can also run the two-daemon real-SAM pair through the `i2p-pair`
profile. Configure `HIL_I2P_PAIR_SAM_HOST`,
`HIL_I2P_PAIR_SAM_PORT`, and `HIL_I2P_PAIR_TIMEOUT_SECS` for the runner's SAM
router, and set `HIL_I2P_PAIR_SOAK_SECS` plus
`HIL_I2P_PAIR_SOAK_POLL_SECS` when bounded connected-row stability samples are
required. Artifacts are uploaded as `i2p-prepared-host-pair-artifacts`,
including `target/i2p-hil-pair/report.json` and `target/i2p-hil-pair/run.*`.
