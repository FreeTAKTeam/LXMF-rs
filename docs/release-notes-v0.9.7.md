# LXMF-rs v0.9.7

v0.9.7 is a stabilization release prepared from the current `origin/main`
after the published v0.9.6 baseline. These notes remain release-candidate
notes until every gate in `docs/runbooks/release-readiness.md` passes on the
exact candidate commit.

## Highlights

- Add persistent multi-service Reticulum identities with SDK v2 identity
  creation, import, export, activation, and announce operations.
- Harden RPC service boundaries with bounded connection lifetimes, complete
  request and TLS-handshake deadlines, explicit private Unix-socket handling,
  and failed-authentication rate limiting.
- Harden private key and ratchet persistence with exclusive temporary files,
  private Unix permissions, cleanup, and secret-safe key diagnostics.
- Add constant-time authentication-tag checks and address crypto side-channel
  findings in identity and ratchet paths.
- Improve RNS 1.4.2 parity, path invariants, roaming path responses, packet
  hash and hop handling, resource limits, propagation stamps, worker
  supervision, and no-std ratchet time validation.
- Preserve the v0.9.6 LXMF/transport fixes for disabled forwarding, negotiated
  Link cipher mode, shared delivery metadata, TCP reconnect backoff, and
  destination identity resolution.

## Compatibility

This release contains additive SDK/schema capabilities and correctness and
security fixes. No breaking protocol or migration change is planned. The
current parity posture remains bounded by
`docs/status/reticulum-parity-matrix.md` and
`docs/status/lxmf-parity-matrix.md`.

## Promotion requirements

- `cargo xtask release-check` and `rnx e2e` pass on the exact candidate SHA.
- The documented Python/RNS surface inventory is checked for consistency;
  100% parity is explicitly out of scope for this release.
- Boundary-mode recursive path-request gating remains a documented partial
  RNS parity row; the pinned reference baseline deselects only that known
  failing reference case while the remaining reference and Rust interop suites
  remain release gates.
- The Python-to-Rust LXMD remote-relay path remains a documented partial
  interoperability row; the two complementary remote-relay flows remain
  hosted gates for this release.
- Hosted CI, architecture checks, and pinned-Python interop pass on that SHA.
- `v0.9.7-rc.1` evidence is reviewed before promotion to `v0.9.7`.
- The final GitHub release, bundles, checksums, and crates.io publications are
  verified against the final tag.

## v1.0 boundary

Physical RNode/RNodeMulti, Weave, VR-N76, BLE/serial/radio validation, public
network soak, third-party client validation, and manual operator workflows
remain explicit v1.0 evidence targets.
