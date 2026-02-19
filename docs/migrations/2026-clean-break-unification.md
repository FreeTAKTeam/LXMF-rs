# 2026 Clean-Break Unification Migration

Date: 2026-02-19

## Applies To

- `lxmf` `0.3.0+`
- `reticulum-daemon` runtime bridge paths using canonical wire helpers

## Breaking Changes

1. Public attachment key is `attachments` only.
2. Public `files` is rejected.
3. Public numeric key `"5"` is rejected.
4. Attachment text values must be explicit:
   - `hex:<payload>`
   - `base64:<payload>`
5. Client send paths use `send_message_v2` only.

## Before / After

Legacy (no longer accepted):

```json
{
  "fields": {
    "files": [["photo.jpg", [1, 2, 3]]]
  }
}
```

Canonical:

```json
{
  "fields": {
    "attachments": [
      {
        "name": "photo.jpg",
        "data": [1, 2, 3]
      }
    ]
  }
}
```

Text attachment payloads:

```json
{
  "attachments": [
    { "name": "blob.bin", "data": "hex:0a0b0c" },
    { "name": "blob2.bin", "data": "base64:AQID" }
  ]
}
```

## RPC Guidance

- Use `send_message_v2` from CLI/runtime clients.
- `send_message` remains available server-side for compatibility, but strict canonical validation still applies.

## Validation Checklist

1. Update all payload producers to emit `attachments`.
2. Remove `files` and public `"5"` from user-facing JSON.
3. Ensure attachment text values are prefixed (`hex:` / `base64:`).
4. Re-run contract and interop gates before release.
