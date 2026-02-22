# Soak and Chaos Campaign

Status: active  
Scope: long-run stability and regression detection for `rnx` delivery workflows

## Goals

1. Detect long-run delivery regressions before release.
2. Exercise repeated daemon startup/shutdown and multi-node mesh pressure.
3. Produce machine-readable soak artifacts for trend tracking.

## Campaign Modes

- CI smoke gate (`sdk-soak-check`): short deterministic campaign.
- Nightly campaign (`nightly-soak-chaos`): extended runtime with mesh chaos rounds.

## Local Run

```bash
./tools/scripts/soak-rnx.sh
```

With explicit thresholds:

```bash
CYCLES=2 BURST_ROUNDS=6 CHAOS_INTERVAL=2 MAX_FAILURES=0 REPORT_PATH=target/soak/soak-report.json ./tools/scripts/soak-rnx.sh
```

## Report Artifact

Default artifact path:

- `target/soak/soak-report.json`

Report fields include:

- `status`
- `total_rounds`
- `e2e_failures`
- `mesh_runs`
- `mesh_failures`
- `max_failures`
- `duration_secs`

## Regression Threshold Policy

For release gating:

1. `status` must be `pass`.
2. `e2e_failures` must be `<= max_failures`.
3. `max_failures` must be pinned in CI (default `0`).

If the campaign fails:

1. collect report + command output,
2. reproduce with the same env vars,
3. classify as infra flake vs product regression,
4. add/adjust deterministic tests before reducing thresholds.
