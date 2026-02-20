# Incident Response Playbooks

Status: active
Applies to: SDK v2.5 RPC-backed deployments (`reticulumd` + `lxmf-sdk`)

## Incident Severity and Escalation

| Severity | Definition | Target Acknowledge | Target Mitigate | Escalation |
| --- | --- | --- | --- | --- |
| `P0` | active outage, security failure, or data-loss risk | 5 min | 30 min | on-call + security + maintainer owner |
| `P1` | major degraded service with partial workaround | 15 min | 2 h | on-call + runtime owner |
| `P2` | limited impact, no active outage | same business day | 1 day | domain owner |

Escalate from `P1` to `P0` if message delivery stops globally or auth controls fail open.

## Shared Triage Procedure

1. Capture current release and config revision:

```bash
cargo run -p lxmf-cli -- status
cargo run -p lxmf-cli -- snapshot --include-counts
```

2. Capture runtime metrics and health:

```bash
curl -sS http://127.0.0.1:4242/healthz
curl -sS http://127.0.0.1:4242/readyz
curl -sS http://127.0.0.1:4242/metrics
```

3. Preserve evidence before restart:

```bash
cargo run -p rns-tools --bin rnx -- replay --trace docs/fixtures/sdk-v2/rpc/replay_known_send_cancel.v1.json
```

4. Open incident note with:
   - start time (UTC)
   - current commit/release tag
   - affected profile (`desktop-full`, `desktop-local-runtime`, `embedded-alloc`)
   - suspected blast radius

## P0: RPC Auth Failure Spike

### Detection

- `sdk_auth_failures_total` increases rapidly.
- repeated `SDK_SECURITY_*` responses for valid clients.

### Immediate Actions

1. Verify bind/auth mode and policy:

```bash
cargo run -p lxmf-cli -- snapshot
```

2. Confirm endpoint auth path:

```bash
curl -i http://127.0.0.1:4242/metrics
```

3. If token mode is enabled, verify clock skew and issuer/audience alignment.
4. If mTLS mode is enabled, verify certificate SAN policy and client cert presence.

### Mitigation

1. If policy drift is confirmed, apply minimal config patch and re-validate:

```bash
cargo run -p lxmf-cli -- configure --patch '{"extensions":{"rate_limits":{"per_ip_per_minute":120,"per_principal_per_minute":120}}}'
```

2. If malicious traffic is suspected, switch bind mode to local-only and front through trusted proxy.
3. Re-check `sdk_auth_failures_total` slope after mitigation.

### Exit Criteria

- auth failures return to baseline
- legitimate clients can negotiate and send/poll
- no open bypass condition

## P0: Event Stream Degraded or Cursor Expired

### Detection

- `SDK_RUNTIME_CURSOR_EXPIRED` or `SDK_RUNTIME_STREAM_DEGRADED`.
- `sdk_poll_batches_with_gap_total` increases and clients stop consuming new events.

### Immediate Actions

1. Measure queue pressure and drops:

```bash
curl -sS http://127.0.0.1:4242/metrics
```

2. Force client recovery path:
   - reset cursor (`poll_events` with no cursor)
   - resume polling with bounded `max`

### Mitigation

1. Tune overflow policy for active incident:
   - `drop_oldest` for recovery throughput
   - `reject` for strict retention
2. Reduce consumer lag with smaller per-poll batch and tighter poll cadence.
3. Validate no continued drop growth in `sdk_event_drops_total`.

### Exit Criteria

- no sustained growth in dropped events
- event consumers process new events without degraded errors

## P1: Message Delivery Stall

### Detection

- sends accepted but terminal receipt states do not progress.
- `sdk_send_total` rises while delivery confirmations flatten.

### Immediate Actions

1. Collect per-message traces:

```bash
cargo run -p rns-tools --bin rnx -- e2e --timeout-secs 20
```

2. Validate interfaces/peer state in daemon status.

### Mitigation

1. Re-run announce/sync workflows.
2. If isolated to propagation path, temporarily force direct method for critical traffic.
3. Validate outbound bridge health and retry path.

### Exit Criteria

- new sends reach expected terminal states
- no persistent backlog growth

## P1: Durable Store Corruption or Restart Loop

### Detection

- daemon fails to start repeatedly
- snapshot/config revision regresses unexpectedly after restart

### Immediate Actions

1. Stop write traffic.
2. Capture current store and logs.
3. Run schema/contract validation gates locally:

```bash
cargo run -p xtask -- sdk-schema-check
cargo run -p xtask -- sdk-conformance
```

### Mitigation

1. Restore from most recent known-good backup.
2. Replay deterministic traces for verification.
3. If migration-related, apply rollback strategy from release notes before restart.

### Exit Criteria

- service boots and stays healthy
- persisted domain/config state remains stable across restart

## P2: Performance Regression (Latency/Throughput)

### Detection

- release gate fails on `sdk-perf-budget-check` or production p95/p99 exceeds SLO.

### Actions

1. Run benchmark and budget checks:

```bash
cargo run -p xtask -- sdk-bench-check
cargo run -p xtask -- sdk-perf-budget-check
```

2. Compare with prior release artifacts and recent commits.
3. Roll forward with fix or roll back based on canary criteria.

## Post-Incident Review and Follow-up

1. Document timeline, root cause, and why existing gates missed it.
2. Add or strengthen one automated gate (`xtask`, CI stage, schema test, conformance test).
3. Update one contract/runbook artifact as needed.
4. Close incident only after a validated regression test is merged.
