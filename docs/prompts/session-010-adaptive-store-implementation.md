# Ferratomic Continuation — Session 010: AdaptiveStore Implementation

> Generated: 2026-04-03
> Last commit: d466c80 "docs: bd-wa5p gains MphBackend trait for backend-swappable CHD"
> Branch: main

## Read First

1. `QUICKSTART.md` — project orientation
2. `AGENTS.md` — guidelines, constraints, quality standards
3. `spec/09-performance-architecture.md` — INV-FERR-070-076, ADR-FERR-020
4. **bd-h2fz bead description** (`br show bd-h2fz`) — THE implementation spec. Contains exact Rust pseudocode for every type, function, and match pattern. Translate line-for-line.
5. `ferratomic-core/src/store/mod.rs` — THE code you are modifying
6. `ferratomic-core/src/positional.rs` — PositionalStore (DO NOT MODIFY in bd-h2fz)
7. `ferratomic-core/src/store/query.rs` — Snapshot + LIVE queries
8. `ferratomic-core/src/store/merge.rs` — CRDT merge
9. `ferratomic-core/src/store/apply.rs` — transact + insert
10. `ferratomic-core/src/indexes.rs` — SortedVecBackend + Indexes type alias

## Session 009 Summary

### Completed
- Deep first-principles analysis of entire Phase 4a performance architecture
- ALL design decisions resolved — zero remaining ambiguity
- 14 lab-grade performance beads with inlined Rust pseudocode (2,597 lines total)
- 2 new alien artifacts discovered: interpolation search O(log log n), inline CHD MMPH
- Steps 3+4 merged into single AdaptiveStore bead (bd-h2fz)
- FM-Index closed as NO-GO (bd-gzjb) — BLAKE3 max entropy, 4-15x slower
- Columnar store reclassified to Phase 4b (bd-7hmv) — harmful for in-memory point lookups
- 29 phantom edges cleaned from bd-add, 3 from bd-flqz
- 3 CRITICAL dependency wiring bugs fixed (bd-a7s1, bd-83j4, bd-218b)
- Spec authoring beads filed for INV-FERR-077 (interpolation) and INV-FERR-072 extension (demotion)

### Decisions Made
- StoreRepr::Positional(Arc<PositionalStore>) — Arc for O(1) snapshot
- indexes() returns Option<&Indexes> — None for Positional variant
- from_merge always produces Positional — all 4 variant combinations handled
- V3 checkpoint: Option C (fixed header + single bincode V3Payload)
- ADR-FERR-010: Two-struct pattern (V3PayloadWrite/V3PayloadRead with WireDatom)
- mmap: no transmute, 3 documented unsafe sites, value-pool trajectory for Phase 4b
- Inline CHD for MMPH behind MphBackend trait (future crate swap = 1 line change)
- Eytzinger behind perm_layout.rs abstraction (future vEB swap = 1 file change)
- XOR fingerprint (not ECMH) — sufficient for G-Set, 1000x faster
- bd-a7s1 (rayon) depends on bd-83j4 (fingerprint) — both modify from_datoms()

### Stopping Point
Session 009 was pure design/planning — no code was written. All 14 beads contain
exact Rust pseudocode. The next session's job is to IMPLEMENT bd-h2fz by translating
the pseudocode into compiled, tested Rust.

## Next Execution Scope

### Primary Task: Implement bd-h2fz (AdaptiveStore)

This is the single highest-leverage task. It blocks 8 downstream beads.

```bash
br update bd-h2fz --status in_progress
br show bd-h2fz   # Read the FULL description — 326 lines of pseudocode
```

The bead contains EXACT pseudocode for:
- `store/iter.rs` (NEW ~80 lines): DatomIter, DatomSetView, SnapshotDatoms
- `store/mod.rs`: StoreRepr enum, Store struct, constructors, accessors, promote()
- `store/apply.rs`: insert() calls promote()
- `store/merge.rs`: from_merge 4-way variant match
- `store/query.rs`: Snapshot with SnapshotDatoms, snapshot() dispatch
- `store/tests.rs`: test update patterns
- `kani/sharding.rs`: .intersection() rewrite

Translate the pseudocode line-for-line. Do NOT make design decisions — every type,
lifetime, and match pattern is pre-decided. If the code doesn't compile as written,
FLAG the issue to the user — do not guess at a fix.

### After bd-h2fz: Tier 2 Parallelizable

Once bd-h2fz lands, these 4 beads become ready and touch DISJOINT files:
- bd-vhmc: positional.rs (OnceLock perm fields)
- bd-5zc4: store/mod.rs (SortedVecIndexes in OrdMap variant)
- bd-a2vf: checkpoint/v3.rs (NEW file)
- bd-ndok: ferratom-clock/src/lib.rs (ClockSource trait)

### Full Execution Order

```
TIER 1: bd-h2fz (blocks everything)
TIER 2: bd-vhmc | bd-5zc4 | bd-a2vf | bd-ndok (parallel, disjoint files)
TIER 3: bd-u6bq | bd-nwva | bd-83j4 | bd-218b | bd-j7qk | bd-erfj | bd-qpo7 | bd-gkln
TIER 4: bd-a7s1 (after bd-83j4) | bd-ta8c (after bd-erfj) | bd-wa5p (after bd-ta8c)
TIER 5: bd-7fub.22.10 (re-review) → bd-y1w5 (tag) → bd-add (gate) → bd-flqz (A+ gate)
```

## Hard Constraints

- `#![forbid(unsafe_code)]` in all crates (ferratomic-core changes to `#![deny]` only in bd-erfj, not now)
- No `unwrap()` or `expect()` in production code
- `CARGO_TARGET_DIR=/data/cargo-target`
- Zero `#[allow(...)]` anywhere
- Every public function references an INV-FERR in its doc comment
- All functions under 50 lines, all files under 500 LOC
- Pre-commit hook: fmt + clippy --all-targets + strict gate + zero-allow scan

## Stop Conditions

Stop and escalate to the user if:
- The pseudocode in bd-h2fz doesn't compile — do NOT guess at fixes. The pseudocode was carefully designed to resolve every type decision. A compile error means we missed something.
- Any existing test fails and the fix is non-obvious (> 5 minutes to understand)
- Any file exceeds 500 LOC after changes
- DatomIter or DatomSetView needs additional trait implementations not specified in the pseudocode
- The OrdSet → Vec<Datom> collection in from_merge produces different ordering than expected (EAVT vs Datom::Ord mismatch)
- live_causal/live_set construction from positional.datoms().iter() produces different results than from OrdSet.iter() (would indicate a sort-order discrepancy)

## Key Files

```
ferratomic-core/src/positional.rs    — PositionalStore (DO NOT MODIFY in bd-h2fz)
ferratomic-core/src/indexes.rs       — SortedVecBackend + Indexes (DO NOT MODIFY in bd-h2fz)
ferratomic-core/src/store/mod.rs     — Store struct (MODIFY: StoreRepr, constructors, accessors)
ferratomic-core/src/store/iter.rs    — NEW: DatomIter, DatomSetView, SnapshotDatoms
ferratomic-core/src/store/query.rs   — MODIFY: Snapshot, snapshot()
ferratomic-core/src/store/merge.rs   — MODIFY: from_merge 4-way match
ferratomic-core/src/store/apply.rs   — MODIFY: insert() calls promote()
ferratomic-core/src/store/tests.rs   — MODIFY: test assertion patterns
```

## Build Commands

```bash
export CARGO_TARGET_DIR=/data/cargo-target
cargo check --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings
cargo clippy --workspace --lib -- -D warnings -D clippy::unwrap_used -D clippy::expect_used -D clippy::panic
cargo fmt --all -- --check
PROPTEST_CASES=1000 cargo test --workspace
```
