# RDO owner binding v1 (`rdo-core`)

This page covers [RDO #1](https://github.com/FreeTAKTeam/ReticulumDistributedObject/issues/1)
(Mission Library 2/9): the Rust side of the
[shared-Identity owner binding v1](https://github.com/FreeTAKTeam/ReticulumDistributedObject/blob/main/profiles/owner-v1.md).
RPP consumes it; RDO does not depend on RPP or LXMF.

`crates/libs/rdo-core` holds the strict deterministic CBOR codec and RNS
Identity arithmetic that `rpp-core` previously owned, so the dependency runs
`rpp-core -> rdo-core`. `rpp-core` re-exports both modules unchanged, and its
publication records are now verified by `rdo_core::Revision`.

| Module | Responsibility |
|---|---|
| `lookup` | `rdo.object.<hex token>` derivation, the 19-byte `CBOR([1, token])` announce payload, and `bind`: the signed token and public Identity must rebuild both the advertised and the requested Destination. With feature `rns`, `verify_announce` validates a raw RNS announce packet first. |
| `revision` | `[1, object, parent, type, body, signature]`: sign, verify, revision ID over the signed bytes, successor rule, Property/Relationship body structure. |
| `owner` | `/rdo/1/op` READ and PROPOSE. NOT_AUTHORIZED before anything is disclosed, replay keyed by (initiator, request ID) with exact-byte retries, conflicting reuse REJECTED, 1–300 s freshness, STALE with the current head, only the owner signs successors. The caller supplies a `Policy`. |
| `rns` (feature) | `register_object` (also sets the path-response app data), `resolve_object` (starting from only the Destination: path request, signed announce, binding), `open_object_link` (identified Link), `call`, and `OwnerService` serving every registered object. |

## Evidence

- `tests/owner_vectors.rs`: every binding Destination and announce payload;
  revisions verified and re-signed byte for byte; the invalid revision; all six
  operation cases (read, apply, foreign mutation, stale, identical and
  conflicting retry) reproduce the exact result bytes and commit counts.
- `tests/lookup_vectors.rs` (`--features rns`): the seven raw announce packets
  bind to their owner; the five negative lookups are refused.
- `tests/rns_owner.rs`: two Rust transports over loopback TCP. The client
  starts with only the Destination, then READ, PROPOSE, identical retry,
  STALE, a second client's foreign mutation (NOT_AUTHORIZED), and an offline
  read of the cached revision after the owner is gone.
- `tests/python_interop.rs` (ignored; needs `RETICULUM_PY_REPO`): Rust owner
  with a Python Reticulum client, and Python owner with a Rust client, both
  starting from only the Destination. The Python owner signs byte-identical
  successors. Passed against Python RNS 1.3.8.

```sh
RETICULUM_PY_REPO=../Reticulum cargo test -p rdo-core --all-features \
  --test python_interop -- --ignored --test-threads=1
```

## Not yet done

- Stamped path requests, announces and Link establishment against an enforcing
  peer ([#768](https://github.com/FreeTAKTeam/LXMF-rs/issues/768)).
- Owner replay and history are in memory; a durable store and the
  lost-response-after-restart case belong to the embedding application.
- Text items are capped at 1 KiB (the RPP limit); RDO itself only caps the
  whole revision at 48 KiB.
