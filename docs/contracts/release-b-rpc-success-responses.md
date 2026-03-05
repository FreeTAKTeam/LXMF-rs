# Release-B RPC Success Response Contract

Status: draft
Owners: SDK contract maintainers

## Scope

Defines required success envelopes for Release-B attachment streaming RPC methods.

## Required Methods

- `sdk_attachment_upload_start_v2`
- `sdk_attachment_upload_chunk_v2`
- `sdk_attachment_upload_commit_v2`
- `sdk_attachment_download_chunk_v2`

## Envelope Rule

Each method requires:

- request schema
- success response schema (`response_ok`)
- error response schema (`response_error`)

## Fixture Completeness Rule

For each method above, fixtures must include:

- `*.request.valid.json`
- `*.request.invalid.json`
- `*.response.ok.valid.json`
- `*.response.ok.invalid.json`
- `*.response.error.valid.json`

CI rule:

- build fails if any Release-B request fixture exists without paired success+error response fixtures.
