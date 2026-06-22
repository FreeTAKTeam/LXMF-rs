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
- [ ] **S3** Pattern **B/C** (reticulumd) — `announce_names.rs` decode/parse cluster.
- [ ] **S4** Pattern **B/C** (rns-rpc) — `pn_metadata_to_json`, `merge_fields_with_options`,
  `parse_python_int_*`, `module_support`, `http_parts` parsers.
- [ ] **S5** Pattern **C** (reticulumd bin) — `rpc_access_log.rs`, `announce_ingest.rs`.
- [ ] **S6** Pattern **D** — env-var readers (`env_u64`/`env_bool`/`env_usize`).
- [ ] **S7** Pattern **E/H** — lock-poison + file IO (`receipt.rs`, `ratchets.rs`,
  `outbound_resources.rs`, `current_limits`, `sdk_token_auth_config`, `load_private_key`).
- [ ] **S8** Pattern **N** — JSON accessors (`lxmf-sdk/domain_parts/*`, `app/events.rs`).
- [ ] **S9** Pattern **G** — crypto/decrypt/verify (`resource_wire`, `wire`, `tunnels`,
  `parse_link_identify_payload`).
- [ ] **S10** Pattern **F** — FFI null-pointer wrappers.
- [ ] **S11** Pattern **I** — LoRa/BLE encode-frame builders.
- [ ] **S12** Pattern **J/K** — control/zmq decode + timeouts.
- [ ] **S13** Pattern **M** — string→enum parsers (split empty vs unknown).
- [ ] **S14** Pattern **L** — enum-variant accessors (convert where caller expects variant).
- [ ] **S15** **E** plain-`T`/`.expect()` cases + invariant SPLITs.
- [ ] **S16** Pattern **O** pure pipes + BORDERLINE — after decision.

## Notes / decisions log
- S0: reticulumd `announce_names` re-uses `lxmf::announce::AnnounceEncodeError`;
  added a `std`-gated `impl std::error::Error` in lxmf-core so nothing is lost.
- Pre-existing failing test (NOT from this work, fails on clean HEAD too —
  sandbox network lets the connect to TEST-NET-3 succeed):
  `reticulumd` bin test `bootstrap_strict_mode_panics_on_tcp_client_preflight_connect_failure`.
  Treat as known-red baseline when running reticulumd bin tests.
