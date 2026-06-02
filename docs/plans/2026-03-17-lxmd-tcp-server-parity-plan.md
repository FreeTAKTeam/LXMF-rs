# lxmd TCP Server Failure Investigation & Python-Parity Recovery Plan

Last updated: 2026-03-17

## Scope and alignment with existing plans

This plan is an execution addendum to:

- `docs/plans/2026-01-26-lxmf-reticulum-full-parity.md` (full LXMF + Reticulum parity track)
- `docs/status/lxmf-parity-matrix.md` and `docs/status/reticulum-parity-matrix.md` (status source of truth)
- `docs/contracts/compatibility-contract.md` and `docs/contracts/compatibility-matrix.md` (compatibility gates)

Goal for this addendum: close the specific gap where Rust `lxmd` does not behave like Python `lxmd` when configured as a TCP server, then extend the work to broader daemon/runtime parity.

## Investigation summary

### Symptom reported

- "Rust `lxmd` is not working as TCP server" in Python-compatible operation.

### Confirmed implementation gaps

1. **Python-style `lxmd` config ingestion does not parse interface sections.**
   - Current parser consumes `[lxmf]`/`[propagation]` launcher knobs, but there is no interface translation path from Python-style Reticulum config to Rust daemon transport/server startup.

2. **`reticulumd` only starts a TCP server from explicit `--transport` CLI argument.**
   - In bootstrap, `TcpServer` startup is gated by `if let Some(addr) = args.transport.clone()`.
   - A configured `[[interfaces]] type = "tcp_server"` entry does not independently start server transport.

3. **`lxmd` generated daemon config path is currently client-oriented and incomplete for server parity.**
   - `write_generated_reticulumd_config()` emits basic interface stanzas, but the launcher flow still relies on a dedicated `transport` value for TCP server bind.

4. **Test coverage is missing an end-to-end assertion for Python-style TCP server mode parity.**
   - Existing `lxmd` unit coverage validates path creation and launcher TOML behavior, but not Python-style TCP server startup parity.

## Root cause

The Rust `lxmd` compatibility entrypoint currently models "server bind" as a dedicated launcher/CLI transport concern (`--transport`) while Python reference behavior treats TCP server mode as a first-class Reticulum interface configuration concern. This mismatch causes Python-configured server mode to be dropped unless users provide Rust-specific transport overrides.

## Comprehensive parity plan

## Phase 0 — Repro + baseline capture (must complete first)

1. Add a deterministic integration repro:
   - Python-style config with TCP server interface only.
   - Launch Rust `lxmd` without `--transport` override.
   - Assert expected bind does **not** occur (document current failure).
2. Record baseline artifacts:
   - launcher argv/env snapshot
   - generated `reticulumd.generated.toml`
   - daemon startup logs
3. Create a parity fixture in `docs/fixtures/interop/v1/` describing expected server mode observables.

**Exit criteria:** a stable failing test reproduces the reported issue.

## Phase 1 — Config model parity (Python config -> Rust runtime intent)

1. Extend compatibility parser to support Python interface declarations relevant to `lxmd` parity:
   - `TCPServerInterface` (required)
   - `TCPClientInterface` (already partially represented, verify mapping fidelity)
2. Normalize Python keys to canonical runtime intent:
   - bind/listen host defaults
   - port requirements and validation semantics
   - enable/disable handling (`yes/no` style booleans)
3. Introduce explicit conflict policy:
   - precedence between `--transport`, launcher TOML `transport`, and Python interface-derived server bind
   - warning diagnostics when multiple server sources disagree

**Exit criteria:** effective runtime model contains an unambiguous server bind intent from Python-compatible config.

## Phase 2 — `reticulumd` startup parity for `tcp_server`

1. Refactor bootstrap startup orchestration so TCP server startup can come from:
   - explicit CLI transport override **or**
   - enabled `tcp_server` interface record.
2. Keep single active server constraint unless contract says otherwise:
   - if multiple enabled server interfaces are configured, fail fast with actionable error.
3. Ensure runtime introspection reflects source of truth:
   - interface startup records include server entries with `startup_state`, `runtime_state`, and bind endpoint.

**Exit criteria:** daemon starts TCP server from interface config without requiring launcher-only `transport`.

## Phase 3 — Behavior parity hardening vs Python reference

1. Build an interop matrix for server mode:
   - Rust server <-> Python client
   - Python server <-> Rust client
   - restart/rebind behavior
   - strict vs best-effort startup modes
2. Validate policy parity:
   - startup failure semantics
   - logging and operator diagnostics
   - status/reporting parity (`--status`/RPC surface)
3. Add parity assertions into CI smoke lane (non-flaky, bounded runtime).

**Exit criteria:** interop matrix passes and is wired into regular CI gates.

## Phase 4 — Contract and docs convergence

1. Update contracts/runbooks after implementation:
   - `docs/contracts/compatibility-contract.md`
   - `docs/runbooks/*` where TCP server startup is described
2. Update parity matrices:
   - `docs/status/lxmf-parity-matrix.md`
   - `docs/status/reticulum-parity-matrix.md`
3. Add migration notes for operators currently depending on `transport` overrides.

**Exit criteria:** docs describe one coherent parity model and operators have migration guidance.

## Proposed implementation order (small, reviewable PRs)

1. PR1: failing repro/integration test + baseline fixture.
2. PR2: parser/model updates for Python TCP server interface mapping.
3. PR3: daemon bootstrap support for interface-driven server startup.
4. PR4: interop + CI smoke automation.
5. PR5: docs/contracts/matrix updates and closeout notes.

## Risks and mitigations

- **Risk:** breaking existing users who rely on `transport` override semantics.
  - **Mitigation:** maintain override precedence and emit clear compatibility notes.
- **Risk:** duplicate server bind attempts from mixed config sources.
  - **Mitigation:** centralize server intent resolution before process spawn.
- **Risk:** flaky interop tests.
  - **Mitigation:** fixed ports, bounded retry windows, explicit readiness probes.

## Definition of done

- Rust `lxmd` accepts Python-style config for TCP server mode and binds correctly without Rust-only transport overrides.
- Rust/Python mixed interop scenarios pass in automated tests.
- Parity matrices and runbooks are updated to reflect the new status.
- No regressions in existing `lxmd` query mode and propagation-node behavior.
