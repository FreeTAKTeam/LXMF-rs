# Fabric: Purpose and Boundaries

**Fabric** is FreeTAKTeam's reusable communications infrastructure. Its purpose
is to connect devices, peers, services, and applications across heterogeneous
networks while keeping application-specific workflows outside the communications
layer. Fabric can be used within R3AKT or independently by other projects.

In the R3AKT architecture, Fabric occupies the **Communication** layer.
Application behavior (for example, RCH server logic and REM workflows) and user
experience are separate concerns. Fabric supplies transport, message exchange,
client integration, and delivery infrastructure; it does not define missions,
incident handling, map objects, business decisions, or application UX.

## What the repository implements today

The repository was originally published as **LXMF-rs**. Its existing Rust code
provides:

- A Reticulum Network Stack implementation, including destinations, transport,
  interfaces, links, channels, and resources.
- LXMF message handling and delivery, including direct and propagation paths.
- Reusable Rust crates, typed SDK interfaces, embedded integration surfaces,
  daemon processes, command-line tools, and an optional HTTP/WebSocket adapter.
- An SDK/daemon integration path using ZeroMQ. Its optional durable broker
  provides explicitly configured custody, replay, and storage features; ordinary
  ZeroMQ transport alone does not guarantee durable delivery.
- The independently reusable `r3akt-event-broker` crate, with a local durable
  journal, subscriptions, receipt and recovery operations. This embedded broker
  has no intrinsic dependency on Reticulum, LXMF, ZeroMQ, or R3AKT business logic.

The actual supported behavior, compatibility limits, release baselines, and
validation evidence remain governed by the [current roadmap](status/current-roadmap.md),
[architecture](architecture/overview.md), and
[compatibility contract](contracts/compatibility-contract.md).

## Communication responsibilities

Fabric is responsible for exposing reusable communication capabilities and
clear integration contracts. Applications determine recipient selection,
authorization policies, message semantics, and business processing. A network
receipt, a broker custody acknowledgement, and successful business processing
are different events and must not be presented as equivalent.

Reticulum/LXMF is a supported implementation within Fabric, not the definition
of Fabric itself. Fabric's public project identity therefore does not imply
that all applications must use Reticulum or LXMF.

## Future IP-native communication

A separate high-speed, IP-native publish/subscribe capability was discussed
for many-to-one, one-to-many, and group traffic. Its intended interface uses
opaque payloads, topic subscriptions, and explicit fan-in/fan-out and flow
control, without requiring HTTP or depending on Reticulum/LXMF.

**That IP-native fabric is an intended direction, not a shipped feature of this
repository.** ZeroMQ pub/sub, multicast, or other implementations require
separate design, implementation, and performance validation. Existing ZeroMQ
SDK transport and the durable event-broker library must not be described as an
already-completed distributed multicast system. Likewise, a group subscription
cannot combine separately encrypted Reticulum packets into one valid shared
packet without changing protocol semantics.

## Identity, compatibility, and migration

**Fabric** is the project name. **Reticulum**, **LXMF**, and the existing Rust
package/binary names remain accurate technical identifiers. A documentation-only
rebrand does not change code, Cargo package IDs, crate names, protocol formats,
public APIs, version numbers, releases, or compatibility guarantees.

Keep historical LXMF-rs release notes, performance measurements, citations,
artifact names, and reproducible evidence intact. A GitHub repository rename
from `FreeTAKTeam/LXMF-rs` to `FreeTAKTeam/Fabric` is a distinct administrative
step. Review external package metadata, CI links, badges, publishing automation,
documentation redirects, and downstream consumers separately before changing
those references. Until then, existing repository URLs are authoritative.

The goal is a clear project-level identity without an unnecessary migration
for existing Reticulum/LXMF users.
