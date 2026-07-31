# Design-principles improvement plan

## Goal

Realize the five project improvements identified from the Rust design-principles
assessment: enforce architecture checks, complete propagation relay stamp
policy, close the remaining RNS 1.4.2 delta, make `rns-core` authoritative for
core cryptography and errors, and make project status concise and verifiable.

## Design principles

Correctness and robustness require reference-backed parity tests before behavior
changes. Separation of concerns, cohesion, DRY, and information hiding give
each policy and protocol primitive one owner. KISS and YAGNI keep the work in
small, independently reviewable PRs. Linus's Law makes architecture and status
checks visible in normal pull-request CI.

## Implementation order

1. **Baseline and architecture inventory**
   - Start every increment from refreshed `origin/main` on a dedicated branch.
   - Run and record module-size, boundary, unsafe, `sdk-api-break`, and parity
     baselines.
   - Do not add module-size exemptions. Retire existing exemptions only through
     cohesive, behavior-preserving module extraction.
   - Current baseline note: at `c21dadf6`, the gate identified eight
     unallowlisted production modules. This checkpoint splits them by
     responsibility without adding exemptions.

2. **Primary architecture gate**
   - Run `cargo xtask ci --stage architecture-checks` in primary CI for pull
     requests and pushes to `main`; retain it in full CI and make the job a
     required branch-protection check after its first successful merge.

3. **Concise, self-validating status**
   - Reduce `docs/status/current-roadmap.md` to release position, blockers,
     ordered parity-ledger IDs, verification baseline, and next actions.
   - Move historical capability detail into a linked evidence archive and keep
     row-level evidence in the parity matrices.
   - Add `cargo xtask status-docs --write|--check` to generate the release
     block from Cargo metadata and Git tags, validate roadmap IDs and links,
     and run existing parity-artifact drift checks in CI.

4. **Core cryptography and errors**
   - Move `CachedFernet` beside `Fernet` in `rns-core`; re-export it through
     the existing transport path.
   - Replace the duplicate transport `RnsError` with a re-export of
     `rns_core::RnsError`, preserving existing transport paths and forwarding
     the AES-128 feature to core.
   - Remove only dependencies that become demonstrably unused.

5. **Propagation relay stamp policy**
   - Use `lxmf_core::pn_stamp_cost_from_app_data` as the sole propagation
     announce-cost decoder.
   - Add a validated `PropagationRelayPolicy` and an additive structured
     request extension containing destination and stamp cost. Keep legacy relay
     selection with its cost-16 fallback.
   - Mine the propagation stamp at the resolved advertised cost and reject
     malformed, negative, and above-256 costs before mining.

6. **RNS 1.4.2 completion**
   - Use Python RNS commit `b48b96e61676504e0a4e527b33b9a0b4495c6872` as the
     behavior authority and add probes before each change.
   - Land request/response limits; interface gravity, autoconnect and internal
     announce policy; deterministic authenticated path rebalancing and boundary
     requests; then daemon/SDK status and `rnstatus` visibility.
   - Keep defaults, persisted configuration, and SDK v2 compatibility intact.

7. **Module-debt retirement and final verification**
   - Split remaining allowlisted production modules by responsibility in
     separate core/transport, daemon/RPC, and app/tool batches until the
     production allowlist is empty.
   - For each parity increment update the roadmap and relevant matrix, then
     run formatting, Clippy, focused and workspace tests, boundary/unsafe and
     architecture checks, parity-artifact checks, and release readiness gates.

## Public interfaces planned

- `lxmf_runtime::PropagationRelayPolicy`, its validation error,
  `InProcessBackend::set_propagation_relay_policy`, and a structured propagation
  relay extension.
- `rns_core::crypt::fernet::CachedFernet`, while the existing transport Fernet
  and `RnsError` paths remain available as re-exports.
- Additive RNS request-limit and interface policy/status accessors with default
  compatibility.
- `cargo xtask status-docs --write|--check`.

## Evidence required

- Architecture CI: a passing local `cargo xtask ci --stage architecture-checks`
  run and a passing hosted `architecture-checks` job.
- Propagation: valid advertised costs, legacy fallback, invalid-cost rejection,
  precedence, and Python stamp validation.
- RNS: boundary values, policy modes, trusted versus untrusted rebalance,
  status propagation, and pinned-Python probes.
- Final: formatting, workspace Clippy/tests, API/contract checks, parity drift
  checks, and `cargo xtask release-check`.
