# CI Pipeline Expectations -- Phase 4a Quality Gates

> **Status**: All gates are currently enforced **manually** before each commit.
> This document serves as the blueprint for future CI automation.
> Once CI is configured, every check listed here blocks merge on failure.

---

## Overview

Phase 4a quality gates ensure that every commit to `main` satisfies the
Ferratomic specification's invariants, negative cases, and coding standards.
The gates below are ordered from fastest to slowest; CI should run them in
this order and fail fast.

---

## Gate 1: Compilation

| Field | Value |
|-------|-------|
| **Command** | `CARGO_TARGET_DIR=/data/cargo-target cargo check --workspace` |
| **Enforces** | Basic type-level correctness across all four crates (ferratom, ferratomic-core, ferratomic-datalog, ferratomic-verify). A failing `cargo check` means the workspace has type errors, missing imports, or unresolved dependencies. |
| **INV/NEG** | Prerequisite for all other INV-FERR checks; no single invariant -- the entire type system is the proof. |
| **Current status** | Manual |
| **CI behavior** | Block merge. No downstream gates run if compilation fails. |

---

## Gate 2: Lint Enforcement

| Field | Value |
|-------|-------|
| **Command** | `CARGO_TARGET_DIR=/data/cargo-target cargo clippy --workspace -- -D warnings` |
| **Enforces** | Clippy lints at deny-warnings severity. Catches common correctness bugs, performance anti-patterns, and idiomatic violations. Also enforces project-specific lints configured in `clippy.toml` (cognitive complexity <= 10, max 5 parameters, max 50 lines per function). |
| **INV/NEG** | Supports NEG-FERR-001 (no panics) via `clippy::unwrap_used`, `clippy::expect_used`, `clippy::panic`, `clippy::todo`, `clippy::unimplemented` when those denies are active. Supports general code quality across all INV-FERR. |
| **Current status** | Manual |
| **CI behavior** | Block merge. |

---

## Gate 3: Format Consistency

| Field | Value |
|-------|-------|
| **Command** | `CARGO_TARGET_DIR=/data/cargo-target cargo fmt --all -- --check` |
| **Enforces** | Uniform formatting across the entire workspace. Prevents style drift and merge noise from formatting differences. |
| **INV/NEG** | No direct INV-FERR mapping. Process hygiene: deterministic formatting reduces review burden and prevents spurious diffs. |
| **Current status** | Manual |
| **CI behavior** | Block merge. |

---

## Gate 4: Full Test Suite

| Field | Value |
|-------|-------|
| **Command** | `CARGO_TARGET_DIR=/data/cargo-target cargo test --workspace` |
| **Enforces** | All unit tests, integration tests, and verification harnesses in ferratomic-verify (Stateright model-checking, Kani bounded verification stubs, proptest property-based tests). Every public function has at least one test. Test names reference the invariant they exercise (e.g., `test_inv_ferr_001_merge_commutativity`). |
| **INV/NEG** | INV-FERR-001 through INV-FERR-055 (every invariant with a Level 2 contract has a corresponding test). NEG-FERR-001 through NEG-FERR-005 (negative case falsification tests). |
| **Current status** | Manual |
| **CI behavior** | Block merge. |

---

## Gate 5: Proptest Depth (10K+ Cases)

| Field | Value |
|-------|-------|
| **Command** | `CARGO_TARGET_DIR=/data/cargo-target cargo test --workspace` (proptest cases configured in test code via `proptest! { #![proptest_config(ProptestConfig::with_cases(10_000))] ... }`) |
| **Enforces** | Property-based testing with a minimum of 10,000 cases for CRDT operations, ordering, identity, schema validation, WAL properties, and index consistency. Algebraic laws (commutativity, associativity, idempotence) are tested at statistical depth, not just with hand-picked examples. |
| **INV/NEG** | INV-FERR-001 (merge commutativity), INV-FERR-002 (merge associativity), INV-FERR-003 (merge idempotence), INV-FERR-004/005 (index consistency), INV-FERR-008 (schema identity), INV-FERR-010 (WAL ordering). Proptest strategies are specified in each invariant's Level 2 contract. |
| **Current status** | Manual (runs as part of Gate 4, but case count is a separate concern) |
| **CI behavior** | Block merge. Proptest regressions are persisted to `proptest-regressions/` files and committed. |

---

## Gate 6: Lean 4 Formal Verification

| Field | Value |
|-------|-------|
| **Command** | `cd ferratomic-verify/lean && lake build` |
| **Enforces** | Machine-checked proofs for core algebraic properties. The Lean formalization must build with **zero `sorry`** axioms -- every theorem is fully proved. Covers the CRDT semilattice laws, VKN merge, snapshot isolation, and content-addressed identity. |
| **INV/NEG** | INV-FERR-001 through INV-FERR-003 (CRDT semilattice), INV-FERR-012 (content-addressed identity), INV-FERR-006 (snapshot isolation). The Lean theorems are the Level 0 algebraic laws made machine-verifiable. |
| **Current status** | Manual |
| **CI behavior** | Block merge. Any `sorry` in the Lean source is a proof gap and must be resolved before merge. |

---

## Gate 7: `#![forbid(unsafe_code)]` -- INV-FERR-023

| Field | Value |
|-------|-------|
| **Command** | Verified by compilation (Gate 1). Each crate's `lib.rs` contains `#![forbid(unsafe_code)]`. |
| **Enforces** | No `unsafe` blocks, `unsafe fn`, `unsafe impl`, or `unsafe trait` in any Ferratomic crate. The Rust compiler rejects any file containing unsafe code when this attribute is present. This means no raw pointer dereference, no `extern "C"` FFI, no `transmute`, no `unsafe impl Send/Sync`. |
| **INV/NEG** | **INV-FERR-023** (No Unsafe Code). Traces to SEED.md section 4 and NEG-FERR-002. |
| **Current status** | Manual (structural -- enforced at compile time by the `forbid` attribute) |
| **CI behavior** | Block merge. The `forbid` attribute is stronger than `deny` -- it cannot be overridden by inner `#[allow]` attributes. Compilation failure IS the enforcement. |

---

## Gate 8: `#[deny(clippy::unwrap_used)]` -- NEG-FERR-001

| Field | Value |
|-------|-------|
| **Command** | Verified by Gate 2 (clippy). Each crate's `lib.rs` contains `#![deny(clippy::unwrap_used)]`, `#![deny(clippy::expect_used)]`, `#![deny(clippy::panic)]`, `#![deny(clippy::todo)]`, `#![deny(clippy::unimplemented)]`. |
| **Enforces** | No panicking constructs (`unwrap()`, `expect()`, `panic!()`, `todo!()`, `unimplemented!()`, `unreachable!()`) in production code. A panic in a database engine corrupts the caller's process, leaves the WAL in an inconsistent state, and loses all in-flight operations. Database engines must return errors, not abort. |
| **INV/NEG** | **NEG-FERR-001** (No Panics in Production Code). Traces to INV-FERR-019 (Error Exhaustiveness). |
| **Current status** | Manual. Unwrap elimination is in progress as part of Phase 4a hardening. Once complete, these denies become enforced by clippy in Gate 2. |
| **CI behavior** | Block merge (once unwrap elimination is complete). Test code (`#[cfg(test)]`) is exempt -- `unwrap()` with descriptive messages is permitted in tests. |

---

## Summary Table

| Gate | Command | Primary INV/NEG | Status |
|------|---------|-----------------|--------|
| 1 | `cargo check --workspace` | All (type system) | Manual |
| 2 | `cargo clippy --workspace -- -D warnings` | NEG-FERR-001, code quality | Manual |
| 3 | `cargo fmt --all -- --check` | Process hygiene | Manual |
| 4 | `cargo test --workspace` | INV-FERR-001..055, NEG-FERR-001..005 | Manual |
| 5 | proptest 10K+ cases | INV-FERR-001..003, 004/005, 008, 010 | Manual |
| 6 | `lake build` (0 sorry) | INV-FERR-001..003, 006, 012 | Manual |
| 7 | `#![forbid(unsafe_code)]` | INV-FERR-023 | Manual (compile-time) |
| 8 | `#![deny(clippy::unwrap_used)]` | NEG-FERR-001 | Manual (in progress) |

All commands assume `CARGO_TARGET_DIR=/data/cargo-target` is set. Omitting
this variable defaults to `/tmp` (RAM-backed tmpfs), which will exhaust
available memory on large builds.

---

## Future CI Configuration

When CI is set up, the pipeline should:

1. Run gates 1-3 in parallel (compilation, clippy, fmt) as a fast-feedback stage.
2. Run gate 4 (tests including proptest) after gates 1-3 pass.
3. Run gate 6 (Lean proofs) in a separate job with `elan` and `lake` installed.
4. Gates 7-8 are subsumed by gates 1-2 (the attributes are checked by the compiler and clippy respectively) but should be verified by a grep-based check that the attributes remain present in each `lib.rs`.
5. All gates block merge to `main`. No exceptions, no manual overrides.
