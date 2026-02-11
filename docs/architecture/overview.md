# LXMF-rs Architecture

## Core Principles
- Protocol code must be independent from operator workflows.
- Runtime behavior must be explicit and testable.
- Public API should remain compact and intentional.

## Public API Surface
- `lxmf::message`
- `lxmf::identity`
- `lxmf::router_api`
- `lxmf::errors`

## Internal Modules
Internal modules remain available for crate composition but are not contract-stable.
