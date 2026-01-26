# LXMF Stamps/Tickets Verification Parity Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Implement strict verification parity for LXMF stamps and tickets using Python-generated fixtures.

**Architecture:** Add stamp/ticket parsing and validation modules that mirror Python LXStamper/LXMessage behavior. Use golden fixtures for valid/invalid cases and keep logic pure and deterministic. Integrate only verification (no stamp creation/cost policies).

**Tech Stack:** Rust 2021, rmp-serde, sha2, hkdf, reticulum crate.

---

### Task 1: Add Python fixture generator and loader test

**Files:**
- Create: `tests/fixtures/python/lxmf/gen_stamp_ticket_fixtures.py`
- Create: `tests/fixtures/python/lxmf/stamp_valid.msgpack`
- Create: `tests/fixtures/python/lxmf/stamp_invalid.msgpack`
- Create: `tests/fixtures/python/lxmf/pn_stamp_valid.msgpack`
- Create: `tests/fixtures/python/lxmf/ticket_valid.msgpack`
- Create: `tests/fixtures/python/lxmf/ticket_expired.msgpack`
- Test: `tests/stamp_ticket_fixtures.rs`

**Step 1: Write the failing test**

```rust
#[test]
fn loads_stamp_ticket_fixtures() {
    let stamp_valid = std::fs::read("tests/fixtures/python/lxmf/stamp_valid.msgpack").unwrap();
    let stamp_invalid = std::fs::read("tests/fixtures/python/lxmf/stamp_invalid.msgpack").unwrap();
    let pn_stamp = std::fs::read("tests/fixtures/python/lxmf/pn_stamp_valid.msgpack").unwrap();
    let ticket_valid = std::fs::read("tests/fixtures/python/lxmf/ticket_valid.msgpack").unwrap();
    let ticket_expired = std::fs::read("tests/fixtures/python/lxmf/ticket_expired.msgpack").unwrap();

    assert!(!stamp_valid.is_empty());
    assert!(!stamp_invalid.is_empty());
    assert!(!pn_stamp.is_empty());
    assert!(!ticket_valid.is_empty());
    assert!(!ticket_expired.is_empty());
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test -p lxmf loads_stamp_ticket_fixtures -v`
Expected: FAIL (missing fixture files)

**Step 3: Write minimal implementation**

```python
# tests/fixtures/python/lxmf/gen_stamp_ticket_fixtures.py
import os
import time
import msgpack
import LXMF
from LXMF import LXStamper

OUT = os.path.join("tests", "fixtures", "python", "lxmf")

os.makedirs(OUT, exist_ok=True)

# Stamp case
material = b"lxmf-stamp-material-0001"
workblock = LXStamper.stamp_workblock(material)
stamp, _ = LXStamper.generate_stamp(material, 4)
valid_case = {
    "material": material,
    "target_cost": 4,
    "stamp": stamp,
    "expected_value": LXStamper.stamp_value(workblock, stamp),
}
invalid_case = dict(valid_case)
invalid_case["stamp"] = bytes([b ^ 0xFF for b in stamp])

# PN stamp case
transient_data = b"lxmf-transient-0001" + stamp
pn_case = {
    "transient_data": transient_data,
    "target_cost": 4,
}

# Ticket cases
now = time.time()
expires = now + 60
expired = now - 60
valid_ticket = {
    "expires": expires,
    "ticket": os.urandom(LXMF.LXMessage.TICKET_LENGTH),
    "now": now,
}
expired_ticket = {
    "expires": expired,
    "ticket": os.urandom(LXMF.LXMessage.TICKET_LENGTH),
    "now": now,
}

with open(os.path.join(OUT, "stamp_valid.msgpack"), "wb") as f:
    f.write(msgpack.packb(valid_case))
with open(os.path.join(OUT, "stamp_invalid.msgpack"), "wb") as f:
    f.write(msgpack.packb(invalid_case))
with open(os.path.join(OUT, "pn_stamp_valid.msgpack"), "wb") as f:
    f.write(msgpack.packb(pn_case))
with open(os.path.join(OUT, "ticket_valid.msgpack"), "wb") as f:
    f.write(msgpack.packb(valid_ticket))
with open(os.path.join(OUT, "ticket_expired.msgpack"), "wb") as f:
    f.write(msgpack.packb(expired_ticket))
```

**Step 4: Run generator and test again**

Run: `python3 tests/fixtures/python/lxmf/gen_stamp_ticket_fixtures.py`
Expected: fixture files created

Run: `cargo test -p lxmf loads_stamp_ticket_fixtures -v`
Expected: PASS

**Step 5: Commit**

```bash
git add tests/fixtures/python/lxmf/gen_stamp_ticket_fixtures.py tests/fixtures/python/lxmf/*.msgpack tests/stamp_ticket_fixtures.rs

git commit -m "test: add LXMF stamp/ticket fixtures"
```

---

### Task 2: Implement stamp primitives (workblock/value/valid)

**Files:**
- Modify: `src/stamper.rs`
- Modify: `src/constants.rs`
- Modify: `src/lib.rs`
- Test: `tests/stamp_parity.rs`

**Step 1: Write the failing test**

```rust
use serde::Deserialize;

#[derive(Deserialize)]
struct StampCase {
    material: Vec<u8>,
    target_cost: u32,
    stamp: Vec<u8>,
    expected_value: u32,
}

#[test]
fn stamp_verifies_against_python_fixture() {
    let bytes = std::fs::read("tests/fixtures/python/lxmf/stamp_valid.msgpack").unwrap();
    let case: StampCase = rmp_serde::from_slice(&bytes).unwrap();

    let workblock = lxmf::stamper::stamp_workblock(&case.material, lxmf::constants::WORKBLOCK_EXPAND_ROUNDS);
    assert!(lxmf::stamper::stamp_valid(&case.stamp, case.target_cost, &workblock));
    assert_eq!(
        lxmf::stamper::stamp_value(&workblock, &case.stamp),
        case.expected_value
    );
}

#[test]
fn stamp_rejects_invalid_fixture() {
    let bytes = std::fs::read("tests/fixtures/python/lxmf/stamp_invalid.msgpack").unwrap();
    let case: StampCase = rmp_serde::from_slice(&bytes).unwrap();

    let workblock = lxmf::stamper::stamp_workblock(&case.material, lxmf::constants::WORKBLOCK_EXPAND_ROUNDS);
    assert!(!lxmf::stamper::stamp_valid(&case.stamp, case.target_cost, &workblock));
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test -p lxmf stamp_verifies_against_python_fixture -v`
Expected: FAIL (missing functions)

**Step 3: Write minimal implementation**

```rust
// src/constants.rs
pub const WORKBLOCK_EXPAND_ROUNDS: usize = 3000;

// src/stamper.rs
use hkdf::Hkdf;
use sha2::Sha256;

pub fn stamp_workblock(material: &[u8], expand_rounds: usize) -> Vec<u8> {
    let mut workblock = Vec::new();
    for n in 0..expand_rounds {
        let salt = reticulum::hash::Hash::new_from_slice(&[material, &rmp_serde::to_vec(&n).unwrap()].concat());
        let hk = Hkdf::<Sha256>::new(Some(salt.as_slice()), material);
        let mut okm = [0u8; 32];
        hk.expand(&[], &mut okm).unwrap();
        workblock.extend_from_slice(&okm);
    }
    workblock
}

pub fn stamp_value(workblock: &[u8], stamp: &[u8]) -> u32 {
    let material = reticulum::hash::Hash::new_from_slice(&[workblock, stamp].concat());
    let mut value = 0u32;
    let mut i = u128::from_be_bytes(material.as_slice()[0..16].try_into().unwrap());
    while (i & (1 << 127)) == 0 {
        i <<= 1;
        value += 1;
    }
    value
}

pub fn stamp_valid(stamp: &[u8], target_cost: u32, workblock: &[u8]) -> bool {
    let material = reticulum::hash::Hash::new_from_slice(&[workblock, stamp].concat());
    let mut target: [u8; 32] = [0u8; 32];
    target[0] = 0x80 >> (target_cost % 8);
    material.as_slice() <= &target
}
```

**Step 4: Run test to verify it passes**

Run: `cargo test -p lxmf stamp_verifies_against_python_fixture -v`
Expected: PASS

**Step 5: Commit**

```bash
git add src/constants.rs src/stamper.rs src/lib.rs tests/stamp_parity.rs

git commit -m "feat: add LXMF stamp verification primitives"
```

---

### Task 3: Validate propagation node stamps

**Files:**
- Modify: `src/stamper.rs`
- Test: `tests/pn_stamp_parity.rs`

**Step 1: Write the failing test**

```rust
use serde::Deserialize;

#[derive(Deserialize)]
struct PnStampCase {
    transient_data: Vec<u8>,
    target_cost: u32,
}

#[test]
fn pn_stamp_validation_matches_python_fixture() {
    let bytes = std::fs::read("tests/fixtures/python/lxmf/pn_stamp_valid.msgpack").unwrap();
    let case: PnStampCase = rmp_serde::from_slice(&bytes).unwrap();

    let result = lxmf::stamper::validate_pn_stamp(&case.transient_data, case.target_cost);
    assert!(result.is_some());
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test -p lxmf pn_stamp_validation_matches_python_fixture -v`
Expected: FAIL (missing function)

**Step 3: Write minimal implementation**

```rust
pub fn validate_pn_stamp(transient_data: &[u8], target_cost: u32) -> Option<(Vec<u8>, Vec<u8>, u32, Vec<u8>)> {
    let stamp_size = reticulum::hash::HASH_SIZE;
    if transient_data.len() <= stamp_size { return None; }
    let (lxm_data, stamp) = transient_data.split_at(transient_data.len() - stamp_size);
    let transient_id = reticulum::hash::Hash::new_from_slice(lxm_data).to_bytes().to_vec();
    let workblock = stamp_workblock(&transient_id, lxmf::constants::WORKBLOCK_EXPAND_ROUNDS_PN);
    if !stamp_valid(stamp, target_cost, &workblock) { return None; }
    let value = stamp_value(&workblock, stamp);
    Some((transient_id, lxm_data.to_vec(), value, stamp.to_vec()))
}
```

**Step 4: Run test to verify it passes**

Run: `cargo test -p lxmf pn_stamp_validation_matches_python_fixture -v`
Expected: PASS

**Step 5: Commit**

```bash
git add src/stamper.rs tests/pn_stamp_parity.rs

git commit -m "feat: add propagation stamp verification"
```

---

### Task 4: Implement ticket parsing and validation

**Files:**
- Create: `src/ticket.rs`
- Modify: `src/lib.rs`
- Test: `tests/ticket_parity.rs`

**Step 1: Write the failing test**

```rust
use serde::Deserialize;

#[derive(Deserialize)]
struct TicketCase {
    expires: f64,
    ticket: Vec<u8>,
    now: f64,
}

#[test]
fn ticket_validates_python_fixture() {
    let bytes = std::fs::read("tests/fixtures/python/lxmf/ticket_valid.msgpack").unwrap();
    let case: TicketCase = rmp_serde::from_slice(&bytes).unwrap();

    let ticket = lxmf::ticket::Ticket::new(case.expires, case.ticket);
    assert!(ticket.is_valid(case.now));
}

#[test]
fn ticket_rejects_expired_fixture() {
    let bytes = std::fs::read("tests/fixtures/python/lxmf/ticket_expired.msgpack").unwrap();
    let case: TicketCase = rmp_serde::from_slice(&bytes).unwrap();

    let ticket = lxmf::ticket::Ticket::new(case.expires, case.ticket);
    assert!(!ticket.is_valid(case.now));
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test -p lxmf ticket_validates_python_fixture -v`
Expected: FAIL (missing module)

**Step 3: Write minimal implementation**

```rust
// src/ticket.rs
pub struct Ticket {
    pub expires: f64,
    pub token: Vec<u8>,
}

impl Ticket {
    pub fn new(expires: f64, token: Vec<u8>) -> Self {
        Self { expires, token }
    }

    pub fn is_valid(&self, now: f64) -> bool {
        now <= self.expires
    }
}
```

**Step 4: Run test to verify it passes**

Run: `cargo test -p lxmf ticket_validates_python_fixture -v`
Expected: PASS

**Step 5: Commit**

```bash
git add src/ticket.rs src/lib.rs tests/ticket_parity.rs

git commit -m "feat: add ticket parsing and validation"
```

---

### Task 5: Update LXMF parity matrix status

**Files:**
- Modify: `docs/plans/lxmf-parity-matrix.md`

**Step 1: Write the failing test**

```rust
#[test]
fn parity_matrix_marks_stamp_ticket_progress() {
    let text = std::fs::read_to_string("docs/plans/lxmf-parity-matrix.md").unwrap();
    assert!(text.contains("LXMF/LXStamper.py") && text.contains("partial"));
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test -p lxmf parity_matrix_marks_stamp_ticket_progress -v`
Expected: FAIL (matrix not updated)

**Step 3: Write minimal implementation**

```text
# Update LXStamper status to partial (verification parity) and add note for tickets.
```

**Step 4: Run test to verify it passes**

Run: `cargo test -p lxmf parity_matrix_marks_stamp_ticket_progress -v`
Expected: PASS

**Step 5: Commit**

```bash
git add docs/plans/lxmf-parity-matrix.md tests/parity_matrix_gate.rs

git commit -m "chore: update LXMF parity matrix for stamps/tickets"
```

---

Plan complete and saved to `docs/plans/2026-01-26-lxmf-stamps-tickets-implementation.md`. Two execution options:

1. Subagent-Driven (this session) - I dispatch fresh subagent per task, review between tasks, fast iteration
2. Parallel Session (separate) - Open new session with executing-plans, batch execution with checkpoints

Which approach?
