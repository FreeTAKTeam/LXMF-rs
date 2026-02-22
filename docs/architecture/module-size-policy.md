# Module Size Policy

Status: active  
Scope: active Rust code in `crates/libs/*`, `crates/apps/*`, and `xtask/src`

## Goals

1. Keep hot-path implementation modules maintainable and reviewable.
2. Prevent new oversized files from entering the codebase.
3. Track temporary exceptions explicitly until split work lands.

## Limits

- Default module budget: `500` LOC per `.rs` file.
- Test/fuzz/bench budget: `1200` LOC per `.rs` file.

## Enforcement

Gate command:

```bash
./tools/scripts/check-module-size.sh
```

The gate is wired into:

- `cargo run -p xtask -- architecture-checks`
- CI job `architecture-checks`

## Exception Handling

Temporary exceptions are listed in:

- `docs/architecture/module-size-allowlist.txt`

Rules:

1. Only add exceptions with a clear split plan.
2. Remove allowlist entries as soon as files are under budget.
3. Do not add broad directory globs; only file paths are allowed.
