# LXMF Rust API (v0.2)

## Stable Surface
The stable compatibility contract is the explicit module subset:

- `lxmf::message`
- `lxmf::identity`
- `lxmf::router_api`
- `lxmf::errors`

Other public modules may exist for internal composition and testing, but are not contract-stable.

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
