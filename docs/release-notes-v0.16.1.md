# LXMF-rs v0.16.1

The 0.16.1 release train aligns the daemon, SDK and all 22 public Rust packages,
including the new `r3akt-event-broker` library. It carries the changes since
v0.16.0 and the subsequent v0.16.1 Linux candidate into the full Linux, macOS
and Windows distribution. Binary bundles, the OCI image and crates.io packages
use the same version and source tag.

## Changes since v0.16.0

- Durable event storage is now available as the transport-independent
  [`r3akt-event-broker`](../crates/libs/r3akt-event-broker/README.md) library.
  The standalone API and LXMF adapter share journal, authenticated receipt,
  admission and recovery behavior. Existing RPC broker and SDK durable-broker
  import paths remain available through re-exports.
- Opportunistic LXMF delivery proofs are deferred until delivery admission and
  durable storage succeed. Accepted duplicates can regenerate the original
  wire-hash proof without another inbound delivery. Local-client Data and
  destination proofs use the shared owner's bound return route even when
  ordinary transit forwarding is disabled. Transport proofs, durable custody
  ACKs and application responses retain their distinct meanings.
- MeshChat HTTP requests, static assets and WebSocket upgrades validate the
  local request host and browser origin. Configured interface discovery and live
  display-name updates are also included. See the upgrade requirements below.
- Invalid startup interfaces remain visible as sanitized failed records when
  credential-only IFAC configuration or I2P-local aliases accompany a malformed
  carrier field. Nonempty legacy I2P IFAC aliases retain literal whitespace and
  remain the fallback for empty canonical credentials. Native RNode BLE startup
  validation failures now publish an offline state and the error before reconnect.
- Propagation `/get` and control responses respect the negotiated Link MDU and
  use Resource transfer for larger responses. Control and Resource workers
  continue after broadcast lag. Outbound hashes enter duplicate membership only
  after carrier acceptance, preserving retry and ingress behavior after rejection.
- Propagation expiry, peer quota and handled-ID queries use indexes to reduce
  scans and temporary sorts. Global quota selection uses two additional ordered
  partial indexes to avoid sorting retained history for small excesses. Refill
  respects the global peer-entry budget; payload pruning releases the writer
  between bounded candidate windows. An indexed dispatch deadline excludes
  future-deferred operations from empty durable-dispatch polls.
- Announce retention runs in bounded batches while preserving each peer's latest
  event and stamp-cost information. Durable consumers can reconcile bounded
  latest-per-peer pages so busy peers cannot hide quiet peers; the legacy
  latest-32 history response remains available.
- Systemd readiness notification follows successful RPC and ZeroMQ listener
  binding, with graceful SIGTERM shutdown in the same service lifecycle. Expired
  remote destination and peer caches are pruned while preserving live links,
  local destinations and pending proof contexts. `lxmd` inbound hooks follow
  the daemon event stream.
- Designated protocol cores have isolated bare-metal feature checks that detect
  accidental `std` unification. The daemon uses mimalloc and exposes a
  requested-Rust-allocation gauge. Experimental guarded SQLite pager-cache
  release remains subject to the resource acceptance limits below. Performance
  reports compare matched work, and resident-evidence replay rejects invalid
  timing, overlapping captures and reset maintenance counters.

## Distribution and verification

The full release workflow builds these targets:

| Platform | Targets | Distribution |
| --- | --- | --- |
| Linux | x86-64, AArch64, ARMv7 hard-float | Static musl archives; `.deb` and `.rpm` packages for `amd64`, `arm64` and `armhf` |
| Windows | x86-64 MSVC | ZIP archive and x64 MSI |
| macOS | Intel x86-64 and Apple Silicon | Architecture archives and a universal archive |
| OCI | Release container images | `ghcr.io/freetakteam/lxmf-rs:0.16.1`; the final release also updates `latest` |

Asset names begin with `lxmf-rs_0.16.1_`. The public library, facade, embedded
and command packages listed in the [publish plan](runbooks/crates-io-publish-plan.md)
all target crates.io version `0.16.1`. Publication status and exact tagged build
results belong to the GitHub release record; this source document does not
assert that an unfinished build or registry upload has succeeded.

Download `SHA256SUMS.txt` with the selected assets and verify before installation:

```bash
sha256sum --check --ignore-missing SHA256SUMS.txt
```

The release workflow supplies a keyless cosign checksum bundle, build
attestations, component and container SBOMs, and `release-build-source.json`.
The source record identifies the release tag, product commit, packaging workflow
commit and run, and OCI digest. Check the product commit against the annotated
`v0.16.1` tag. Native Windows/macOS code signing depends on the configured release
credentials and is separate from the checksum signature and build provenance.

## Upgrade and compatibility

Rust 1.88 remains the workspace minimum; the Bluetooth leaf retains its Rust
1.85 contract. The canonical compatibility baseline remains Python Reticulum
1.5.2 at `ea98db4f53dcf0defc0e71a16e60d28b1229c4e6` and Python LXMF 0.9.6 at
`727830cefda83d9c6e3982b48675425f3f988f9c`. Forward-version deltas and the SDK
contract retain their documented boundaries.

Preserve identities, configuration, databases and paired consumer state. Stop
writers and take SQLite-consistent backups before replacing a daemon. Follow the
[v0.14.0 migration guide](migrations/v0.14.0-zmq.md) and
[durable-broker operations](goals/zmq-rch-durable-broker/OPERATIONS.md) for custody,
checkpoints, restore and downgrade. Do not open a migrated store with an older
binary as a rollback shortcut; restore a consistent backup and account for
already admitted work.

Existing databases perform index creation at startup before traffic admission.
The durable-dispatch index changes from `(state, message_id)` to
`(state, next_attempt_at, message_id)`. Propagation storage adds two ordered
partial indexes on `(updated_at, transient_id, peer)` for unhandled and completed
marks. Allow for one-time startup work, temporary space, WAL and persistent disk
space. Index creation is transactional; the propagation indexes also add ordinary
write-maintenance cost. See the [quota-index migration](migrations/propagation-global-quota-order-indexes.md).
Retry deadlines, quota ordering, limits and custody semantics remain unchanged.
Completed peer history still counts against the global peer-entry limit and can
defer refill until capacity is available.

MeshChat clients must use the listener IP or `localhost` and its actual port in
`Host`; IPv6 literals need brackets. Browser `Origin`, when supplied, must use
HTTP and the same hostname and effective port. `localhost` and an IP literal
are separate origins. Cross-site and same-site Fetch Metadata requests are
rejected with HTTP 403 before routing or WebSocket upgrade. Forwarded headers
do not override these checks; reverse-proxy and cross-origin UI deployments are
unsupported. Serve the UI and API from the same local endpoint. Non-browser
clients still need a valid host. This policy does not authenticate other local
processes; see the [MeshChat runbook](runbooks/meshchat-api.md).

## Evidence boundaries and remaining work

The source includes focused regressions for the changes above. Required CI,
reference compatibility, independent interoperability, release builds and
post-download smoke results must be checked for the exact released source.
Earlier PR results and historical performance reports are not substitutes for
that release evidence.

The carrier-family lifecycle matrix under [#608](https://github.com/FreeTAKTeam/LXMF-rs/issues/608),
broader shared-instance and repeated recovery matrix under
[#609](https://github.com/FreeTAKTeam/LXMF-rs/issues/609), and current-release
Columba/Pixel and Ratspeak/Windows delivery acceptance under
[#719](https://github.com/FreeTAKTeam/LXMF-rs/issues/719) remain separate from the
focused software fixes. `no_std` support applies to the designated cores and
feature sets. MeshChat remains a partial application API. Hardware and
public-network qualification are not established by software or packaging checks.

[#663](https://github.com/FreeTAKTeam/LXMF-rs/issues/663) and
[#709](https://github.com/FreeTAKTeam/LXMF-rs/issues/709) retain production resource,
sustained memory and delivery acceptance limits. Stored resident-mapping evidence
passed attribution, but the tagged baseline failed RSS-window stability; SQLite
cache release has no completed stability qualification. This release does not
claim a new soak, production deployment or broad high-load acceptance. See the
[resource investigation](performance/rch-resource-fixes-663.md),
[roadmap](status/current-roadmap.md) and maintained parity matrices.

[Full changelog](https://github.com/FreeTAKTeam/LXMF-rs/compare/v0.16.0...v0.16.1).
