# Ferratomic Continuation — Session 028

> Generated: 2026-04-11
> Last commit: `4d30e48` "docs: file 4 benchmark beads, reopen bd-p8n4n"
> Branch: main (synced with master)

## Read First

1. `QUICKSTART.md` — project orientation
2. `AGENTS.md` — guidelines, hard constraints, crate map
3. `spec/README.md` — 94 invariants, load only what you need
4. `GOALS.md` §7 — Six-Dimension Decision Framework

## Session Summary

### Completed (18 commits, 6 beads closed)

**Prolly tree modules (bd-85j.13)**:
- `read.rs`: walk root→leaves, collect key-value pairs. 7 tests.
- `read_prolly_tree_vec`: O(n) Vec-based read — **5.1x faster** than BTreeMap at 10M.
- `diff.rs`: O(d) lazy DiffIterator + `diff_exact` with phantom cancellation. 11 tests.
- `transfer.rs`: `ChunkTransfer` trait + `RecursiveTransfer`. 7 tests.
- `snapshot.rs`: `RootSet` (160-byte manifest), `create_manifest`, `resolve_manifest`. 10 tests.
- `build.rs`: `decode_internal_children`, sort-order validation, 4 new tests.
- Cleanroom reviewed twice: DEFECT-001 (sort validation), DEFECT-002 (decode tests), DEFECT-003 (multi-level diff test).
- **diff_exact Vec-sort dedup**: 143ms→40ms at 10M (3.6x faster). Profile-driven.

**Roaring bitmap (bd-qgxjl CLOSED)**:
- `BitVec<u64, Lsb0>` → `RoaringBitmap` in PositionalStore. 10-100x memory at scale.
- Checkpoint format preserved via boundary conversion. 195 tests pass across 3 crates.

**Wavelet library (bd-jolx + bd-xck9t CLOSED)**:
- qwt (`HQWT256Pfs`), sucds, vers-vecs all validated. 20 correctness tests.
- `WaveletBackend` trait with `wavelet-qwt` / `wavelet-vers` feature flags. 6 algebraic laws.

**ANS entropy codec (bd-qk1mu)**:
- `ans` crate (pure Rust, MIT) wrapped in `ferratomic-positional/src/ans.rs`. 8 tests.
- Near-optimal H(X)+ε bits/symbol compression for federation bandwidth reduction.

**Proptest (INV-FERR-046..049)**:
- 8 property-based tests at 10K cases. Found phantom diff bug (fixed).

**Scale benchmark (prolly_scale.rs)** — session's most valuable artifact:
- Measured at 1M and 10M. Identified BTreeMap as bottleneck (not tree structure).
- Led to read_prolly_tree_vec (5.1x) and diff_exact Vec-sort (3.6x).

**Performance roadmap (federation-first)**:
- Reordered: bd-6joz2 (CS+IBLT) → P0, bd-qk1mu (ANS) → P1.
- Closed 3 duplicate RADICAL beads (IBLT, mmap, CS → existing beads).
- Authored lab-grade specs for bd-55fca (fractional cascading) and bd-qk1mu (ANS).

### Decisions Made

- **Federation-first priority**: network boundary is the default case. CS+IBLT and ANS have higher ceiling than local-only optimizations.
- **vers-vecs as qwt alternative**: zero deps, `#![forbid(unsafe_code)]` on wavelet module. Feature-flag swappable.
- **diff_exact separator key design**: Option C (separate array in SuccinctTree, on-disk format unchanged). Scored 9.17 composite.
- **BP+RMM deprioritized**: scale benchmark showed tree structure is 17ms at 10M — not the bottleneck. Reopened for future evaluation.
- **Vec over BTreeMap**: for ANY sorted collection path. Profile-driven — BTreeMap's O(n log n) is the dominant cost at scale.

### Bugs Found

- **Phantom diff entries**: proptest discovered that merge_join_children produces spurious LeftOnly+RightOnly pairs when chunk boundaries shift. Fixed with diff_exact post-processing.
- **ANS hand-rolled FSE**: encode/decode state machine inversion was wrong. Fixed by using `ans` crate instead of hand-rolling.

### Stopping Point

**Exactly where I stopped**: All code committed and pushed at `4d30e48`. Scale benchmark validates all operations at 1M and 10M. ANS codec works. WaveletBackend trait works. Roaring LIVE works.

**Last thing verified working**: `cargo bench --bench prolly_scale` at 10M — all operations within expected bounds. diff_exact 40ms, read_vec 3.7s, transfer_incr 12ms.

**Next thing to do**: Start bd-6joz2 (CS+IBLT federation sync) — the P0 item. Or run the 4 new benchmark beads (memory HWM, ANS on real chunks, concurrent read/write).

## Next Execution Scope

### Primary Task

**bd-6joz2** (P0): Hybrid CS+IBLT+Fingerprint federation anti-entropy protocol. This is the highest-ceiling optimization — eliminates tree traversal entirely for federation sync. Two peers exchange ~4 KB of algebraic sketch instead of walking 660 chunks over the wire.

Prerequisites met: Transport trait exists (Phase 4a.5 done), prolly tree diff works, ANS codec available for chunk compression.

### Alternative: Benchmark-first path

Run the 4 new benchmark beads before implementing:
- bd-4d8z5: Memory high-water mark at 10M-100M
- bd-5jocb: ANS compression on real prolly chunks
- bd-m2g8v: Concurrent read throughput
- bd-1m0qg: Concurrent write throughput

The scale benchmark taught us that profiling before optimizing is the 10.0 approach.

### Ready Queue
```bash
br ready          # Show unblocked issues
bv --robot-next   # Top pick with reasoning
```

## Hard Constraints

- `#![forbid(unsafe_code)]` in all crates.
- No `unwrap()`, `expect()`, `panic!()` in production code.
- Zero `#[allow(...)]` anywhere.
- `CARGO_TARGET_DIR=/data/cargo-target`.
- `cargo fmt --all` BEFORE `git add`.
- All prolly tree files (`ferratomic-core/src/prolly/*`) are ours.
- `ans` crate is MIT, pure Rust. `roaring` is in Cargo.lock.
- The scale benchmark (`prolly_scale.rs`) is the compass — validate every optimization against it.

## Stop Conditions

Stop and escalate if:
- Scale benchmark shows regression on any operation
- ANS codec fails round-trip on real chunk data
- `cargo deny check` flags `ans` or `roaring` crate
- Any of the 64 prolly unit tests or 8 proptests fail
