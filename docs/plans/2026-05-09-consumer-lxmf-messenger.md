# Consumer LXMF Messenger Plan

Status: product direction note

This note captures a possible product direction for this repository: a
mainstream messenger that makes Reticulum usable without exposing normal users
to Reticulum internals.

## Product Thesis

Build a normal-feeling messenger whose baseline protocol is standard LXMF over
Reticulum.

The user-facing promise is:

> A private backup messenger that works online, nearby, or off-grid.

The product should not be positioned as a proprietary replacement for the
existing Reticulum ecosystem. Reticulum is the routing engine, LXMF is the
messaging contract, and the app is the approachable user experience on top.

## Non-Negotiable Compatibility Rule

Baseline chat must remain compatible with LXMF clients such as MeshChat,
Sideband, Nomad Network, and other standard LXMF implementations.

The app may add richer features, but those features must degrade cleanly:

- A standard LXMF client can send a message to this app.
- This app can send a normal message to a standard LXMF client.
- App-specific metadata must not be required to read the core message.
- Unknown extensions must be ignorable without breaking delivery.

This rule keeps the project aligned with the existing network instead of
creating another isolated chat island.

## Mainstream UX Rules

Normal users should not need to understand:

- announces
- destinations
- propagation nodes
- transports
- interfaces
- identities
- LXMF packets
- Reticulum routing

The default product language should use concepts like:

- contacts
- chats
- groups
- nearby
- internet relay
- community relay
- radio device
- waiting
- sent
- delivered

Advanced Reticulum controls can exist, but they belong behind an advanced mode.

## MVP Shape

The first useful product slice is a desktop app that manages the local runtime
for the user.

MVP capabilities:

- create or import a profile on first launch
- generate and store the local identity automatically
- start and supervise a local `reticulumd` or SDK-backed runtime
- send and receive baseline LXMF messages
- exchange contacts through QR codes or short invite files
- show clear delivery states: waiting, sent, delivered, failed
- provide default internet relay settings for immediate use
- support local-network or nearby mode without manual config
- expose logs and Reticulum details only in advanced mode

## Compatibility Core

The core messenger path should use standard LXMF for:

- identity and addressing
- one-to-one text messages
- propagation and store-and-forward delivery
- Reticulum-supported transports
- basic delivery behavior where available

This is the surface that should be tested against other clients.

## Optional Enhancement Layer

Features beyond standard LXMF should be optional and capability-gated.

Possible enhancements:

- profile display names and avatars
- typing indicators
- reactions
- richer attachment presentation
- group management UX
- read receipts
- voice notes
- relay recommendations
- device sync and encrypted backup

Each enhancement should have a fallback behavior for standard LXMF peers.

## Adoption Wedge

The first mainstream wedge should be a setting where ordinary messaging apps
already fail:

- festivals and large events with overloaded cellular networks
- outdoor groups, overlanding, boating, and camping
- rural properties and farms
- neighborhood outage preparedness
- small organizations that want user-owned communications infrastructure

The consumer story should be practical rather than protocol-driven:

> Download this before the event so our group chat still works when signal gets
> bad.

## Architecture Implications

The repository should preserve a clean split between:

- protocol-compatible LXMF behavior
- SDK and daemon runtime control
- app-specific UX metadata and extensions
- platform shells such as desktop or mobile UI clients

This suggests an implementation order:

1. Keep LXMF compatibility and Python interop gates green.
2. Expose a small SDK profile suitable for consumer apps.
3. Build a desktop shell that can start and supervise the runtime.
4. Add contact exchange and default relay onboarding.
5. Add optional app-specific features behind capability negotiation.

## Open Questions

- Should the first client be Tauri, egui, Swift/Kotlin, or another shell?
- Should the MVP bundle `reticulumd`, embed the SDK directly, or support both?
- What is the minimum LXMF compatibility suite for MeshChat/Sideband/Nomad
  message exchange?
- How should app-specific metadata be represented so old clients ignore it
  safely?
- Which default relay model is acceptable for a mainstream first launch?
