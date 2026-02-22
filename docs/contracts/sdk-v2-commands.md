# SDK Contract v2.5 Remote Commands Domain

Status: Draft, Release C target  
Contract release: `v2.5`  
Schema namespace: `v2`

## Capability IDs

1. `sdk.capability.remote_commands`

## SDK Trait Surface

1. `command_invoke`
2. `command_reply`

## Core Types

1. `RemoteCommandRequest`
2. `RemoteCommandResponse`

## Rules

1. `command_invoke` correlation token is the command envelope `request_id`.
2. `command_reply` must include `params.correlation_id` and target a previously accepted `command_invoke.request_id`.
3. `RemoteCommandResponse` only carries response material (`accepted`, `payload`, `extensions`); correlation is transport-level.
