# SDK Contract v2.5 Attachments Domain

Status: Draft, Release B target  
Contract release: `v2.5`  
Schema namespace: `v2`

## Capability IDs

1. `sdk.capability.attachments`
2. `sdk.capability.attachment_delete`

## SDK Trait Surface

1. `attachment_store`
2. `attachment_get`
3. `attachment_list`
4. `attachment_delete`
5. `attachment_download`
6. `attachment_associate_topic`

## Core Types

1. `AttachmentId`
2. `AttachmentStoreRequest`
3. `AttachmentMeta`
4. `AttachmentListRequest`
5. `AttachmentListResult`

## Rules

1. Attachments are backend-owned persistence objects addressed by `attachment_id`.
2. Delete behavior is capability-gated; unsupported backends must return typed capability errors.
3. Checksums and byte length are authoritative metadata returned by the backend.
