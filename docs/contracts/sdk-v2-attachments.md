# SDK Contract v2.5 Attachments Domain

Status: Draft, Release B target  
Contract release: `v2.5`  
Schema namespace: `v2`

## Capability IDs

1. `sdk.capability.attachments`
2. `sdk.capability.attachment_delete`
3. `sdk.capability.attachment_streaming`

## SDK Trait Surface

1. `attachment_store`
2. `attachment_get`
3. `attachment_list`
4. `attachment_delete`
5. `attachment_download`
6. `attachment_upload_start`
7. `attachment_upload_chunk`
8. `attachment_upload_commit`
9. `attachment_download_chunk`
10. `attachment_associate_topic`

## Core Types

1. `AttachmentId`
2. `AttachmentStoreRequest`
3. `AttachmentMeta`
4. `AttachmentListRequest`
5. `AttachmentListResult`
6. `AttachmentUploadStartRequest`
7. `AttachmentUploadSession`
8. `AttachmentUploadChunkRequest`
9. `AttachmentUploadChunkAck`
10. `AttachmentUploadCommitRequest`
11. `AttachmentDownloadChunkRequest`
12. `AttachmentDownloadChunk`

## Rules

1. Attachments are backend-owned persistence objects addressed by `attachment_id`.
2. Delete behavior is capability-gated; unsupported backends must return typed capability errors.
3. Checksums and byte length are authoritative metadata returned by the backend.
4. Streaming upload uses `upload_id` with monotonic `offset` semantics; offset mismatches return `SDK_RUNTIME_INVALID_CURSOR`.
5. `attachment_upload_commit` must reject incomplete uploads and checksum mismatches.
6. `attachment_download_chunk` supports resumable reads by caller-supplied `offset`.
7. `attachment_download_chunk` must return deterministic `next_offset`, `done`, and `checksum_sha256`.
