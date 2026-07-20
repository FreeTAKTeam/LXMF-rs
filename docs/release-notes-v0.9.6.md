# LXMF-rs v0.9.6

v0.9.6 is a stabilization release. These notes remain release-candidate notes
until every gate in `docs/runbooks/release-readiness.md` passes on the exact
candidate commit.

## Hardening in this release

- Correct link-context data and channel fan-out so packets use each active
  link's bound interface rather than a path-table lookup on the ephemeral link
  identifier.
- Return structured `LinkSendReport` evidence from new fan-out reporting APIs,
  including partial packet-build and dispatch failures; compatibility helpers
  now log those failures.
- Generate and validate single-destination delivery proofs with packet-cache
  correlation, preventing unrelated known identities from forging receipts.
- Keep inbound plain-resource routing and identified-peer propagation behavior
  covered by real link/resource regressions.
- Implement standard `Error` and `Display` behavior for core and transport
  `RnsError` values so applications can propagate them through normal Rust
  error stacks.
- Expand the issue-369 regression scanner to reject ignored channel-send
  results in single-line and multiline forms, channel-send failures discarded
  through `.ok()`, and mutex-poison branches that conflate failure with absence.
- Stop packet-ingress workers when their transport receive queue closes, and
  make RPC/store reply cancellation and subscriber-free event fan-out explicit
  instead of silently discarding send results.
- Preserve delivery stamp/propagation metadata errors in diagnostics, propagate
  event-stream encoding and write failures, and report RPC listener/writer task
  failures with connection context.
- Upgrade legacy message databases transactionally: required columns are added
  before dependent indexes, legacy announce fields are backfilled, partial
  migrations roll back, and malformed persisted JSON is returned as an error.
- Add fallible public identity construction: `try_new_from_slices` rejects
  malformed key lengths and invalid Ed25519 encodings, while hex constructors
  reject non-hex and overlong key/hash strings instead of panicking, truncating,
  or substituting default key material.
- Add `WireMessage::try_message_id` and use it in runtime, daemon, embedded, and
  signing/verification paths so payload-encoding failures cannot collapse to an
  empty-payload hash. The existing infallible `message_id` helper remains for
  source compatibility when the caller already owns a valid in-memory payload.
- Fail closed when delivery or propagation identity-policy RPC checks fail, and
  preserve storage, receipt, task-join, interface shutdown, and child-process
  cleanup failures in actionable diagnostics.
- Reject malformed typed propagation recovery, node-selection, node-config,
  and node-list fields in SDK responses instead of silently defaulting them;
  `null` remains absence and oversized sync-state integers are rejected rather
  than truncated.
- Split wire receipt validation and link fan-out into focused modules that
  remain inside the repository's module-size policy.

## Examples and migration

- New users should start with `docs/examples.md` and `docs/sdk/quickstart.md`.
- ZeroMQ examples now use the canonical single ROUTER/DEALER endpoint through
  `ZmqPipelineBackendConfig::local` and `--zmq-rpc-endpoint`.
- The legacy dual-endpoint `local_tcp` / `--zmq-rpc-command` path remains
  available for compatibility but is not the recommended setup.
- Use `Identity::try_new_from_slices` for untrusted material and propagate the
  result with `?` or attach local context with `map_err`. The existing
  `new_from_slices` signature remains source-compatible for typed/invariant
  conversions.
- `PropagationRecoveryStateResult::try_from_propagation` returns `Result` so
  callers can distinguish an absent optional field from a malformed typed
  response. The existing infallible `from_propagation` method remains
  source-compatible for callers that need the pre-v0.9.6 best-effort behavior.

## Release evidence

The complete local `cargo xtask release-check` gate passed on the 2026-07-19
hardening working tree. It covered formatting, workspace Clippy and tests,
dry-run crate packaging, boundary and architecture checks, pinned-reference
contracts, SDK API/contract drift, supply-chain checks, Miri and Loom, local
E2E/mesh/soak scenarios, reproducible builds, and embedded footprint checks.

Promotion is still pending: workspace and project metadata are aligned to
`0.9.6`, but the reviewed tree must become an exact candidate commit and hosted
CI must pass on that SHA before tagging. Live external-Python and
physical-interface suites remain environment-bound evidence tracks.

## v1.0 boundary

Physical RNode/RNodeMulti, Weave, VR-N76, BLE/serial/radio validation, public
network soak, third-party clients, and manual operator workflows remain explicit
v1.0 evidence targets.
