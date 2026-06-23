# Audit & fix: `Option`-returning functions that should be `Result`

Branch `fix/silent-error-drops`. Goal: stop collapsing errors into `None`. When a
function returns `Option<T>` but a `None` actually means *something went wrong*
(parse/decode/UTF-8/lock/IO/timeout/encode failure), the caller can't tell
absence from failure, the error detail is lost, and nothing is logged.

Full audit, rationale, per-site catalog (patterns A–O), KEEP list, and the
classification rules live in the approved plan
(`~/.claude/plans/can-you-find-all-elegant-scroll.md`). This file is the **living
checklist**: each step is one commit (implement → build → clippy → test → tick
box → commit). Stop on red; never commit a broken tree.

## Classification shapes
- **R** = `Result<T, E>` (every `None` is a failure).
- **S** = `Result<Option<T>, E>` (real absence *and* error share one `None`) — default shape.
- **E** = plain `T` / `.expect()` (`None` impossible).
- **P** = pure `Option`→`Option` pipe: log at the inner failure now, ideally thread `Result<Option<T>>`.

## Checklist
- [x] **S0** Finish in-flight encode work: share one `AnnounceEncodeError`
  (de-dup lxmf-core vs reticulumd). (pn_metadata `decode_utf8*` → `Result` folded
  into S1/S4 where the shared helper lands.)
- [x] **S1** Pattern **A** — `decode_utf8`/`decode_utf8_owned -> Result`,
  deleted the copies, updated callers. (committed)
  - No single cross-crate helper is possible (rns-rpc depends on `lxmf-reference`,
    not lxmf-core; no shared foundation crate), so consolidated **per crate**:
    new `reticulumd::text` (6 copies → 1, keeps `context` + warn log); rns-rpc
    shared `decode_utf8`/`_owned` in the `include!`d helpers module (3 → 1, keeps
    debug log); lxmf-core `announce::decode_utf8_owned` → `Result`.
  - Callers still returning `Option` use `.ok()` / `.is_ok_and()` for now; these
    interim `.ok()`s are removed as each caller is converted in S2–S5.
- [x] **S2** Pattern **B** (lxmf-core) — `decode_msgpack<T>`, `rmpv_to_json*`,
  `decode_*` and `decode_msgpack_value*` in `wire_fields_parts` + `inbound_decode.rs`.
  (committed) Followed the plan's literal **R**: the client/telemetry/sideband
  decoders + `decode_msgpack_value*` now return `Result` and a malformed/odd-typed
  client field is a hard error (no raw fallback). Per that decision, rewrote
  `rmpv_to_json_preserves_nonbinary_telemetry_payload_as_string` →
  `rmpv_to_json_errors_on_nondecodable_telemetry_payload`. `wire_message_id_hex`
  is now `Result<String>` (a too-short payload fails the inbound decode; the
  destination-hash id fallback is removed). Also folded in `decode_hex_attachment_data`
  (Pattern C, same file). columba meta keeps its always-succeeding preservation
  fallback (R-typed but infallible by design). `display_name_from_delivery_app_data`
  → `Result<Option<String>, AnnounceDecodeError>`; sdk bridge `TODO(S8)`.
- [x] **S3** Pattern **B/C** (reticulumd) — `announce_names.rs` decode/parse cluster.
  All decode/parse helpers return `Result`. Early exits return `Err`, not `Ok(None)`;
  the only `Ok(None)` paths are genuine absence (Nil protocol marker, cost out of
  range). Callers in `announce_ingest.rs` and `bridge_announce.rs` pre-check
  `is_empty()` and use explicit `match` + `log::warn!`/`log::debug!`; no `.ok()`
  bridges. `announce_stamp_cost` promoted to `Result<Option<u32>>` (propagates `?`).
- [x] **S4** Pattern **B/C** (rns-rpc) — `pn_metadata_to_json`, `merge_fields_with_options`,
  `parse_python_int_*`, `module_support`, `http_parts` parsers. Blast radius spans
  serde deserializers + the msgpack/json coercion graph, so split into file-scoped
  sub-commits:
  - [x] **S4a** `http_parts/module_prelude.rs` — all three R per catalog:
    `parse_content_length` (R: `io::Result<usize>`; absence centralised as
    `InvalidInput` "missing content-length" since no caller treats a missing
    header as valid), `parse_request_line` (R: `io::Result<(String,String)>`),
    `percent_decode` (R: decode failure surfaces; query_param caller logs+falls
    back to raw). Conflicting Content-Length now surfaces as `InvalidData`
    "conflicting content-length headers" instead of collapsing to the generic
    "missing content-length"; two tests updated to assert the sharper diagnostic.
  - [x] **S4b** `types_parts` (`parse_python_int_u64.rs` + `module_support.rs`) —
    `parse_python_int_u64`/`_u32`/`_u8` R, `parse_json_u32` S, `parse_peer_*_bytes`
    helpers; update the `PeerRecord` deserializer call sites.
  - [x] **S4c** `merge_fields_with_options.rs` — `parse_text_to_u32`/`parse_f64_to_u32`/
    `parse_fuzzy_*` R, `outbound_wire_fields` S, `parse_*_from_app_data_hex` R,
    `parse_rch_capabilities_from_lxmf_announce` R, `decode_utf8_field` (A).
  - [x] **S4d** `pn_metadata_to_json.rs` — `pn_metadata_to_json` S,
    `pn_metadata_key_to_string` S, `pn_metadata_value_to_json` R,
    `parse_pn_metadata_name` S, `extract_capabilities_from_msgpack` S.
- [x] **S5** Pattern **C** (reticulumd bin) — `rpc_access_log.rs`, `announce_ingest.rs`,
  `inbound_control_peer.rs`.
  `parse_http_request_line` → R; `parse_status_code` → R; `parse_rpc_response_error` → S
  (genuine absence: `rpc_response.error` field; all other None paths are parse errors);
  `announce_stamp_cost` already R (done in S3); `transfer_limit_kb_from_value` → S
  (`Ok(None)` = positive-infinity no-limit, `Ok(Some(f))` = finite limit, `Err` = parse
  failure; caller in `peer_request_from_data` logs and returns `None` on Err).
  Tests updated. Gaps fixed in commits 2bcedee (rpc_access_log.rs) and 41749e1 (inbound_control_peer.rs).
- [x] **S6** Pattern **D** — env-var readers (`env_u64`/`env_bool`/`env_usize`).
  `env_bool`/`env_u64`/`env_usize` → `Result<Option<T>, &'static str>`: `Ok(None)` when
  unset, `Err` when set-but-invalid, `Ok(Some(v))` when valid. Callers use
  `.unwrap_or_else(|err| { log::warn!(...); None })` to maintain `Option<T>` in struct fields.
  Gap fixed in commit 2bcedee.
- [x] **S7** Pattern **E/H** — lock-poison + file IO (`receipt.rs`, `ratchets.rs`,
  `outbound_resources.rs`, `current_limits`, `sdk_token_auth_config`, `load_private_key`).
  `resolve/lookup_receipt_message_id` → R (`io::Result<String>`; not-found is `Err(NotFound)`);
  `load_record` → S (`io::Result<Option<RatchetRecord>>`); `sdk_token_auth_config` → R (`io::Result<(...)>`);
  `spawn_event_sink_worker` → R (`io::Result<SyncSender>`; enabled-guard moved to caller);
  `sdk_event_sink_allowed_kinds` → S (`io::Result<Option<HashSet<String>>>`);
  `current_limits` → S (`io::Result<Option<EffectiveLimits>>`);
  `auth_metadata_for_request` → S (`Result<Option<...>, SystemTimeError>`);
  `parse_error_category` → R (`Result<ErrorCategory, &'static str>`);
  `parse_control_request_payload` → R (`Result<([u8;16], Option<rmpv::Value>), String>`; was missed in S3).
  `current_inbound_stamp_cost` → S (`Result<Option<u32>, &'static str>`; lock-poison → Err,
  daemon absent → Ok(None), cost out-of-range → Ok(None)); caller logs on Err.
  `take_outbound_resource_tracking` → R (`Result<OutboundResourceTracking, &'static str>`;
  lock-poison → Err, not-found → Err "resource not tracked"); completion/failure callers
  log on Err; tests `.is_none()` → `.is_err()`. Gap fixed in commit 2bcedee.
  `ensure_peer_iface` deferred: uses tokio async mutex (no poison), error type unclear.
- [x] **S8** Pattern **N** — JSON accessors (`lxmf-sdk/domain_parts/*`, `app/events.rs`).
  All `json_*` / `peer_queue_json_*` / `propagation_node_json_*` / `remote_status_json_*` /
  `remote_transfer_json_*` / `propagation_policy_json_*` helpers converted to
  `Result<Option<T>, &'static str>` (S shape). `payload_state` / `receipt_state` /
  `payload_peer_id` in `events.rs` likewise. `json_u32` in `outbound_message_for_query.rs`.
  `propagationremote.rs` (not in canonical catalog but shares helpers via `include!`) also updated.
  Callers use `.ok().flatten()` silently (lxmf-sdk has no log dep).
- [x] **S9** Pattern **G** — crypto/decrypt/verify (`resource_wire`, `wire`, `tunnels`,
  `parse_link_identify_payload`).
  `packet_for_resource_manager` → `Result<Packet, RnsError>` (R; decrypt failure was already
  logged + None, now Err); `validate_tunnel_synthesize` → `Result<Hash, RnsError>` (R;
  bad-len → PacketError, bad-sig → IncorrectSignature; test `.is_none()` → `.is_err()`);
  `validated_receipt_hash` → `Result<Option<[u8;HASH_SIZE]>, RnsError>` (S; no-link/no-dest
  → Ok(None), verify failure → Err(CryptoError); callers: handle_proof uses
  `.unwrap_or_else(|err| { log::warn!(...); None })`, test helper uses `.ok().flatten()`);
  `parse_link_identify_payload` → `Result<Identity, &'static str>` (R; caller now logs
  the error variant instead of silently ignoring).
- [x] **S10** Pattern **F** — FFI null-pointer wrappers.
  `destination_list` → `Result<Vec<[u8;16]>, &'static str>` (null+non-zero-count → Err);
  `v1_node_mut` → `Result<&mut RnsEmbeddedV1Node, &'static str>`;
  `v1_subscription_mut` → `Result<&mut RnsEmbeddedEventSubscription, &'static str>`.
  All 9 call sites updated from `let Some(x) = f() else { return ... }` to `match`.
- [x] **S11** Pattern **I** — LoRa/BLE encode-frame builders.
  `id_beacon_write` (rnode_ble + vrn76_kiss_ble) → S (`Result<Option<T>, &'static str>`):
  Ok(None) = beacon not configured, Ok(Some(w)) = write produced, Err = no write despite
  beacon present. `generate_peering_key_value` → R (`Result<u32, &'static str>`): msgpack
  encode failure → Err, nonce exhausted → Err; caller `peer_peering_key_value` logs+returns
  None. `display_image_frames`: `u8::try_from(line).ok()?` → `.expect()` (always in range
  0..=255 due to `.take(256)` — dead-code None path). lora_parts display-frame builders
  (222,227,232,241) KEEP: `encode_command_frame` is infallible, sole None = display absent.
- [x] **S12** Pattern **J/K** — control/zmq decode + timeouts.
  `handle_zmq_command_message` → R (`Result<ZmqOutboundResponse, &'static str>`): decode/protocol
  failures log inline and return Err; local-auth-error and success return Ok. Caller uses
  `if let Ok(response)`. `parse_transfer_limit_bytes` → S: positive-infinity → Ok(None) (no
  limit), parse errors (NaN, wrong type, bad string) → Err, valid → Ok(Some(bytes)); caller
  logs Err and falls back to None. `wait_for_propagation_signal` → R (`Result<u8, &'static str>`):
  timeout → Err("timeout"), signal out-of-range → Err, received → Ok; callers use `if let Ok`
  / `let Ok ... else { return }`. `resolve_destination_identity_blocking` → S: runtime-build
  failure → Err, thread-panic → Err, not-found-within-timeout → Ok(None), found → Ok(Some);
  callers log Err and treat as None. `resolve_identity` → S: cancellation → Ok(None),
  identity-not-found after 12 s deadline → Err(failure_status); failure_status param tightened
  to `&'static str`. `resolve_destination_identity` → S: propagates `resolve_identity` Result
  via `?`, Ok(None) for cancelled; callers (propagation_preparation_context, run_direct,
  run_opportunistic) match and log Err.
- [x] **S13** Pattern **M** — string→enum parsers (split empty vs unknown).
  `InterfaceMode::parse`, `AutoDiscoveryScope::parse`, `MulticastAddressType::parse`,
  `LoraConfig::for_region`, `normalize_trust_level`, `normalize_voice_state`,
  `parse_python_bool`, `category_for_code` → S (`Result<Option<T>, &'static str>`): `""`
  (or whitespace-only) → Ok(None), valid value → Ok(Some(T)), non-empty unknown → Err("...").
  Callers that treat both absent and unknown as "use default / return None" use
  `.ok().flatten()`; callers that need to error on unknown use explicit match with
  `Ok(None) | Err(_) => error`. `category_for_code` caller `RpcError::new` stays infallible:
  it `match`es and `log::debug!`s the previously-silent "no known category" drop, then falls
  back to `None`. Tests updated to Ok(Some(...)) / is_err().
- [x] **S14** Pattern **L** — enum-variant accessors → S (`Result<Option<T>, &'static str>`).
  `mtls_auth` (EventStreamRequestAuth) and `mtls_for_session_auth` (SessionAuth): Mtls variant
  → Ok(Some(MtlsRequestAuth)); LocalTrusted/Token → Ok(None) (genuine "not an mTLS session");
  Mtls with empty/whitespace `ca_bundle_path` → Err (real invariant — mTLS needs a CA bundle).
  All callers (rpceventstreamio stream-open, call_rpc/_async, negotiate/_async) thread the Err
  as `SdkError::new(code::VALIDATION_INVALID_ARGUMENT, Validation, reason)?`; test asserts the
  empty-ca_bundle Err path. `into_bytes` (PythonPeeringKeyStamp) → Ok(Some)/Ok(None) for
  bytes/nil; `selected_ids` (PeerSyncWantedIds) → Ok(None) for All, Ok(Some) for Selected —
  neither has a malformed variant so no Err is constructed today, but the S signature threads
  the error channel through callers (deserializer via `serde::de::Error::custom`; peer-sync
  validation/response via `io::Error`) so a future failure surfaces instead of collapsing to
  None. LoRa `baud_rate` / `activity_probe` remain KEEP per plan (variant legitimately lacks
  field).
- [ ] **S15** **E** plain-`T`/`.expect()` cases + invariant SPLITs.
- [ ] **S16** Pattern **O** pure pipes + BORDERLINE — after decision.

## Notes / decisions log
- S0: reticulumd `announce_names` re-uses `lxmf::announce::AnnounceEncodeError`;
  added a `std`-gated `impl std::error::Error` in lxmf-core so nothing is lost.
- Pre-existing failing test (NOT from this work, fails on clean HEAD too —
  sandbox network lets the connect to TEST-NET-3 succeed):
  `reticulumd` bin test `bootstrap_strict_mode_panics_on_tcp_client_preflight_connect_failure`.
  Treat as known-red baseline when running reticulumd bin tests.
