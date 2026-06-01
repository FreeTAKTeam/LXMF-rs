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
- `discovery_multicast_address`: derived with the Python `AutoInterface`
  algorithm from `group_id`, `discovery_scope`, and
  `multicast_address_type`; the default is
  `ff12:0:d70b:fb1c:16e4:5e39:485e:31e1`
- `devices` and `ignored_devices`: accepted as string arrays or comma-separated
  strings
- `configured_bitrate`: accepted as an alias for the common `bitrate` field

The full runtime is not complete yet. Python `AutoInterface` enumerates local
network devices, selects link-local IPv6 addresses, joins the derived multicast
group per device, exchanges peering packets, and spawns per-peer UDP interfaces.
The Rust daemon reports an explicit startup failure for enabled `auto`
interfaces until that OS-dependent discovery and peering runtime exists.

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
  and returning the replacement listener binding the runtime must restart
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

## Remaining Runtime Work

- Enumerate OS network interfaces and apply the existing selector to the live
  interface list.
- Join the derived Reticulum multicast discovery group.
- Create discovery and data sockets from the existing startup plan, join the
  derived Reticulum multicast discovery group, start peer-job scheduling, wait
  for the initial peering window, advance the runtime gate, and route valid
  packets through the outbound peering packet planner and authenticated
  discovery helper.
- Spawn per-interface UDP listeners and per-peer UDP packet paths on
  `data_port`, then wire them through the existing listener, outbound target,
  and inbound delivery helpers.
- Restart per-interface UDP listeners from the link-local replacement helper
  when the live address for an adopted interface changes.
- Use the shared timing profile when scheduling duplicate-suppression work.
- Wire the multicast announce scheduler into the live socket runtime.
- Wire the peer-job execution helper into the live socket runtime.
- Wire the spawned peer inbound delivery helper into live UDP packet delivery.
- Wire the runtime `carrier_changed` flag into live status/reporting.
