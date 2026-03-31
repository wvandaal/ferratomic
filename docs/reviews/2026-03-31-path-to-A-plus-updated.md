# Updated Path to A+ (10.0) — 2026-03-31

> **Date**: 2026-03-31
> **Reviewer**: Claude Opus 4.6 (StormyCove)
> **Baseline**: 2026-03-30 path-to-A-plus.md (41 items, 18-24 sessions)
> **Current composite**: B+ (8.44). **Target**: A+ (10.0).
> **Delta**: 28 of 41 items DONE. 13 items remain. Est. 6-8 sessions.

---

## What Changed Since the Original Plan

The cleanroom audit (60 defects) and its remediation session consumed the work that
the original plan allocated to Sessions 1-10. Specifically:

- **All 4 COMP items DONE** (IndexBackend, StorageBackend, AntiEntropy, ReplicaFilter)
- **All QUAL-1 splits DONE** (store/, db/, wal/, writer/, clock/, datom/)
- **QUAL-2 function decomposition DONE** (recover, cold_start, transact all split)
- **QUAL-3 doc comments DONE** (`#![deny(missing_docs)]` enforced in both crates)
- **All 4 ERGO items DONE** (checkpoint methods, error docs, Display test, Quick Start)
- **All ARCH items DONE** (splits, API surface, datalog stub)
- **CORR-1 DONE** (Kani checkpoint harness)
- **CORR-2 DONE** (verify_bijection wired via `release_bijection_check` feature)
- **PERF-2 DONE** (baseline benchmarks recorded)
- **VDEP-1 DONE** (6 Stateright models: CRDT, crash recovery, snapshot isolation, write linearizability, transaction atomicity, backpressure)
- **132 Lean theorems with 0 sorry** (up from 106)
- **HLC wired** (HI-011), **LIVE view** (HI-013), **wire types** (CR-003/004)

---

## Remaining Items: The Updated Path

### Per-Vector Delta

| Vector | Was | Now | Gap | Items Remaining |
|--------|-----|-----|-----|-----------------|
| 1. Correctness (3x) | 8.7 | 8.8 | 1.2 | CORR-3, CORR-4 |
| 2. Completeness (2x) | 8.0 | 9.5 | 0.5 | COMP done; verify 32/32 |
| 3. Verification (2x) | 8.5 | 9.0 | 1.0 | VDEP-2, VDEP-3 |
| 4. Code Quality (1.5x) | 8.2 | 9.0 | 1.0 | QUAL-4, ferratom LOC budget |
| 5. Architecture (1.5x) | 8.6 | 9.5 | 0.5 | Near-done |
| 6. Performance (1.5x) | 6.2 | 8.0 | 2.0 | PERF-1, PERF-3 |
| 7. Durability (2x) | 8.5 | 9.0 | 1.0 | DURA-4 |
| 8. Ergonomics (0.5x) | 7.5 | 9.5 | 0.5 | Near-done |
| 9. Axiological (2x) | 9.2 | 9.5 | 0.5 | AXIO-1, AXIO-3 |
| 10. Process (1x) | 8.0 | 8.5 | 1.5 | PROC-1..4 |

---

## The 13 Remaining Items (Ordered by Impact)

### TIER 1: Highest Leverage (do these first — each lifts 2+ vectors)

#### 1. CORR-3: Add Lean proofs for remaining Phase 4a gaps [M]

**Lifts**: Correctness, Verification Depth, Axiological Alignment
**Current**: 132 theorems across 8 files, 0 sorry. Missing proofs for INV-FERR-005 (index bijection), 006 (snapshot isolation), 007 (write linearizability), 008 (WAL ordering), 009 (schema validation), 011 (observer monotonicity), 020 (transaction atomicity).
**Target**: 139+ theorems. 29/32 with Lean (019, 021, 023 justified as non-algebraic).
**Files**: `ferratomic-verify/lean/Ferratomic/Store.lean`, `Concurrency.lean`
**Effort**: M (2 sessions — 7 theorems, each is a structural property provable from existing definitions)

#### 2. PROC-1: Formal cleanroom review [M]

**Lifts**: Process Health, Code Quality, Correctness
**Prerequisite for**: PROC-2, PROC-3, PROC-4, gate closure
**What**: Run `06-cleanroom-review.md` on all Phase 4a code. Walk every public function. File defects.
**Files**: All ferratom/ and ferratomic-core/ source
**Effort**: M (1 session with subagent swarm — the 2026-03-31 cleanroom audit is a model)

#### 3. PERF-1: Add hard threshold assertions to benchmarks [M]

**Lifts**: Performance (the weakest vector at 8.0)
**What**: Add `#[test]` functions that assert INV-FERR-025..028 thresholds:
- INV-FERR-026: Write amplification < 10x
- INV-FERR-027: Read P99.99 < 10ms at 100K datoms
- INV-FERR-028: Cold start < 5s at 100K datoms
**Files**: `ferratomic-verify/integration/test_thresholds.rs` (extend existing)
**Effort**: S (thresholds exist, baselines recorded — just wire assertions)

### TIER 2: Gap Closers (each lifts 1 vector to A+)

#### 4. CORR-4: Add Kani harnesses for remaining Phase 4a gaps [M]

**Lifts**: Correctness, Verification Depth
**What**: 6 new harnesses: INV-FERR-004, 019, 021, 029, 031, 032
**Files**: `ferratomic-verify/kani/`
**Effort**: M (1 session — bounded model checking, each harness ~30-50 lines)

#### 5. VDEP-2: Fill proptest gaps for INV-FERR with <4 layers [M]

**Lifts**: Verification Depth
**What**: Add proptest strategies for: 023 (forbid unsafe meta-test), 025 (IndexBackend bijection), 024 (InMemoryBackend round-trip), 022 (NullAntiEntropy), 030 (AcceptAll filter), 026 (write amplification), 031 (genesis determinism)
**Files**: `ferratomic-verify/proptest/`
**Effort**: S-M (many are trivial — trait instantiation + round-trip)

#### 6. VDEP-3: Fill integration test gaps [M]

**Lifts**: Verification Depth
**What**: Add integration tests for: 004 (monotonic growth via Database), 022 (AntiEntropy trait), 024 (InMemoryBackend cold_start), 025 (IndexBackend trait), 029 (LIVE resolution via Database), 032 (LIVE correctness end-to-end)
**Files**: `ferratomic-verify/integration/`
**Effort**: M (1 session — each test is ~20-50 lines)

#### 7. DURA-4: Double-crash recovery integration test [S]

**Lifts**: Durability
**What**: genesis → transact 3 → checkpoint → transact 2 → crash → recover → transact 1 → crash → recover. Assert all 6 datoms present.
**Files**: `ferratomic-verify/integration/test_recovery.rs`
**Effort**: S (~30 lines)

#### 8. QUAL-4: Defect triage — close Phase 4a bugs to <5 open [S]

**Lifts**: Code Quality, Process Health
**What**: `br list --status=open --label=phase-4a --type=bug`. Close resolved. Defer Phase 4b+.
**Effort**: S (triage, not code)

### TIER 3: Polish (each gets a vector to 10.0)

#### 9. PERF-3: IndexBackend benchmark comparison [S]

**Lifts**: Performance
**What**: Criterion benchmark comparing OrdMap backend overhead vs raw OrdMap.
**Files**: `ferratomic-verify/benches/index_backend.rs`
**Effort**: S (benchmark exists, just needs assertion)

#### 10. AXIO-1: Ungrounded code audit [S]

**Lifts**: Axiological Alignment
**What**: Find modules without INV-FERR/ADR-FERR/NEG-FERR references. Add references or file spec gaps.
**Effort**: S (grep + inspect)

#### 11. AXIO-3: Lean-Rust coupling verification [S]

**Lifts**: Axiological Alignment
**What**: Verify Refinement.lean function signatures match Rust codebase.
**Effort**: S (cross-reference)

### TIER 4: Gate Closure (sequential chain, do last)

#### 12. PROC-2 + PROC-3: Defect close + full regression [M]

**Lifts**: Process Health
**What**: <5 open bugs. `cargo check + clippy + test + bench + lake build` — zero failures.
**Depends on**: PROC-1 complete, all other items complete
**Effort**: M (run suite, fix breakage)

#### 13. PROC-4: Tag and document gate closure [S]

**Lifts**: Process Health
**What**: `git tag -a v0.4.0-gate`. Close bd-add and bd-flqz. Write gate closure doc.
**Depends on**: Everything else
**Effort**: S

---

## Execution Plan (6-8 Sessions)

```
SESSION 1: Lean proofs (CORR-3)
  └── 7 new theorems: INV-FERR-005, 006, 007, 008, 009, 011, 020

SESSION 2: Threshold assertions + performance (PERF-1, PERF-3)
  ├── Hard threshold tests for INV-FERR-026, 027, 028
  └── IndexBackend benchmark assertion

SESSION 3: Kani + proptest gap fill (CORR-4, VDEP-2)
  ├── 6 new Kani harnesses
  └── 7+ new proptest strategies

SESSION 4: Integration tests + durability (VDEP-3, DURA-4)
  ├── 6+ new integration tests
  └── Double-crash recovery test

SESSION 5: Cleanroom review (PROC-1)
  └── Formal review of all Phase 4a code → file defects

SESSION 6: Defect triage + axiological audit (QUAL-4, AXIO-1, AXIO-3)
  ├── Close Phase 4a bugs to <5
  ├── Ungrounded code audit
  └── Lean-Rust coupling verification

SESSION 7: Regression + gate (PROC-2, PROC-3, PROC-4)
  ├── Full regression suite (zero failures)
  ├── Tag v0.4.0-gate
  └── Close bd-add, bd-flqz with A+ evidence
```

---

## What A+ Looks Like (Updated Target)

When complete, Phase 4a will have:

- **139+ Lean theorems** with 0 sorry (currently 132)
- **40+ proptest functions** at 10K cases (adding ~7)
- **27+ Kani harnesses** all functional (adding ~6)
- **6 Stateright models** with exhaustive exploration (DONE)
- **50+ integration tests** covering every Phase 4a INV-FERR (adding ~6)
- **32/32 Phase 4a INV-FERR** with code + tests (currently 30/32, 2 partial)
- **0 files exceeding 500 LOC** (DONE — all modules split)
- **0 functions exceeding 50 LOC** (DONE — all decomposed)
- **0 undocumented public items** (DONE — `deny(missing_docs)`)
- **<5 open defects** (needs triage)
- **All performance targets benchmarked** with hard assertions (needs PERF-1)
- **Every module traces to spec** (needs AXIO-1 audit)
- **Phase gate formally closed** with tag + A+ documentation

**Delta from current state**: 13 items across 6-8 sessions.
Down from 41 items / 18-24 sessions in the original plan.
**63% of the path is already behind us.**
