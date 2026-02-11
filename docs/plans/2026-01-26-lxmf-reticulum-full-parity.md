# LXMF + Reticulum Full Parity Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

Last updated: 2026-01-26

**Goal:** Achieve byte‑exact and behavioral parity between Python LXMF/Reticulum and Rust LXMF‑rs/Reticulum‑rs, including CLI behavior, storage formats, and transport semantics.

**Architecture:** Build a unified parity matrix that drives TDD‑style, fixture‑backed tasks. Implement Reticulum‑rs primitives first (crypto, identity, addressing, packet framing, transport), then LXMF‑rs features on top (message composition, packing, propagation, stamps/tickets, CLI). Use Python fixtures as the canonical source of truth for bytes and file layouts.

**Tech Stack:** Rust (LXMF‑rs + Reticulum‑rs), Python reference implementations, msgpack (rmp‑serde), serde_bytes, rmpv, tokio (if used), CLI tests.

---

## Task 0: Inventory & parity map bootstrap

**Files:**
- Modify: `https://github.com/FreeTAKTeam/LXMF-rs/docs/plans/lxmf-parity-matrix.md`
- Create: `https://github.com/FreeTAKTeam/LXMF-rs/docs/plans/reticulum-parity-matrix.md`
- Modify: `https://github.com/FreeTAKTeam/LXMF-rs/docs/plans/2026-01-26-lxmf-reticulum-full-parity.md`

**Step 1: Enumerate Python reference surfaces**
- Scan Python LXMF reference tree for message formats, storage, CLI, propagation, stamps/tickets.
- Scan Python Reticulum reference tree for identity, crypto, addressing, packet framing, transport, routing, persistence, CLI.

**Step 2: Build parity matrices**
- Update `docs/plans/lxmf-parity-matrix.md` with any missing LXMF items and map to Rust files.
- Create `docs/plans/reticulum-parity-matrix.md` with a full list of Reticulum features and map to Rust files.

**Step 3: Commit**
```bash
git add https://github.com/FreeTAKTeam/LXMF-rs/docs/plans/lxmf-parity-matrix.md \
  https://github.com/FreeTAKTeam/LXMF-rs/docs/plans/reticulum-parity-matrix.md \
  https://github.com/FreeTAKTeam/LXMF-rs/docs/plans/2026-01-26-lxmf-reticulum-full-parity.md

git commit -m "docs: add full LXMF/Reticulum parity plan"
```

---

## Track A: Reticulum‑rs parity (foundation)

### Task A1: Fixture harness for Reticulum primitives

**Files:**
- Create: `https://github.com/FreeTAKTeam/Reticulum-rs/tests/fixtures/python/gen_reticulum_fixtures.py`
- Create: `https://github.com/FreeTAKTeam/Reticulum-rs/tests/fixtures/python/.reticulum/` (config)
- Create: `https://github.com/FreeTAKTeam/Reticulum-rs/tests/fixtures/reticulum/*.bin`
- Create: `https://github.com/FreeTAKTeam/Reticulum-rs/tests/reticulum_fixtures.rs`

**Step 1: Write failing test**
```rust
#[test]
fn fixture_bytes_exist() {
    assert!(std::path::Path::new("tests/fixtures/reticulum/identity.bin").exists());
}
```

**Step 2: Run test to verify failure**
Run: `cargo test -p reticulum fixture_bytes_exist -v`
Expected: FAIL (file missing)

**Step 3: Write Python fixture generator**
- Generate canonical bytes for identities, destination hashes, packet headers, and encrypted payloads.

**Step 4: Run test to verify pass**
Run: `python3 tests/fixtures/python/gen_reticulum_fixtures.py`
Run: `cargo test -p reticulum fixture_bytes_exist -v`
Expected: PASS

**Step 5: Commit**
```bash
git add https://github.com/FreeTAKTeam/Reticulum-rs/tests/fixtures/python/gen_reticulum_fixtures.py \
  https://github.com/FreeTAKTeam/Reticulum-rs/tests/fixtures/reticulum \
  https://github.com/FreeTAKTeam/Reticulum-rs/tests/reticulum_fixtures.rs

git commit -m "test: add Reticulum fixture harness"
```

### Task A2: Identity & key material parity

**Files:**
- Modify: `https://github.com/FreeTAKTeam/Reticulum-rs/src/identity.rs`
- Create: `https://github.com/FreeTAKTeam/Reticulum-rs/tests/identity_parity.rs`

**Step 1: Write failing test**
```rust
#[test]
fn identity_bytes_match_fixture() {
    let fixture = std::fs::read("tests/fixtures/reticulum/identity.bin").unwrap();
    let id = Identity::from_fixture_bytes(&fixture).unwrap();
    assert_eq!(id.to_bytes(), fixture);
}
```

**Step 2: Run test to verify failure**
Run: `cargo test -p reticulum identity_bytes_match_fixture -v`
Expected: FAIL

**Step 3: Minimal implementation**
- Implement `from_fixture_bytes` and `to_bytes` to match Python identity serialization.

**Step 4: Run test to verify pass**
Run: `cargo test -p reticulum identity_bytes_match_fixture -v`
Expected: PASS

**Step 5: Commit**
```bash
git add https://github.com/FreeTAKTeam/Reticulum-rs/src/identity.rs \
  https://github.com/FreeTAKTeam/Reticulum-rs/tests/identity_parity.rs

git commit -m "feat: add identity serialization parity"
```

### Task A3: Destination/addressing parity

**Files:**
- Modify: `https://github.com/FreeTAKTeam/Reticulum-rs/src/destination.rs`
- Create: `https://github.com/FreeTAKTeam/Reticulum-rs/tests/destination_parity.rs`

**Step 1: Write failing test**
```rust
#[test]
fn destination_hash_matches_fixture() {
    let fixture = std::fs::read("tests/fixtures/reticulum/destination_hash.bin").unwrap();
    let dest = Destination::from_fixture_bytes(&fixture).unwrap();
    assert_eq!(dest.hash(), fixture);
}
```

**Step 2: Run test to verify failure**
Run: `cargo test -p reticulum destination_hash_matches_fixture -v`
Expected: FAIL

**Step 3: Minimal implementation**
- Match Python destination hashing (lengths, hashing algorithm, truncation).

**Step 4: Run test to verify pass**
Run: `cargo test -p reticulum destination_hash_matches_fixture -v`
Expected: PASS

**Step 5: Commit**
```bash
git add https://github.com/FreeTAKTeam/Reticulum-rs/src/destination.rs \
  https://github.com/FreeTAKTeam/Reticulum-rs/tests/destination_parity.rs

git commit -m "feat: add destination hash parity"
```

### Task A4: Packet framing and transport header parity

**Files:**
- Modify: `https://github.com/FreeTAKTeam/Reticulum-rs/src/packet.rs`
- Create: `https://github.com/FreeTAKTeam/Reticulum-rs/tests/packet_parity.rs`

**Step 1: Write failing test**
```rust
#[test]
fn packet_header_matches_fixture() {
    let fixture = std::fs::read("tests/fixtures/reticulum/packet_header.bin").unwrap();
    let packet = Packet::from_fixture_bytes(&fixture).unwrap();
    assert_eq!(packet.header_bytes(), fixture);
}
```

**Step 2: Run test to verify failure**
Run: `cargo test -p reticulum packet_header_matches_fixture -v`
Expected: FAIL

**Step 3: Minimal implementation**
- Match Python header fields, ordering, flags, endian, and length encoding.

**Step 4: Run test to verify pass**
Run: `cargo test -p reticulum packet_header_matches_fixture -v`
Expected: PASS

**Step 5: Commit**
```bash
git add https://github.com/FreeTAKTeam/Reticulum-rs/src/packet.rs \
  https://github.com/FreeTAKTeam/Reticulum-rs/tests/packet_parity.rs

git commit -m "feat: add packet header parity"
```

### Task A5: Encryption/decryption parity for packet payloads

**Files:**
- Modify: `https://github.com/FreeTAKTeam/Reticulum-rs/src/crypto.rs`
- Create: `https://github.com/FreeTAKTeam/Reticulum-rs/tests/crypto_parity.rs`

**Step 1: Write failing test**
```rust
#[test]
fn encrypted_payload_matches_fixture() {
    let fixture = std::fs::read("tests/fixtures/reticulum/encrypted_payload.bin").unwrap();
    let key = std::fs::read("tests/fixtures/reticulum/crypto_key.bin").unwrap();
    let plaintext = std::fs::read("tests/fixtures/reticulum/plaintext.bin").unwrap();
    let cipher = crypto::encrypt(&key, &plaintext).unwrap();
    assert_eq!(cipher, fixture);
}
```

**Step 2: Run test to verify failure**
Run: `cargo test -p reticulum encrypted_payload_matches_fixture -v`
Expected: FAIL

**Step 3: Minimal implementation**
- Match Python crypto algorithm, nonce/iv, padding, and MAC.

**Step 4: Run test to verify pass**
Run: `cargo test -p reticulum encrypted_payload_matches_fixture -v`
Expected: PASS

**Step 5: Commit**
```bash
git add https://github.com/FreeTAKTeam/Reticulum-rs/src/crypto.rs \
  https://github.com/FreeTAKTeam/Reticulum-rs/tests/crypto_parity.rs

git commit -m "feat: add crypto parity for payloads"
```

### Task A6: Transport routing basics and persistence parity

**Files:**
- Modify: `https://github.com/FreeTAKTeam/Reticulum-rs/src/router.rs`
- Modify: `https://github.com/FreeTAKTeam/Reticulum-rs/src/storage.rs`
- Create: `https://github.com/FreeTAKTeam/Reticulum-rs/tests/router_parity.rs`
- Create: `https://github.com/FreeTAKTeam/Reticulum-rs/tests/storage_parity.rs`

**Step 1: Write failing tests**
```rust
#[test]
fn routing_table_serialization_matches_fixture() {
    let fixture = std::fs::read("tests/fixtures/reticulum/routing_table.bin").unwrap();
    let table = RoutingTable::from_fixture_bytes(&fixture).unwrap();
    assert_eq!(table.to_bytes(), fixture);
}
```

**Step 2: Run tests to verify failure**
Run: `cargo test -p reticulum routing_table_serialization_matches_fixture -v`
Expected: FAIL

**Step 3: Minimal implementation**
- Match Python routing table serialization and persistence layout.

**Step 4: Run tests to verify pass**
Run: `cargo test -p reticulum routing_table_serialization_matches_fixture -v`
Expected: PASS

**Step 5: Commit**
```bash
git add https://github.com/FreeTAKTeam/Reticulum-rs/src/router.rs \
  https://github.com/FreeTAKTeam/Reticulum-rs/src/storage.rs \
  https://github.com/FreeTAKTeam/Reticulum-rs/tests/router_parity.rs \
  https://github.com/FreeTAKTeam/Reticulum-rs/tests/storage_parity.rs

git commit -m "feat: add routing/persistence parity"
```

### Task A7: Reticulum CLI parity

**Files:**
- Modify: `https://github.com/FreeTAKTeam/Reticulum-rs/src/cli.rs`
- Create: `https://github.com/FreeTAKTeam/Reticulum-rs/tests/cli_parity.rs`

**Step 1: Write failing test**
```rust
#[test]
fn cli_help_matches_expected() {
    let output = Command::new("reticulum").arg("--help").output().unwrap();
    assert!(String::from_utf8_lossy(&output.stdout).contains("Reticulum"));
}
```

**Step 2: Run test to verify failure**
Run: `cargo test -p reticulum cli_help_matches_expected -v`
Expected: FAIL

**Step 3: Minimal implementation**
- Match Python CLI flags, defaults, and exit codes.

**Step 4: Run test to verify pass**
Run: `cargo test -p reticulum cli_help_matches_expected -v`
Expected: PASS

**Step 5: Commit**
```bash
git add https://github.com/FreeTAKTeam/Reticulum-rs/src/cli.rs \
  https://github.com/FreeTAKTeam/Reticulum-rs/tests/cli_parity.rs

git commit -m "feat: add Reticulum CLI parity"
```

---

## Track B: LXMF‑rs parity (depends on Reticulum‑rs)

### Task B1: LXMF fixture harness expansion

**Files:**
- Modify: `https://github.com/FreeTAKTeam/LXMF-rs/tests/fixtures/python/lxmf/gen_message_fixtures.py`
- Create: `https://github.com/FreeTAKTeam/LXMF-rs/tests/fixtures/lxmf/*.bin`
- Create: `https://github.com/FreeTAKTeam/LXMF-rs/tests/lxmf_fixtures.rs`

**Step 1: Write failing test**
```rust
#[test]
fn lxmf_fixture_bytes_exist() {
    assert!(std::path::Path::new("tests/fixtures/lxmf/message_packed.bin").exists());
}
```

**Step 2: Run test to verify failure**
Run: `cargo test -p lxmf lxmf_fixture_bytes_exist -v`
Expected: FAIL

**Step 3: Expand Python fixture generator**
- Add fixtures for unsigned/signed/propagated, stamps/tickets, storage files.

**Step 4: Run test to verify pass**
Run: `python3 tests/fixtures/python/lxmf/gen_message_fixtures.py`
Run: `cargo test -p lxmf lxmf_fixture_bytes_exist -v`
Expected: PASS

**Step 5: Commit**
```bash
git add https://github.com/FreeTAKTeam/LXMF-rs/tests/fixtures/python/lxmf/gen_message_fixtures.py \
  https://github.com/FreeTAKTeam/LXMF-rs/tests/fixtures/lxmf \
  https://github.com/FreeTAKTeam/LXMF-rs/tests/lxmf_fixtures.rs

git commit -m "test: expand LXMF fixture harness"
```

### Task B2: Message pack/unpack parity

**Files:**
- Modify: `https://github.com/FreeTAKTeam/LXMF-rs/src/message/mod.rs`
- Create: `https://github.com/FreeTAKTeam/LXMF-rs/tests/message_pack_parity.rs`

**Step 1: Write failing test**
```rust
#[test]
fn message_pack_bytes_match_fixture() {
    let fixture = std::fs::read("tests/fixtures/lxmf/message_packed.bin").unwrap();
    let msg = Message::from_fixture_bytes(&fixture).unwrap();
    assert_eq!(msg.pack().unwrap(), fixture);
}
```

**Step 2: Run test to verify failure**
Run: `cargo test -p lxmf message_pack_bytes_match_fixture -v`
Expected: FAIL

**Step 3: Minimal implementation**
- Match Python field ordering, msgpack struct map, and signature handling.

**Step 4: Run test to verify pass**
Run: `cargo test -p lxmf message_pack_bytes_match_fixture -v`
Expected: PASS

**Step 5: Commit**
```bash
git add https://github.com/FreeTAKTeam/LXMF-rs/src/message/mod.rs \
  https://github.com/FreeTAKTeam/LXMF-rs/tests/message_pack_parity.rs

git commit -m "feat: add LXMF message pack parity"
```

### Task B3: Propagation message parity

**Files:**
- Modify: `https://github.com/FreeTAKTeam/LXMF-rs/src/propagation.rs`
- Create: `https://github.com/FreeTAKTeam/LXMF-rs/tests/propagation_parity.rs`

**Step 1: Write failing test**
```rust
#[test]
fn propagation_pack_matches_fixture() {
    let fixture = std::fs::read("tests/fixtures/lxmf/propagation.bin").unwrap();
    let msg = Message::from_fixture_bytes(&fixture).unwrap();
    assert_eq!(msg.pack_propagation().unwrap(), fixture);
}
```

**Step 2: Run test to verify failure**
Run: `cargo test -p lxmf propagation_pack_matches_fixture -v`
Expected: FAIL

**Step 3: Minimal implementation**
- Ensure fields, desired method, and propagation metadata match Python.

**Step 4: Run test to verify pass**
Run: `cargo test -p lxmf propagation_pack_matches_fixture -v`
Expected: PASS

**Step 5: Commit**
```bash
git add https://github.com/FreeTAKTeam/LXMF-rs/src/propagation.rs \
  https://github.com/FreeTAKTeam/LXMF-rs/tests/propagation_parity.rs

git commit -m "feat: add propagation pack parity"
```

### Task B4: Stamps/tickets parity

**Files:**
- Modify: `https://github.com/FreeTAKTeam/LXMF-rs/src/stamps.rs`
- Modify: `https://github.com/FreeTAKTeam/LXMF-rs/src/tickets.rs`
- Create: `https://github.com/FreeTAKTeam/LXMF-rs/tests/stamps_parity.rs`
- Create: `https://github.com/FreeTAKTeam/LXMF-rs/tests/tickets_parity.rs`

**Step 1: Write failing tests**
```rust
#[test]
fn stamp_bytes_match_fixture() {
    let fixture = std::fs::read("tests/fixtures/lxmf/stamp.bin").unwrap();
    let stamp = Stamp::from_fixture_bytes(&fixture).unwrap();
    assert_eq!(stamp.to_bytes(), fixture);
}
```

**Step 2: Run tests to verify failure**
Run: `cargo test -p lxmf stamp_bytes_match_fixture -v`
Expected: FAIL

**Step 3: Minimal implementation**
- Match Python stamp/ticket derivation, cryptographic verification, and serialization.

**Step 4: Run tests to verify pass**
Run: `cargo test -p lxmf stamp_bytes_match_fixture -v`
Expected: PASS

**Step 5: Commit**
```bash
git add https://github.com/FreeTAKTeam/LXMF-rs/src/stamps.rs \
  https://github.com/FreeTAKTeam/LXMF-rs/src/tickets.rs \
  https://github.com/FreeTAKTeam/LXMF-rs/tests/stamps_parity.rs \
  https://github.com/FreeTAKTeam/LXMF-rs/tests/tickets_parity.rs

git commit -m "feat: add stamps/tickets parity"
```

### Task B5: Storage file layout parity

**Files:**
- Modify: `https://github.com/FreeTAKTeam/LXMF-rs/src/storage.rs`
- Create: `https://github.com/FreeTAKTeam/LXMF-rs/tests/storage_parity.rs`

**Step 1: Write failing test**
```rust
#[test]
fn storage_container_matches_fixture() {
    let fixture = std::fs::read("tests/fixtures/lxmf/storage_signed.bin").unwrap();
    let container = MessageContainer::from_fixture_bytes(&fixture).unwrap();
    assert_eq!(container.to_msgpack().unwrap(), fixture);
}
```

**Step 2: Run test to verify failure**
Run: `cargo test -p lxmf storage_container_matches_fixture -v`
Expected: FAIL

**Step 3: Minimal implementation**
- Match Python packed_container structure and any metadata keys.

**Step 4: Run test to verify pass**
Run: `cargo test -p lxmf storage_container_matches_fixture -v`
Expected: PASS

**Step 5: Commit**
```bash
git add https://github.com/FreeTAKTeam/LXMF-rs/src/storage.rs \
  https://github.com/FreeTAKTeam/LXMF-rs/tests/storage_parity.rs

git commit -m "feat: add storage container parity"
```

### Task B6: Router API and CLI parity

**Files:**
- Modify: `https://github.com/FreeTAKTeam/LXMF-rs/src/router.rs`
- Modify: `https://github.com/FreeTAKTeam/LXMF-rs/src/cli.rs`
- Create: `https://github.com/FreeTAKTeam/LXMF-rs/tests/router_parity.rs`
- Create: `https://github.com/FreeTAKTeam/LXMF-rs/tests/cli_parity.rs`

**Step 1: Write failing tests**
```rust
#[test]
fn cli_help_matches_expected() {
    let output = Command::new("lxmf").arg("--help").output().unwrap();
    assert!(String::from_utf8_lossy(&output.stdout).contains("LXMF"));
}
```

**Step 2: Run tests to verify failure**
Run: `cargo test -p lxmf cli_help_matches_expected -v`
Expected: FAIL

**Step 3: Minimal implementation**
- Match Python CLI flags, router defaults, and exit codes.

**Step 4: Run tests to verify pass**
Run: `cargo test -p lxmf cli_help_matches_expected -v`
Expected: PASS

**Step 5: Commit**
```bash
git add https://github.com/FreeTAKTeam/LXMF-rs/src/router.rs \
  https://github.com/FreeTAKTeam/LXMF-rs/src/cli.rs \
  https://github.com/FreeTAKTeam/LXMF-rs/tests/router_parity.rs \
  https://github.com/FreeTAKTeam/LXMF-rs/tests/cli_parity.rs

git commit -m "feat: add LXMF router/CLI parity"
```

---

## Task C: Cross‑repo integration checks

**Files:**
- Modify: `https://github.com/FreeTAKTeam/LXMF-rs/docs/plans/lxmf-parity-matrix.md`
- Modify: `https://github.com/FreeTAKTeam/LXMF-rs/docs/plans/reticulum-parity-matrix.md`

**Step 1: Update parity status**
- Mark each feature as complete, partial, or blocked.

**Step 2: Run full tests**
Run: `cargo test -p reticulum -v` (from `https://github.com/FreeTAKTeam/Reticulum-rs`)
Run: `cargo test -p lxmf -v` (from `https://github.com/FreeTAKTeam/LXMF-rs`)
Expected: PASS

**Step 3: Commit**
```bash
git add https://github.com/FreeTAKTeam/LXMF-rs/docs/plans/lxmf-parity-matrix.md \
  https://github.com/FreeTAKTeam/LXMF-rs/docs/plans/reticulum-parity-matrix.md

git commit -m "docs: update parity matrices"
```

---

## Notes & Constraints
- No worktrees. All work in the current repo and `https://github.com/FreeTAKTeam/Reticulum-rs`.
- Python reference trees should be checked out locally by contributors before running parity tasks.
- All parity tests must be byte‑exact against fixtures generated from Python.
- Prefer small commits per task.
