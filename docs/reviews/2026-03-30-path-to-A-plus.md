# Path to A+ (10.0) on All Vectors -- Phase 4a Gate

> **Date**: 2026-03-30
> **Reviewer**: Claude Opus 4.6 (1M context)
> **Scope**: Every action required to achieve 10.0/A+ on all 10 quality vectors
> before closing Phase 4a gate (bd-add)
>
> **Current composite**: B+ (8.3). **Target**: A+ (10.0).

---

## Overview

This document is the complete, ordered work plan. Nothing is omitted. Every item
traces to a specific rubric criterion and a specific file or INV-FERR. Items are
grouped by vector, then by effort size. Cross-vector dependencies are noted.

**Total estimated effort**: 18-24 sessions (at ~2-3 hours each).

---

## Vector 1: Correctness (8.7 -> 10.0)

**Rubric for A+**: All CRDT laws (INV-FERR-001/002/003/010) proven in Lean with 0
sorry AND proptest 10K+ AND Stateright model. No known violations. All Phase 4a
INV-FERR with code have multi-layer verification.

**Current strengths**: 001/002/003/010 have Lean + proptest + Kani + Stateright.
Zero sorry. Core algebraic foundation is already A+.

**Gaps to close**:

### CORR-1: Fix Kani checkpoint_roundtrip harness [S]

The harness in `ferratomic-verify/kani/durability.rs` calls `Store::to_checkpoint_bytes()`
and `Store::from_checkpoint_bytes()` which do not exist. The actual checkpoint API lives in
`ferratomic-core/src/checkpoint.rs` via `write_checkpoint()` and `load_checkpoint()`.

**Action**: Rewrite the Kani harness to use the real checkpoint API. The harness should:
1. Create a small Store (2-3 datoms, Kani-bounded)
2. Write checkpoint via `write_checkpoint()` to a temp buffer
3. Load via `load_checkpoint()`
4. Assert round-trip equivalence

**Files**: `ferratomic-verify/kani/durability.rs`
**Traces to**: INV-FERR-013
**Unblocks**: Full Kani suite functional (20/21 -> 21/21)

### CORR-2: Wire verify_bijection() into snapshot publish [S]

`SecondaryIndexes::verify_bijection()` is defined in `ferratomic-core/src/indexes.rs:233`
but **never called anywhere**. INV-FERR-005 (index bijection) is a core invariant that
currently has proptest + Kani + integration coverage but zero runtime enforcement.

**Action**: Call `verify_bijection()` at snapshot publication in `Database::transact()`.
Use `debug_assert!` for the hot path, but add a cfg-gated release-mode check that runs
on every Nth transaction (configurable, default N=100) as a canary.

**Files**: `ferratomic-core/src/db.rs` (transact method), `ferratomic-core/src/indexes.rs`
**Traces to**: INV-FERR-005
**Unblocks**: Type-level enforcement claim becomes real

### CORR-3: Add Lean proofs for missing Phase 4a algebraic properties [M]

Phase 4a has 32 INV-FERR. Lean currently covers 22/32. Missing Lean proofs for:

| INV-FERR | Description | Why Lean-provable |
|----------|-------------|-------------------|
| 005 | Index bijection | Provable as cardinality invariant: |A| = |EAVT| = |AEVT| = |VAET| = |AVET| after apply/merge |
| 006 | Snapshot isolation | Provable as epoch ordering: snapshot(e).datoms subset_of store(e) |
| 007 | Write linearizability | Provable as epoch strict monotonicity on commit sequence |
| 008 | WAL fsync ordering | Provable as happens-before: wal_write < snapshot_publish |
| 009 | Schema validation | Provable as predicate preservation under schema_check |
| 011 | Observer monotonicity | Provable as monotone sequence on observer epoch stream |
| 019 | Error exhaustiveness | Not algebraic -- skip (enforced by type system) |
| 020 | Transaction atomicity | Provable as epoch uniformity: all datoms in tx share epoch |
| 021 | Backpressure safety | Not algebraic -- skip (behavioral, not structural) |
| 023 | No unsafe code | Not provable in Lean -- enforced by #![forbid(unsafe_code)] |

**Action**: Add theorems for INV-FERR-005, 006, 007, 008, 009, 011, 020 to
`ferratomic-verify/lean/Ferratomic/Store.lean` or `Concurrency.lean`. These are
structural properties of the Store/Snapshot model, provable from existing definitions.

**Target**: 29/32 with Lean proofs (019, 021, 023 justified as non-algebraic).

**Files**: `ferratomic-verify/lean/Ferratomic/Store.lean`, `Concurrency.lean`
**Traces to**: INV-FERR-005, 006, 007, 008, 009, 011, 020

### CORR-4: Add Kani harnesses for remaining Phase 4a gaps [M]

Current Kani coverage: 16/32 Phase 4a INV-FERR. Missing harnesses for:

| INV-FERR | Harness needed |
|----------|---------------|
| 004 | monotonic_growth (bounded apply sequence) |
| 008 | Already has kani_inv_ferr_008_wal_fsync_ordering -- DONE |
| 010 | Already has convergence_two_replicas + convergence_under_merge -- DONE |
| 015 | Already has hlc_monotonicity -- DONE |
| 016 | Already has hlc_causality -- DONE |
| 017 | Already has shard_equivalence + shard_disjointness -- DONE |
| 019 | error_variant_coverage (verify FerraError Display for all variants) |
| 021 | backpressure_bounds (verify WriteLimiter capacity enforcement) |
| 029 | live_resolution (verify retraction removes from live view) |
| 031 | genesis_determinism (verify two genesis() calls produce equal stores) |
| 032 | live_correctness (verify live query matches expected after assert+retract) |

**Action**: Add 6 new Kani harnesses: 004, 019, 021, 029, 031, 032.
**Target**: 22/32 Kani coverage.

**Files**: `ferratomic-verify/kani/crdt_laws.rs`, `ferratomic-verify/kani/durability.rs`, new files as needed
**Traces to**: INV-FERR-004, 019, 021, 029, 031, 032

---

## Vector 2: Completeness (8.0 -> 10.0)

**Rubric for A+**: >90% of INV-FERR in the assessed phase have code + tests.
No untracked gaps.

**Current state**: 28/32 Phase 4a INV-FERR have code + tests = 87.5%. Need 30/32 (93.75%) minimum.

**4 deferred INV-FERR**: 022 (anti-entropy), 024 (substrate), 025 (index backend),
030 (read replica). These have **zero code**.

**Strategy**: For A+ we don't need to fully implement all 4 -- they are legitimately
future-phase infrastructure. But we need to demonstrate that Phase 4a provides the
**foundation** for them. Specifically:

### COMP-1: Implement IndexBackend trait for INV-FERR-025 [M]

The spec defines `IndexBackend` as a trait (spec/03-performance.md Level 2). The
implementation hardcodes `im::OrdMap`. For A+, the trait must exist even if there's
only one implementation.

**Action**:
1. Define `IndexBackend` trait in `ferratomic-core/src/indexes.rs` with the operations
   used by `SecondaryIndexes` (insert, get, range, len)
2. Implement it for `im::OrdMap` (the current concrete type)
3. Make `SecondaryIndexes` generic over `B: IndexBackend`
4. Default to `im::OrdMap` via type alias
5. Add proptest: instantiate with `im::OrdMap` backend, run bijection test
6. Add integration test: verify the trait exists and is implementable

**Files**: `ferratomic-core/src/indexes.rs`, `ferratomic-verify/proptest/index_properties.rs`
**Traces to**: INV-FERR-025, ADR-FERR-001

### COMP-2: Implement StorageBackend trait for INV-FERR-024 [M]

Substrate agnosticism means the storage layer is trait-abstracted. Currently
`storage.rs` has `cold_start()` hardcoded to filesystem.

**Action**:
1. Define `StorageBackend` trait in `ferratomic-core/src/storage.rs` with operations:
   `write_checkpoint`, `load_checkpoint`, `open_wal`, `recover_wal`
2. Implement for `FsBackend` (the current filesystem path)
3. Make `cold_start()` generic over `B: StorageBackend`
4. Add a trivial `InMemoryBackend` for testing
5. Add integration test using `InMemoryBackend`

**Files**: `ferratomic-core/src/storage.rs`, `ferratomic-core/src/checkpoint.rs`,
`ferratomic-core/src/wal.rs`
**Traces to**: INV-FERR-024

### COMP-3: Define AntiEntropy trait for INV-FERR-022 [S]

Anti-entropy is Phase 4c implementation, but the trait boundary belongs to Phase 4a
because it defines how stores synchronize.

**Action**:
1. Define `AntiEntropy` trait in a new `ferratomic-core/src/anti_entropy.rs`:
   `fn diff(local: &Store, remote_root: RootHash) -> DiffSet`
   `fn apply_diff(store: &mut Store, diff: DiffSet) -> Result<()>`
2. Provide a `NullAntiEntropy` implementation (returns empty diff, no-op apply)
3. Document with INV-FERR-022 reference: "Implementations must guarantee eventual convergence"
4. Add unit test verifying the null implementation compiles

**Files**: `ferratomic-core/src/anti_entropy.rs`, `ferratomic-core/src/lib.rs`
**Traces to**: INV-FERR-022

### COMP-4: Define ReplicaSubset trait for INV-FERR-030 [S]

Read replicas need a subset selection interface.

**Action**:
1. Define `ReplicaFilter` trait in `ferratomic-core/src/topology.rs`:
   `fn accepts(&self, datom: &Datom) -> bool`
2. Provide `AcceptAll` filter (current behavior)
3. Document with INV-FERR-030 reference
4. Add unit test

**Files**: `ferratomic-core/src/topology.rs`
**Traces to**: INV-FERR-030

**After COMP-1..4**: 32/32 Phase 4a INV-FERR have at least trait + test = 100%.

---

## Vector 3: Verification Depth (8.5 -> 10.0)

**Rubric for A+**: Current-phase INV-FERR have 4+ verification layers populated.

**Current state**: Core CRDT (001-004, 010, 012) have 4+ layers. Many others have
2-3. Some have 1.

**Target**: Every Phase 4a INV-FERR with implementation has 4+ layers from:
Lean, proptest, Kani, Stateright, integration, type-level.

### VDEP-1: Add 4 Stateright models [L]

**Priority order** (from Stateright audit):

1. **`snapshot_isolation_model.rs`** (INV-FERR-006)
   - Model: Store with epoch counter, snapshot queue, write queue
   - Actions: Read/capture snapshot, commit transaction, concurrent observers
   - Properties: Readers at epoch e see no datoms from epoch e' > e
   - Bounded domain: 3 epochs, 2-3 readers, 2-3 writes, 2-3 datoms/tx
   - Est: ~400 lines

2. **`write_linearizability_model.rs`** (INV-FERR-007)
   - Model: Write serialization, epoch assignment, crash points
   - Actions: BeginWrite, AcquireLock, AssignEpoch, FsyncWal, ReleaseLock, Crash
   - Properties: Epochs strictly monotonic; recovery never regresses epoch
   - Bounded domain: 2-3 writes, 2-3 epochs, 2 crash modes
   - Est: ~350 lines

3. **`transaction_atomicity_model.rs`** (INV-FERR-020)
   - Model: Multi-datom transactions, visibility, crash recovery
   - Actions: StartTx, CommitUnderLock, AdvanceEpoch, GetSnapshot, Crash
   - Properties: All datoms in tx share epoch; tx is all-or-nothing visible
   - Est: ~450 lines

4. **`backpressure_model.rs`** (INV-FERR-021)
   - Model: Write queue (bounded capacity), rejection, saturation
   - Actions: Submit, Pop, Process, Checkpoint (slows queue), BackpressureError
   - Properties: Queue full -> error (not drop/OOM); no data loss
   - Est: ~400 lines

**Files**: `ferratomic-verify/stateright/` (4 new files)
**Traces to**: INV-FERR-006, 007, 020, 021

### VDEP-2: Fill proptest gaps for INV-FERR with <3 layers [M]

After Stateright models, check coverage matrix for remaining gaps. Add proptest
strategies for any Phase 4a INV-FERR that has <4 layers:

| INV-FERR | Current layers | Needs |
|----------|---------------|-------|
| 023 | 1 (type-level only) | Add proptest: verify `#![forbid(unsafe_code)]` is present in all crate lib.rs files (meta-test) |
| 025 | 0 (after COMP-1, trait exists) | Add proptest: instantiate IndexBackend<OrdMap>, run bijection |
| 024 | 0 (after COMP-2, trait exists) | Add proptest: cold_start with InMemoryBackend round-trip |
| 022 | 0 (after COMP-3, trait exists) | Add proptest: NullAntiEntropy round-trip (trivial) |
| 030 | 0 (after COMP-4, trait exists) | Add proptest: AcceptAll filter passes all datoms |
| 026 | 1 (benchmark scaffold) | Add proptest: write N datoms, measure WAL bytes / logical bytes < 10x |
| 027 | 1 (benchmark scaffold) | Add proptest: lookup latency assertion (wall clock < threshold) |
| 028 | 1 (benchmark scaffold) | Add proptest: cold_start time < threshold |
| 031 | 2 (Lean + type-level) | Add proptest: two genesis() calls produce equal stores |

**Files**: `ferratomic-verify/proptest/` (multiple files)

### VDEP-3: Fill integration test gaps [M]

After all the above, add integration tests for any Phase 4a INV-FERR missing them.
Target: every INV-FERR has at least one integration test that exercises the real
`Database` API end-to-end.

Current integration test gap list:
| INV-FERR | Needs |
|----------|-------|
| 004 | test_inv_ferr_004_monotonic_growth_database (transact 3x, assert store size only grows) |
| 022 | test_inv_ferr_022_anti_entropy_trait (NullAntiEntropy compiles, diff returns empty) |
| 024 | test_inv_ferr_024_in_memory_backend (cold_start with InMemoryBackend) |
| 025 | test_inv_ferr_025_index_backend_trait (OrdMapBackend satisfies IndexBackend) |
| 026 | test_inv_ferr_026_write_amplification (100 txns, assert WA < 10x) |
| 027 | test_inv_ferr_027_read_latency (10K datoms, assert EAVT lookup < 1ms) |
| 028 | test_inv_ferr_028_cold_start_time (1K datoms, assert cold_start < 100ms) |
| 029 | test_inv_ferr_029_live_resolution_database (assert + retract, verify live view) |
| 030 | test_inv_ferr_030_replica_filter (AcceptAll + namespace filter) |
| 031 | test_inv_ferr_031_genesis_determinism (two genesis, byte-equal) |
| 032 | test_inv_ferr_032_live_correctness_database (end-to-end live query) |

**Files**: `ferratomic-verify/integration/`

---

## Vector 4: Code Quality (8.2 -> 10.0)

**Rubric for A+**: All hard limits met (500 LOC/file, 50 LOC/fn, complexity 10,
5 params). `#![forbid(unsafe_code)]` in all crates. No `unwrap()` in production
code. <5 open defects.

**Current violations**:

### QUAL-1: Split 6 files exceeding 500 LOC [L]

| File | Current LOC | Action |
|------|-------------|--------|
| `ferratomic-core/src/store.rs` | 1003 | Split into `store/mod.rs` (Store struct + core methods), `store/apply.rs` (apply_datoms, apply_tx), `store/query.rs` (snapshot, indexes), `store/merge.rs` (merge logic if not already separate) |
| `ferratomic-core/src/db.rs` | 724 | Split into `db/mod.rs` (Database struct + lifecycle), `db/transact.rs` (transact + commit), `db/observe.rs` (observer management) |
| `ferratomic-core/src/wal.rs` | 629 | Split into `wal/mod.rs` (Wal struct + open/close), `wal/writer.rs` (append, fsync), `wal/recover.rs` (recovery logic -- the 86-line function lives here) |
| `ferratomic-core/src/writer.rs` | 618 | Split into `writer/mod.rs` (Transaction struct + typestate), `writer/validate.rs` (schema validation, value_matches_type), `writer/commit.rs` (commit logic) |
| `ferratom/src/clock.rs` | 571 | Split into `clock/mod.rs` (HybridClock + AgentId), `clock/frontier.rs` (Frontier struct + merge), `clock/txid.rs` (TxId struct) |
| `ferratom/src/datom.rs` | 567 | Split into `datom/mod.rs` (Datom struct + accessors), `datom/entity.rs` (EntityId + content addressing), `datom/value.rs` (Value enum + NonNanFloat) |

**Each split must**:
- Preserve all public API (re-export from mod.rs)
- Maintain `#![deny(missing_docs)]` in each submodule
- Not change any logic -- pure refactor
- Keep tests with their code or move to dedicated test files

**Files**: 6 files -> ~18 files
**Traces to**: AGENTS.md hard limit: 500 LOC/file

### QUAL-2: Decompose 3 functions exceeding 50 LOC [M]

| Function | File | Lines | Action |
|----------|------|-------|--------|
| `Wal::recover()` | wal.rs:241-326 | 86 | Extract `parse_frame()` helper (~30 lines), `validate_frame()` helper (~20 lines). Recovery loop becomes: read -> parse -> validate -> append. Remove `#[allow(clippy::too_many_lines)]`. |
| `Database::transact()` | db.rs:357-417 | 61 | Extract `publish_snapshot()` helper (ArcSwap update + observer notification). Transact becomes: validate -> apply -> wal_write -> publish. |
| `cold_start()` | storage.rs:90-143 | 54 | Extract `try_checkpoint_plus_wal()`, `try_checkpoint_only()`, `try_wal_only()` as separate fns. The cascade becomes a chain of fallible attempts. |

**Files**: `ferratomic-core/src/wal.rs`, `db.rs`, `storage.rs`
**Traces to**: AGENTS.md hard limit: 50 LOC/fn

### QUAL-3: Document all 115 undocumented public items [L]

The `#![deny(missing_docs)]` lint does not catch methods inside impl blocks. 64% of
public items (115/181) lack doc comments.

**Breakdown by priority**:

**Tier 1 -- Most-used APIs (document first)**:
| File | Undocumented items | Count |
|------|-------------------|-------|
| `ferratomic-core/src/store.rs` | Store constructors, query methods, index accessors | 16 |
| `ferratomic-core/src/db.rs` | Database constructor, transact, snapshot | 14 |
| `ferratomic-core/src/indexes.rs` | EAVT/AEVT/VAET/AVET constructors + accessors | 13 |
| `ferratomic-core/src/writer.rs` | Transaction building + committing | 12 |

**Tier 2 -- Core types**:
| File | Undocumented items | Count |
|------|-------------------|-------|
| `ferratom/src/clock.rs` | AgentId, TxId, HybridClock, Frontier | 18 |
| `ferratom/src/datom.rs` | EntityId, Attribute, NonNanFloat, Datom | 13 |
| `ferratom/src/schema.rs` | AttributeDef, Schema | 13 |

**Tier 3 -- Infrastructure**:
| File | Undocumented items | Count |
|------|-------------------|-------|
| `ferratomic-core/src/wal.rs` | Wal creation, recovery, durability | 10 |
| Remaining files | Assorted | 6 |

**Every doc comment must**:
1. State the invariant, not the implementation
2. Reference INV-FERR where applicable (already 89-94% of documented items do this)
3. Follow format: `/// Brief description. INV-FERR-NNN: what this upholds.`

**Files**: All .rs files in ferratom/src/ and ferratomic-core/src/
**Traces to**: AGENTS.md: "Every public item has a doc comment"

### QUAL-4: Reduce open defects below 5 [M]

Current open issues: 37. For A+ Code Quality, need <5 open **defects** (bugs).
The 37 open issues include tasks, epics, and bugs.

**Action**: Triage all open bugs. Close those that are Phase 4b+ scoped. Ensure
remaining Phase 4a bugs are fewer than 5.

**Files**: `.beads/issues.jsonl`

---

## Vector 5: Architecture (8.6 -> 10.0)

**Rubric for A+**: Crate DAG acyclic. LOC budgets met. Every module has one concept.
No God modules. Public API surface is minimal.

**Current state**: Already strong. DAG acyclic, budgets met.

### ARCH-1: Eliminate God modules via QUAL-1 splits [dependency: QUAL-1]

`store.rs` at 1003 LOC is a God module (store + apply + query + index management).
After QUAL-1 splits, every module has exactly one concept.

### ARCH-2: Minimize public API surface [S]

After QUAL-1 splits, audit re-exports in each `mod.rs`. Only re-export types that
external consumers need. Internal helpers should be `pub(crate)`.

**Action**: For each new mod.rs:
1. List all `pub` items
2. Demote to `pub(crate)` anything not used outside the crate
3. Re-export through `lib.rs` only the minimal public surface

**Files**: All new mod.rs files from QUAL-1

### ARCH-3: Ensure ferratomic-datalog stub has correct architecture [S]

At 26 LOC, ferratomic-datalog is a stub. But its module structure should reflect
the target architecture even now:
- `parser.rs` -- Datalog query parser
- `planner.rs` -- Query planner
- `evaluator.rs` -- Query evaluator

Verify these exist and have correct module-level doc comments referencing
their INV-FERR targets.

**Files**: `ferratomic-datalog/src/`

---

## Vector 6: Performance (6.2 -> 10.0)

**Rubric for A+**: All measurable INV-FERR-025..028 targets benchmarked and met.

**Critical correction**: Benchmark infrastructure DOES exist in
`ferratomic-verify/benches/` with Criterion. 5 benchmark harnesses:
`cold_start.rs`, `merge_throughput.rs`, `read_latency.rs`, `snapshot_creation.rs`,
`write_amplification.rs`. All properly configured in Cargo.toml.

**The gap is not infrastructure but hard assertions and execution.**

### PERF-1: Add hard threshold assertions to benchmarks [M]

Each benchmark currently measures but does not assert against spec targets:

| Benchmark | INV-FERR | Current | Needed |
|-----------|----------|---------|--------|
| `write_amplification.rs` | 026 | Logs warning at 10x WA | Assert WA < spec target. Add a dedicated assertion test (not just criterion measurement). |
| `read_latency.rs` | 027 | Measures throughput | Add P99.99 latency tracking (Criterion supports custom measurements). Assert < 10ms at 100K datoms. |
| `cold_start.rs` | 028 | Measures recovery time | Assert recovery < 5s for 100K datoms. |
| `merge_throughput.rs` | 001 | Measures merge throughput | Document baseline. No spec target to assert against -- this is a regression guard. |
| `snapshot_creation.rs` | 006 | Measures snapshot creation | Document baseline under contention. |

**Action**: For each benchmark:
1. Add a `#[test]` function that calls the same setup and asserts the threshold
2. This test is separate from the criterion benchmark (which measures, not asserts)
3. Gate: if threshold test fails, it's a build failure

**Files**: `ferratomic-verify/benches/*.rs`, new test file for threshold assertions

### PERF-2: Run benchmarks and record baselines [S]

**Action**:
```bash
export CARGO_TARGET_DIR=/data/cargo-target
cargo bench --package ferratomic-verify
```

Record baseline numbers in `docs/reviews/baseline-benchmarks.md`. These become the
regression floor for future phases.

### PERF-3: Add IndexBackend benchmark comparison [S] [dependency: COMP-1]

After COMP-1 adds the IndexBackend trait, add a criterion benchmark that compares
`OrdMapBackend` against a hypothetical `BTreeMapBackend` to validate that the trait
abstraction adds no overhead.

**Files**: `ferratomic-verify/benches/index_backend.rs`
**Traces to**: INV-FERR-025

---

## Vector 7: Durability (8.5 -> 10.0)

**Rubric for A+**: WAL fsync ordering (INV-FERR-008) verified. Checkpoint round-trip
(INV-FERR-013) proven. Recovery correctness (INV-FERR-014) tested. Cold start
cascade (storage.rs) implemented.

**Current state**: All items implemented and tested. Gaps are verification depth.

### DURA-1: Fix Kani checkpoint harness [same as CORR-1]

Cross-reference: CORR-1 fixes the Kani harness for INV-FERR-013.

### DURA-2: Add Stateright crash recovery coverage for WAL [dependency: VDEP-1]

The existing `crash_recovery_model.rs` covers INV-FERR-014. Extend it or create
a companion model that specifically tests INV-FERR-008 (WAL fsync ordering):
- Crash between WAL write and fsync -> recovery must not include unfsynced entry
- Crash after fsync but before snapshot publish -> recovery must include entry

**Files**: `ferratomic-verify/stateright/crash_recovery_model.rs`
**Traces to**: INV-FERR-008

### DURA-3: Lean proof for WAL ordering [dependency: CORR-3]

Cross-reference: CORR-3 adds Lean proof for INV-FERR-008.

### DURA-4: Integration test for double-crash recovery [S]

Test scenario: genesis -> transact 3 -> checkpoint -> transact 2 -> crash ->
recover -> transact 1 -> crash -> recover. Assert all 6 datoms present.

**Files**: `ferratomic-verify/integration/test_recovery.rs`
**Traces to**: INV-FERR-014

---

## Vector 8: Ergonomics (7.5 -> 10.0)

**Rubric for A+**: Typestate enforced for all lifecycles. Errors are actionable
(caller can match on category). API surface is minimal and intuitive.

### ERGO-1: Unify checkpoint API surface [M]

The Kani harness assumed `Store::to_checkpoint_bytes()` / `Store::from_checkpoint_bytes()`.
This is actually the more ergonomic API. The current split between `checkpoint.rs`
functions and Store is confusing.

**Action**: Add convenience methods to Store:
```rust
impl Store {
    pub fn to_checkpoint(&self) -> CheckpointData { ... }
    pub fn from_checkpoint(data: &CheckpointData) -> Result<Self> { ... }
}
```
These delegate to `checkpoint::write_checkpoint()` / `checkpoint::load_checkpoint()`.
The free functions remain for flexibility; the Store methods are the ergonomic surface.

**Files**: `ferratomic-core/src/store.rs`, `ferratomic-core/src/checkpoint.rs`
**Traces to**: INV-FERR-013

### ERGO-2: Document all error variants with recovery guidance [M]

`FerraError` is exhaustive, but do error variants tell the caller what to DO?

**Action**: For each FerraError variant, ensure the doc comment includes:
- **What happened** (brief)
- **Who is at fault** (caller bug, infrastructure, our bug)
- **What to do** (retry, fix input, file a bug)

Example:
```rust
/// IO error during storage operation.
///
/// **Cause**: Infrastructure (disk, network).
/// **Recovery**: Retry with backoff. If persistent, check disk health.
/// INV-FERR-008: WAL write failure means transaction was NOT committed.
Io(std::io::Error),
```

**Files**: `ferratom/src/error.rs`
**Traces to**: INV-FERR-019

### ERGO-3: Add Display impl quality check [S]

Verify every FerraError variant produces a human-readable Display message that
includes the INV-FERR context. Add a test that formats each variant and asserts
the message is non-empty and contains the error category.

**Files**: `ferratom/src/error.rs`, test file

### ERGO-4: Document API usage patterns [S]

Add a module-level doc example in `ferratomic-core/src/lib.rs` showing the
canonical usage pattern:
```rust
//! # Quick Start
//! ```
//! let db = Database::open(path)?;
//! let tx = Transaction::new();
//! tx.assert_datom(entity, attr, value);
//! db.transact(tx)?;
//! let snap = db.snapshot();
//! ```

This makes the API self-documenting. Verify the example compiles via `cargo test --doc`.

**Files**: `ferratomic-core/src/lib.rs`

---

## Vector 9: Axiological Alignment (9.2 -> 10.0)

**Rubric for A+**: Every module traces to a named INV-FERR, ADR-FERR, or constraint.
No speculative code. No features without spec grounding.

**Current state**: Already near-A+. The 0.8 gap is:

### AXIO-1: Audit for ungrounded code [S]

Search for any module, struct, function, or trait that does NOT trace to an INV-FERR,
ADR-FERR, or NEG-FERR. If found, either:
1. Add the INV-FERR reference (if it maps to a spec invariant)
2. Remove the code (if it's speculative)
3. File a spec gap (if the code is necessary but the spec is incomplete)

**Action**: `grep -rL "INV-FERR\|ADR-FERR\|NEG-FERR" ferratomic-core/src/*.rs` to
find files without any spec reference. Investigate each.

### AXIO-2: Ensure new traits (COMP-1..4) have spec grounding [dependency: COMP-1..4]

The new traits from COMP-1..4 (IndexBackend, StorageBackend, AntiEntropy, ReplicaFilter)
must each cite their INV-FERR in the trait doc comment and in the module-level docs.

### AXIO-3: Verify Lean-Rust coupling invariant coverage [S]

`Refinement.lean` proves CI-FERR-001 (coupling invariant). Verify that every
function cited in the coupling invariant exists in the Rust codebase and that the
Lean model's function signatures match the Rust signatures.

**Files**: `ferratomic-verify/lean/Ferratomic/Refinement.lean`, Rust source

---

## Vector 10: Process Health (8.0 -> 10.0)

**Rubric for A+**: Phase gates respected. Defects tracked in beads with dependency
edges. Cleanroom reviews performed. Steady commit velocity.

### PROC-1: Perform formal cleanroom review [M]

Before closing the gate, perform a cleanroom review (prompt 06-cleanroom-review.md)
of all Phase 4a code. This review must:
1. Walk every public function in ferratom + ferratomic-core
2. Verify each upholds its cited INV-FERR
3. File defects for any discrepancies
4. Close defects before gate closure

**Prompt**: 06-cleanroom-review.md

### PROC-2: Close all Phase 4a bugs [M]

After cleanroom review, triage all remaining Phase 4a bugs to <5 open.
Close resolved items. Defer future-phase items with explicit labels.

### PROC-3: Run full regression suite [S]

```bash
export CARGO_TARGET_DIR=/data/cargo-target
cargo check --workspace
cargo clippy --workspace -- -D warnings
cargo test --workspace
cargo bench --package ferratomic-verify
cd ferratomic-verify/lean && lake build
```

All must pass with zero warnings, zero failures, zero sorry.

### PROC-4: Tag and document gate closure [S]

```bash
git tag -a v0.4.0-gate -m "Phase 4a gate: A+ on all 10 vectors"
br close bd-add --reason "Phase 4a gate closed: composite 10.0/A+"
```

Write gate closure document citing composite score, each vector's score, and
the evidence trail.

---

## Execution Order (Critical Path)

Dependencies flow top-to-bottom. Items at the same level can be parallelized.

```
SESSION 1-2: Foundation (no dependencies)
├── CORR-1: Fix Kani checkpoint harness [S]
├── CORR-2: Wire verify_bijection() [S]
├── QUAL-3 Tier 1: Document Store, Database, Indexes, Writer [M]
└── PERF-2: Run benchmarks, record baselines [S]

SESSION 3-5: Completeness traits
├── COMP-1: IndexBackend trait [M]
├── COMP-2: StorageBackend trait [M]
├── COMP-3: AntiEntropy trait [S]
└── COMP-4: ReplicaFilter trait [S]

SESSION 6-8: File splits (can parallelize across files)
├── QUAL-1a: Split store.rs [M]
├── QUAL-1b: Split db.rs [M]
├── QUAL-1c: Split wal.rs [S]
├── QUAL-1d: Split writer.rs [S]
├── QUAL-1e: Split clock.rs [S]
├── QUAL-1f: Split datom.rs [S]
└── QUAL-2: Decompose 3 long functions [M]

SESSION 9-10: Documentation
├── QUAL-3 Tier 2: Document clock, datom, schema [M]
├── QUAL-3 Tier 3: Document wal, remaining [S]
├── ERGO-2: Error variant recovery guidance [M]
└── ERGO-4: API usage example in lib.rs [S]

SESSION 11-13: Lean proofs
└── CORR-3: Add Lean proofs for 005, 006, 007, 008, 009, 011, 020 [L]

SESSION 14-16: Stateright models
├── VDEP-1a: snapshot_isolation_model.rs [M]
├── VDEP-1b: write_linearizability_model.rs [M]
├── VDEP-1c: transaction_atomicity_model.rs [M]
└── VDEP-1d: backpressure_model.rs [M]

SESSION 17-18: Kani + proptest + integration gap fill
├── CORR-4: 6 new Kani harnesses [M]
├── VDEP-2: Proptest gap fill (9 INV-FERR) [M]
├── VDEP-3: Integration test gap fill (11 INV-FERR) [M]
└── DURA-4: Double-crash recovery integration test [S]

SESSION 19-20: Performance hardening
├── PERF-1: Hard threshold assertions in benchmarks [M]
├── PERF-3: IndexBackend benchmark comparison [S]
└── ERGO-1: Unify checkpoint API on Store [M]

SESSION 21-22: Audit + review
├── AXIO-1: Ungrounded code audit [S]
├── AXIO-2: Verify new trait spec grounding [S]
├── AXIO-3: Lean-Rust coupling verification [S]
├── ARCH-2: Minimize public API surface [S]
└── PROC-1: Formal cleanroom review [M]

SESSION 23-24: Gate closure
├── PROC-2: Close all Phase 4a bugs (<5 open) [M]
├── QUAL-4: Defect triage [S]
├── PROC-3: Full regression suite [S]
└── PROC-4: Tag and document gate closure [S]
```

---

## Summary: Item Count by Vector

| Vector | Current | Target | Items | Est. Sessions |
|--------|---------|--------|-------|---------------|
| Correctness | 8.7 | 10.0 | CORR-1..4 | 3-4 |
| Completeness | 8.0 | 10.0 | COMP-1..4 | 2-3 |
| Verification Depth | 8.5 | 10.0 | VDEP-1..3 | 4-5 |
| Code Quality | 8.2 | 10.0 | QUAL-1..4 | 4-5 |
| Architecture | 8.6 | 10.0 | ARCH-1..3 | 1 (rides QUAL-1) |
| Performance | 6.2 | 10.0 | PERF-1..3 | 2 |
| Durability | 8.5 | 10.0 | DURA-1..4 | 1-2 (rides CORR, VDEP) |
| Ergonomics | 7.5 | 10.0 | ERGO-1..4 | 2 |
| Axiological | 9.2 | 10.0 | AXIO-1..3 | 1 |
| Process | 8.0 | 10.0 | PROC-1..4 | 2-3 |
| **TOTAL** | **8.3** | **10.0** | **41 items** | **18-24** |

---

## What 10.0 Looks Like

When every item above is complete, Phase 4a will have:

- **106+ Lean theorems** with 0 sorry (currently 106, adding ~7 more for Phase 4a gaps)
- **37+ proptest functions** at 10K cases (adding ~9 more)
- **27+ Kani harnesses** all functional (currently 21, adding 6)
- **6 Stateright models** with exhaustive state-space exploration (currently 2, adding 4)
- **46+ integration tests** covering every Phase 4a INV-FERR (currently 35, adding 11)
- **32/32 Phase 4a INV-FERR** with code + tests (currently 28/32)
- **0 files exceeding 500 LOC** (currently 6)
- **0 functions exceeding 50 LOC** (currently 3)
- **0 undocumented public items** (currently 115)
- **<5 open defects** (currently ~20 Phase 4a bugs)
- **All performance targets benchmarked** with hard assertions
- **Every module traces to spec** with zero ungrounded code
- **Phase gate formally closed** with tag + documentation

This is the zero-defect cleanroom standard. No shortcuts. No technical debt.
Every type encodes an invariant. Every function proves a property.
