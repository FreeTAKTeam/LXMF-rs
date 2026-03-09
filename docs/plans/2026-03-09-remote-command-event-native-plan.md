# Remote Command Event-Native Plan

## Goal

Make remote command and workflow execution over LXMF event-native instead of pretending to be a synchronous request/response path.

The target behavior is:

- local and daemon-local workflows may still return direct results
- remote LXMF-dispatched commands return a dispatch handle immediately
- transport delivery and domain execution are tracked separately
- command progress and completion arrive through the existing event pipeline as correlated domain events

## Problem Statement

The repository now has a strong operation runtime:

- `lxmf-sdk` exposes operation registry and envelope execution
- `reticulumd` exposes the same operation runtime over RPC
- Flutter and other clients can consume typed operations and higher-level workspace flows

However, remote command execution still risks coupling two different lifecycles:

1. transport delivery lifecycle
   - queued
   - sending
   - sent
   - delivered
   - failed

2. domain command lifecycle
   - dispatched
   - acknowledged
   - processing
   - completed
   - failed

If a remote LXMF command waits synchronously for a logical result or stores domain response payloads as if they were part of transport send, the model becomes brittle:

- transport success looks like command success
- retries and timeout semantics get confused
- clients cannot cleanly observe progress
- inbound correlated replies are treated as special cases instead of ordinary inbound command events

## Key Decisions

1. Event-native behavior applies to remote LXMF command execution, not every SDK call.
   Plain `send()` remains immediate and returns a message identifier. Local app and daemon-local workflows may continue to return direct results.

2. Remote command execution must stop at dispatch.
   A remote envelope-dispatch path should:
   - persist a command session
   - send the LXMF message
   - emit `command.dispatched`
   - return a handle immediately

3. Delivery state and domain state are separate contracts.
   Transport delivery snapshots remain transport-oriented. Domain progress and outcomes are modeled separately.

4. Domain command events need first-class stable names.
   The command lifecycle should use a stable event family:
   - `command.dispatched`
   - `command.receipt_acknowledged`
   - `command.processing_started`
   - `command.progress`
   - `command.completed`
   - `command.failed`

5. Inbound correlated replies should flow through the normal receive/event pipeline.
   A later ack, progress notification, or result message should update a stored command session and emit command events; it should not be treated as a hidden synchronous return path.

## Non-Goals

- Changing plain transport send semantics to become asynchronous command sessions.
- Forcing local in-process app workflows to become event-only.
- Replacing the current core runtime and delivery event families.
- Solving every product-specific domain workflow in the first slice.

## Proposed Architecture

### 1. Split Execution Modes

Add an explicit distinction between:

- local execution
- daemon-local composed execution
- remote LXMF-dispatched execution

Only the remote mode uses command-session tracking and event-native completion.

### 2. Add a Remote Command Session Model

Introduce a typed session record for remote command execution:

- `command_id`
- `correlation_id`
- `operation_id`
- `target`
- `message_id`
- `delivery_state`
- `command_state`
- `created_at_ms`
- `updated_at_ms`
- optional result payload
- optional error payload

This record should be queryable and observable by clients.

### 3. Add a Domain Command Event Family

Build on the existing event bus and app event mapping with a separate command-domain family carrying:

- `command_id`
- `correlation_id`
- `operation_id`
- `message_id`
- `peer_id`
- progress/result/error payload

These events should be mappable by wrappers without forcing them to inspect raw transport payloads.

### 4. Make Remote Dispatch Fire-And-Stream

The remote execution path should:

- validate the envelope
- allocate a command session
- send the LXMF command message
- emit `command.dispatched`
- return `command_id` / `correlation_id`

It should not synchronously wait for a correlated logical reply.

### 5. Correlate Inbound Replies Back Into Sessions

The inbound pipeline should recognize command progress/result/failure payloads, resolve them against the stored session, and emit the correct follow-up event:

- receipt ack -> `command.receipt_acknowledged`
- remote worker accepted -> `command.processing_started`
- partial progress -> `command.progress`
- final result -> `command.completed`
- final domain error -> `command.failed`

### 6. Preserve Local Ergonomics

Keep direct request/response behavior for:

- local app execution
- daemon-local composition
- direct RPC queries that do not cross LXMF peer boundaries

This keeps local workflows simple while fixing the distributed path.

## Required Contract Additions

Create or update contracts for:

- command session schema
- command event family and payload rules
- correlation identifier ownership and matching
- remote command timeout semantics
- terminality rules for command sessions
- relationship between transport delivery state and command state

## Recommended PR Sequence

### PR-1: Remote Command Session Foundation

Branch: `codex/remote-command-session-foundation`

Scope:

- add remote command session types and storage
- define command and delivery state separation
- expose query/get helpers for command sessions

Acceptance criteria:

- remote dispatch can create a persistent command session
- command and delivery state are stored separately
- no logical result is required at dispatch time

Suggested commits:

- `feat: add remote command session model`
- `feat: persist command state separately from delivery status`

### PR-2: Domain Command Event Family

Branch: `codex/domain-command-events`

Scope:

- add stable command-domain event names
- extend event mapping in `lxmf-sdk`
- document event payload requirements

Acceptance criteria:

- event stream can represent the full remote command lifecycle
- wrappers do not need raw payload inspection to detect progress/completion/failure

Suggested commits:

- `feat: add command domain event family`
- `test: map command events through sdk app event adapters`

### PR-3: Async Remote Envelope Dispatch

Branch: `codex/remote-envelope-dispatch-async`

Scope:

- change remote LXMF envelope execution to dispatch-only
- return a command handle/session instead of a logical result
- emit `command.dispatched`

Acceptance criteria:

- remote dispatch never blocks on correlated LXMF replies
- local and daemon-local execution remain direct

Suggested commits:

- `refactor: make remote envelope dispatch fire-and-stream`
- `test: remote dispatch returns command handle without waiting on reply`

### PR-4: Inbound Correlation and Session Updates

Branch: `codex/inbound-command-correlation`

Scope:

- recognize inbound ack/progress/result/failure payloads
- resolve them to stored command sessions
- append lifecycle events and terminal results

Acceptance criteria:

- inbound correlated replies drive command state transitions
- terminal replies close out sessions cleanly
- retry/timeout behavior is contract-defined

Suggested commits:

- `feat: correlate inbound command replies into command sessions`
- `test: command sessions advance on inbound progress and completion`

### PR-5: Wrapper Command Watchers

Branch: `codex/flutter-command-watchers`

Scope:

- expose command-handle watching in Flutter
- add typed watcher helpers for progress and completion

Acceptance criteria:

- app clients can dispatch a remote command and observe it without custom correlation logic

Suggested commits:

- `feat: add flutter command watcher helpers`
- `test: watch remote command lifecycle through rpc app client`

## Immediate Recommendation

Do not try to retrofit this behavior into every existing send path at once.

Start by introducing a clearly separate remote command-dispatch path and prove it for one command family first. Once that path is stable, migrate higher-level remote workflows onto it incrementally.
