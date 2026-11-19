# RPP engine and RNS Resource contract

This page covers [#766](https://github.com/FreeTAKTeam/LXMF-rs/issues/766)
(RNS Link/Resource APIs for RPP) and
[#767](https://github.com/FreeTAKTeam/LXMF-rs/issues/767) (the `rpp-core`
crate). The wire contract is
[RPP 0.3 RC1](https://github.com/FreeTAKTeam/rreticulum-package-protocol/releases/tag/v0.3.0-rc.1):
version `3`, Link request path `/rpp/3`.

## RNS Resource surface for RPP (#766)

RPP needs no daemon internals. Everything below is public on
`reticulum_rs_transport::transport::Transport`.

| RPP need | Public API |
|---|---|
| Send a manifest or bundle as one native Resource, logical bytes, bounded memory | `send_resource_from_reader_with_completion(link, reader, size, Some(metadata), false, observe)`. Only the current ≤1 MiB segment is read. `auto_compress = false` is the RC1 baseline. *(new)* |
| Observe the terminal outcome without the lossy event bus | The `observe` callback receives a `ResourceCompletion` before the first packet leaves; `wait()` returns `Complete`, `Failed`, `Rejected` or `Cancelled`. |
| Refuse unsolicited or over-budget inbound Resources before allocation | `set_link_resource_admission(link, Some(LinkResourceAdmission { max_data_size, max_concurrent }))`. Advertisements above the budget get a receiver cancel. The budget is dropped with the Link. *(new)* |
| Progress | `resource_events()`: `Progress`, `SegmentComplete`, `Complete`, `InboundFailed`, `Outbound*`. |
| Cancel | `cancel_resource(link, hash)` / `cancel_resource_on_link`. |
| Link requests and responses | `send_request_on_link` (client), `received_data_events()` + `send_response` (server), as `rngit` does. |
| Retry / timeouts | `TransportConfig::set_resource_retry_interval_secs`, `set_resource_retry_limit`. |

Global inbound caps (64 MiB, 8192 parts; #514) still apply first. RPP's own
caps are smaller (manifest ≤4 MiB, bundle ≤4 MiB, ≤8 outstanding per Link),
so a completed inbound Resource fits in memory by construction.

Regression evidence reused, not duplicated: #514 inbound limits, #552 MTU,
#553 metadata, #554 abandoned assembly and #610 mixed peer/stream semantics
are closed, with tests in `crates/libs/rns-transport/src/resource/tests*`.
Python interop for the reader sender is
`rust_reader_to_python_split_resource_roundtrip` (ignored unless a local
Python Reticulum checkout is present).

New tests: `resource::tests::link_admission::*` and
`transport::tests::reader_resource_completion_is_registered_before_dispatch`.

**Still open for #766:** a priority lane that keeps Resource traffic from
starving REM SOS/acknowledgement packets is not implemented here, and the
live loss/restart/mixed TCP-LoRa/Python matrix has not been rerun for RPP.

## `rpp-core` (#767)

`crates/libs/rpp-core` is transport-independent. It depends only on
`sha2`, `ed25519-dalek`, `x25519-dalek`, `rand_core`, `thiserror` and `log`:
no daemon, LXMF, RCH or REM code. Only the optional `rns` feature pulls in
`reticulum-rs-transport` (allowed boundary edge `rpp-core->reticulum-rs-transport`).

| Module | Responsibility |
|---|---|
| `cbor` (re-exported from `rdo-core`) | Strict RFC 8949 §4.2.1 deterministic codec: rejects floats, tags, negatives, indefinite lengths, duplicate/unsorted keys, overlong heads, trailing bytes, >12 nesting, >1 KiB text, and checks declared lengths before allocating. |
| `manifest`, `manifest_build` | Signed manifest build/verify: package ID, owner signature, H, file/chunk partition, path safety, directory collisions, extensions/critical IDs, RDO owner binding and `rpp.receive` derivation. `FileDraft::from_reader` hashes a file one chunk at a time. |
| `publication` | RDO revision verification for the Package, Manifest and File references, and comparison against the signed manifest. |
| `control`, `selection` | The seven commands, header rules, RDO v1 result wrapper and index/range selections. |
| `publisher` | Serves a Package: authorization on every request, recipient/inbox check, freshness, replay, plan building within window and bundle budget, single-use link-local plans, receipts. |
| `receiver` | `Pull`: cache-hit or ManifestRequest, MissingChunks for gaps only, plan validation, quarantine until APPLIED, bundle/chunk verification, resume after restart, promotion before `complete_verified`. |
| `inbox` | Answers Offers on `rpp.receive`; the sender must be the independently resolved owner. |
| `store` | `ChunkStore` / `PackageSource` traits; `FsStore` (atomic writes, re-verification on open, atomic promotion) and `DirSource`. |

The caller supplies the verified RDO owner binding, a clock (`now`), local
policy (`Policy` closure) and the transport.

### Running it over RNS (feature `rns`)

`rpp_core::rns` drives the engines over public `reticulum-rs-transport` APIs:

- `PublisherService::new(transport, publisher, clock).run()` answers `/rpp/3`
  Link requests (packet or request-Resource) on every in-Link whose
  Destination is a published Package. It takes the peer from Link
  identification, replies with the RDO result envelope through
  `send_response`, and sends each accepted manifest or bundle as one
  uncompressed native Resource with packed 16-byte metadata, releasing the
  window slot when the Resource completes. Link close drops link-local plans.
- `run_pull(transport, me, package_desc, &mut pull, timeout, clock)` opens and
  identifies a Link to the Package Destination, applies
  `LinkResourceAdmission` (4 MiB + framing, 8 concurrent), sends requests,
  feeds Resource completions on that Link into `Pull`, and tears the Link down.
  On error it drops link-local state; calling it again (also after a process
  restart with the same store) resumes from verified chunks over a new Link.
- `rdo_object_name(token)` gives the `rdo` / `object.<hex>` name to register
  Package Destinations with `add_destination`.

Callers still resolve the Package `DestinationDesc` and owner binding through
RDO, and register/announce Package Destinations.

### Conformance evidence

`cargo test -p rpp-core` runs with no network; add `--features rns` for the
loopback RNS test:

- `tests/rc1_vectors.rs`: all six packages rebuilt byte-for-byte from their
  files, including Ed25519 signatures, package IDs and H; all commands,
  results and additional controls; the three publication records; codec and
  manifest negative cases (truncated, duplicate key, overlong integer,
  trailing byte, wrong version, unknown critical, bad signature,
  overlapping ranges).
- `tests/transfer.rs`: the publisher reproduces the fixture result bytes;
  standalone transfer with a simulated crash after 40 of 101 chunks, then a
  restart that reuses the cached manifest (0 manifest bytes) and moves only the
  61 missing bytes to `complete_verified`; the fixture 17/63 resume (two
  bytes, payload `113f`); unauthorized, expired and false-completion
  refusals; replay retry and conflict; wrong-owner manifest, oversized plan,
  corrupt chunk, conflicting duplicate and unassociated Resources; three
  concurrent receivers; a 3 MiB disk file served from `DirSource`.
- `tests/rns_transfer.rs` (`--features rns`): two transports over loopback
  TCP. The publisher announces an RDO Package Destination; the receiver
  resolves it from the announce, opens an identified Link and pulls a
  50-chunk file. Authorization is withdrawn mid-transfer, the receiver
  "restarts", and a new Link resumes with the cached manifest (one
  ManifestRequest in total) to `complete_verified` and a promoted file.

### Not yet done

- Python/Rust network interop for RPP itself. The RDO network gate now
  passes in both directions; see [rdo-core](rdo-core.md).
- Transport-level loss injection and LoRa-MTU runs of the RNS test.
- Replay state is in memory; requests issued before a restart are refused
  through their expiry instead of persisted.
- Capability tokens are passed to the caller's policy unparsed (RC1 assigns
  them no format).
