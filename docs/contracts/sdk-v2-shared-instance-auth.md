# SDK Contract v2.5 Shared Instance Auth

Status: Draft, Release C target  
Contract release: `v2.5`  
Schema namespace: `v2`

## Capability IDs

1. `sdk.capability.shared_instance_rpc_auth`

## Scope

1. Multi-client authorization for shared runtime instances.
2. Correlation and audit requirements for per-client command attribution.

## Rules

1. Shared-instance mode must keep per-principal rate and auth controls enabled.
2. Every privileged command path must emit principal identity in audit events.
3. Break-glass paths, when enabled, require explicit expiration and audit records.
