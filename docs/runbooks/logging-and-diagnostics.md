# Logging and Diagnostics

This runbook defines the default logging posture for operators and the failure
visibility rules for contributors.

## Operator Defaults

Use `RUST_LOG=info` for normal daemon operation. Increase selected modules to
`debug` only while investigating a concrete failure.

```bash
RUST_LOG=info reticulumd --config /etc/lxmf/reticulumd/config.toml
RUST_LOG=reticulumd=debug,reticulum_rs_transport=debug,lxmf_sdk=debug lxmd --config /etc/lxmf/lxmd/config
```

Set `LXMF_LOG_PRETTY=1` for local terminal sessions where compact human-readable
RPC access logs are more useful than JSON-shaped log lines. Keep service units
on structured logs so journald and collectors can index fields.

Raise `RUST_LOG=reticulumd=trace,reticulum_rs_transport=trace` only during
focused transport, resource, or interop debugging. The hot-path delivery,
transport (`[tp-diag]`), and resource (`[resource-diag]`) diagnostics are
emitted at `debug`/`trace` on those module targets and may add noise on busy
nodes, so keep them off by default.

## What Logs Must Include

Failure logs should include:

- component or operation name
- stable reason or error code
- retryability or terminal state when known
- redacted peer, link, resource, message, or request identifier when available
- short operator action when the failure is user-actionable

Do not log bearer tokens, shared secrets, private keys, ticket material, full
message payloads, or unredacted message bodies. Prefer existing redaction
helpers for JSON event/error payloads.

## Contributor Rules

`ResourceEventKind` is public. Consumers should avoid exhaustive matches
without a catch-all arm because new terminal event variants may be added when a
previously silent failure class becomes visible to operators. Issue #369 added
`InboundFailed(ResourceFailure)` for this reason.

Do not turn expected errors into absence unless absence is the real domain
meaning. Use `Result<T, E>` or `Result<Option<T>, E>` when malformed input,
serialization failure, IO failure, or crypto failure must be distinguishable
from "not present".

Avoid silent `let _ = ...` on fallible operations. If a send, write,
serialization, or persistence trigger can fail in production, either propagate
the error or log a warning with context. Teardown best-effort operations may
remain quiet when the caller cannot use the result.

Avoid `.ok()` on production decode paths unless the error is intentionally
non-actionable. When lossy behavior is acceptable, make it explicit with
`from_utf8_lossy` or a named helper that logs or documents the downgrade.

## Useful Checks

Run the issue-369 regression scan after changing error handling:

```bash
cargo test -p reticulumd --test code_quality_issue_369
```

Run focused runtime checks for touched components:

```bash
cargo test -p reticulum-rs-transport resource::
cargo test -p reticulumd
```
