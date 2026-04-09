# 05 — Implementation (Programs ARE Proofs)

> **Purpose**: Implement workspace crate modules. Make the red tests green.
> The tests define the contract. The Lean proofs define the algebra.
> Your job is to write the program that satisfies both.
>
> **DoF**: Low. The tests pass or they don't.

---

## Phase 0: Load Context

```bash
ms load rust-formal-engineering -m --full  # Implementation skill
```

---

## Execution Loop

```
Select task from `br ready`
    --> Mark in-progress: `br update <id> --status in_progress`
    --> Read: spec invariant + Lean proof + failing test
    --> Implement: make the test pass
    --> Verify: cargo test + clippy + fmt
    --> Close: `br close <id> --reason "Tests pass, INV-FERR-NNN verified"`
```

---

## Demonstration: Implementing Store::merge

### Step 1: Check the spec

`spec/01-core-invariants.md` INV-FERR-001 Level 2 says:

```rust
pub fn merge(a: &Store, b: &Store) -> Store {
    let mut result = a.datoms.clone();
    for datom in b.datoms.iter() {
        result.insert(datom.clone()); // BTreeSet insert is idempotent
    }
    Store::from_datoms(result) // rebuilds indexes
}
```

### Step 2: Check the Lean proof

`ferratomic-verify/lean/Ferratomic/Store.lean` proves:

```lean
theorem merge_comm (a b : Finset Datom) : merge a b = merge b a :=
  Finset.union_comm a b
```

Merge is set union. Union is commutative. Our implementation must be
structurally equivalent to set union for the proof to hold.

### Step 3: Check the test

`ferratomic-verify/proptest/crdt_properties.rs` expects:

```rust
let ab = merge(&a, &b);
let ba = merge(&b, &a);
prop_assert_eq!(ab.datom_set(), ba.datom_set());
```

The test calls `merge` with two stores and asserts the result is
identical regardless of argument order.

### Step 4: Write the implementation

```rust
// ferratomic-store/src/merge.rs

use ferratom::{Datom, FerraError};
use crate::store::Store;

/// Merge two stores by set union (INV-FERR-001, INV-FERR-002, INV-FERR-003).
///
/// The result contains exactly the union of both datom sets.
/// Commutative (INV-FERR-001), associative (INV-FERR-002),
/// idempotent (INV-FERR-003).
///
/// Both input stores are preserved (INV-FERR-004: monotonic growth).
pub fn merge(a: &Store, b: &Store) -> Result<Store, FerraError> {
    // Fast path: self-merge is idempotent (INV-FERR-003)
    if a.identity_hash() == b.identity_hash() {
        return Ok(a.clone());
    }

    // Set union: iterate smaller into larger
    let (base, other) = if a.len() >= b.len() { (a, b) } else { (b, a) };
    let mut datoms = base.datom_set().clone();
    for datom in other.datom_set() {
        datoms.insert(datom.clone());
    }

    Store::from_datoms(datoms)
}
```

### Step 5: Run tests

```bash
CARGO_TARGET_DIR=/data/cargo-target cargo test --workspace
CARGO_TARGET_DIR=/data/cargo-target cargo clippy --workspace -- -D warnings
CARGO_TARGET_DIR=/data/cargo-target cargo fmt --all -- --check
```

All three must pass. If any test fails, the implementation is wrong.
Do not modify the test. Fix the implementation.

### Step 6: Close the task

```bash
br close <id> --reason "merge implemented, INV-FERR-001..004 tests pass"
```

---

## Implementation Order

Follow the dependency DAG. A module cannot be implemented until
its dependencies are complete.

```
1. ferratom types       (Phase 3 — must be done first)
2. Store (in-memory)    INV-FERR-001..004
3. Indexes              INV-FERR-005..007
4. Schema validation    INV-FERR-009..011
5. WAL                  INV-FERR-008
6. Snapshot             INV-FERR-006, INV-FERR-013
7. Database (MVCC)      INV-FERR-006, INV-FERR-014
8. HLC                  INV-FERR-015, INV-FERR-016
9. Checkpoint           INV-FERR-013
10. Merge (full)        INV-FERR-001..004, INV-FERR-010
```

---

## Quality Gates (every commit)

```bash
# All eleven must pass. No exceptions.

# Gate 1: Formatting
CARGO_TARGET_DIR=/data/cargo-target cargo fmt --all -- --check

# Gate 2: Lint (all targets)
CARGO_TARGET_DIR=/data/cargo-target cargo clippy --workspace --all-targets -- -D warnings

# Gate 3: NEG-FERR-001 — no unwrap/expect/panic in production code
CARGO_TARGET_DIR=/data/cargo-target cargo clippy --workspace --lib -- -D warnings \
  -D clippy::unwrap_used -D clippy::expect_used -D clippy::panic

# Gate 4: Tests
CARGO_TARGET_DIR=/data/cargo-target cargo test --workspace

# Gate 5: Supply chain audit
CARGO_TARGET_DIR=/data/cargo-target cargo deny check

# Gate 6: INV-FERR-023 — #![forbid(unsafe_code)] verified in all crate roots

# Gate 7: Documentation builds without warnings
CARGO_TARGET_DIR=/data/cargo-target cargo doc --workspace --no-deps -- -D warnings

# Gate 8: File complexity limits (500 LOC, clippy.toml thresholds)

# Gate 9: Lean proofs (0 sorry) — unconditional
cd ferratomic-verify/lean && lake build

# Gate 10: MIRI (pure-logic subset)
CARGO_TARGET_DIR=/data/cargo-target cargo +nightly miri test

# Gate 11: Coverage >= thresholds (no regression)
```

A commit that fails any gate is a defect.

**Zero clippy suppressions.** Never add `#[allow(clippy::...)]` or `#[allow(dead_code)]`
to production code. If clippy flags something, fix the root cause:
- `too_many_lines` -> decompose the function
- `cast_possible_truncation` -> use `try_from` with explicit overflow handling
- `needless_pass_by_value` -> actually consume the value or take a reference
- `dead_code` -> remove the dead code or use it
Suppressions are defects. They defeat the purpose of static analysis.

**Defensive engineering standards**: See GOALS.md §6 for the full standard including MIRI, fuzzing, mutation testing, coverage thresholds, and supply chain security.

---

## Subagent Orchestration

When multiple agents work in parallel:

1. **Disjoint file sets.** Each agent edits different files. Overlap = serialize.
2. **Agents do NOT run cargo.** The orchestrator runs build/test once after all agents complete.
3. **Claim tasks.** `br update <id> --status in_progress` before starting.
4. **Agent verification is code-level.** Read back your files, check logical correctness. Do not compile.

---

## When a Test Fails

1. Read the failure message. It cites the INV-FERR and shows the inputs.
2. Read the spec for that INV-FERR. Understand the algebraic law.
3. Read the Lean proof. Understand why the property must hold.
4. Fix the implementation. Never fix the test (unless the test has a bug
   in its generator, which is rare).
5. Re-run. If it passes, move on. If it fails differently, repeat from 1.

---

## What NOT To Do

- Do not modify tests to make them pass. Fix the implementation.
- Do not use `#[ignore]` to skip failures.
- Do not add `TODO` comments. Implement fully or do not create the function.
- Do not introduce dependencies without justification.
- Do not implement features not covered by a test. No test = no implementation.
