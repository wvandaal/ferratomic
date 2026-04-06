# Ferratomic Continuation — Session 015 (Final State)

> Generated: 2026-04-06
> Last commit: `16319cb` "docs: final session 015 continuation prompt — full handoff"
> Branch: main

## Read First

1. `QUICKSTART.md` — project orientation
2. `AGENTS.md` — guidelines and constraints
3. `spec/README.md` — load only the spec modules you need
4. `spec/09-performance-architecture.md` — heavily amended this session

## Session Summary

### Completed
- Cleanroom re-review (5 parallel agents): 0 CRITICALs, 0 MAJORs, 5 MINORs
- Progress review: composite A (9.38/10.0), performance flagged at 8.5
- 11-crate decomposition designed: ferratomic-core → 8 focused crates, renamed to ferratomic-db
- Radical performance stack: 20 performance beads across 4 tiers + 8 decomposition beads + 4 review finding beads = 32 total
- Spec amendments: C9 (balanced workload), INV-FERR-072 rewrite (3 mutation paths), INV-FERR-078 (SoA columnar), INV-FERR-081-085, ADR-FERR-031-033
- Spec audit: 2 CRITICALs found and fixed (085 ordering axiom, 084 Bloom safety)
- Full bead audit (28/28 sequential, lifecycle/14): 6 critical findings corrected (fictional exports, circular dependency, missing dep edges, hidden coupling)
- 17 commits pushed

### Decisions Made
- **C9 Balanced workload**: NOT read-heavy 99:1. Write bursts + read bursts + interleaved. Drives entire performance architecture.
- **Merge-sort splice** (INV-FERR-072 Path A): transact bypasses promote/demote, stays Positional. O(N + K) not O(N log N).
- **Primitive vs injectable indexes** (ADR-FERR-033): deterministic projections are primitive; app-model-dependent indexes are injectable.
- **Checkpoint decoupled from Store**: serialize/deserialize accept raw data, not &Store. Breaks circular dependency.
- **WAL decoupled from Transaction**: append_raw(&[u8]) only. No Transaction import in WAL crate.
- **Performance weight 2.5x** (up from 1.5x): second only to correctness.
- **All performance beads stay Phase 4a**: multiple sessions, profile-validated.

### Bugs Found
- bd-pb3b: 4 threshold tests fail (cold start genesis fallback + indexes().unwrap() on Positional)
- bd-l64y: merge_causal homomorphism inexact for same-TxId cross-Op
- bd-fcta: WireEntityId pub inner field bypasses trust boundary
- bd-8rvz: 2 functions over 50 LOC

### Stopping Point
All planning, spec authoring, and bead auditing complete. Zero implementation code written. The project is fully designed, specified, and task-graphed for the decomposition + performance work. Next session begins actual code changes.

## Next Execution Scope

### Primary Task
**Start crate decomposition (bd-cly9).** 4 extractions can run in parallel:

```
bd-bc41: ferratomic-wal (~850 LOC) — READY
bd-nb12: ferratomic-index (~600 LOC) — READY
bd-8fr9: ferratomic-storage (~500 LOC) — READY
bd-nt71: ferratomic-tx (~365 LOC) — READY
```

Each bead has been source-verified with exact import maps, visibility requirements, and step-by-step verification plans. Start with bd-nt71 (tx) — it's the smallest and cleanest (zero crate:: imports).

### Ready Queue
```bash
br ready          # Show unblocked issues
bv --robot-next   # Top pick with reasoning
```

### Dependency Context (decomposition)
```
Track A: bd-bc41 (wal) → bd-bb9r (checkpoint, needs wal+positional)
Track B: bd-nb12 (index) → bd-q0ys (positional) → bd-ipln (store, needs index+positional+tx+checkpoint)
Track C: bd-nt71 (tx) — independent
Track D: bd-8fr9 (storage) — independent
Final:   bd-wrrg (rewire ferratomic-db, depends on all 7)
```

### Dependency Context (performance, after decomposition)
```
Tier 1: bd-k4ex (into_datoms) → bd-886d (splice) → bd-ks5d (batch)
         bd-0zfw (chunks) → bd-nq6v (inc. LIVE)
Tier 2: bd-fnod (intern) → bd-574c (SoA) → bd-mdfq (entity RLE), bd-3ta0 (TxId perm)
         bd-t84f (rank/select), bd-iltk (SIMD fp), bd-wv6v (Eytzinger)
Tier 3: bd-wows (PinSketch), bd-m7te (entropy checkpoint), bd-eusk (WAL dedup)
```

## Hard Constraints

- Safe callable surface: `#![forbid(unsafe_code)]` by default; internal unsafe only per ADR-FERR-020 (mmap in ferratomic-checkpoint)
- No `unwrap()` in production code (NEG-FERR-001)
- `CARGO_TARGET_DIR=/data/cargo-target` (prevents /tmp exhaustion)
- C9: Balanced workload assumption — never optimize only for reads
- Performance weight 2.5x — profile every structural change
- Zero `#[allow(...)]` anywhere — fix root causes
- All 32 beads are lab-grade audited — follow the Pseudocode Contract exactly
- **NEVER batch bead audits or delegate to subagents** — sequential, source-verified, one at a time

## Stop Conditions

Stop and escalate to the user if:
- A crate extraction reveals a circular dependency not caught in the audit
- A file has imports from >2 extracted crates (may indicate the split point is wrong)
- cargo test reveals new failures beyond the 4 known threshold bugs (bd-pb3b)
- Any bead's Pseudocode Contract doesn't match what the code actually needs
- The 11-crate DAG has a cycle (`cargo tree` will catch this)
