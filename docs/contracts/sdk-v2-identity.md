# SDK Contract v2.5 Identity Domain

Status: Draft, Release C target  
Contract release: `v2.5`  
Schema namespace: `v2`

## Capability IDs

1. `sdk.capability.identity_multi`
2. `sdk.capability.identity_discovery`
3. `sdk.capability.identity_import_export`
4. `sdk.capability.identity_hash_resolution`
5. `sdk.capability.contact_management`

## SDK Trait Surface

1. `identity_list`
2. `identity_announce_now`
3. `identity_presence_list`
4. `identity_activate`
5. `identity_import`
6. `identity_export`
7. `identity_resolve`
8. `identity_contact_update`
9. `identity_contact_list`
10. `identity_bootstrap`

## Core Types

1. `IdentityRef`
2. `IdentityBundle`
3. `IdentityImportRequest`
4. `IdentityResolveRequest`
5. `TrustLevel`
6. `ContactUpdateRequest`
7. `ContactRecord`
8. `ContactListRequest`
9. `ContactListResult`
10. `PresenceListRequest`
11. `PresenceRecord`
12. `PresenceListResult`
13. `IdentityBootstrapRequest`

## Rules

1. Discovery announce requests must be explicit (`identity_announce_now`), never implicit side effects of unrelated identity calls.
2. Presence listing is cursor-based and deterministic for stable replay.
3. Contact updates are patch-like:
- omitted fields keep prior values
- non-null provided fields replace prior values
4. `trust_level` is constrained to `unknown|untrusted|trusted|blocked`.
5. Bootstrap marks contact as `trusted` and `bootstrap=true`; `auto_sync=true` also seeds peer presence.
