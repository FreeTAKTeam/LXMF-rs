# LXMF interop fixtures

Deterministic fixtures for Rust ⇄ Python LXMF envelope compatibility checks.

## Fixtures
- `fixtures.json`: semantic fixture index + hex-encoded binary fixture payloads.
  - `direct_no_stamp`
  - `direct_with_stamp`
  - `direct_with_metadata`
  - `propagation_with_stamp`
  - `malformed_missing_stamp_tail`

## Regenerate
```bash
LXMF_REGEN_FIXTURES=1 cargo test -p lxmf-wire regenerate_fixtures_when_env_set -- --exact
# or
LXMF_REGEN_FIXTURES=1 scripts/generate_lxmf_interop_fixtures.py
```

## Run tests
- Rust-only deterministic fixture suite:
```bash
cargo test -p lxmf-wire --test lxmf_interop
```

Python live parity is optional/manual; CI uses committed fixture bytes only.
