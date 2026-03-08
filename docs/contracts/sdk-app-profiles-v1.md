# SDK App Profiles v1

Status: Draft, implementation target  
Contract family: `sdk-app`  
Contract release: `v1`

## Purpose

This document defines the profile presets and policy defaults for app-api consumers.

Profiles exist so most apps can adopt the SDK with minimal configuration and without encoding custom policy logic.

## Profile Model

App API profiles are named policy bundles.

Required presets:

- `mobile_default`
- `desktop_default`
- `embedded_default`
- `testing_default`

Rules:

1. Profiles are immutable for a running app-api session unless the contract explicitly allows a mutable subset.
2. Effective profile semantics are frozen at successful start.
3. All wrappers must implement the same semantic meaning for a given profile name.

## Profile Fields

Each profile must define:

- lifecycle posture
- retry policy
- reconnect policy
- queue pressure policy
- timeout policy
- persistence/durability support
- event buffering expectations
- redaction/security defaults
- capability assumptions

## `mobile_default`

Intended for:

- user-facing mobile apps
- intermittent connectivity
- battery- and UX-sensitive runtimes

Required defaults:

- reconnect enabled with bounded backoff
- retry enabled for retryable delivery failures
- queue pressure policy favors bounded buffering or bounded retry over immediate failure when supported
- redaction enabled
- callback/event delivery safe for UI integration through binding-specific dispatch rules

## `desktop_default`

Intended for:

- desktop apps
- local agents
- service-style host integrations

Required defaults:

- reconnect enabled
- retry enabled with more generous resource assumptions than mobile
- stronger event buffering than mobile
- redaction enabled
- richer diagnostics permitted within the default redaction policy

## `embedded_default`

Intended for:

- constrained host-side integrations
- lower-resource deployments
- profiles closer to embedded/native runtime constraints

Required defaults:

- conservative buffering
- conservative retry policy
- explicit handling when durability is unsupported
- capability assumptions may be narrower than desktop/mobile, but semantics must stay explicit

## `testing_default`

Intended for:

- integration tests
- deterministic harnesses
- contract/conformance execution

Required defaults:

- deterministic timing knobs where possible
- reduced jitter in retry/backoff policy
- explicit failure visibility over silent auto-healing
- diagnostics suitable for contract assertions

## Policy Defaults

### Retry

Rules:

1. Retry defaults are part of the profile, not wrapper-local behavior.
2. Retry policy must define:
   - eligible failure classes
   - max attempts or exhaustion policy
   - backoff strategy
   - terminal exhaustion behavior

### Reconnect

Rules:

1. Reconnect defaults are part of the profile.
2. Reconnect behavior must define:
   - whether auto-reconnect is enabled
   - scheduling strategy
   - escalation to degraded or failed state

### Queue Pressure

Rules:

1. Queue pressure behavior must be explicit:
   - fail fast
   - bounded retry
   - bounded buffering
2. The chosen policy must be visible through typed events and errors.
3. Apps should not need custom queue-pressure handling for normal use.

### Timeout

Rules:

1. Operation timeouts must have profile-defined defaults.
2. Wrappers may expose overrides, but default behavior must be consistent across languages.

### Persistence and Durability

Rules:

1. Profiles must state whether durable queueing and restart recovery are supported.
2. Durability must never be implied by default if unsupported by the runtime/profile.
3. If durability is unsupported, the profile must define the failure or degradation semantics clearly.

## Security Defaults

Required baseline:

- redaction enabled
- least-privilege bind/auth posture
- explicit secure auth requirements for remote/shared-instance use

Rules:

1. Profiles must not weaken security posture silently.
2. Any profile-specific security tradeoff must be explicit.

## Profile Evolution Rules

1. Changing the semantic meaning of an existing profile is a contract change.
2. Additive tunables are allowed if defaults preserve prior meaning.
3. Breaking behavioral shifts require a new profile version or new profile identifier.
