# LXMF Rust Library Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Build a full Rust implementation of the LXMF stack (library-only first), interoperable with Reticulum-rs and the existing LXMF wire format.

**Architecture:** Create a Rust crate in this repo that mirrors LXMF core concepts (message, router, peer, propagation node) and integrates with Reticulum-rs for transport, identities, destinations, crypto, and routing. Keep LXMF logic in its own modules and layer Reticulum bindings behind a thin adapter to minimize coupling and allow Reticulum-rs updates.

**Tech Stack:** Rust 2021, Reticulum-rs (path dependency), msgpack (rmp-serde), ed25519 (via Reticulum-rs identity/crypto), blake/sha2 for message-id hash (if Reticulum-rs doesn’t expose), serde, tokio (if async is required by Reticulum-rs).

---

### Task 1: Scaffold the Rust crate and core module layout

**Files:**
- Create: `Cargo.toml`
- Create: `src/lib.rs`
- Create: `src/error.rs`
- Create: `src/message/mod.rs`
- Create: `src/router/mod.rs`
- Create: `src/peer/mod.rs`
- Create: `src/propagation/mod.rs`
- Create: `src/storage/mod.rs`
- Create: `src/reticulum/mod.rs`
- Test: `tests/smoke.rs`

**Step 1: Write the failing test**

```rust
// tests/smoke.rs
#[test]
fn crate_builds_and_exports_expected_modules() {
    let _ = lxmf::Message::default();
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test -q`
Expected: FAIL with "can't find crate `lxmf`" or missing `Message`.

**Step 3: Write minimal implementation**

```rust
// src/lib.rs
pub mod error;
pub mod message;
pub mod peer;
pub mod propagation;
pub mod router;
pub mod storage;
pub mod reticulum;

pub use message::Message;
```

```rust
// src/message/mod.rs
#[derive(Default, Debug, Clone)]
pub struct Message;
```

```rust
// src/error.rs
#[derive(Debug)]
pub enum LxmfError {
    Unimplemented,
}
```

```toml
# Cargo.toml
[package]
name = "lxmf"
version = "0.1.0"
edition = "2021"

[dependencies]
reticulum = { path = "../Reticulum-rs" }
serde = { version = "1", features = ["derive"] }
thiserror = "1"
```

**Step 4: Run test to verify it passes**

Run: `cargo test -q`
Expected: PASS

**Step 5: Commit**

```bash
git add Cargo.toml src tests/smoke.rs
git commit -m "chore: scaffold lxmf rust crate"
```

---

### Task 2: Define LXMF message struct + msgpack payload encode/decode

**Files:**
- Modify: `src/message/mod.rs`
- Create: `src/message/payload.rs`
- Test: `tests/message_payload.rs`

**Step 1: Write the failing test**

```rust
// tests/message_payload.rs
use lxmf::message::{Payload, Message};

#[test]
fn payload_roundtrip_msgpack() {
    let payload = Payload::new(1_700_000_000.0, Some("hi".into()), None, None);
    let bytes = payload.to_msgpack().unwrap();
    let decoded = Payload::from_msgpack(&bytes).unwrap();
    assert_eq!(decoded.timestamp, payload.timestamp);
    assert_eq!(decoded.content, payload.content);
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test -q`
Expected: FAIL with missing `Payload` or methods.

**Step 3: Write minimal implementation**

```rust
// src/message/payload.rs
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Payload {
    pub timestamp: f64,
    pub content: Option<String>,
    pub title: Option<String>,
    pub fields: Option<serde_json::Value>,
}

impl Payload {
    pub fn new(timestamp: f64, content: Option<String>, title: Option<String>, fields: Option<serde_json::Value>) -> Self {
        Self { timestamp, content, title, fields }
    }

    pub fn to_msgpack(&self) -> Result<Vec<u8>, crate::error::LxmfError> {
        let list = (self.timestamp, self.content.clone(), self.title.clone(), self.fields.clone());
        rmp_serde::to_vec(&list).map_err(|_| crate::error::LxmfError::Unimplemented)
    }

    pub fn from_msgpack(bytes: &[u8]) -> Result<Self, crate::error::LxmfError> {
        let (timestamp, content, title, fields): (f64, Option<String>, Option<String>, Option<serde_json::Value>) =
            rmp_serde::from_slice(bytes).map_err(|_| crate::error::LxmfError::Unimplemented)?;
        Ok(Self { timestamp, content, title, fields })
    }
}
```

```rust
// src/message/mod.rs
mod payload;

pub use payload::Payload;

#[derive(Default, Debug, Clone)]
pub struct Message;
```

Update Cargo.toml deps:

```toml
rmp-serde = "1"
serde_json = "1"
```

**Step 4: Run test to verify it passes**

Run: `cargo test -q`
Expected: PASS

**Step 5: Commit**

```bash
git add Cargo.toml src/message tests/message_payload.rs
git commit -m "feat: add lxmf payload msgpack roundtrip"
```

---

### Task 3: Implement LXMF message ID + signature format

**Files:**
- Modify: `src/message/mod.rs`
- Create: `src/message/wire.rs`
- Test: `tests/message_wire.rs`

**Step 1: Write the failing test**

```rust
// tests/message_wire.rs
use lxmf::message::{Payload, WireMessage};

#[test]
fn wire_message_id_is_stable() {
    let payload = Payload::new(1_700_000_000.0, Some("hi".into()), None, None);
    let msg = WireMessage::new([0u8;16], [1u8;16], payload);
    let id1 = msg.message_id();
    let id2 = msg.message_id();
    assert_eq!(id1, id2);
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test -q`
Expected: FAIL with missing `WireMessage`.

**Step 3: Write minimal implementation**

```rust
// src/message/wire.rs
use crate::message::Payload;
use sha2::{Digest, Sha256};

#[derive(Debug, Clone)]
pub struct WireMessage {
    pub destination: [u8; 16],
    pub source: [u8; 16],
    pub payload: Payload,
}

impl WireMessage {
    pub fn new(destination: [u8;16], source: [u8;16], payload: Payload) -> Self {
        Self { destination, source, payload }
    }

    pub fn message_id(&self) -> [u8; 32] {
        let mut hasher = Sha256::new();
        hasher.update(self.destination);
        hasher.update(self.source);
        hasher.update(self.payload.to_msgpack().unwrap_or_default());
        let bytes = hasher.finalize();
        let mut out = [0u8; 32];
        out.copy_from_slice(&bytes);
        out
    }
}
```

```rust
// src/message/mod.rs
mod payload;
mod wire;

pub use payload::Payload;
pub use wire::WireMessage;
```

Update Cargo.toml deps:

```toml
sha2 = "0.10"
```

**Step 4: Run test to verify it passes**

Run: `cargo test -q`
Expected: PASS

**Step 5: Commit**

```bash
git add Cargo.toml src/message tests/message_wire.rs
git commit -m "feat: add wire message id"
```

---

### Task 4: Add Reticulum adapter layer for identities and destinations

**Files:**
- Modify: `src/reticulum/mod.rs`
- Test: `tests/reticulum_adapter.rs`

**Step 1: Write the failing test**

```rust
// tests/reticulum_adapter.rs
use lxmf::reticulum::Adapter;

#[test]
fn adapter_exports_destination_hash_len() {
    assert_eq!(Adapter::DEST_HASH_LEN, 16);
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test -q`
Expected: FAIL with missing `Adapter`.

**Step 3: Write minimal implementation**

```rust
// src/reticulum/mod.rs
pub struct Adapter;

impl Adapter {
    pub const DEST_HASH_LEN: usize = 16;
}
```

**Step 4: Run test to verify it passes**

Run: `cargo test -q`
Expected: PASS

**Step 5: Commit**

```bash
git add src/reticulum tests/reticulum_adapter.rs
git commit -m "feat: add reticulum adapter placeholder"
```

---

### Task 5: Implement packing/unpacking of LXMF wire format

**Files:**
- Modify: `src/message/wire.rs`
- Test: `tests/wire_pack.rs`

**Step 1: Write the failing test**

```rust
// tests/wire_pack.rs
use lxmf::message::{Payload, WireMessage};

#[test]
fn pack_unpack_roundtrip() {
    let payload = Payload::new(1_700_000_000.0, Some("hi".into()), None, None);
    let msg = WireMessage::new([2u8;16], [3u8;16], payload);
    let bytes = msg.pack().unwrap();
    let decoded = WireMessage::unpack(&bytes).unwrap();
    assert_eq!(decoded.destination, msg.destination);
    assert_eq!(decoded.source, msg.source);
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test -q`
Expected: FAIL with missing `pack`/`unpack`.

**Step 3: Write minimal implementation**

```rust
// src/message/wire.rs
use crate::error::LxmfError;

impl WireMessage {
    pub fn pack(&self) -> Result<Vec<u8>, LxmfError> {
        let mut out = Vec::new();
        out.extend_from_slice(&self.destination);
        out.extend_from_slice(&self.source);
        let payload = self.payload.to_msgpack().map_err(|_| LxmfError::Unimplemented)?;
        out.extend_from_slice(&payload);
        Ok(out)
    }

    pub fn unpack(bytes: &[u8]) -> Result<Self, LxmfError> {
        if bytes.len() < 32 { return Err(LxmfError::Unimplemented); }
        let mut dest = [0u8;16];
        let mut src = [0u8;16];
        dest.copy_from_slice(&bytes[0..16]);
        src.copy_from_slice(&bytes[16..32]);
        let payload = crate::message::Payload::from_msgpack(&bytes[32..]).map_err(|_| LxmfError::Unimplemented)?;
        Ok(Self::new(dest, src, payload))
    }
}
```

**Step 4: Run test to verify it passes**

Run: `cargo test -q`
Expected: PASS

**Step 5: Commit**

```bash
git add src/message tests/wire_pack.rs
git commit -m "feat: add wire pack/unpack"
```

---

### Task 6: Define router API surface (inbound/outbound queues, receipts)

**Files:**
- Modify: `src/router/mod.rs`
- Test: `tests/router_api.rs`

**Step 1: Write the failing test**

```rust
// tests/router_api.rs
use lxmf::router::Router;
use lxmf::message::WireMessage;

#[test]
fn router_can_queue_outbound() {
    let mut router = Router::default();
    let msg = WireMessage::new([0u8;16],[1u8;16], Default::default());
    router.enqueue_outbound(msg);
    assert_eq!(router.outbound_len(), 1);
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test -q`
Expected: FAIL with missing `Router`.

**Step 3: Write minimal implementation**

```rust
// src/router/mod.rs
use crate::message::WireMessage;

#[derive(Default)]
pub struct Router {
    outbound: Vec<WireMessage>,
}

impl Router {
    pub fn enqueue_outbound(&mut self, msg: WireMessage) {
        self.outbound.push(msg);
    }

    pub fn outbound_len(&self) -> usize {
        self.outbound.len()
    }
}
```

**Step 4: Run test to verify it passes**

Run: `cargo test -q`
Expected: PASS

**Step 5: Commit**

```bash
git add src/router tests/router_api.rs
git commit -m "feat: add router outbound queue api"
```

---

### Task 7: Implement propagation storage interfaces and a file-backed store

**Files:**
- Modify: `src/storage/mod.rs`
- Create: `src/storage/file_store.rs`
- Test: `tests/storage_file.rs`

**Step 1: Write the failing test**

```rust
// tests/storage_file.rs
use lxmf::storage::{FileStore, Store};
use lxmf::message::{Payload, WireMessage};

#[test]
fn file_store_can_save_and_load() {
    let dir = tempfile::tempdir().unwrap();
    let store = FileStore::new(dir.path());
    let msg = WireMessage::new([1u8;16],[2u8;16], Payload::new(1.0, Some("hi".into()), None, None));
    store.save(&msg).unwrap();
    let loaded = store.get(&msg.message_id()).unwrap();
    assert_eq!(loaded.source, msg.source);
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test -q`
Expected: FAIL with missing `Store`/`FileStore`.

**Step 3: Write minimal implementation**

```rust
// src/storage/mod.rs
mod file_store;

pub use file_store::FileStore;
use crate::message::WireMessage;

pub trait Store {
    fn save(&self, msg: &WireMessage) -> Result<(), crate::error::LxmfError>;
    fn get(&self, id: &[u8;32]) -> Result<WireMessage, crate::error::LxmfError>;
}
```

```rust
// src/storage/file_store.rs
use std::path::{Path, PathBuf};
use std::fs;

use crate::error::LxmfError;
use crate::message::WireMessage;
use crate::storage::Store;

pub struct FileStore {
    root: PathBuf,
}

impl FileStore {
    pub fn new(root: &Path) -> Self {
        Self { root: root.to_path_buf() }
    }
}

impl Store for FileStore {
    fn save(&self, msg: &WireMessage) -> Result<(), LxmfError> {
        let id = msg.message_id();
        let path = self.root.join(hex::encode(id));
        fs::write(path, msg.pack().map_err(|_| LxmfError::Unimplemented)?).map_err(|_| LxmfError::Unimplemented)
    }

    fn get(&self, id: &[u8;32]) -> Result<WireMessage, LxmfError> {
        let path = self.root.join(hex::encode(id));
        let bytes = fs::read(path).map_err(|_| LxmfError::Unimplemented)?;
        WireMessage::unpack(&bytes).map_err(|_| LxmfError::Unimplemented)
    }
}
```

Update Cargo.toml deps:

```toml
tempfile = "3"
hex = "0.4"
```

**Step 4: Run test to verify it passes**

Run: `cargo test -q`
Expected: PASS

**Step 5: Commit**

```bash
git add Cargo.toml src/storage tests/storage_file.rs
git commit -m "feat: add file-backed propagation store"
```

---

### Task 8: Integrate Router with Reticulum-rs transport skeleton

**Files:**
- Modify: `src/router/mod.rs`
- Modify: `src/reticulum/mod.rs`
- Test: `tests/router_transport.rs`

**Step 1: Write the failing test**

```rust
// tests/router_transport.rs
use lxmf::router::Router;
use lxmf::reticulum::Adapter;

#[test]
fn router_accepts_reticulum_adapter() {
    let adapter = Adapter::new();
    let _router = Router::with_adapter(adapter);
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test -q`
Expected: FAIL with missing `Adapter::new` or `Router::with_adapter`.

**Step 3: Write minimal implementation**

```rust
// src/reticulum/mod.rs
pub struct Adapter;

impl Adapter {
    pub const DEST_HASH_LEN: usize = 16;
    pub fn new() -> Self { Self }
}
```

```rust
// src/router/mod.rs
use crate::reticulum::Adapter;

impl Router {
    pub fn with_adapter(_adapter: Adapter) -> Self {
        Self::default()
    }
}
```

**Step 4: Run test to verify it passes**

Run: `cargo test -q`
Expected: PASS

**Step 5: Commit**

```bash
git add src/router src/reticulum tests/router_transport.rs
git commit -m "feat: add reticulum adapter plumbing"
```

---

### Task 9: Implement propagation node behavior (store/forward)

**Files:**
- Modify: `src/propagation/mod.rs`
- Modify: `src/router/mod.rs`
- Test: `tests/propagation.rs`

**Step 1: Write the failing test**

```rust
// tests/propagation.rs
use lxmf::propagation::PropagationNode;
use lxmf::storage::{FileStore, Store};
use lxmf::message::{Payload, WireMessage};

#[test]
fn propagation_store_and_fetch() {
    let dir = tempfile::tempdir().unwrap();
    let store = FileStore::new(dir.path());
    let mut node = PropagationNode::new(Box::new(store));

    let msg = WireMessage::new([7u8;16],[8u8;16], Payload::new(1.0, Some("hi".into()), None, None));
    node.store(msg.clone()).unwrap();
    let fetched = node.fetch(&msg.message_id()).unwrap();
    assert_eq!(fetched.source, msg.source);
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test -q`
Expected: FAIL with missing `PropagationNode`.

**Step 3: Write minimal implementation**

```rust
// src/propagation/mod.rs
use crate::error::LxmfError;
use crate::message::WireMessage;
use crate::storage::Store;

pub struct PropagationNode {
    store: Box<dyn Store + Send + Sync>,
}

impl PropagationNode {
    pub fn new(store: Box<dyn Store + Send + Sync>) -> Self {
        Self { store }
    }

    pub fn store(&mut self, msg: WireMessage) -> Result<(), LxmfError> {
        self.store.save(&msg)
    }

    pub fn fetch(&self, id: &[u8;32]) -> Result<WireMessage, LxmfError> {
        self.store.get(id)
    }
}
```

**Step 4: Run test to verify it passes**

Run: `cargo test -q`
Expected: PASS

**Step 5: Commit**

```bash
git add src/propagation tests/propagation.rs
git commit -m "feat: add propagation node storage"
```

---

### Task 10: Define peer/session API and receipts placeholders

**Files:**
- Modify: `src/peer/mod.rs`
- Test: `tests/peer.rs`

**Step 1: Write the failing test**

```rust
// tests/peer.rs
use lxmf::peer::Peer;

#[test]
fn peer_tracks_last_seen() {
    let mut peer = Peer::new([1u8;16]);
    peer.mark_seen(123.0);
    assert_eq!(peer.last_seen(), Some(123.0));
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test -q`
Expected: FAIL with missing `Peer`.

**Step 3: Write minimal implementation**

```rust
// src/peer/mod.rs
#[derive(Debug, Clone)]
pub struct Peer {
    dest: [u8;16],
    last_seen: Option<f64>,
}

impl Peer {
    pub fn new(dest: [u8;16]) -> Self {
        Self { dest, last_seen: None }
    }

    pub fn mark_seen(&mut self, ts: f64) {
        self.last_seen = Some(ts);
    }

    pub fn last_seen(&self) -> Option<f64> {
        self.last_seen
    }
}
```

**Step 4: Run test to verify it passes**

Run: `cargo test -q`
Expected: PASS

**Step 5: Commit**

```bash
git add src/peer tests/peer.rs
git commit -m "feat: add peer basics"
```

---

### Task 11: Expand error types and replace Unimplemented with real errors

**Files:**
- Modify: `src/error.rs`
- Modify: `src/message/*`
- Modify: `src/storage/*`
- Test: `tests/error_smoke.rs`

**Step 1: Write the failing test**

```rust
// tests/error_smoke.rs
use lxmf::error::LxmfError;

#[test]
fn error_variants_format() {
    let err = LxmfError::Decode("payload".into());
    assert!(err.to_string().contains("payload"));
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test -q`
Expected: FAIL with missing variant.

**Step 3: Write minimal implementation**

```rust
// src/error.rs
use thiserror::Error;

#[derive(Debug, Error)]
pub enum LxmfError {
    #[error("decode error: {0}")]
    Decode(String),
    #[error("encode error: {0}")]
    Encode(String),
    #[error("io error: {0}")]
    Io(String),
}
```

**Step 4: Run test to verify it passes**

Run: `cargo test -q`
Expected: PASS

**Step 5: Commit**

```bash
git add src/error.rs tests/error_smoke.rs
git commit -m "refactor: add structured lxmf errors"
```

---

### Task 12: Add docs for public API and Reticulum-rs integration notes

**Files:**
- Modify: `README.md`
- Create: `docs/lxmf-rs-api.md`

**Step 1: Write the failing test**

(No tests; documentation only.)

**Step 2: Update README with Rust usage stub**

```markdown
## Rust Library (WIP)

This repository now includes a Rust crate that implements LXMF on top of Reticulum-rs.
See `docs/lxmf-rs-api.md` for the evolving API.
```

**Step 3: Add API doc skeleton**

```markdown
# LXMF Rust API (WIP)

- Message: `lxmf::message::WireMessage`
- Payload: `lxmf::message::Payload`
- Router: `lxmf::router::Router`
- Propagation: `lxmf::propagation::PropagationNode`
```

**Step 4: Commit**

```bash
git add README.md docs/lxmf-rs-api.md
git commit -m "docs: add lxmf rust api stub"
```

---

## Reticulum-rs Follow-ups (separate repo)

The following are expected changes to `/Users/tommy/Documents/TAK/Reticulum-rs` to enable full LXMF compatibility. Track these as a parallel plan when implementing:

1. Expose identity signing + verification APIs needed for LXMF signatures.
2. Provide destination hash derivation compatible with Reticulum Python.
3. Provide link/session callbacks for message delivery receipts.
4. Implement GROUP destination encryption API for LXMF group messages.
5. Ensure packet size limits and fragmentation behavior align with LXMF expectations.

---

**Plan complete.**
