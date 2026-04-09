# Ferratomic Continuation — Session 013

> Generated: 2026-04-04
> Last commit: `531bd2f` "feat: session 012 — 46 beads closed, Fsynced phase, Lean crash-recovery theorem"
> Branch: main

## Read First

1. `QUICKSTART.md` — project orientation
2. `AGENTS.md` — guidelines and constraints
3. `spec/README.md` — load only the spec modules you need
4. `docs/prompts/lifecycle/06-cleanroom-review.md` — the 9-phase review protocol (everything must pass this)

## Session 012 Summary

### Completed

**Stateright models (8 beads: bd-7fub.6.1 through 6.8):**
- INV-FERR-004/005/008/009/011/013/015/018 properties across 5 models
- NEW: schema_validation_model.rs (INV-009), hlc_model.rs (INV-015)
- crash_recovery_model: **Fsynced phase** splits FsyncWal into fsync + commit, activating the CrashAfterFsync path and fsynced:true recovery. Zero dead code. 11 BFS properties (8 safety + 3 liveness).

**Lean theorems (2 beads: bd-7fub.2.1, bd-7fub.2.10, 6 theorems, zero sorry):**
- `crash_recovery_monotone_prefix`: universal proof recover(prefix(WAL)) ⊆ recover(WAL)
- `recovery_preserves_committed`, `recovery_no_phantoms`, `recovery_idempotent_clean`
- `replica_subset_preserved`, `replica_catches_up`

**Durability tests (4 beads: bd-7fub.19.2/4/5/6):**
- Triple-crash WAL truncation, power-cut atomic rename, ENOSPC simulation, concurrent snapshot + crash

**Bug fixes (2 beads: bd-ltek, bd-9ecq):**
- DEFECT-001: debug_assert on SortedVecBackend::backend_values()
- DEFECT-005: O(n+m) merge_sort_dedup + from_sorted_canonical for mixed-repr merge

**Architecture + docs (14 beads):**
- 5 trait contract docs, API surface audit, wire/core boundary audit
- Type-level enforcement catalog, CI-FERR-002 proptest suite (refinement.rs)
- ADR-FERR-015 authored, spec drift INV-023 fixed, GOALS.md alignment verified

**CRITICALs fixed (2):**
- write_checkpoint_from_db epoch-0 → Database::store_for_checkpoint()
- CrashAfterFsync dead code → Fsynced phase with committed_count in recovery

**Other (14 beads):** Kani harnesses verified, proptest 10K cases, regression seeds, rust-toolchain, benchmarks, review doc, INV-031 Level 1, catalog drift fix

### Decisions Made

- **Crash-recovery model architecture**: Split FsyncWal into separate fsync (WAL durable) and commit (store updated) steps. The original enum variants CrashAfterFsync and Commit were designed for this — we completed the intent.
- **Lean proof strategy**: WAL recovery = fold(∪, ∅, WAL). Crash = prefix truncation. Correctness = monotone fold on prefix. One algebraic insight, 4 theorems.
- **Database::store_for_checkpoint()**: The correct API for checkpoint writing. Clones the actual Store, preserving epoch/schema/genesis_agent/LIVE metadata.

### Bugs Found

43 audit findings filed as beads from 5-agent comprehensive cleanroom review:
- 2 CRITICAL (pre-existing): bd-1o59 (INV-023 proptest forbid/deny), bd-2hlf (verify_bijection cfg gating)
- 9+ MAJOR: bd-k8i9 (merge_sort_dedup no tests), bd-dbk1 (Datom::Ord coupling), bd-w30y (power-cut WAL delta), bd-tc1g (HLC sentinel), bd-n53r (ferratom-clock in INV-023 tests), bd-v505 (coverage_by_layer), bd-ff46 (ADR-020 canonical location)
- 32 MINOR+STYLE: see `br list --status=open --label=phase-4a`

### Stopping Point

All session 012 work committed and pushed. 46 beads closed. 43 audit beads filed. Catalog drift partially fixed (INV-022/024 Kani entries updated; CI-FERR-002 proptest registration still needed).

The codebase compiles, passes clippy with zero warnings, passes all pre-commit gates including strict no-unwrap and zero #[allow]. Lean builds 759/759 with zero sorry.

## Next Execution Scope

### Primary Task

Fix the 43 audit findings from the comprehensive cleanroom review, starting with the 2 pre-existing CRITICALs:

1. **bd-1o59 (CRITICAL)**: INV-023 proptest checks `forbid(unsafe_code)` for all crates but ferratomic-core uses `deny`. Fix the proptest to match the integration test's ferratomic-core exception logic.

2. **bd-2hlf (CRITICAL)**: `verify_bijection` uses `#[cfg(any(test, debug_assertions))]` to gate the identity check. Replace with `debug_assert!` so the code stays visible to the type checker in all builds.

Then the MAJORs:
3. **bd-k8i9**: Add direct regression tests for `merge_sort_dedup` (empty inputs, full overlap, full disjoint, single element)
4. **bd-dbk1**: Add proptest verifying `OrdSet::iter()` order matches EAVT key order
5. **bd-w30y**: Add `assert_pc_entities_present(&result.database, 5, ...)` to power-cut test
6. **bd-n53r**: Add ferratom-clock to INV-023 test file lists
7. **bd-tc1g**: Document HLC initial (0,0) sentinel in init_states

### Ready Queue
```bash
br ready          # Show unblocked issues
bv --robot-next   # Top pick with reasoning
```

### Dependency Context

Gate chain: all audit beads closed → bd-7fub.22.10 (re-review at 10.0) → bd-y1w5 (tag) → bd-add (Phase 4a gate closes) → unblocks 17+ Phase 4b beads.

The remaining non-audit Phase 4a work items (MPH, LIVE checkpoint, XOR fingerprint, cuckoo filter, spec authoring) are deep engineering — each is a dedicated session.

## Hard Constraints

- `#![forbid(unsafe_code)]` in all crates except ferratomic-core (which uses `#![deny(unsafe_code)]` per ADR-FERR-020, mmap.rs sole exception)
- No `unwrap()` in production code (test code may use `expect()` with descriptive messages)
- `CARGO_TARGET_DIR=/data/cargo-target` — NOT auto-configured
- Zero lint suppressions (`#[allow(...)]`) outside mmap.rs
- Phase N+1 cannot start until Phase N passes isomorphism check
- Every fix must pass cleanroom review (lifecycle/06) before bead closure
- Do NOT close quality-score EPICs (10.0 targets) without independent progress review
- Subagents must read GOALS.md and lifecycle/05-implementation.md in full before writing code

## Stop Conditions

Stop and escalate to the user if:
- A fix requires modifying a spec invariant's Level 0 algebraic law
- A dependency edge removal would bypass a verification gate
- A test failure suggests an algebraic correctness issue (not just a test gap)
- The progress review scores below 8.0 on any vector
- Any proposed change conflicts with GOALS.md Tier 1 values (algebraic correctness, append-only durability, safety)
- Uncertainty about whether a change is correct — ask rather than guess
