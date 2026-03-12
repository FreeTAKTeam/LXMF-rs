# ADR 0009: gRPC as the Canonical Internal API

- Status: Proposed
- Date: 2026-03-12

## Context

The current daemon API is centered on JSON-RPC plus an OpenRPC contract. That surface works for
compatibility and browser-friendly tooling, but it does not align with the primary goals for the
next phase of `lxmf-rs`:

- strongly typed generated clients
- internal service-to-service RPC
- non-browser clients as the main target

Reviewer feedback identified several blockers in the initial migration sketch:

1. Transport drift is unacceptable if JSON-RPC and gRPC own separate business logic.
2. Pagination semantics are not yet strict enough for a generated-client-first API.
3. Event flow must make an explicit decision between unary polling and server streaming.
4. JSON-RPC compatibility commitments in `docs/contracts/rpc-contract.md` remain active.
5. Interface management and reload operations are operational/admin concerns and must be modeled as
   a first-class admin surface rather than left as permanently split legacy commands.

## Decision

Adopt gRPC/protobuf as the canonical API for new internal integrations while retaining JSON-RPC as
the compatibility surface during migration.

The migration must satisfy these rules:

1. Both transports call a single shared application/domain layer.
2. JSON-RPC remains supported until an explicit deprecation policy is accepted.
3. Pagination moves to a common opaque page-token contract.
4. Events support both unary polling and server-streaming subscriptions.
5. Admin/interface management is modeled separately from the core domain services.

## Canonical Transport Model

### Shared Core

`reticulumd` will expose two transport adapters:

- JSON-RPC adapter for compatibility
- gRPC adapter for canonical internal use

Both adapters must call the same shared application/service layer. Transport-specific code may map
metadata, auth, deadlines, and errors, but it must not own domain behavior.

### gRPC Service Groups

Phase-1 canonical services:

- `runtime.v1.RuntimeService`
- `delivery.v1.DeliveryService`
- `events.v1.EventService`
- `admin.v1.InterfaceAdminService`

Planned phase-2 domain services:

- `topics.v1.TopicService`
- `attachments.v1.AttachmentService`
- `markers.v1.MarkerService`
- `identity.v1.IdentityService`
- `workflow.v1.WorkflowService`
- `voice.v1.VoiceService`

## Phase Order

### Phase 0: Preconditions

- Define a shared domain/application service layer inside the existing workspace.
- Define a common protobuf error model and transport mapping rules.
- Define common pagination semantics:
  - stable ordering
  - opaque page tokens
  - token scope bound to method/filter/identity context
- Define auth, rate-limit, replay, and deadline behavior for gRPC transport parity.

### Phase 1: Dual-Stack Foundation

Add a gRPC listener alongside the existing JSON-RPC loop and implement:

- `runtime.v1.RuntimeService.GetSnapshot`
- `runtime.v1.RuntimeService.Negotiate`
- `events.v1.EventService.PollEvents`
- `events.v1.EventService.SubscribeEvents`
- `delivery.v1.DeliveryService.Send`
- `delivery.v1.DeliveryService.GetStatus`
- `delivery.v1.DeliveryService.Cancel`
- `admin.v1.InterfaceAdminService.ListInterfaces`
- `admin.v1.InterfaceAdminService.SetInterfaces`
- `admin.v1.InterfaceAdminService.ReloadConfig`

This phase proves:

- dual-stack daemon startup
- shared-core transport parity
- unary plus streaming event semantics
- generated-client viability
- first-class interface/admin management without forcing operators onto the legacy-only path

### Phase 2: Domain Expansion

Move stable domain families to gRPC:

- topics
- attachments
- markers
- identity
- workflow
- voice

Phase 2 should reuse the shared pagination and error contracts from phase 1.

Admin/interface methods remain operationally separate from the core domain services, but they are
included in phase 1 because they are an important operator workflow and should not stay split
across transports indefinitely.

## Proto Layout

Proto sources live under:

`api/proto/lxmf/...`

Initial package layout:

- `lxmf/common/v1/pagination.proto`
- `lxmf/common/v1/errors.proto`
- `lxmf/common/v1/interfaces.proto`
- `lxmf/runtime/v1/runtime.proto`
- `lxmf/delivery/v1/delivery.proto`
- `lxmf/events/v1/events.proto`
- `lxmf/topics/v1/topics.proto`
- `lxmf/attachments/v1/attachments.proto`
- `lxmf/markers/v1/markers.proto`
- `lxmf/identity/v1/identity.proto`
- `lxmf/admin/v1/interface_admin.proto`

## Crate and Tooling Direction

- Keep `rns-rpc` as the initial transport-boundary crate for gRPC server integration.
- Add an `xtask`-driven proto generation/check workflow rather than scattering ad hoc build logic.
- Introduce any new crate edges only with an explicit boundary-allowlist update in
  `Cargo.toml` and `tools/scripts/check-boundaries.sh`.
- Add proto governance before first public rollout:
  - lint checks
  - breaking-change checks
  - transport-parity tests between JSON-RPC and gRPC

## Consequences

Positive:

- Internal callers gain generated typed clients.
- Service-to-service integrations use a transport that fits the target usage.
- JSON-RPC compatibility remains intact during the migration window.
- Event streaming can use gRPC strengths without losing replay/poll compatibility.

Tradeoffs:

- Two transport adapters must coexist temporarily.
- Pagination, auth, and error semantics must be tightened before rollout.
- Interface/admin commands still carry restart-required semantics and need careful parity testing.

## Enforcement

- Compatibility contract: `docs/contracts/rpc-contract.md`
- Architecture overview: `docs/architecture/overview.md`
- Boundary enforcement: `tools/scripts/check-boundaries.sh`
- Follow-up implementation should add:
  - proto generation/check `xtask` commands
  - parity tests across transports
  - explicit deprecation policy before JSON-RPC removal
