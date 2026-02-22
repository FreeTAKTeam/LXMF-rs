# SDK Contract v2.5 Paper Domain

Status: Draft, Release C target  
Contract release: `v2.5`  
Schema namespace: `v2`

## Capability IDs

1. `sdk.capability.paper_messages`

## SDK Trait Surface

1. `paper_encode`
2. `paper_decode`

## Core Types

1. `PaperMessageEnvelope`

## Rules

1. Paper envelope payloads are signaling artifacts only.
2. Decode behavior is idempotent for duplicate envelope scans.
