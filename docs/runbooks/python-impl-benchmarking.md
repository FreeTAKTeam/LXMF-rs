# Python Implementation Benchmarking

This runbook defines the benchmark workflow for apples-to-apples comparison
between Rust protocol hot paths and the canonical Python `RNS` and `LXMF`
implementations.

## Scope

Use this suite for protocol-core comparisons only:

- LXMF message encode/decode
- LXMF large-message encode/decode
- Reticulum announce create/validate
- Reticulum identity sign/verify
- Reticulum identity encrypt/decrypt

Do not use this suite for SDK or RPC performance claims. Those surfaces do not
map cleanly to canonical Python implementations.

## Prerequisites

- `python3` can import `RNS` and `LXMF`
- Rust workspace builds successfully
- Run on a quiet machine when possible

## Configuration

Benchmark parameters and workload mappings live in
`tools/benchmarks/python_impl.toml`.

That file is the source of truth for:

- Criterion sample size
- Criterion warmup and measurement windows
- Python iteration count
- Rust/Python benchmark pairings used in the comparison report

## Commands

Run the full comparison:

```bash
cargo xtask python-impl-bench-compare
```

Shortcut:

```bash
make python-impl-bench
```

## Outputs

The command writes:

- `target/criterion/python-impl-benchmarks.json`
- `target/criterion/python-impl-environment.json`
- `target/criterion/python-impl-compare.json`
- `target/criterion/python-impl-compare.txt`

Interpretation:

- `.json` files are machine-readable artifacts for CI or trend tooling
- `.txt` is the operator-facing summary
- `python-impl-environment.json` records toolchain, Python module paths, host
  info, and the config file used for the run

## Operating Rules

- Compare only workloads defined in the config file
- Do not compare results from runs that used different config values
- Re-run on the same machine after a warm cache when validating regressions
- Prefer multiple runs before concluding a small regression
- Treat order-of-magnitude differences as meaningful; treat low single-digit
  percentage shifts as noise until reproduced
