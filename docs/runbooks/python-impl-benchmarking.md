# Python Implementation Benchmarking

This runbook defines the benchmark workflow for apples-to-apples comparison
between Rust protocol hot paths and the canonical Python `RNS` and `LXMF`
implementations.

## Scope

The current report publishes protocol-core and transport-hotpath comparisons:

- LXMF message encode/decode
- LXMF small, 2 KiB, and 16 KiB message encode/decode
- Reticulum packet pack/unpack
- Reticulum announce create/validate
- Reticulum identity sign/verify
- Reticulum identity encrypt/decrypt
- Reticulum resource segmentation, request-window handling, and reassembly
- Two-node loopback TCP cold discovery plus warm direct, opportunistic,
  propagated, and 16 KiB resource delivery

SDK transport and same-topology end-to-end results are kept in separate dataset
tiers. Do not infer daemon or whole-system performance from protocol/core rows.

## Profiles

Benchmark parameters and workload mappings live in
`tools/benchmarks/python_impl.toml`.

That file is the source of truth for:

- Quick developer runs in the `fast` profile
- Publishable runs in the `report` profile
- Rust/Python workload pairings
- Repeated-run and resource-measurement counts

## Commands

Quick comparison:

```bash
cargo xtask python-impl-bench-compare
```

Stricter single comparison pass:

```bash
cargo xtask python-impl-bench-compare --profile report
```

Aggregated report for serious claims:

```bash
cargo xtask python-impl-bench-report
```

Shortcuts:

```bash
make python-impl-bench
make python-impl-bench-report
```

## Outputs

`docs/PerformancesComparison.html` is historical and non-current. The current
generated page is `docs/performance.md`, sourced from
`docs/performance/v0.9.5.json`.

Quick comparison writes:

- `target/criterion/python-impl-benchmarks.json`
- `target/criterion/python-impl-environment.json`
- `target/criterion/python-impl-compare.json`
- `target/criterion/python-impl-compare.txt`

Aggregated report mode writes:

- `target/criterion/python-impl-report/report.json`
- `target/criterion/python-impl-report/report.txt`
- `target/criterion/python-impl-report/runs/run-XX/...`
- `target/criterion/python-impl-report/resources/...`

Interpretation:

- Per-run artifacts preserve raw timing for each repeated comparison pass
- `report.json` is the machine-readable summary for a benchmark page or CI
- `report.txt` is the operator-facing summary
- Resource artifacts come from isolated subprocess runs and are suitable for
  CPU-time and peak-RSS claims
- Resource-run iterations are auto-scaled per workload so very fast paths run
  long enough to produce non-zero CPU measurements
- Rust resource measurements are executed via a prebuilt `target/release/xtask`
  workload runner so they are not distorted by debug-build overhead

## Operating Rules

- Run report mode on a quiet machine
- Do not compare results from different profiles
- Keep the same host, toolchain, and config when making serious claims
- Import the exact pinned Python revisions; the runner rejects revision drift
- Alternate Rust-first and Python-first execution order across report runs
- Use repeated-run medians, not a single lucky pass
- Keep claims scoped to the workloads actually measured
- Prefer peak RSS and CPU time over raw `%CPU`
- Treat low single-digit differences as noise until reproduced

Publish and verify generated documentation:

```bash
python3 tools/scripts/performance_docs.py --release v0.9.5 \
  --report target/criterion/python-impl-report/report.json
python3 tools/scripts/performance_docs.py --check
```

Release candidates are measured on the same runner as a checkout of `v0.9.1`.
The hosted release workflow keeps report-profile Criterion settings but uses
three interleaved comparison runs, 2,000 Python iterations per workload, two
isolated resource runs, and 5,000 resource iterations per checkout. The
workflow applies the same bounded Python count to both checked-out benchmark
configs because the `v0.9.1` command does not expose that value as a CLI
override. Generated datasets record the effective counts. Use the full
five-comparison/three-resource defaults and the report profile's 10,000 Python
iterations for independently published benchmark claims.

Apply the release budgets to those two generated datasets with:

```bash
python3 tools/scripts/performance_release_gate.py \
  --candidate target/performance/candidate.json \
  --baseline target/performance/v0.9.1.json \
  --output target/performance/release-gate.json
```

The gate rejects more than 10% geometric-mean Rust throughput regression and
more than 20% CPU, peak-RSS, or matching critical ZeroMQ p95 regression. Core
relative MAD must remain at or below 10%; E2E relative MAD must remain at or
below 20% after the benchmark runner's single automatic retry.

## Claim Discipline

Acceptable claim examples:

- “On matched protocol-core workloads, Rust delivered 5.0x lower p50 latency.”
- “Across 5 repeated report-profile runs, Rust maintained higher throughput on every measured workload.”
- “In isolated workload runs, Rust used less peak RSS and less CPU time per 1k operations.”

Not acceptable from this suite alone:

- “The Rust daemon is X faster overall.”
- “The SDK is X faster.”
- “The whole system uses X less memory.”
