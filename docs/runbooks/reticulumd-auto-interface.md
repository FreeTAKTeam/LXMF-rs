# `reticulumd` AutoInterface Runbook

This runbook tracks the Rust daemon surface for Reticulum's Python
`AutoInterface`.

## Current State

`reticulumd` accepts Python-style `AutoInterface` configuration and preserves
the Reticulum defaults in interface status:

- `group_id`: defaults to `reticulum`
- `discovery_scope`: defaults to `link`
- `discovery_port`: defaults to `29716`
- `data_port`: defaults to `42671`
- `multicast_address_type`: defaults to `temporary`
  - valid values are `temporary` and `permanent`; Python-compatible parsing
    falls back to `temporary` for unknown values
- `discovery_multicast_address`: derived with the Python `AutoInterface`
  algorithm from `group_id`, `discovery_scope`, and
  `multicast_address_type`; the default is
  `ff12:0:d70b:fb1c:16e4:5e39:485e:31e1`
- `devices` and `ignored_devices`: accepted as string arrays or comma-separated
  strings
- `configured_bitrate`: accepted as an alias for the common `bitrate` field

Python `AutoInterface` enumerates local network devices, selects link-local IPv6
addresses, joins the derived multicast group per device, exchanges peering
packets, and spawns per-peer UDP interfaces. The Rust daemon now enumerates host
link-local IPv6 interface candidates with
`if-addrs`, applies the Python-compatible `devices` and `ignored_devices`
selector, and records the resulting discovery/data listener startup plan in
interface runtime status, including the initial multicast `peer_announce`
packets that the live socket runtime must send per adopted interface. Those
planned sends include `payload_hex`, which is the exact Python-compatible
peering-token UDP payload for the target address and port, plus
`destination_host`, `destination_scope_ifname`, and
`destination_socket_target`, which brackets IPv6 destinations and scopes default
link-local multicast sends with `%ifname`. The daemon also reports
`planned_initial_peer_announce_count`, and its send hook returns deterministic
destination-tagged errors when a future socket sender fails. The initial
peer-announce bridge can now resolve structured host/port/scope targets into
socket addresses through an injected interface-index resolver and send the
payloads through a supplied UDP socket. The daemon also has a native resolver
that reads interface indexes from `if-addrs`, and `_runtime.auto` records
`native_scope_id_source = "if-addrs interface index"` so scoped IPv6 multicast
and reverse-unicast sends have a concrete OS-backed lookup path. Runtime status
also includes
`planned_discovery_socket_binds` for the unicast and multicast discovery
listener sockets the daemon must own. The daemon can now bind planned unicast
discovery sockets from those targets, and it has a staged multicast discovery
socket binder that resolves link-scope joins to an interface index before
joining the multicast group. Bound discovery sockets can now receive a UDP
datagram into typed AutoInterface metadata containing socket kind, interface
name, bind address, optional multicast group, source address, and raw payload.
The daemon can feed that typed datagram into the shared authenticated discovery
state helper, preserving the source address while classifying local multicast
echoes, remote peer additions/refreshes, and invalid-token rejections.
The daemon also reports planned peer data socket binds for each adopted
interface, can bind those `data_port` sockets with native scope IDs, receives
typed peer data datagrams, and classifies known-peer, duplicate, and
unknown-peer inbound packets through the shared discovery/deduplication state.
Enabled `auto` startup now attempts to bind those discovery sockets with native
scope IDs, sends the initial multicast `peer_announce` packets, starts the
cancellable receive loops, starts the repeat multicast `peer_announce`
scheduler, starts the peer-job scheduler, starts peer data receive loops, and records
`auto_discovery_runtime.bound_socket_count`, `receive_loop_count`,
`initial_peer_announce_count`, `repeat_peer_announce_scheduler_count`, and
`peer_job_scheduler_count`, `data_socket_count`, and
`data_receive_loop_count` in interface runtime metadata. Accepted peer-data
packets are injected into the normal transport ingress path through per-peer
virtual interfaces, and direct/broadcast transport sends are serialized and
routed back out over the matching peer UDP data sockets.

The reusable transport layer now also includes Python-compatible helpers for:

- descoping link-local IPv6 addresses such as `fe80::1234%eth0`
- deriving peering tokens as `full_hash(group_id || link_local_address)`
- planning outbound multicast peering packets for `peer_announce` and reverse
  unicast peering packets for `reverse_announce`, including the correct token,
  target address, and `discovery_port + 1` reverse port
- planning spawned peer packet delivery targets as `peer_address%ifname` on the
  configured `data_port`
- planning per-adopted-interface UDP listener bind targets as
  `link_local_address%ifname` on the configured `data_port`
- planning per-interface unicast and multicast discovery listener bind targets,
  including Windows' empty-host discovery socket binds and link-scope multicast
  interface scoping
- aggregating Python `final_init` startup targets into one plan: discovery
  listeners, data listeners, peer-job interval, and initial peering wait
- tracking Python's runtime gate for `final_init_done` and `online`: discovery
  packets are ignored until the initial peering wait completes, while spawned
  peer inbound packets require the interface to be online
- tracking Python's `carrier_changed` runtime flag when multicast carrier
  lost/recovered events occur or link-local address replacement requires a
  listener restart
- updating adopted link-local address state when an interface address changes
  and returning the replacement listener binding for runtime restart
- verifying incoming discovery packets against the sender address before they
  update local echo or remote peer state, including Python's behavior of
  comparing only the first full-hash bytes of the packet payload
- tracking peer add, refresh, strict timeout expiry, and reverse-peering due
  times with Python-compatible timing semantics
- planning `peer_jobs` maintenance without mutating live state: timed-out peer
  removal first, reverse peering only for still-live peers, and diagnostics for
  adopted interfaces that have not produced an initial multicast echo
- executing the peer-job state transitions a live scheduler needs: remove stale
  peers, mark reverse-peering sends so they are not repeated in the same
  interval, and update multicast carrier timeout state
- scheduling multicast `peer_announce` packets per adopted interface with
  Python's immediate first send and `ANNOUNCE_INTERVAL` repeat behavior
- exposing Python-compatible timing defaults for announce, peer job, peering
  timeout, reverse peering, initial discovery wait, multicast echo, and
  multi-interface duplicate suppression windows, including Android's 1.25x
  peering-timeout multiplier
- constructing discovery state and multi-interface duplicate suppression
  directly from the shared Python-compatible timing profile
- applying Python-compatible `devices` and `ignored_devices` filtering, including
  Darwin and Android default interface skip lists
- selecting descoped per-interface `fe80:` IPv6 link-local addresses from
  adopted interface candidates
- daemon-side OS interface enumeration of operational link-local IPv6
  candidates, adoption through the shared selector, and `_runtime.auto`
  startup-plan plus initial peer-announce reporting for enabled `auto`
  interfaces, backed by a reusable send hook over structured
  destination/payload datagrams with host/port/scope metadata and
  destination-tagged failure reporting, plus a supplied-UDP-socket send bridge
  that resolves interface scopes through either an injected interface-index
  lookup or the native `if-addrs` interface-index resolver
- `_runtime.auto` planned discovery socket bind reporting for per-interface
  unicast and multicast discovery listeners, plus a staged unicast discovery
  socket binder that resolves scoped bind addresses through an injected
  interface-index lookup
- staged multicast discovery socket bind/join resolution that binds multicast
  sockets on the unspecified address, joins the derived multicast group with
  the correct link-scope interface index, and reports deterministic target
  errors before the live receive loop is connected
- typed discovery datagram receive bridging from bound discovery sockets,
  preserving socket kind, interface name, bind address, multicast group, source
  address, and raw payload for the shared authenticated discovery helper
- daemon-side authenticated discovery datagram processing that classifies local
  multicast echoes, accepted peer events, and invalid-token rejects before the
  long-running receive loop is connected
- a cancellable daemon receive-loop primitive that reads bound discovery
  sockets, authenticates each datagram into shared discovery state, and reports
  accepted/rejected outcomes through a channel
- `_runtime.auto` planned peer data socket bind reporting for per-interface
  `data_port` listeners, plus a native-scope data socket binder and typed
  peer-data datagram receive bridge
- a cancellable daemon peer-data receive-loop primitive that reads bound data
  sockets, classifies inbound datagrams with the shared known-peer and
  duplicate-suppression state, and reports accepted/duplicate/unknown decisions
  through a channel
- daemon startup that binds native-scope discovery sockets, starts the receive
  loops under a discovery listener supervisor, starts the repeat multicast
  peer-announce scheduler, peer-job scheduler, and peer-data receive loops,
  injects accepted peer-data packets into transport, routes direct/broadcast
  transport sends to peer UDP data sockets, and records discovery/data runtime
  counts
- a supervised discovery listener primitive that groups the unicast and
  multicast discovery receive loops for each adopted interface, lets startup
  manage listener tasks per interface, cleanly stops managed loops, and tracks
  replacement-stop tasks so daemon shutdown drains dynamically replaced
  listeners
- a supervised peer-data listener restart primitive that stops only the
  affected interface listener, binds the replacement link-local data listener,
  prunes stale outbound routes that referenced the old socket, and records the
  link-local update in carrier runtime status. Loopback bridge tests prove that
  direct per-peer sends stop after listener removal/restart and resume only
  after the peer is accepted on the replacement socket. Replaced peer-data
  listeners are retained as daemon-owned stop tasks until restart, removal, or
  runtime shutdown awaits them.
- a polling OS interface-address reconciler that re-enumerates link-local
  candidates for already adopted devices, invokes the supervised listener
  restart when an adopted address changes, and commits the new discovery state
  only after the replacement listener restart succeeds
- transport-side adopted-interface change planning for added, removed, and
  link-local-replaced devices, with explicit apply semantics that clear stale
  echo, timeout, announce, and removed-peer state only after the daemon has
  completed the required listener lifecycle work
- daemon-side polling reconciliation for adopted-interface add/remove changes
  while the AutoInterface runtime is active: added interfaces bind discovery
  and data listeners before state is committed, and removed interfaces stop the
  supervised listeners before stale peer/interface state is cleared
- zero-initial AutoInterface startup keeps the polling reconciler and
  multicast/reverse announce schedulers alive even when no devices are adopted
  at startup, so a later matching link-local device can be adopted by the live
  daemon runtime
- live `_runtime.auto.carrier_runtime` reporting for Python-style `online`,
  `final_init_done`, `carrier_changed`, multicast carrier events, and staged
  link-local listener restart metadata
- classifying local multicast echoes separately from remote peers so discovery
  packets from this node's own link-local addresses update echo state instead
  of spawning peer state
- tracking multicast echo timeout state with Python's strict `MCAST_ECHO_TIMEOUT`
  boundary and carrier-lost/carrier-recovered transitions
- suppressing duplicate inbound packets seen across multiple peer interfaces
  for Python's `MULTI_IF_DEQUE_TTL` window while retaining the 48-entry
  `MULTI_IF_DEQUE_LEN` history
- deciding spawned peer inbound delivery for a live UDP path: reject unknown
  peers, suppress duplicates without refreshing peer state, and refresh known
  peers only when their packet is accepted

## Example

```toml
interfaces = [
  { type = "AutoInterface", enabled = true, name = "auto-main" }
]
```

```toml
interfaces = [
  { type = "AutoInterface", enabled = true, name = "field-net", group_id = "field-net", discovery_scope = "global", discovery_port = 48555, data_port = 49555, multicast_address_type = "permanent", devices = ["wlan0", "eth1"], ignored_devices = "tun0,eth0" }
]
```

## Prepared-Host Churn Smoke

The opt-in prepared-host smoke exercises the live AutoInterface reconciler on a
Linux host with `ip netns`, dummy interfaces, and permission to manage network
namespaces. It starts `reticulumd` in an isolated namespace with a zero-initial
`AutoInterface`, then churns one link-local interface through add, link-local
replacement, and removal.

```bash
AUTO_CHURN_DEVICE=lxauto0 \
AUTO_CHURN_INITIAL_ADDR=fe80::1200 \
AUTO_CHURN_REPLACEMENT_ADDR=fe80::1201 \
./tools/scripts/auto-interface-prepared-host-smoke.sh
```

The script validates refreshed RPC status with `rnstatus-rs --json` after each
phase. Required evidence fields include:

- `_runtime.auto.carrier_runtime.adopted_add_count`
- `_runtime.auto.carrier_runtime.adopted_remove_count`
- `_runtime.auto.carrier_runtime.link_local_replacement_count`
- `_runtime.auto.carrier_runtime.last_adopted_change`
- `_runtime.auto.carrier_runtime.adopted_devices`

Artifacts are written under `target/auto-interface-hil/`, including
`report.json`, the daemon log, the last `rnstatus-rs` JSON payload, and phase snapshots
for zero-initial startup, add, replacement, and removal. The report records
`evidence_scope = "linux_namespace_dummy_churn"` plus a `product_boundary`
note that broader prepared-host parity still requires evidence across real
Wi-Fi, Ethernet, and platform interface churn. The nightly HIL matrix exposes
this through the `auto-interface` profile and retains the prepared-host
artifacts.

## Operational Follow-Up

- AutoInterface dynamic add/remove now has daemon-side lifecycle application,
  including zero-initial startup polling and tracked shutdown for dynamically
  replaced discovery/data listeners. The Linux namespace prepared-host churn
  smoke records add/remove/link-local replacement evidence, while broader
  prepared-host evidence across real Wi-Fi/Ethernet devices and platforms
  remains follow-up.
