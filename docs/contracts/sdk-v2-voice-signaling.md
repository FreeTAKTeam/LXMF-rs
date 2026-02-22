# SDK Contract v2.5 Voice Signaling Domain

Status: Draft, Release C target  
Contract release: `v2.5`  
Schema namespace: `v2`

## Capability IDs

1. `sdk.capability.voice_signaling`

## SDK Trait Surface

1. `voice_session_open`
2. `voice_session_update`
3. `voice_session_close`

## Core Types

1. `VoiceSessionId`
2. `VoiceSessionState`
3. `VoiceSessionOpenRequest`
4. `VoiceSessionUpdateRequest`

## Rules

1. This contract covers signaling only, never media transport.
2. Session state transitions must be monotonic and auditable.
