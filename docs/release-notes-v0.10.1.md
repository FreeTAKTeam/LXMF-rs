# LXMF-rs v0.10.1

v0.10.1 is the maintenance release after the RNS 1.5.0-aligned v0.10.0
release. It updates the compatibility baseline to Python Reticulum 1.5.2 and
keeps physical hardware, public-network, and third-party-client validation as
separate evidence tracks.

## Reticulum 1.5.2 alignment

The release is compared with these exact Python references:

- Reticulum `1.5.2` at
  `ea98db4f53dcf0defc0e71a16e60d28b1229c4e6`;
- LXMF at `727830cefda83d9c6e3982b48675425f3f988f9c`.

The strict generated inventory contains 1,858 entries: 1,857 applicable and
complete, zero partial, zero unmapped, and one provenance-backed not-applicable
`CRNS` package. The historical RNS 1.5.0 rows and the 1.5.1/1.5.2 maintenance
delta are recorded in [`docs/status/rns-1.5-delta.md`](status/rns-1.5-delta.md).

Highlights include:

- RNS 1.5.2 queue defaults (data 1024, announce 128, path request 128,
  ingress-limited 8) and exclusion of shared LocalClient interfaces from
  Backbone dataplane control;
- empty HDLC keepalive frames discarded before packet admission;
- optimized and legacy IFAC framing helpers with deterministic pinned-Python
  vectors, while daemon IFAC configuration remains fail-closed until a carrier
  opts into the codec;
- process-wide profiler capture and `Transport::get_profiling_results` status
  snapshots; and
- the owned-buffer Resource sender boundary that is equivalent to the RNS
  1.5.2 stream-initialization fix.

## Compatibility boundaries

The pre-1.0 maintenance release advances the workspace and publishable crates
to `0.10.1`. The additive transport status/profiling fields do not change the
existing SDK capability contract. Consumers that deserialize SDK/RPC responses
should continue ignoring unknown fields and regenerate schema-derived clients
when applicable. Source and configuration notes are in the
[`v0.10.1 migration guide`](migrations/v0.10.1-rns-1.5.2.md).

Reticulum IFAC authentication is exposed as a tested transport-library codec,
but daemon interface configuration still rejects IFAC credentials rather than
creating a carrier that cannot authenticate packets. Physical RNode/RNodeMulti,
BLE, Weave, VR-N76, serial/radio operation, public networks, and third-party
clients remain separate evidence axes.

## Validation and publication

The release candidate is qualified by the focused transport regressions,
strict pinned-Python inventory, workspace format/lint/build/test gates,
architecture and boundary checks, and the hosted pull-request workflows. The
tag-triggered Release, crates.io, independent-interoperability, performance,
signing, provenance, and OCI workflows publish evidence for the immutable
`v0.10.1` tag. Homebrew publication is skipped when its tap/token is not
configured; that does not affect the archives, native packages, crates, or OCI
publication.
