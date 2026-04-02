# Ferratomic Continuation — Session 007

> Generated: 2026-04-02
> Last commit: 3c5b289 "chore: close 3 more completed beads from Phase 4a audit"
> Branch: main

## Read First

1. `QUICKSTART.md` — project orientation
2. `AGENTS.md` — guidelines and constraints
3. `spec/09-performance-architecture.md` — the spec you are implementing (INV-FERR-070-075, ADR-FERR-020)
4. `spec/01-core-invariants.md` — INV-FERR-005 (index bijection), INV-FERR-006 (snapshot isolation)
5. `spec/03-performance.md` — INV-FERR-025 (index backend interchangeability)

## Session Summary

### Completed (Session 006)
- Deep progress review: composite 8.84 → ~9.1 after fixes
- Zero lint escape hatches enforced everywhere (CLAUDE.md hard constraint, pre-commit hook)
- Kani harnesses compile under normal cargo (7 API drift bugs found and fixed)
- 6 Kani harnesses verified by CBMC (WAL ordering, 3x backpressure, 2x error)
- Kani toolchain installed (cargo-kani 0.67.0 + CBMC)
- FerraError::Io now preserves ErrorKind (struct variant with kind + message)
- 200K scale benchmarks: WA<5x PASS, P99<100us PASS in release mode
- Cold start at 200K: 89s (im::OrdMap index rebuild bottleneck — the problem we're solving)
- benches/common.rs eliminated (inlined into each bench file)
- New spec section: `spec/09-performance-architecture.md` (INV-FERR-070-075, ADR-FERR-020)
- Spec audited twice, all findings fixed (4 CRITICAL including false LIVE homomorphism, ID collision, multimap/map confusion, false iff)
- Cross-cutting longitudinal audit: 0 contradictions across full spec surface
- Lean proofs for INV-FERR-022/024/025/030 added (0 sorry)
- Invariant catalog: type_level field + layer_count() method added
- Back-references added to upstream specs (01, 02, 03)
- 12 beads closed (completed work formally verified and closed)
- Bead audit: 6 phantom edges removed, 1 duplicate closed, 1 factual error fixed

### Decisions Made
- ALL ALIEN-* performance beads are Phase 4a gate requirements — the architecture refactor happens now, not Phase 4b
- ADR-FERR-020: Localized unsafe permitted for mmap cold start (one function, one module, BLAKE3-guarded)
- INV-FERR-060-065 renumbered to 070-075 (collision with federation spec)
- Zero `#[allow(...)]` anywhere — pre-commit hook enforces this

### Bugs Found
- 7 Kani harness API drift bugs (fixed: transact/transact_test, HLC type mismatch, Store::empty, OrdSet::intersection, temporary borrow, datoms().cloned(), missing Debug derive)
- False LIVE homomorphism claim in spec (LIVE does NOT distribute over merge — fixed)
- Multimap/map semantic confusion in SortedVec spec Level 0 (fixed)
- XOR fingerprint "iff" convergence claim mathematically false (fixed to probabilistic)

### Stopping Point
All spec, audit, and bead work is done. Zero implementation of the new performance architecture exists. The next session is pure implementation, starting with SortedVecBackend.

## Next Execution Scope

### Primary Task
**Implement SortedVecBackend** (bd-1c5r) — the cache-optimal sorted-array IndexBackend that unblocks 5 downstream beads.

This is ~150 LOC in `ferratomic-core/src/indexes.rs`. The `IndexBackend` trait already exists with `backend_insert`, `backend_get`, `backend_len`, `backend_values`. You are adding a second implementation alongside the existing `im::OrdMap` implementation.

**Acceptance criteria** (from INV-FERR-071 in `spec/09-performance-architecture.md`):
1. `SortedVecBackend<K, V>` implements `IndexBackend<K, V>` for all `K: Ord + Clone + Debug, V: Clone + Debug`
2. `backend_insert` appends to unsorted buffer (O(1) amortized)
3. `backend_get` uses binary search on sorted array (O(log n), ~4 cache misses)
4. `sort()` method sorts + deduplicates (map semantics: last-write-wins for duplicate keys)
5. `backend_values` iterates contiguously (cache-optimal sequential access)
6. Proptest: `SortedVecBackend` and `OrdMap` return identical results for all operations
7. All existing tests pass unchanged
8. `cargo clippy --workspace --all-targets -- -D warnings` passes with zero warnings

**After SortedVecBackend**, the next beads in order:
- bd-h2fz: Eliminate redundant primary OrdSet (EAVT index IS the store)
- bd-erfj: ADR-FERR-020 decision (localized unsafe boundary for mmap)
- bd-bkff: Lazy OrdMap promotion (SortedVec on cold start, OrdMap on first write)
- bd-5zc4: Yoneda index fusion (1 sorted array + 3 permutation arrays)
- bd-ndok: ClockSource trait injection for Kani-compatible HLC

### Ready Queue
```bash
br ready          # Show unblocked issues
bv --robot-next   # Top pick with reasoning
```

Ready beads (no blockers):
- **bd-1c5r** [P0]: SortedVecBackend — THE critical path
- **bd-h2fz** [P0]: Eliminate redundant primary OrdSet
- **bd-erfj** [P0]: ADR-FERR-020 localized unsafe decision
- **bd-ndok** [P1]: ClockSource trait injection
- **bd-7fub.22.10** [P0]: Final re-review (run lifecycle/13 deep mode)
- **bd-y1w5** [P0]: Tag and document gate closure

### Dependency Context
```
bd-1c5r (SortedVecBackend)          <- IMPLEMENT FIRST
    |
bd-5zc4 (Yoneda fusion)            <- needs bd-1c5r
bd-bkff (Lazy OrdMap promotion)     <- needs bd-1c5r
bd-a2vf (Checkpoint V3)            <- needs bd-1c5r
bd-218b (Cuckoo filter)            <- needs bd-1c5r
    |
bd-erfj (ADR-FERR-020)             <- independent, but gates mmap
    |
bd-ta8c (mmap zero-copy)           <- needs bd-erfj + bd-1c5r
    |
bd-wa5p (MPH)                      <- needs bd-ta8c
```

## Hard Constraints

- Zero `#[allow(...)]` anywhere — pre-commit hook enforces scan
- `#![forbid(unsafe_code)]` in all crates (until ADR-FERR-020 is implemented)
- No `unwrap()` or `expect()` in production code (NEG-FERR-001)
- `CARGO_TARGET_DIR=/data/cargo-target` — always
- Phase 4b cannot start until Phase 4a passes gate at 10.0
- Every function must reference an INV-FERR in its doc comment
- All functions under 50 lines, all files under 500 LOC
- `cargo clippy --workspace --all-targets -- -D warnings` must pass

## Stop Conditions

Stop and escalate to the user if:
- SortedVecBackend would require changing the `IndexBackend` trait signature (this affects all existing code)
- The `sort()` deduplication strategy (last-write-wins) conflicts with how `Store::transact` uses indexes
- Any existing test fails after adding SortedVecBackend (indicates a behavioral difference)
- The sorted-vec `Clone` being O(n) creates issues for snapshot isolation paths you didn't anticipate
- You discover that eliminating the primary OrdSet (bd-h2fz) requires changes outside `ferratomic-core/src/store/`
- You need to add a new dependency to any Cargo.toml
