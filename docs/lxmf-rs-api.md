# LXMF Rust API (v0.2)

## Stable Surface
The intended public contract is the explicit module set exported from `crates/lxmf/src/lib.rs`:

- `lxmf::message`
- `lxmf::identity`
- `lxmf::router_api`
- `lxmf::errors`

## Core Re-exports
- `lxmf::Message`
- `lxmf::Payload`
- `lxmf::WireMessage`
- `lxmf::Router`
- `lxmf::LxmfError`

## Policy
- CLI/runtime tooling is feature-gated (`feature = "cli"`).
- Default build targets lightweight protocol usage.
- Breaking changes are expected during `0.x`, but contract updates must be documented.
