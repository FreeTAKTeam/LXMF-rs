# E2E Benchmarking

This manual-only harness runs container topologies for end-to-end correctness
and benchmark work. It is intentionally not a CI gate.

## Toolchain Boundary

The main workspace and standalone runner at `tools/e2e-runner/` both require
Rust 1.88 or newer. The runner's separate lockfile isolates its benchmark
dependencies from the workspace packages.

Install the runner toolchain:

```bash
rustup toolchain install 1.88.0 --profile minimal
```

The runner is excluded from the root Cargo workspace through its own empty
`[workspace]` table. Its lockfile is committed independently, so adding or
updating runner dependencies does not change the main workspace dependency
graph.

## Prerequisites

- Rust 1.88.0 through `rustup`
- A reachable Docker daemon
- Docker Compose; the runner uses the local plugin when available and otherwise
  uses the containerised Testcontainers Compose client

## Commands

Inspect the selected scenarios without starting containers:

```bash
cargo xtask e2e-bench --dry-run
```

Run the smoke correctness profile:

```bash
cargo xtask e2e-bench --mode correctness --profile smoke
```

Filter by scenario or implementation:

```bash
cargo xtask e2e-bench --scenario c1-tcp-smoke --implementation tcp
```

Keep containers for debugging:

```bash
cargo xtask e2e-bench --keep
```

Run artifacts are written below `target/e2e-bench/<timestamp>/`. The initial
slice records `run.json` and validates the plain-TCP control topology. Rust,
Python-reference, impaired-link, resource, and percentile reporting scenarios
will extend the same runner and scenario matrix.
