# Python Compatibility Execution Board

Date: 2026-03-19

Goal:

- make Rust nodes interoperable with Python Reticulum and LXMF nodes without
  special casing
- close the remaining numbered compatibility issues in
  `docs/plans/2026-03-18-rust-python-compat-issue-list.md`

Merged foundation PRs:

- `#110` buffer writer parity
- `#111` buffer callback parity
- `#112` resource lifecycle truth and generic-resource handling
- `#113` daemon receipt semantics for resource-backed sends
- `#114` honor LXMF delivery modes in reticulumd bridge
- `#115` path tag lifetime parity

Working rule:

- one subsystem per branch
- one parity claim per PR
- every PR must point to exact Python reference behavior
- every PR must add or tighten a parity test

Foundation status:

- the initial channel, resource, delivery-mode, and path-tag parity stack is on `main`
- the compatibility harness can treat those behaviors as the starting baseline
- keep formatting-only diffs out of future parity branches

## Workstreams

### A. Path and Announce Parity

Owner: `Lorentz`

Issues:

- `20`
- `21`
- `22`
- `23`
- `24`
- `25`
- `26`
- `27`
- `28`
- `29`

Execution order:

1. `codex/path-tag-lifetime-parity`
2. `codex/announce-interface-parity`

First PR scope:

- preserve original path request tags in direct responses
- preserve tags during recursive forwarding
- bound duplicate-suppression lifetime

Second PR scope:

- interface-aware announce pacing
- held-announce release behavior
- announce forwarding/rate-limiting/cache restoration semantics

Acceptance:

- path tags match Python behavior
- duplicate suppression expires instead of persisting forever
- announce behavior is interface-aware rather than transport-global

### B. Propagation Router Parity

Owner: `Newton`

Issues:

- remaining parts of `4`
- `21`
- `22`
- `23`
- `24`
- `25`
- `36`

Execution order:

1. `codex/propagation-router-parity`

Scope:

- deepen the selected propagation-node behavior past the first `#114` routing slice
- make propagation link lifecycle and retry behavior align with Python router logic
- close propagation transient-id lifecycle gaps

Acceptance:

- propagated delivery behaves like Python propagation-node delivery rather than a best-effort bridge shim

### C. Resource Follow-Through

Owner: `Dirac`

Issues:

- verify closure of `18`
- verify final closure of `19`

Execution order:

1. `codex/resource-allocation-parity` if `18` remains open

Scope:

- confirm inbound allocation is bounded by advertised parts
- confirm generic-resource completion is no longer misdecoded as LXMF

Acceptance:

- no unbounded inbound allocation path remains
- non-LXMF resource traffic stays generic end to end

### D. Stamps, Tickets, and Propagation Stamps

Owner: `McClintock`

Issues:

- `30`
- `31`
- `32`
- `33`
- `34`
- `35`
- `36`

Execution order:

1. `codex/stamp-wire-parity`
2. `codex/ticket-lifecycle-parity`
3. `codex/propagation-stamp-parity`

Acceptance:

- API options drive real wire/runtime behavior
- inbound stamp checks happen before acceptance
- ticket generation and renewal semantics match Python expectations

### E. LXMF Fidelity and Interchange

Owner: `Lovelace`

Issues:

- `37`
- `38`
- `39`
- `40`
- `41`

Execution order:

1. `codex/lxmf-fidelity-parity`

Scope:

- preserve inbound stamp validity state
- keep floating timestamp precision
- preserve binary title/content fidelity
- relax outbound field-shape handling where Python accepts broader forms
- maintain `.lxm` interchange compatibility

Acceptance:

- Python-originated messages do not lose fidelity when decoded by Rust
- Rust-generated `.lxm` files load correctly in Python

### F. Compatibility Test Matrix

Owner: `Volta`

Scope:

- build the matrix and first scaffolding for mixed Rust/Python compatibility checks

Required scenarios:

- direct Rust -> Python
- direct Python -> Rust
- opportunistic Rust -> Python
- propagated Rust -> Python via Python router
- propagated Python -> Rust via Rust router
- Rust/Python resource transfer on shared links
- `.lxm` round-trip interchange

Acceptance:

- the project has an explicit test plan for the compatibility claim

### G. Review Gate

Owner: `James`

Scope:

- review every parity PR before merge

Required review question:

- what exact Python behavior became true now that was false before this PR?

## Branch and Commit Templates

Branch name:

- `codex/<subsystem>-parity`

Commit message:

- `<subsystem> parity`

Examples:

- `path tag lifetime parity`
- `announce interface parity`
- `stamp wire parity`

Draft PR title:

- `Align <subsystem> behavior with Python Reticulum/LXMF`

Draft PR body:

- Summary
- Issues covered
- Python references
- Rust files changed
- What is now compatible
- What remains out of scope
- Testing

## PM Notes

- Do not start stamp/ticket implementation before path/announce and propagation router behavior are in better shape.
- Keep formatting-only cleanup separate from parity PRs.
- After each merge, refresh the numbered issue document before opening the next major PR.
