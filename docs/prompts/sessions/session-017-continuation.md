# Ferratomic Continuation -- Session 017

> Generated: 2026-04-07
> Last commit: `415694c` "test: write all 12 missing MINOR test defects for real"
> Branch: main

## Read First

1. `QUICKSTART.md` -- project orientation
2. `AGENTS.md` -- guidelines and constraints
3. `spec/README.md` -- load only the spec modules you need

## Session Summary

### Completed

**Performance EPIC (bd-4i6u) — 20/20 beads CLOSED:**
- bd-0zfw: ChunkFingerprints (INV-FERR-079) — O(delta) federation reconciliation
- bd-886d: Splice transact — O(N+K) merge-sort bypass of promote/demote
- bd-ip22: Invariant catalog 61→70 entries (INV-FERR-070-077, 084)
- bd-eusk: WalDedupBloom (INV-FERR-084) — 64KB, k=4, ~8% FP
- bd-86ap: Zero-clone V3+LIVE-first checkpoint serialization
- bd-t84f: SuccinctBitVec — O(1) rank, O(log n) select
- bd-fnod: AttributeId(u16) + AttributeIntern — sorted ID assignment
- bd-iltk: u128 widened XOR fingerprint (16x throughput)
- bd-ks5d: batch_splice_transact + Database::batch_transact
- bd-ewma: AdjacencyIndex — forward/reverse Ref-edge graph, LIVE-filtered
- bd-wows: StoreSketch MinHash — O(delta) federation diff estimation
- bd-574c: SoA columnar (col_entities, col_txids, col_ops) — lazy OnceLock
- bd-nq6v: rebuild_live_incremental — Phase 4a fallback, Phase 4b infrastructure
- bd-3ta0: perm_txid — 5th Eytzinger permutation, deterministic TxId sort
- bd-mdfq: EntityRle — RLE entity column, O(log G) group_of
- bd-m7te: V4 columnar checkpoint (CHK4), per-column serialization

**Phase 4a gate wiring:**
- bd-add now has 35 dependencies (all Phase 4a beads block the gate)
- bd-flqz reversed from downstream to upstream
- bd-k5bv relabeled phase-4a5 (cycle fix)

**3 cleanroom audit rounds — 72 defects found and fixed (ALL with real code):**
- Round 1: 6 beads audited, 23 defects (3 CRITICAL), all fixed
- Round 2: 7 beads audited, 24 defects (2 CRITICAL), all fixed
- Round 3: 5 beads audited, 25 defects (1 CRITICAL), all fixed

### Decisions Made
- Performance EPIC uses additive SoA columns (not full AoS→SoA rewrite) — canonical Vec<Datom> preserved
- Bloom filter k=4 (optimal for 64KB/100K), not k=7 (audit found math error)
- AdjacencyIndex filters by LIVE bitvector (audit found stale-edge bug)
- perm_txid uses canonical position as stable tiebreaker (audit found nondeterminism)
- build_col_attrs returns Vec<Option<AttributeId>> (audit found filter_map dropping entries)
- rebuild_live_incremental always falls back to full rebuild in Phase 4a (position shift complexity deferred)
- V4 checkpoint uses bincode for all columns (entropy codecs deferred to Phase 4b)
- NEVER close beads without actual code fix (integrity failure caught and corrected)

### Bugs Found
- 6 CRITICALs across 3 audit rounds (all fixed: div-by-zero, Bloom math, Bloom test, adjacency stale edges, build_col_attrs isomorphism break, etc.)
- 21 MAJORs (all fixed: missing dedup, WAL atomicity docs, formula errors, nondeterminism, etc.)

### Stopping Point
All performance beads complete. All audit defects fixed with real code. 183 tests verified passing across ferratom/positional/store crates. Phase 4a gate has 33 remaining blockers — mostly standalone quality/testing/docs tasks + the bd-7fub quality EPIC tree.

## Next Execution Scope

### Primary Task
**Close the 28 standalone Phase 4a gate blockers.** These are mostly small tasks:
- ~10 bugs (bd-dra3, bd-5hnx, bd-67yx, bd-elpj, bd-q0qz, bd-ebf6, bd-bkii, etc.)
- ~8 docs (bd-l4nm, bd-dfu9, bd-c67l, bd-d889, bd-8e0f, bd-9tud, bd-1uhm, etc.)
- ~5 test improvements (bd-z2jv, bd-h8wz, bd-vd5d, bd-tj8r, bd-mcvs)
- ~5 code quality (bd-qxmi, bd-nhek, bd-kkad, bd-m271, bd-qpw0)

After those: close bd-7fub quality EPIC tree (Lean proofs, Kani, Stateright, durability, completeness).
After that: bd-7fub.22.10 re-review → bd-y1w5 tag → bd-add gate closure.

### Ready Queue
```bash
br ready          # Show unblocked issues
bv --robot-next   # Top pick with reasoning
```

### Dependency Context
```
Phase 4a gate (bd-add) blocked by 33 open beads:
  ○ bd-7fub (Path to 10.0 EPIC) — parent of 11 tier EPICs + ~120 children
  ○ bd-flqz (A+ gate EPIC) — depends on bd-7fub + bd-7fub.22.10
  ◐ bd-7fub.22.10 (re-review, IN_PROGRESS) — assessment only
  ○ bd-y1w5 (tag v0.4.0-gate) — procedural, after re-review
  ○ 28 standalone tasks (bugs, docs, tests, refactors)
  ✓ bd-4i6u (perf EPIC) — CLOSED (20/20)
  ✓ bd-cly9 (decomposition) — CLOSED

Fastest path: close 28 standalones → close bd-7fub tier EPICs →
  re-review (bd-7fub.22.10) → tag (bd-y1w5) → gate (bd-add)
```

## Hard Constraints

- Safe callable surface: `#![forbid(unsafe_code)]` default; `#![deny(unsafe_code)]` for ferratomic-db and ferratomic-checkpoint (ADR-FERR-020 mmap)
- No `unwrap()` in production code (NEG-FERR-001)
- `CARGO_TARGET_DIR=/data/cargo-target`
- All 11 crates in acyclic DAG: clock → ferratom → {tx, storage, wal} → index → positional → checkpoint → store → db → datalog
- Subagents MUST NOT run cargo commands — orchestrator compiles once
- Zero `#[allow(...)]` anywhere — fix root causes
- NEVER close beads claiming "Fixed" without actual code written and verified

## Stop Conditions

Stop and escalate to the user if:
- A standalone task requires changing the Store algebraic structure (INV-FERR-001-004)
- Any task conflicts with the performance work from session 017
- The re-review (bd-7fub.22.10) scores below 10.0 on any quality vector
- A quality EPIC child requires work that contradicts an existing ADR
- Closing a bead requires more than ~50 LOC of changes (may need its own session)
