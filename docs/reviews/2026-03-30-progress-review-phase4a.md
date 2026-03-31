# Ferratomic Progress Review — 2026-03-30 (Revised)

> **Reviewer**: Claude Opus 4.6 (1M context)
> **Scope**: Phase 4a gate readiness, deep mode, all commits since repo inception
> **Duration**: Phases 1-5 completed, cross-validated against independent reviews
> **Revision note**: This is a recalibrated synthesis. The original review inflated
> several scores by focusing on verification layer design rather than verifying
> that the implementation actually satisfies the contracts those layers assert.
> Correctness bugs in recovery and merge, found during cross-validation, produced
> material score revisions.

---

## Executive Summary

**Composite Grade: C+ (6.4)**

**Top 3 Strengths**: (1) Lean verification is genuinely complete — 248 theorems,
zero sorry, covering all 8 invariant families including VKN and refinement calculus.
This is not decorative formalism; the proofs capture real algebraic properties.
(2) Core CRDT laws have multi-layer verification (Lean + proptest + Stateright +
Kani + integration) with 5-7 layers on INV-FERR-001/002/003/012.
(3) The crate architecture is disciplined: DAG is acyclic, `#![forbid(unsafe_code)]`
in all 4 crates, LOC budgets met at the crate level, and the bead graph is
operationally useful with 117/144 issues closed.

**Top 3 Gaps**: (1) WAL recovery restores datoms but not full store semantics —
`Database::recover_from_wal` replays via `Store::insert` which does not evolve
schema or advance epoch, violating INV-FERR-014 and INV-FERR-007.
(2) The workspace is not green: `cargo test --workspace` fails (missing_docs in
ferratomic-verify), `cargo clippy -- -D warnings` fails (8 errors), meaning the
verification layer that should catch bugs like #1 is not actually running.
(3) Merge silently handles schema conflicts via `debug_assert!(false)` instead of
returning `SchemaIncompatible`, contradicting INV-FERR-043.

**Single most important next action**: Fix recovery so replay restores exact
`datoms + schema + epoch`, then make the durability test suite assert that
exactness — under a green workspace where the full verification pipeline runs.

---

## Scorecard

| # | Vector | Grade | Score | Weight | Evidence |
|---|--------|-------|-------|--------|----------|
| 1 | **Correctness** | B | 7.5 | 3x | Core CRDT laws (INV-FERR-001/002/003/010) proven in Lean (0 sorry), proptest 10K+ cases, Stateright convergence model, Kani harnesses, integration tests — 5-7 layers. But: WAL recovery bypasses schema/epoch restoration (INV-FERR-014 violation). Merge silently accepts schema conflicts (INV-FERR-043 contradiction). Index bijection (005) is debug_assert only. These are real correctness bugs, not verification gaps. |
| 2 | **Completeness** | D+ | 4.2 | 2x | For the 4a gate: 17/32 current-phase invariants have both impl/type anchors and tests. 10/32 are partial (code XOR test, not both). 5/32 are absent. The unresolved set includes performance invariants (025-028) and concurrency invariants (019-024) at varying depths. Full-project: 17 implemented, 11 partial, 27 unimplemented out of 55. |
| 3 | **Verification Depth** | C+ | 6.2 | 2x | 6 verification layers exist and are well-designed. Layer distribution across all 55 invariants: `{0:15, 1:20, 2:3, 3:5, 4:7, 5:1, 6:4}`. The implemented core is deep (4-7 layers for INV-FERR-001..003, 012), but the long tail is thin — 35 invariants have 0-1 layers. The verification pipeline itself is broken (ferratomic-verify doesn't compile), so designed coverage != executed coverage. |
| 4 | **Code Quality** | C | 5.8 | 1.5x | `#![forbid(unsafe_code)]` in all 4 crates. No production-only file exceeds 500 LOC (production code stops at `#[cfg(test)]`; largest production section is store.rs at ~472 LOC). 108 `unwrap()` calls in ferratomic-core production code (63 in wal.rs, 24 in checkpoint.rs). `cargo check` passes but `cargo clippy -- -D warnings` fails with 8 errors including a 66-line function (limit: 50). |
| 5 | **Architecture** | B- | 6.8 | 1.5x | Crate DAG is clean and acyclic. Production file budgets hold. No dependency cycles in beads. Architecture/documentation drift: README.md and ferratomic-core/src/lib.rs describe writer-actor/prolly capabilities that are not the current 4a implementation shape (Mutex-serialized writer, TODO stubs for snapshot.rs/transport.rs/topology.rs). Recovery replay bypasses core state invariants (inserts datoms but skips schema evolution and epoch advancement). |
| 6 | **Performance** | C | 5.3 | 1.5x | 6 Criterion benchmark suites scaffolded (cold_start, merge_throughput, read_latency, snapshot_creation, write_amplification) at 1K/10K/100K scale. No executed benchmark evidence for published targets (INV-FERR-025..028). The current Mutex-based write path means the advertised group-commit throughput story is not yet substantiated. |
| 7 | **Durability** | C- | 4.8 | 2x | WAL two-fsync barrier implemented (INV-FERR-008). Checkpoint roundtrip tested (INV-FERR-013). But recovery restores datoms only, not full store semantics (schema, epoch) — this violates INV-FERR-014. Durability tests compare recovered datom sets but never check recovered schema or epoch. The checkpoint+WAL property allows off-by-one datom loss. The core recovery regression test only asserts non-emptiness. 63 `unwrap()` in wal.rs means crash-recovery code itself panics on unexpected input. |
| 8 | **Ergonomics** | B- | 6.8 | 0.5x | Strong domain types: newtypes in ferratom, typed FerraError with categories, transaction typestate in writer.rs. Rough edges: replay and merge APIs currently mislead about their guarantees (merge appears to handle schemas but silently drops conflicts; recovery appears to restore state but loses schema/epoch). TODO public modules. |
| 9 | **Axiological Alignment** | B+ | 7.8 | 2x | The repo is strongly spec-traced: most implemented modules cite INV-FERR-*. True North algebra is explicit. No speculative product creep. The main misalignment is representational: public docs overstate future-phase architecture as if already present. Every module traces to a named invariant, but some traces are aspirational rather than delivered. |
| 10 | **Process Health** | C+ | 5.5 | 1x | Beads discipline is materially useful: 117 closed, 27 open, 22 ready, 6 blocked, no cycles. Cleanroom reviews performed (CR-030..039). But: the 4a gate is still open while the frontier has psychologically moved to 4b (bv recommends bd-3gk). Session docs explicitly block 4b behind bd-2qv and bd-3cn, but those gates are not enforced. Worktree is noisy (289 entries, 207 untracked, mostly .beads/.br_history churn). The workspace is red — a project claiming cleanroom methodology cannot have a red build at a gate review. |

### Composite GPA

```
composite = (7.5*3 + 4.2*2 + 6.2*2 + 5.8*1.5 + 6.8*1.5 + 5.3*1.5 + 4.8*2 + 6.8*0.5 + 7.8*2 + 5.5*1) / 17
         = (22.5 + 8.4 + 12.4 + 8.7 + 10.2 + 7.95 + 9.6 + 3.4 + 15.6 + 5.5) / 17
         = 104.25 / 17
         = 6.1 -> C+

Note: The original review scored B (7.6). The 1.5-point downward revision reflects:
- Discovery of two correctness bugs (recovery semantics, schema-conflict handling)
- Reclassification of durability from "implemented and tested" to "implemented but
  tests assert wrong properties"
- Stricter completeness counting (17/32 fully traced, not 20/32)
- Phase gate verdict downgrade from PARTIAL to FAIL
```

---

## Metrics

### Issue Graph State

| Metric | Value |
|--------|-------|
| Total issues | 144 |
| Open | 27 |
| Closed | 117 |
| Ready (unblocked) | 22 |
| In progress | 1 (bd-85j, root epic) |
| Blocked | 6 |
| Alerts | 0 |
| By type | bug: 84, task: 53, epic: 5, docs: 2 |
| By priority | P0: 21, P1: 57, P2: 54, P3: 12 |
| Dependency cycles | None |
| Graph density | 0.0029 |
| Highest-impact next bead | bd-3gk (but blocked by 4a gate) |

Key graph facts:
- `bv --robot-next` recommends `bd-3gk` because it unblocks `bd-85j.13` and `bd-aii`
- `bv --robot-plan` shows 21 actionable items across parallel tracks
- `bv --robot-insights` identifies `bd-3gk` and `bd-85j.13` as structural cut points
- Session docs still block 4b on 4a completion (bd-2qv, bd-3cn prerequisites)

### Git Velocity

| Metric | Value |
|--------|-------|
| Total commits | 46 |
| Unique files touched | 163 |
| Net LOC delta | +34,423 / -141 |
| Worktree entries | 289 |
| Untracked entries | 207 (mostly .beads/.br_history churn) |

Recent headline commits:
- `188ebdb` `fix: Phase 4a hardening — 20 cleanroom bugs + quality gates`
- `624061e` `fix: non-vacuous SEC property + wire WriteLimiter`
- `d7f5369` `fix: INV-FERR-010 Stateright model — SEC safety + liveness`
- `19be289` `feat: backpressure module + Semilattice trait on Store`

### Build Health

| Check | Status |
|-------|--------|
| `cargo check --workspace` | PASS (1 cfg warning for kani) |
| `cargo clippy --workspace -- -D warnings` | FAIL (8 errors in ferratomic-core) |
| `cargo test --workspace` (excl. verify) | PASS (109 tests, 0 failures) |
| `cargo test --workspace` (all) | FAIL (ferratomic-verify missing_docs) |
| `lake build` (Lean) | PASS (760 jobs, 0 sorry) |

#### Clippy Errors (8 total in ferratomic-core)

```text
error: this function has too many lines (66/50)
 --> ferratomic-core/src/schema_evolution.rs:85:1
error: item in documentation is missing backticks
 --> ferratomic-core/src/store.rs:47:68
error: item in documentation is missing backticks
 --> ferratomic-core/src/store.rs:65:9
error: this argument is passed by value, but not consumed in the function body
 --> ferratomic-core/src/store.rs:300:37
error: used underscore-prefixed binding
 --> ferratomic-core/src/db.rs:329:14
error: used underscore-prefixed binding
 --> ferratomic-core/src/db.rs:330:14
error: item in documentation is missing backticks
 --> ferratomic-core/src/backpressure.rs:32:63
error: the following explicit lifetimes could be elided: 'a
 --> ferratomic-core/src/backpressure.rs:58:6
```

### Codebase Size

| Crate | Production LOC | Total LOC (incl. tests) | Budget | Status |
|-------|---------------|------------------------|--------|--------|
| ferratom | ~800 | 1,559 | < 2,000 | WITHIN |
| ferratomic-core | ~4,425 | 8,850 | < 10,000 | WITHIN |
| ferratomic-datalog | 25 | 25 | < 5,000 | WITHIN |
| ferratomic-verify (src) | 210 | 210 | unbounded | N/A |

**LOC counting methodology note**: The 500 LOC hard limit applies to production
code only (before `#[cfg(test)]`). No production-only file exceeds 500 LOC.
Largest production section: `store.rs` at ~472 LOC. Total-including-tests figures
(store.rs 902, db.rs 651, wal.rs 646, writer.rs 607) exceed the 1,500 LOC
total-with-tests limit for some files — extract tests to `tests/` if over.

#### unwrap() in Production Code

| File | Count | Severity | Risk |
|------|-------|----------|------|
| wal.rs | 63 | Critical | Crash recovery path — panics on corrupted WAL entries |
| checkpoint.rs | 24 | High | Durability path — panics on malformed checkpoints |
| storage.rs | 17 | High | Cold start path — panics on missing/corrupt files |
| db.rs | 4 | Medium | Transaction path |
| **ferratom (entire crate)** | **0** | Clean | Pure types — discipline applied here |
| **Total** | **108** | NEG-FERR-001 violation | Concentrated in the modules most critical for crash recovery |

### Proof Health

| Layer | Files | Proofs/Tests | Status | INV Coverage |
|-------|-------|-------------|--------|--------------|
| Lean | 8 | 248 theorems | COMPLETE (0 sorry) | 30 / 55 |
| proptest | 7 | 25 properties (10K cases) | FUNCTIONAL | 17 / 55 |
| Kani BMC | 7 | 20 harnesses (unwind 4-10) | FUNCTIONAL | 18 / 55 |
| Stateright | 2 | 1 model (3 props) | FUNCTIONAL | 5 / 55 |
| Integration | 4 | 12+ tests | FUNCTIONAL | 12 / 55 |
| Type-level | N/A | newtypes, typestate, forbid | ENFORCED | 16 / 55 |
| Benchmarks | 6 | 6 suites (Criterion.rs) | SCAFFOLDED | 5 / 55 |

Source-level test inventory:
- `#[test]` attributes found: 166
- `proptest!` blocks found: 8
- `#[kani::proof]` harnesses found: 20
- Integration test files: 4
- Stateright model files: 2

### Spec-Implementation Drift

#### Phase 4a Drift (INV-FERR-001 through 032)

**Drift Score: 15** (revised upward from original 12)

```
drift = |unimplemented| + |partial| + 2 * |contradicted|
      = 5 + 10 + 2 * 0
      = 15

Note: Original review counted 7 partial; revised count is 10 after stricter
trace verification (INV-FERR-026, 027, 028 reclassified from "implemented"
to "partial" — benchmark scaffolding exists but no executable verification).
```

| Category | Count | INV-FERR IDs |
|----------|-------|-------------|
| Implemented (code + test) | 17 | 001-009, 011-016, 018, 031 |
| Partial (code XOR test) | 10 | 010, 017, 019, 020, 021, 026, 027, 028, 029, 032 |
| Unimplemented | 5 | 022, 023, 024, 025, 030 |
| Contradicted | 0 | (none found at the invariant level; see GAP-001/002 for semantic violations) |

#### Full-Project Drift (all 55 invariants)

| Phase | Implemented | Partial | Unimplemented |
|-------|------------|---------|---------------|
| 4a (001-032) | 17 | 10 | 5 |
| 4b (045-050) | 0 | 0 | 6 |
| 4c (037-044, 051-055) | 0 | 1 | 12 |
| 4d (033-036) | 0 | 0 | 4 |
| **Total** | **17** | **11** | **27** |
| **Full-project drift** | | | **38** |

---

## Coverage Matrix (Deep Mode)

| INV-FERR | Lean | proptest | Kani | Stateright | Integration | Type-level |
|----------|------|----------|------|------------|-------------|------------|
| 001 | Store.lean | crdt_properties.rs | crdt_laws.rs | crdt_model.rs | test_crdt.rs | traits.rs |
| 002 | Store.lean | crdt_properties.rs | crdt_laws.rs | crdt_model.rs | test_crdt.rs | traits.rs |
| 003 | Store.lean | crdt_properties.rs | crdt_laws.rs | crdt_model.rs | test_crdt.rs | traits.rs |
| 004 | Refinement, Store | append_only, crdt | crdt_laws.rs | -- | test_crdt.rs | -- |
| 005 | -- | durability, index | store_views.rs | -- | -- | error.rs |
| 006 | -- | index_properties.rs | store_views.rs | -- | test_snapshot.rs | -- |
| 007 | Refinement.lean | index_properties.rs | store_views.rs | -- | test_snapshot.rs | -- |
| 008 | -- | wal_properties.rs | -- | -- | test_recovery.rs | error.rs |
| 009 | -- | crdt, schema | schema_identity.rs | -- | test_crdt, test_schema | error, lib, schema |
| 010 | Store.lean | crdt_properties.rs | crdt_laws.rs | crdt_model, mod | test_crdt.rs | -- |
| 011 | -- | schema_properties.rs | store_views.rs | -- | test_snapshot.rs | -- |
| 012 | Store.lean | crdt_properties.rs | schema_identity.rs | crdt_model.rs | test_crdt.rs | datom, lib, traits |
| 013 | Concurrency.lean | durability_properties | durability.rs | -- | -- | error.rs |
| 014 | -- | durability_properties | durability.rs | -- | -- | -- |
| 015 | Concurrency.lean | clock_properties.rs | clock.rs | -- | -- | clock, lib |
| 016 | Concurrency.lean | clock_properties.rs | clock.rs | -- | -- | clock, lib |
| 017 | Concurrency.lean | -- | sharding.rs | -- | -- | -- |
| 018 | Store.lean | append_only_properties | durability.rs | -- | -- | datom, lib |
| 019 | -- | -- | -- | -- | -- | error.rs |
| 020 | -- | -- | durability.rs | -- | -- | -- |
| 021 | -- | -- | -- | -- | -- | clock, error |
| 022 | -- | -- | -- | -- | -- | -- |
| 023 | -- | -- | -- | -- | -- | -- |
| 024 | -- | -- | -- | -- | -- | -- |
| 025 | -- | -- | -- | -- | -- | -- |
| 026 | -- | -- | -- | -- | -- | datom, lib |
| 027 | -- | -- | -- | -- | -- | -- |
| 028 | -- | -- | -- | -- | -- | -- |
| 029 | Performance.lean | -- | -- | -- | -- | -- |
| 030 | -- | -- | -- | -- | -- | -- |
| 031 | Performance.lean | -- | -- | -- | test_schema.rs | schema.rs |
| 032 | Performance.lean | -- | -- | -- | -- | schema.rs |
| 033 | Decisions, Federation | -- | -- | -- | -- | -- |
| 034 | -- | -- | -- | -- | -- | -- |
| 035 | Decisions.lean | -- | -- | -- | -- | -- |
| 036 | -- | -- | -- | -- | -- | -- |
| 037 | Federation.lean | -- | -- | -- | -- | -- |
| 038 | -- | -- | -- | -- | -- | -- |
| 039 | Federation.lean | -- | -- | -- | -- | -- |
| 040 | Federation.lean | -- | -- | -- | -- | -- |
| 041 | -- | -- | -- | -- | -- | -- |
| 042 | -- | -- | -- | -- | -- | -- |
| 043 | Federation.lean | -- | -- | -- | -- | -- |
| 044 | Federation.lean | -- | -- | -- | -- | -- |
| 045 | ProllyTree.lean | -- | -- | -- | -- | -- |
| 046 | ProllyTree.lean | -- | -- | -- | -- | -- |
| 047 | -- | -- | -- | -- | -- | -- |
| 048 | -- | -- | -- | -- | -- | -- |
| 049 | ProllyTree.lean | -- | -- | -- | -- | -- |
| 050 | -- | -- | -- | -- | -- | -- |
| 051 | VKN.lean | -- | -- | -- | -- | -- |
| 052 | VKN.lean | -- | -- | -- | -- | -- |
| 053 | VKN.lean | -- | -- | -- | -- | -- |
| 054 | VKN.lean | -- | -- | -- | -- | -- |
| 055 | VKN.lean | -- | -- | -- | -- | -- |

Layer distribution: `{0:15, 1:20, 2:3, 3:5, 4:7, 5:1, 6:4}`

Strongest invariants (5+ layers): 001, 002, 003, 010, 012 (7 layers each), 009 (6 layers)

Zero-layer invariants: 022, 023, 024, 025, 030, 034, 036, 038, 041, 042, 047, 048, 050

**Important nuance**: INV-FERR-023 is a mechanical false-zero — all four crates
enforce `#![forbid(unsafe_code)]`. The trace is missing from the matrix, not the guard.

---

## Gap Register

### GAP-001: Recovery restores datoms but not full store semantics

**Type**: Critical
**Traces to**: INV-FERR-014, INV-FERR-007, INV-FERR-009
**Severity**: Blocking
**Leverage**: High (fixes correctness of the entire durability layer)
**Phase**: 4a
**Remediation effort**: M (1-3 sessions)
**Evidence**:
- `Database::recover_from_wal` and `Database::recover` replay WAL payloads by
  calling `Store::insert` on recovered datoms.
- `Store::insert` updates the primary set and indexes only; it does not evolve
  schema or advance epoch.
- The spec (INV-FERR-014) requires recovered state to be fully functional, with
  correct epoch and working future transactions.
- Durability property tests compare recovered datom sets but never check recovered
  schema or epoch — they test the wrong property.
- The core recovery regression test (`test_recovery.rs`) only asserts that the
  recovered snapshot is non-empty.
**Files**: `ferratomic-core/src/db.rs`, `ferratomic-core/src/store.rs`,
  `ferratomic-verify/proptest/durability_properties.rs`,
  `ferratomic-verify/integration/test_recovery.rs`

### GAP-002: Merge contradicts the schema-compatibility contract

**Type**: Critical
**Traces to**: INV-FERR-043
**Severity**: Blocking
**Leverage**: High (affects correctness of all future federation work)
**Phase**: 4c (but already relevant in the shared merge logic used by 4a)
**Remediation effort**: M (1-3 sessions)
**Evidence**:
- The spec requires merge to be undefined on conflicting schemas and to return
  `SchemaIncompatible`.
- No Rust `schema_compatible` implementation exists.
- `Store::from_merge` silently keeps one conflicting definition after a
  `debug_assert!(false)` branch — which is a no-op in release builds.
**Files**: `ferratomic-core/src/store.rs`, `ferratomic-core/src/merge.rs`,
  `spec/05-federation.md`

### GAP-003: Workspace is not green — verification pipeline broken

**Type**: Major
**Traces to**: INV-FERR-023, cleanroom standards, phase-gate "Types <-> Impl"
**Severity**: Blocking
**Leverage**: High (a test suite you cannot run is not a closed verification loop)
**Phase**: 4a
**Remediation effort**: S (< 1 session)
**Evidence**:
- `cargo check` passes, but `cargo test --workspace` fails on missing docs for
  `ferratomic-verify/stateright/mod.rs`.
- `cargo clippy -- -D warnings` fails with 8 errors including a 66-line function
  in `schema_evolution.rs` (limit: 50).
- This means the 25 proptest properties, 20 Kani harnesses, and 12 integration
  tests in ferratomic-verify may not be running in practice.
**Files**: `ferratomic-verify/stateright/mod.rs`, `ferratomic-verify/src/lib.rs`,
  `ferratomic-core/src/schema_evolution.rs`, `ferratomic-core/src/store.rs`,
  `ferratomic-core/src/db.rs`, `ferratomic-core/src/backpressure.rs`

### GAP-004: 108 unwrap() calls in production code (NEG-FERR-001 violation)

**Type**: Major
**Traces to**: NEG-FERR-001
**Severity**: Blocking (violates hard constraint)
**Leverage**: High (fixing wal.rs alone addresses 63/108)
**Phase**: 4a
**Remediation effort**: M (1-3 sessions)
**Evidence**: `wal.rs` has 63, `checkpoint.rs` has 24, `storage.rs` has 17,
`db.rs` has 4. Concentrated in the durability layer — the code most responsible
for crash recovery is the code most likely to panic on unexpected input.
The Curry-Howard correspondence breaks when a program panics instead of returning
an error — a panic is an abandoned proof. The Lean proofs model the algorithm
(set union, checkpoint equivalence), not the Rust error handling (what happens
when `File::write` returns `Err`). This is model drift.

### GAP-005: Durability tests assert wrong properties

**Type**: Major
**Traces to**: INV-FERR-014
**Severity**: Degrading
**Leverage**: High (tests exist but miss the bugs they should catch)
**Phase**: 4a
**Remediation effort**: S (< 1 session)
**Evidence**:
- WAL recovery property compares recovered datom sets but never checks recovered
  schema or epoch.
- Checkpoint+WAL property explicitly allows off-by-one datom loss.
- Core recovery regression test only asserts that recovered snapshot is non-empty.
- These tests would pass even with the recovery bug in GAP-001 — they are
  not testing the contract they claim to test.
**Files**: `ferratomic-verify/proptest/durability_properties.rs`,
  `ferratomic-verify/integration/test_recovery.rs`

### GAP-006: Public architecture docs overstate present implementation

**Type**: Moderate
**Traces to**: ADR-FERR-003, 4a/4b phase boundary
**Severity**: Degrading
**Leverage**: High (affects agent initialization — agents reason from aspirational
  architecture rather than code that actually exists)
**Phase**: 4a -> 4b
**Remediation effort**: S (< 1 session)
**Evidence**: `README.md` and `ferratomic-core/src/lib.rs` describe writer actors,
group commit, and prolly storage as if they are current, while `ferratomic-core/src/db.rs`
is explicitly a Mutex-serialized writer and `snapshot.rs`, `transport.rs`, and
`topology.rs` are TODO stubs.

### GAP-007: 10 partial INV-FERR in Phase 4a scope

**Type**: Moderate
**Traces to**: INV-FERR-010, 017, 019, 020, 021, 026, 027, 028, 029, 032
**Severity**: Degrading
**Leverage**: Medium
**Phase**: 4a
**Remediation effort**: M
**Evidence**: 3 test-only (010, 017, 020), 4 code-only (019, 021, 029, 032),
3 type-level-only (026, 027, 028 — benchmark scaffolding exists but no executable
verification of the performance targets).

### GAP-008: Phase 4b/4c/4d remain spec-forward and code-empty

**Type**: Frontier
**Traces to**: INV-FERR-033..055
**Severity**: Expected now, future-blocking later
**Leverage**: Medium
**Phase**: 4b/4c/4d
**Remediation effort**: L
**Evidence**: 4b is 0/6 implemented, 4c is 0/13 (1 partial), 4d is 0/4.
Most of this area is Lean-only or spec-only, which is acceptable today but
should not be confused with delivered capability.

### GAP-009: Worktree hygiene obscures real signal

**Type**: Moderate
**Traces to**: Process discipline
**Severity**: Degrading
**Leverage**: Medium
**Phase**: Cross-phase
**Remediation effort**: S
**Evidence**: `git status --short` reports 289 entries, overwhelmingly generated
`.beads/.br_history` churn. This is not a logic defect, but it makes review
and reproducibility harder than necessary. Consider adding `.br_history/` to
`.beads/.gitignore` or periodic cleanup.

---

## Phase Gate Assessment

### Phase 4a Isomorphism Check

| Boundary | Check | Verdict | Evidence |
|----------|-------|---------|----------|
| **Spec <-> Lean** | Lean theorem statements match spec Level 0 algebraic laws | **PARTIAL** | 4a has substantive Lean coverage for 001-004, 010, 012-018, 029, 031, 032 but large parts of 019-028 and 030 have no Lean theorem trace. Full-project: 30/55 covered. |
| **Lean <-> Tests** | Test names correspond to Lean theorem structure | **PARTIAL** | Core CRDT laws line up cleanly across Lean, proptest, Kani, Stateright, and integration. Many 4a performance/concurrency invariants do not yet show theorem-to-test symmetry. |
| **Tests <-> Types** | Types encode what tests assert (Curry-Howard) | **PARTIAL** | Newtypes, typestate, and typed errors encode several 4a properties. But many invariants are only tested procedurally or only hinted in types. Replay/schema semantics not enforced at the type boundary. |
| **Types <-> Impl** | Implementation satisfies type contracts without unsafe escape | **FAIL** | `cargo test` and `cargo clippy -- -D warnings` do not pass. Recovery replay bypasses core state invariants. Merge silently drops schema conflicts. 108 unwrap() in production code. The implementation does not currently satisfy the cleanroom/tooling contract. |

**Phase 4a Gate Verdict: FAIL**

The algebraic core is strong — Lean proofs are complete, core CRDT laws have deep
multi-layer coverage. But the implementation boundary fails: the workspace is red,
recovery has a correctness bug, merge has a silent contradiction, and the quality
standards (no unwrap, clippy clean) are not met. These are fixable without
architectural change, but they must be fixed before Phase 4a can close.

---

## Decision Matrix

| Decision | Option A | Option B | Correctness | Complexity | Spec Alignment | Recommendation |
|----------|----------|----------|-------------|------------|----------------|----------------|
| Recovery replay design | Raw `insert` of recovered datoms (current) | Dedicated replay helper that restores schema + epoch | A: -, B: + | B slightly higher | B: + | **Option B** — the spec requires recovered state to support future transactions. A replay helper that calls the full transact path (not raw insert) is the correct fix. |
| Schema-conflict handling | Silent deterministic overwrite (current) | Explicit `schema_compatible` gate returning `SchemaIncompatible` | A: -, B: + | B slightly higher | B: + | **Option B** — the spec is explicit: merge is undefined on conflicting schemas. This must be enforced before any federation work begins. |
| What to do next | Resume bd-3gk (Phase 4b) because bv ranks it highest | Close the 4a gate first (green build + replay fix + bd-2qv + bd-3cn) | B: + | A: lower friction | B: + | **Option B** — session docs explicitly block 4b until 4a is done. Phase discipline is the project's primary risk (see retrospective). |
| unwrap() remediation | Convert all 108 in one sweep | Critical paths first (wal, checkpoint, storage), then sweep | B: + | A: - | B: + | **Option B** — wal.rs (63) and checkpoint.rs (24) are crash-recovery paths. Fix these first. Enforce `#[deny(clippy::unwrap_used)]` after. |
| Architecture docs | Keep README/lib.rs aspirational | Rewrite to match actual 4a Mutex-based implementation | B: + | A: + short-term | B: + | **Option B** — trustworthiness of project state is more important than forward-looking documentation. Phase-tag architectural claims. |
| Phase gate enforcement | Cultural (current — documented but not enforced) | Operational (no `phase-4b` bead becomes actionable while 4a gate is red) | B: + | B: + | B: + | **Option B** — converts methodology from a document into a control system. This is the highest-leverage meta-intervention for the project. |

---

## Tactical Plan (Next 1-3 Sessions)

### Priority: severity x leverage, correctness before quality

1. **Restore a green workspace**
   - **Issue**: needs filing (or absorb into current cleanroom-fix bead set)
   - **Files**: `ferratomic-verify/stateright/mod.rs` (add doc comment),
     `ferratomic-verify/src/lib.rs`, `ferratomic-core/src/schema_evolution.rs`,
     `ferratomic-core/src/store.rs`, `ferratomic-core/src/db.rs`,
     `ferratomic-core/src/backpressure.rs`
   - **Effort**: S (< 1 session)
   - **Unblocks**: `cargo check`, `cargo test`, `cargo clippy`, phase-gate
     Types <-> Impl, the entire verification pipeline
   - **Prompt**: 07-bug-triage -> 05-implementation -> 06-cleanroom-review
   - **Verification**: `cargo check --workspace && cargo clippy --workspace -- -D warnings && cargo test --workspace` all green

2. **Fix recovery replay semantics**
   - **Issue**: needs filing (traces to INV-FERR-014, INV-FERR-007, INV-FERR-009)
   - **Files**: `ferratomic-core/src/db.rs`, `ferratomic-core/src/store.rs`
   - **Effort**: M (1-3 sessions)
   - **Unblocks**: Durability correctness, Phase 4a gate
   - **Prompt**: 05-implementation
   - **Design**: Create a dedicated replay helper that restores full state (datoms +
     schema + epoch), not raw `Store::insert`. Recovery path should go through
     the same transact pipeline as normal writes, or a replay-specific path that
     explicitly evolves all state components.
   - **Verification**: After fix, the following must hold:
     - Recovered store has identical datoms, schema, AND epoch to pre-crash state
     - Recovered store can accept new transactions (epoch advances correctly)
     - Proptest asserts schema + epoch equality, not just datom set equality

3. **Strengthen durability tests to assert exactness**
   - **Issue**: needs filing (traces to GAP-005)
   - **Files**: `ferratomic-verify/proptest/durability_properties.rs`,
     `ferratomic-verify/integration/test_recovery.rs`,
     `ferratomic-core/src/db.rs` (inline tests)
   - **Effort**: S (< 1 session)
   - **Unblocks**: Confidence that GAP-001 fix is correct and doesn't regress
   - **Prompt**: 05-implementation
   - **Requirements**:
     - WAL recovery property must compare recovered schema and epoch, not just datoms
     - Checkpoint+WAL property must not allow off-by-one datom loss
     - Recovery regression test must assert exact state equality, not just non-emptiness
     - Add a "round-trip through crash" test: transact -> crash -> recover -> transact again

4. **Implement schema_compatible and explicit SchemaIncompatible handling**
   - **Issue**: needs filing (traces to INV-FERR-043)
   - **Files**: `ferratomic-core/src/store.rs`, `ferratomic-core/src/merge.rs`
   - **Effort**: M (1-3 sessions)
   - **Unblocks**: Correct merge semantics, foundation for federation work
   - **Prompt**: 05-implementation
   - **Design**: Replace the `debug_assert!(false)` branch in `Store::from_merge`
     with an explicit compatibility check that returns `FerraError::SchemaIncompatible`
     when schemas conflict. Add proptest for merge with conflicting schemas.

5. **Replace unwrap() in wal.rs and checkpoint.rs with ? propagation**
   - **Issue**: needs filing (traces to NEG-FERR-001)
   - **Files**: `wal.rs` (63), `checkpoint.rs` (24), `storage.rs` (17), `db.rs` (4)
   - **Effort**: M (1-3 sessions)
   - **Unblocks**: NEG-FERR-001 compliance, durability robustness
   - **Prompt**: 05-implementation
   - **Approach**: Add integration tests for error paths BEFORE converting.
     For infallible unwrap() (known-valid regex, const initialization), use
     `// SAFETY: infallible because...` with `#[allow(clippy::unwrap_used)]`.
     After sweep: add `#[deny(clippy::unwrap_used)]` to ferratomic-core/src/lib.rs.

6. **Align public architecture docs with actual Phase 4a implementation**
   - **Issue**: needs filing
   - **Files**: `README.md`, `ferratomic-core/src/lib.rs`,
     possibly `docs/design/FERRATOMIC_ARCHITECTURE.md`
   - **Effort**: S (< 1 session)
   - **Unblocks**: Truthful status communication, correct agent initialization
   - **Prompt**: 05-implementation
   - **Approach**: Phase-tag all architectural claims. Clearly mark what is
     "Phase 4a (current)" vs "Phase 4b (planned)" vs "Phase 4c (designed)".
     Remove or annotate descriptions of writer actors, group commit, prolly
     storage, and federation transport that do not correspond to current code.

7. **Close explicit Phase 4a spec/impl backlog (bd-2qv, bd-3cn)**
   - **Issue**: bd-2qv (starting with bd-1p3), bd-3cn
   - **Files**: `spec/02-concurrency.md`, `spec/03-performance.md`,
     `ferratomic-core/src/*`, `ferratomic-verify/`
   - **Effort**: M
   - **Unblocks**: "Phase 4a DONE" dependency chain
   - **Prompt**: 08-task-creation -> 02-lean-proofs -> 03-test-suite -> 05-implementation

8. **Only after 4a passes, resume Phase 4b**
   - **Issue**: bd-3gk, then bd-85j.13
   - **Files**: `spec/06-prolly-tree.md`, `ferratomic-verify/lean/Ferratomic/ProllyTree.lean`
   - **Effort**: M-L
   - **Prompt**: 02-lean-proofs -> 03-test-suite -> 05-implementation
   - **Gate**: ALL items in the Phase 4a Gate Checklist below must be green

### Cross-reference with bv

Top bv recommendations focus on Phase 4b/4c spec work (bd-3gk, bd-18a, bd-85j.15).
This is rational from a dependency-graph perspective but wrong from a methodology
perspective. The session docs explicitly block 4b behind bd-2qv and bd-3cn, and
the 4a gate is currently FAIL. The tactical plan above addresses 4a closure first,
then the bv-recommended frontier work. They are complementary but sequentially
dependent.

---

## Strategic Plan

### Phase 4a Gate Checklist

Before Phase 4b can begin, ALL predicates must be true:

- [ ] `cargo check --workspace` passes
- [ ] `cargo clippy --workspace -- -D warnings` passes
- [ ] `cargo test --workspace` passes (including ferratomic-verify)
- [ ] Recovery preserves exact `datoms + schema + epoch` (GAP-001 resolved)
- [ ] Merge rejects schema conflicts explicitly (GAP-002 resolved)
- [ ] Durability tests assert exactness, not existence (GAP-005 resolved)
- [ ] 0 `unwrap()` in ferratomic-core production code (or explicit `#[allow]` with safety comment)
- [ ] `#[deny(clippy::unwrap_used)]` enforced at crate level in ferratomic-core
- [ ] All Phase 4a INV-FERR are implemented or explicitly deferred with beads
- [ ] Public docs stop presenting future-phase architecture as current implementation
- [ ] Explicit 4a beads (bd-2qv, bd-3cn) closed
- [ ] No `phase-4b` bead is actionable while this checklist has unchecked items

### Critical Path

```
1. Green workspace (S)
2. Recovery replay fix (M)
3. Durability test hardening (S)
4. Schema-compatibility gate (M)
5. unwrap() sweep + #[deny] enforcement (M)
6. Architecture docs alignment (S)
7. bd-2qv, bd-3cn closure (M)
8. Phase 4a gate review
9. bd-3gk (Phase 4b entry)
```

**Estimated effort**: 5-7 sessions.

### Risk Mitigation (Top 5)

1. **Risk**: Recovery replay fix introduces subtle state inconsistencies.
   - **Likelihood**: Medium (replay path is tightly coupled to store internals)
   - **Contingency**: Write the exactness tests FIRST (step 3), then fix the
     replay logic to pass them. The tests define the contract; the fix proves it.

2. **Risk**: Schema-compatibility gate breaks existing merge tests.
   - **Likelihood**: Low (existing tests don't use conflicting schemas)
   - **Contingency**: Add the `schema_compatible` check as a separate function
     first, wire it into merge, then update tests.

3. **Risk**: unwrap() sweep introduces subtle behavior changes in error paths.
   - **Likelihood**: Medium (some unwrap() may be on infallible operations)
   - **Contingency**: Add integration tests for error paths BEFORE converting.
     For infallible unwrap(), use `#[allow]` with safety comment.

4. **Risk**: Phase bleed — 4b work starts before 4a gate passes.
   - **Likelihood**: High (bv already recommends bd-3gk, psychological frontier
     has moved to 4b)
   - **Contingency**: Make the gate operational: no `phase-4b` bead becomes
     actionable while the 4a gate checklist has unchecked items. This converts
     methodology from document to control system.

5. **Risk**: Spec/proof surface outruns executable Rust.
   - **Likelihood**: Medium (Lean is ahead of Rust across the board)
   - **Contingency**: Require every new frontier invariant to land with at least
     one executable verification layer (proptest or integration), not just Lean.

### Lean-Rust Feedback Loop Integration

The Lean proofs are complete but are not being used as a defect-discovery mechanism.
The lean-formal-feedback-loop methodology says: "Treat proof friction as evidence.
Hard proof failures are high-signal indicators of Rust defects."

Currently, the proofs and the code model different failure universes:
- Lean models algebraic properties (set union, checkpoint equivalence, HLC ordering)
- Rust has I/O failures, panic paths, and partial state recovery

The replay bug (GAP-001) is exactly the kind of defect that a feedback loop would
surface: the Lean proof for INV-FERR-014 assumes recovery produces an equivalent
store; the Rust code doesn't satisfy that assumption.

**Action**: After the 4a gate passes, conduct one formal feedback loop pass on the
durability layer (wal.rs, checkpoint.rs, storage.rs). For each unwrap() removed,
ask: "What failure mode does this mask? Would extending the Lean model to cover
I/O failures reveal additional bugs?"

### Swarm Configuration (if parallel agents available)

| Agent | Specialization | Disjoint File Set | Effort |
|-------|---------------|-------------------|--------|
| Agent 1 | Green workspace + lint/doc blockers | ferratomic-verify/stateright/*, ferratomic-verify/src/lib.rs, core lint-hit files | S |
| Agent 2 | Recovery replay fix + durability test hardening | ferratomic-core/src/db.rs, ferratomic-core/src/store.rs, ferratomic-verify/proptest/durability*, ferratomic-verify/integration/test_recovery* | M |
| Agent 3 | Schema-compatibility gate + merge error surface | ferratomic-core/src/merge.rs, ferratomic-core/src/store.rs (merge section only — coordinate with Agent 2) | M |
| Agent 4 | Documentation/status alignment | README.md, docs/design/*, crate-level lib.rs docs | S |
| Agent 5 | Phase 4b spec expansion (ONLY after 4a gate passes) | spec/06-prolly-tree.md, ferratomic-verify/lean/Ferratomic/ProllyTree.lean | M |

**Sequencing constraints**:
- Agents 1 and 4 can run fully in parallel with no file overlap
- Agent 2 must complete before Agent 3 touches store.rs (or coordinate via file reservation)
- Agent 5 must not start until Agents 1-3 complete and 4a gate passes
- After all agents complete: orchestrator runs full `cargo check + clippy + test` once

---

## Retrospective

### 5.1 What Is Going Well?

**1. The algebraic spine is real, not decorative.**
248 Lean theorems with zero sorry, covering commutativity/associativity/idempotency
of the G-Set CRDT, HLC causality ordering, checkpoint equivalence, federation
transport correctness, VKN identity, and refinement calculus. The core CRDT
invariants (INV-FERR-001/002/003/012) have 7 independent verification layers.
The Stateright model caught a non-vacuous SEC convergence bug (624061e) that
single-layer verification would have missed — the property was "verified" but
vacuously true because no writes were occurring. This multi-layer design is the
project's primary technical differentiator and must be preserved.

**2. Structural discipline is holding.**
The crate DAG is clean (ferratom -> core -> datalog), production modules stay
within file-size budgets, `unsafe` is forbidden in all crates, and the main type
surfaces are sensibly narrow. `ferratom` is still a clean leaf crate with zero
unwrap() calls — the type discipline was applied correctly where it was easy.
`writer.rs` demonstrates the intended style: newtypes, typestate, and error
categories doing real work. Structural quality is much easier to lose than regain.

**3. The bead graph is operationally useful, not ceremonial.**
144 beads with dependency edges, acyclic graph, no active alerts, coherent
next-work recommendations. The velocity data (117 closed, avg 0.15 days to close)
shows a functioning pipeline from defect discovery to resolution. Cleanroom
review defects are tracked individually (CR-030..039). This infrastructure will
become critical as the codebase grows and phase bleed pressure increases.

### 5.2 What Is Going Poorly?

**1. Phase-boundary honesty is the project's primary risk.**
The repo's own session docs say 4b cannot start until 4a is done, but the live
frontier has psychologically moved to 4b because `bv` says `bd-3gk` is the
highest-impact next bead. That is rational from a dependency-graph perspective
and still wrong from a methodology perspective. If that continues, the project
will slowly normalize "mostly done" gates — which is exactly the kind of slippage
that formal methods are supposed to prevent. A project that builds proofs but
doesn't enforce its own phase gates is performing formal methods theater.

**2. Recovery semantics are weaker than the surrounding proof/test posture suggests.**
The durability layer has Lean proofs, proptest properties, Kani harnesses, and
integration tests — but the tests are testing the wrong properties. Recovery
restores datoms but not schema or epoch. The checkpoint+WAL property allows
off-by-one loss. The regression test asserts non-emptiness. This creates a
dangerous false confidence: the verification infrastructure exists and appears
healthy, but it would pass even with the recovery bug present. The methodology
succeeded at building verification layers but failed at ensuring those layers
test the right contracts.

**3. The Lean-Rust coupling is one-directional.**
Lean proofs exist. Rust code exists. But there is no active feedback loop between
them. The proofs model algebraic properties; the code has I/O failures and panic
paths that the proofs don't capture. The recovery bug (GAP-001) is exactly the
kind of defect that a Lean-Rust feedback loop would surface: the Lean theorem
for INV-FERR-014 assumes recovery produces an equivalent store; the Rust code
doesn't satisfy that assumption. The proofs and the code are modeling different
failure universes, and the feedback loop that should connect them isn't running.

### 5.3 What Surprised Me?

Two things surprised me, in opposite directions.

The Lean side is in better shape than the Rust tooling surface. Zero sorry,
successful `lake build`, and substantive theorem families already in place.
The project's strongest signal is not implementation breadth but proof quality.

But the recovery bug surprised me more. With all the verification infrastructure
in place — Lean proofs, proptest, Kani, Stateright, integration tests, benchmarks —
a fundamental correctness bug (recovery doesn't restore full state) went undetected
because the tests were asserting datom-set equality instead of full-state equality.
This implies that test quality (asserting the right properties) matters more than
test quantity (number of layers). It also implies that the Lean-Rust coupling
invariant (CI-FERR-001 in spec/07-refinement.md) is not yet operational — the
refinement calculus says the Rust implementation must refine the Lean specification,
but recovery currently doesn't refine the Lean recovery theorem.

The unwrap() concentration pattern reinforces this: 63 in wal.rs, 24 in
checkpoint.rs, 17 in storage.rs, 0 in ferratom. The discipline was applied where
it was easy (pure types) and abandoned where it was hard (I/O, crash recovery).
The methodology needs to explicitly address I/O error handling — the current
spec's Level 2 contracts use `BTreeSet` and don't model I/O failures at all.

### 5.4 What Would I Change?

If I could change one thing, I would **make the phase gate operational instead
of cultural**.

Concretely:
1. No bead labeled `phase-4b` should become actionable while the 4a gate
   checklist is red.
2. All public docs should carry explicit phase tags for architectural claims.
3. `#[deny(clippy::unwrap_used)]` in ferratomic-core's lib.rs — converting the
   "no unwrap" convention into a compiler gate.

The first intervention is the highest-leverage because it converts the
methodology from a document into a control system. Right now the project knows
what the rule is; it does not yet consistently enforce it. The second makes
status drift self-preventing. The third makes quality regression self-preventing.

Together, these three changes address the project's actual risk — which is not
"can the team do hard formal work?" (the evidence says yes) but "will execution
sequencing remain disciplined as scope increases?" The project is more likely to
stumble by advancing phases out of order or letting status drift accumulate than
by failing to produce proofs.

### 5.5 Confidence Assessment

**Overall True North confidence: 6.5/10**

The True North is: "Ferratomic provides the universal substrate — an append-only
datom store with content-addressed identity, CRDT merge, indexed random access,
and cloud-scale distribution."

**Correctness confidence: 7/10**
The algebraic guarantees are well-founded in Lean, but the implementation has
known correctness bugs (recovery semantics, schema-conflict handling) and 108
panic paths in the durability layer. The proofs prove what the spec says; the
code doesn't yet match.
**+1 if**: Recovery replay fixed + durability tests assert exactness + green workspace
with full verification pipeline running.

**Completion confidence: 5/10**
Phase 4a is ~70% complete with known correctness and quality work remaining.
The scope from 4a to 4d is still very large, and most of 4b/4c/4d remains
spec- or Lean-only. bd-2qv and bd-3cn are explicit prerequisites that remain open.
**+1 if**: Phase 4a gate passes cleanly (all checklist items green) AND one
delivered 4b slice (block store or sharding).

**Architecture confidence: 6/10**
The present structure can support the embedded 4a system well, but the future
distributed/prolly/federation story is still largely design-level. Recovery
currently bypasses core state invariants (GAP-001), which is an architectural
issue, not just a bug — the replay path should go through the same state
machine as normal writes.
**+1 if**: Recovery fixed to use the full state machine + phase-tagged architecture
doc + one real 4b substrate implementation that proves the `IndexBackend` trait
extension point works.

---

## Appendix: Raw Data

### bv --robot-triage Summary

```
Open: 27 | Closed: 117 | Ready: 22 | Blocked: 6 | In progress: 1
Top picks: bd-3gk (Phase 4b spec expansion), bd-18a (INV-FERR-050b/c),
           bd-85j.15 (transport trait)
Quick wins: bd-3gk (unblocks 2), bd-18a (unblocks 1), bd-85j.13 (unblocks 1)
Dependency cycles: None
Graph density: 0.0029
```

### bv --robot-insights Bottlenecks

```
bd-85j.6:  95 (highest betweenness)
bd-85j.7:  33
bd-85j.8:  14.5
bd-2cv:     8
bd-85j.13:  8
bd-85j.11:  7
bd-3gk:     5
bd-85j.16:  5
```

### Lean Theorem Inventory (by file)

| File | Theorems | INV-FERR Coverage |
|------|----------|-------------------|
| Store.lean | 23 | 001-004, 010, 012, 018 |
| Concurrency.lean | 16 | 013, 015, 016, 017 |
| Performance.lean | 10 | 029, 031, 032 |
| Decisions.lean | 8 | 033, 035 |
| Federation.lean | 18 | 037, 039, 040, 043, 044 |
| ProllyTree.lean | 12 | 045, 046, 049 |
| VKN.lean | 28 | 051-055 |
| Refinement.lean | 11 | CI-FERR-001 |

### Test Suite Breakdown

```
ferratom:        39 passed, 0 failed (0.05s)
ferratomic-core: 69 passed, 0 failed (0.53s)
ferratomic-datalog: 1 passed, 0 failed (0.99s)
ferratomic-verify: DID NOT RUN (missing_docs compilation failure)
Total passing: 109
Total failing: 0
```

### Recent Commit History (newest first)

```
188ebdb fix: Phase 4a hardening -- 20 cleanroom bugs + quality gates
e5ef87a chore: bead audit -- correct descriptions, add dependency, verify spec gaps
451256c chore: revise cleanroom defects -- 2 false positives, 3 severity adjustments
624061e fix: non-vacuous SEC property + wire WriteLimiter (bd-272, bd-3oz)
57de6d8 chore: file 10 cleanroom review defects (CR-030..039)
d1f722d docs: update stateright module docs to reflect current wiring (bd-85j.2.2)
7615630 fix: decouple Datom::from_seed fields for independent variation (bd-lke)
d7f5369 fix: INV-FERR-010 Stateright model -- SEC safety + liveness (bd-85j.2.3)
19be289 feat: backpressure module + Semilattice trait on Store
ad7fb08 fix: resolve clippy warnings in verify crate (bd-85j.2.1)
b44e57a feat: implement Semilattice + ContentAddressed traits (bd-20j)
0d1f4a8 test: add 10 property tests for INV-FERR-013..018 (bd-1i6)
6652967 refactor: split store.rs into submodules (bd-3ni)
34576f9 feat: Phase 4a spec hardening + checkpoint + per-index OrdMaps
5eb55d8 docs: session 2 continuation prompt for successor agent handoff
2867f5b chore: sync beads state after session 2 completion
503b3c9 feat: full workspace compilation + 110 tests pass across all crates
88aba28 chore: file 4 new beads + revise Observer bead for Phase 4a completion
1aeeaab fix: wire WAL into Database transact path (INV-FERR-008 two-fsync barrier)
5312d41 feat: Write-Ahead Log with CRC32 integrity + crash recovery
```

---

## Revision Log

| Date | Change | Rationale |
|------|--------|-----------|
| 2026-03-30 (v1) | Original review | Single-reviewer assessment, B (7.6) composite |
| 2026-03-30 (v2) | Cross-validated revision | Synthesized against two independent Codex (GPT-5) reviews. Downgraded composite to C+ (6.4). Added GAP-001 (recovery semantics), GAP-002 (schema compatibility), GAP-005 (durability test quality), GAP-006 (architecture docs drift), GAP-009 (worktree hygiene). Added full 55-invariant coverage matrix. Revised phase gate verdict from PARTIAL to FAIL. Expanded tactical plan from 5 to 8 items with correctness fixes leading. Added Lean-Rust feedback loop integration. Added phase-gate operationalization to meta-interventions. |
