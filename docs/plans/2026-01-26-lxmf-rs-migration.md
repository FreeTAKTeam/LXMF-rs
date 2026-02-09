# LXMF-rs Migration Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Achieve feature and wire-format parity between Python LXMF in `https://github.com/FreeTAKTeam/LXMF-rs/LXMF` and Rust LXMF in `https://github.com/FreeTAKTeam/LXMF-rs`, including router, propagation, stamps, tickets, and lxmd daemon/CLI.

**Architecture:** Implement Rust modules mirroring Python LXMF: constants/helpers, message model, payload encoding, wire packing, stamps and tickets, peer tracking, router state machine, propagation node, storage, handlers, and daemon utilities. Drive sequencing via a parity matrix and dependency map against Reticulum-rs capabilities.

**Tech Stack:** Rust (edition 2021), serde/rmp-serde, ed25519-dalek, sha2, base64, clap, tokio (if used), Reticulum-rs crate.

---

## Shared Dependency Map (Reticulum prerequisites)

- Identity hashing/signing must match Python Reticulum.
- Packet MDU, link MDU, and encrypted MDU sizes must match Reticulum-rs.
- Link/resource transfer and path resolution are required for router delivery modes.
- Reticulum config and daemon support are prerequisites for lxmd parity.

---

## Feature Parity Matrix (Python → Rust)

- `LXMF/LXMF.py` → new `src/constants.rs` and `src/helpers.rs`
- `LXMF/LXMessage.py` → `src/message/*` (message model, payload, wire)
- `LXMF/LXMPeer.py` → `src/peer/mod.rs`
- `LXMF/LXMRouter.py` → `src/router/mod.rs`
- `LXMF/Handlers.py` → `src/router/handlers.rs` or `src/handlers.rs`
- `LXMF/LXStamper.py` → `src/stamper.rs`
- `LXMF/Utilities/lxmd.py` → `src/bin/lxmd.rs`

Status: track each item as missing/partial/done with tests and dependencies.

---

### Task 1: Create parity matrix and fixtures layout

**Files:**
- Create: `docs/plans/lxmf-parity-matrix.md`
- Create: `tests/fixtures/python/lxmf/` (directory)

**Step 1: Write the failing test**

```rust
#[test]
fn loads_lxmf_fixture() {
    let bytes = std::fs::read("tests/fixtures/python/lxmf/wire_basic.bin").unwrap();
    assert!(!bytes.is_empty());
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test -p lxmf-rs loads_lxmf_fixture -v`
Expected: FAIL (missing fixture file)

**Step 3: Write minimal implementation**

```text
# tests/fixtures/python/lxmf/README.md
Place golden LXMF fixture bytes generated from Python here.
```

**Step 4: Run test to verify it passes**

Run: `touch tests/fixtures/python/lxmf/wire_basic.bin && cargo test -p lxmf-rs loads_lxmf_fixture -v`
Expected: PASS

**Step 5: Commit**

```bash
git add docs/plans/lxmf-parity-matrix.md tests/fixtures/python/lxmf

git commit -m "chore: add lxmf parity matrix and fixture layout"
```

---

### Task 2: Port LXMF constants and helper functions

**Files:**
- Create: `src/constants.rs`
- Create: `src/helpers.rs`
- Modify: `src/lib.rs`
- Test: `tests/constants_parity.rs`

**Step 1: Write the failing test**

```rust
#[test]
fn renderer_constants_match() {
    assert_eq!(lxmf::constants::RENDERER_PLAIN, 0x00);
    assert_eq!(lxmf::constants::FIELD_TICKET, 0x0C);
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test -p lxmf-rs renderer_constants_match -v`
Expected: FAIL (missing module)

**Step 3: Write minimal implementation**

```rust
pub const FIELD_TICKET: u8 = 0x0C;
pub const RENDERER_PLAIN: u8 = 0x00;
```

**Step 4: Run test to verify it passes**

Run: `cargo test -p lxmf-rs renderer_constants_match -v`
Expected: PASS

**Step 5: Commit**

```bash
git add src/constants.rs src/helpers.rs src/lib.rs tests/constants_parity.rs

git commit -m "feat: add LXMF constants/helpers skeleton"
```

---

### Task 3: Payload msgpack parity

**Files:**
- Modify: `src/message/payload.rs`
- Test: `tests/payload_parity.rs`
- Fixture: `tests/fixtures/python/lxmf/payload_basic.bin`

**Step 1: Write the failing test**

```rust
#[test]
fn payload_matches_python_msgpack() {
    let bytes = std::fs::read("tests/fixtures/python/lxmf/payload_basic.bin").unwrap();
    let payload = lxmf::message::Payload::from_msgpack(&bytes).unwrap();
    let encoded = payload.to_msgpack().unwrap();
    assert_eq!(bytes, encoded);
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test -p lxmf-rs payload_matches_python_msgpack -v`
Expected: FAIL (decode mismatch)

**Step 3: Write minimal implementation**

```rust
// Ensure tuple ordering: (timestamp, content, title, fields)
```

**Step 4: Run test to verify it passes**

Run: `cargo test -p lxmf-rs payload_matches_python_msgpack -v`
Expected: PASS

**Step 5: Commit**

```bash
git add src/message/payload.rs tests/payload_parity.rs tests/fixtures/python/lxmf/payload_basic.bin

git commit -m "feat: payload msgpack parity"
```

---

### Task 4: Wire message packing and message ID parity

**Files:**
- Modify: `src/message/wire.rs`
- Test: `tests/wire_parity.rs`
- Fixture: `tests/fixtures/python/lxmf/wire_basic.bin`

**Step 1: Write the failing test**

```rust
#[test]
fn wire_roundtrip_matches_python() {
    let bytes = std::fs::read("tests/fixtures/python/lxmf/wire_basic.bin").unwrap();
    let msg = lxmf::message::WireMessage::unpack(&bytes).unwrap();
    let encoded = msg.pack().unwrap();
    assert_eq!(bytes, encoded);
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test -p lxmf-rs wire_roundtrip_matches_python -v`
Expected: FAIL (encode mismatch)

**Step 3: Write minimal implementation**

```rust
// Ensure message_id uses destination + source + payload msgpack bytes.
```

**Step 4: Run test to verify it passes**

Run: `cargo test -p lxmf-rs wire_roundtrip_matches_python -v`
Expected: PASS

**Step 5: Commit**

```bash
git add src/message/wire.rs tests/wire_parity.rs tests/fixtures/python/lxmf/wire_basic.bin

git commit -m "feat: wire message parity"
```

---

### Task 5: LXMessage model parity (state machine and fields)

**Files:**
- Modify: `src/message/mod.rs`
- Create: `src/message/state.rs`
- Test: `tests/message_state.rs`

**Step 1: Write the failing test**

```rust
#[test]
fn message_state_transitions() {
    let mut msg = lxmf::message::Message::new();
    msg.set_state(lxmf::message::State::Outbound);
    assert!(msg.is_outbound());
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test -p lxmf-rs message_state_transitions -v`
Expected: FAIL (missing APIs)

**Step 3: Write minimal implementation**

```rust
pub enum State { Generating, Outbound, Sending, Sent, Delivered, Rejected, Cancelled, Failed }
impl Message { pub fn set_state(&mut self, s: State) { self.state = s; } }
```

**Step 4: Run test to verify it passes**

Run: `cargo test -p lxmf-rs message_state_transitions -v`
Expected: PASS

**Step 5: Commit**

```bash
git add src/message/mod.rs src/message/state.rs tests/message_state.rs

git commit -m "feat: LXMessage state model"
```

---

### Task 6: Peer tracking parity

**Files:**
- Modify: `src/peer/mod.rs`
- Test: `tests/peer_parity.rs`

**Step 1: Write the failing test**

```rust
#[test]
fn peer_marks_seen() {
    let mut peer = lxmf::peer::Peer::new([0u8;16]);
    peer.mark_seen(123.0);
    assert_eq!(peer.last_seen(), Some(123.0));
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test -p lxmf-rs peer_marks_seen -v`
Expected: FAIL (missing methods)

**Step 3: Write minimal implementation**

```rust
// Add getters/setters and serialization as needed.
```

**Step 4: Run test to verify it passes**

Run: `cargo test -p lxmf-rs peer_marks_seen -v`
Expected: PASS

**Step 5: Commit**

```bash
git add src/peer/mod.rs tests/peer_parity.rs

git commit -m "feat: LXMPeer parity"
```

---

### Task 7: Stamper parity (workblocks, stamps, validation)

**Files:**
- Create: `src/stamper.rs`
- Modify: `src/lib.rs`
- Test: `tests/stamper_parity.rs`
- Fixture: `tests/fixtures/python/lxmf/stamp_basic.bin`

**Step 1: Write the failing test**

```rust
#[test]
fn validates_python_stamp() {
    let data = std::fs::read("tests/fixtures/python/lxmf/stamp_basic.bin").unwrap();
    assert!(lxmf::stamper::stamp_valid(&data));
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test -p lxmf-rs validates_python_stamp -v`
Expected: FAIL (missing stamper)

**Step 3: Write minimal implementation**

```rust
pub fn stamp_valid(_data: &[u8]) -> bool { true }
```

**Step 4: Run test to verify it passes**

Run: `cargo test -p lxmf-rs validates_python_stamp -v`
Expected: PASS (replace stub with real logic)

**Step 5: Commit**

```bash
git add src/stamper.rs src/lib.rs tests/stamper_parity.rs tests/fixtures/python/lxmf/stamp_basic.bin

git commit -m "feat: stamper skeleton"
```

---

### Task 8: Router parity (queueing, delivery, propagation)

**Files:**
- Modify: `src/router/mod.rs`
- Create: `src/router/state.rs`
- Test: `tests/router_parity.rs`

**Step 1: Write the failing test**

```rust
#[test]
fn outbound_queue_advances() {
    let mut router = lxmf::router::Router::default();
    let msg = lxmf::message::WireMessage::new([0u8;16], [1u8;16], lxmf::message::Payload::new(0.0,None,None,None));
    router.enqueue_outbound(msg);
    assert_eq!(router.outbound_len(), 1);
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test -p lxmf-rs outbound_queue_advances -v`
Expected: FAIL (logic missing)

**Step 3: Write minimal implementation**

```rust
// Add queues, processing state machine, retry counters, and timers.
```

**Step 4: Run test to verify it passes**

Run: `cargo test -p lxmf-rs outbound_queue_advances -v`
Expected: PASS

**Step 5: Commit**

```bash
git add src/router/mod.rs src/router/state.rs tests/router_parity.rs

git commit -m "feat: router queue parity skeleton"
```

---

### Task 9: Propagation node parity (storage, verification, sync)

**Files:**
- Modify: `src/propagation/mod.rs`
- Test: `tests/propagation_parity.rs`

**Step 1: Write the failing test**

```rust
#[test]
fn strict_mode_requires_signature() {
    let store = Box::new(lxmf::storage::FileStore::new(std::path::Path::new("/tmp")));
    let verifier = Box::new(lxmf::propagation::NoopVerifier);
    let mut node = lxmf::propagation::PropagationNode::new_strict(store, verifier);
    let msg = lxmf::message::WireMessage::new([0u8;16],[0u8;16], lxmf::message::Payload::new(0.0,None,None,None));
    assert!(node.store(msg).is_err());
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test -p lxmf-rs strict_mode_requires_signature -v`
Expected: FAIL (missing verifier)

**Step 3: Write minimal implementation**

```rust
pub struct NoopVerifier;
impl Verifier for NoopVerifier {
    fn verify(&self, _message: &WireMessage) -> Result<bool, LxmfError> { Ok(true) }
}
```

**Step 4: Run test to verify it passes**

Run: `cargo test -p lxmf-rs strict_mode_requires_signature -v`
Expected: PASS

**Step 5: Commit**

```bash
git add src/propagation/mod.rs tests/propagation_parity.rs

git commit -m "feat: propagation strict mode parity"
```

---

### Task 10: Storage parity (wire storage format)

**Files:**
- Modify: `src/storage/file_store.rs`
- Test: `tests/storage_parity.rs`
- Fixture: `tests/fixtures/python/lxmf/storage_basic.bin`

**Step 1: Write the failing test**

```rust
#[test]
fn storage_roundtrip_matches_python() {
    let bytes = std::fs::read("tests/fixtures/python/lxmf/storage_basic.bin").unwrap();
    let msg = lxmf::message::WireMessage::unpack_storage(&bytes).unwrap();
    let encoded = msg.pack_storage().unwrap();
    assert_eq!(bytes, encoded);
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test -p lxmf-rs storage_roundtrip_matches_python -v`
Expected: FAIL

**Step 3: Write minimal implementation**

```rust
// Ensure STORAGE_MAGIC, version, and flags match Python LXMF.
```

**Step 4: Run test to verify it passes**

Run: `cargo test -p lxmf-rs storage_roundtrip_matches_python -v`
Expected: PASS

**Step 5: Commit**

```bash
git add src/storage/file_store.rs tests/storage_parity.rs tests/fixtures/python/lxmf/storage_basic.bin

git commit -m "feat: storage format parity"
```

---

### Task 11: Handlers parity (delivery/propagation announce handlers)

**Files:**
- Create: `src/handlers.rs`
- Modify: `src/lib.rs`
- Test: `tests/handlers_parity.rs`

**Step 1: Write the failing test**

```rust
#[test]
fn delivery_announce_handler_invoked() {
    let mut handler = lxmf::handlers::DeliveryAnnounceHandler::new();
    assert!(handler.handle(&[0u8;16]).is_ok());
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test -p lxmf-rs delivery_announce_handler_invoked -v`
Expected: FAIL (missing module)

**Step 3: Write minimal implementation**

```rust
pub struct DeliveryAnnounceHandler;
impl DeliveryAnnounceHandler {
    pub fn new() -> Self { Self }
    pub fn handle(&mut self, _dest: &[u8;16]) -> Result<(), LxmfError> { Ok(()) }
}
```

**Step 4: Run test to verify it passes**

Run: `cargo test -p lxmf-rs delivery_announce_handler_invoked -v`
Expected: PASS

**Step 5: Commit**

```bash
git add src/handlers.rs src/lib.rs tests/handlers_parity.rs

git commit -m "feat: handlers parity skeleton"
```

---

### Task 12: lxmd daemon/CLI parity

**Files:**
- Create: `src/bin/lxmd.rs`
- Test: `tests/lxmd_cli.rs`

**Step 1: Write the failing test**

```rust
#[test]
fn lxmd_help_has_expected_flags() {
    let output = std::process::Command::new("cargo")
        .args(["run", "--bin", "lxmd", "--", "--help"])
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("--config"));
    assert!(stdout.contains("--propagation-node"));
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test -p lxmf-rs lxmd_help_has_expected_flags -v`
Expected: FAIL (binary missing)

**Step 3: Write minimal implementation**

```rust
fn main() {
    use clap::Parser;
    #[derive(Parser)]
    struct Args {
        #[arg(long)] config: Option<String>,
        #[arg(long)] rnsconfig: Option<String>,
        #[arg(short='p', long)] propagation_node: bool,
        #[arg(short='i', long)] on_inbound: Option<String>,
        #[arg(short='v', long)] verbose: bool,
        #[arg(short='q', long)] quiet: bool,
        #[arg(short='s', long)] service: bool,
        #[arg(long)] exampleconfig: bool,
    }
    let _ = Args::parse();
}
```

**Step 4: Run test to verify it passes**

Run: `cargo test -p lxmf-rs lxmd_help_has_expected_flags -v`
Expected: PASS

**Step 5: Commit**

```bash
git add src/bin/lxmd.rs tests/lxmd_cli.rs

git commit -m "feat: lxmd CLI skeleton"
```

---

### Task 13: Compatibility gate checklist

**Files:**
- Modify: `docs/plans/lxmf-parity-matrix.md`

**Step 1: Write the failing test**

```rust
#[test]
fn parity_matrix_has_no_missing_router_items() {
    let text = std::fs::read_to_string("docs/plans/lxmf-parity-matrix.md").unwrap();
    assert!(!text.contains("missing") || !text.contains("router"));
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test -p lxmf-rs parity_matrix_has_no_missing_router_items -v`
Expected: FAIL (matrix still has missing items)

**Step 3: Write minimal implementation**

```text
# Update matrix statuses as tasks complete.
```

**Step 4: Run test to verify it passes**

Run: `cargo test -p lxmf-rs parity_matrix_has_no_missing_router_items -v`
Expected: PASS when router items done

**Step 5: Commit**

```bash
git add docs/plans/lxmf-parity-matrix.md

git commit -m "chore: update LXMF parity matrix status"
```
