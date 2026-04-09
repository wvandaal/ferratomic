# Ferratomic Continuation — Session 009: Store Wiring & OrdMap Elimination

> Generated: 2026-04-02
> Last commit: f7cc89c "test: add merge commutativity + all-index lookup tests (INV-FERR-076)"
> Branch: main

## Read First

1. `QUICKSTART.md` — project orientation
2. `AGENTS.md` — guidelines and constraints
3. `spec/09-performance-architecture.md` — INV-FERR-070-076, ADR-FERR-020
4. `ferratomic-core/src/positional.rs` — **THE data structure this session wires in**
5. `ferratomic-core/src/store/mod.rs` — **THE code you are modifying**
6. `ferratomic-core/src/store/query.rs` — LIVE view + snapshot (must stay working)
7. `ferratomic-core/src/indexes.rs` — SortedVecBackend + SortedVecIndexes

## Recover Design Context from Prior Sessions

The performance architecture was designed in Session 006 (CASS session
`97a9dbc5`). That session contains the deep reasoning behind every design
choice — why arrays replace trees, why positions replace hashes, why the
bitvector replaces nested OrdMap, and the full "alien artifacts" analysis.

**Run these CASS searches before writing any code.** The design rationale
will prevent you from making incorrect assumptions about why things are
structured the way they are.

```bash
# The session where the entire performance architecture was designed.
# Contains: 89s cold start root-cause analysis, "alien artifacts" ranking,
# positional content addressing invention, memory/latency/merge analysis,
# and the "faithful functor from datom semilattice to natural number ordering" insight.
cass export "/home/ubuntu/.claude/projects/-data-projects-ddis-ferratomic/97a9dbc5-6bf9-43ac-976e-7eb36fe1dfca.jsonl" --format markdown 2>/dev/null | head -500

# Key sections in that session (search for these anchors):
# - "89s" — cold start root cause (OrdMap bulk construction bottleneck)
# - "Alien Artifact" — the full ranked optimization candidates
# - "Positional Content Addressing" — the core invention
# - "faithful functor" — why this is algebraically sound
# - "arrays ARE the index" — the checkpoint-IS-runtime insight
# - "bitvector" — why BitVec replaces nested OrdMap for LIVE
# - "merge-sort" — why CRDT merge reduces to merge-sort on arrays

# The spec authoring subagent that wrote INV-FERR-076 Level 0/1/2:
cass search "sort_unstable" --robot --fields minimal --limit 10 --workspace /data/projects/ddis/ferratomic

# The performance architecture discussion that produced the build order:
cass search "performance architecture" --robot --fields minimal --limit 10 --workspace /data/projects/ddis/ferratomic

# The cold start bottleneck analysis (OrdMap is the problem, not the threshold):
cass search "OrdMap cold start 89" --robot --fields minimal --limit 5

# The zero-copy mmap design discussion:
cass search "zero-copy mmap checkpoint" --robot --fields minimal --limit 5 --workspace /data/projects/ddis/ferratomic

# CRITICAL: The merge-sort insight — why CRDT merge IS merge-sort on arrays.
# This is the theoretical foundation for merge_positional. The session proves
# that merge-sort on contiguous canonical arrays is algebraically equivalent
# to OrdSet union, but 1,780x faster at 200K datoms (50ms vs 89s). It also
# shows that LIVE merge reduces to bitwise OR (~1 microsecond for 200K datoms).
# Lines 615, 824, 832 in session 97a9dbc5 contain the core reasoning.
cass search "merge sort sequential access cache" --robot --fields minimal --limit 5 --workspace /data/projects/ddis/ferratomic

# The "Five Concrete Wins" analysis — quantified impact per subsystem.
# Covers: (1) 32-byte→4-byte internal references, (2) LIVE as bitvector,
# (3) checkpoint file IS the runtime format, (4) merge as merge-sort,
# (5) federation diff as position XOR.
cass search "50ms merge sort contiguous" --robot --fields minimal --limit 5

# To read the full session end-to-end (10K lines, ~30min read):
cass export "/home/ubuntu/.claude/projects/-data-projects-ddis-ferratomic/97a9dbc5-6bf9-43ac-976e-7eb36fe1dfca.jsonl" --format markdown 2>/dev/null > /tmp/session006.md
# Then search within it:
# grep -n "merge-sort\|Concrete Wins\|faithful functor\|LIVE.*bitvector\|arrays ARE" /tmp/session006.md
```

**Why this matters for THIS session:** Steps 3-5 are surgery on Store
internals. The design rationale explains WHY PositionalStore is a frozen
snapshot representation (not a mutable store), WHY transact must keep
OrdMap (positions shift on insert), and WHY the lazy promotion pattern
exists. Without this context, you will likely try to make PositionalStore
mutable — which contradicts the architecture.

The merge-sort insight is especially critical for Step 3: when wiring
`merge_positional` into `Store::from_merge`, you need to understand that
the O(n+m) merge-sort + O(n log n) permutation rebuild is the CORRECT
cost — it replaces O(n log n) OrdSet tree union + O(n) 4x OrdMap rebuild
with cache-hostile pointer chasing. The constant factors make merge-sort
dramatically faster despite the same asymptotic complexity.

## Session 007-008 Summary

### Completed
- bd-1c5r: SortedVecBackend — `IndexBackend` with `Vec<(K,V)>` + `sort_unstable_by` + binary search. 2 proptests. Cleanroom reviewed.
- bd-vpca: PositionalStore (INV-FERR-076) — `Vec<Datom>` canonical + `BitVec<u64, Lsb0>` LIVE + 3× `Vec<u32>` permutations. 7 proptests (datoms, LIVE, merge, bitvec len, perms valid, determinism, all-index lookups). Cleanroom reviewed, 2 defects fixed.
- CASS repaired (stuck indexer 41h, corrupted Tantivy index, watcher restarted)

### Decisions Made
- `sort_unstable` everywhere (not `sort_by`) — O(1) aux memory. At 100M datoms, stable sort's temporary buffer would be ~20GB. Matches PositionalStore::from_datoms pattern.
- No map-key dedup in SortedVecBackend — index keys never collide (full 5-tuple). `debug_assert` for unique keys instead. Dedup was not in the original spec; it was incorrectly added and removed after tracing through CASS session history.
- `bitvec` crate added for LIVE bitvector (1 bit/datom vs 1 byte with Vec<bool>).

### Stopping Point
Steps 1-2 of 9 complete. PositionalStore exists as a standalone data structure with full proptest coverage proving equivalence to the OrdMap-based Store. **It is not yet wired into the Store API.** The existing Store still uses `OrdSet<Datom>` + 4× `OrdMap` + nested `OrdMap` LIVE.

## The Architecture Change

PositionalStore was built and tested in isolation. This session wires it in. The transformation:

```
BEFORE (Store internals):
  datoms:      OrdSet<Datom>         ~24 MB   ← REDUNDANT (canonical array has same data)
  indexes:     4× OrdMap             ~120 MB  ← REDUNDANT (permutation arrays replace 3 of 4)
  live_causal: nested OrdMap         ~15 MB   ← REDUNDANT (BitVec replaces)
  live_set:    OrdMap                ~5 MB    ← REDUNDANT (BitVec replaces)
  Total: ~159 MB

AFTER (Store delegates to PositionalStore):
  positional:  PositionalStore       ~26 MB   ← canonical + perms + bitvec
  schema:      Schema                (unchanged)
  epoch:       u64                   (unchanged)
  genesis_agent: AgentId             (unchanged)
  Total: ~26 MB
```

## Execution Plan

### Build Order (strict — each step depends on the previous)

```
Step 3: bd-h2fz  Eliminate redundant primary OrdSet
        Wire Store to hold PositionalStore internally.
        Store::datoms() → self.positional.datoms()
        Store::len() → self.positional.len()
        Store::from_datoms() builds PositionalStore, not OrdSet + Indexes.

Step 4: bd-bkff  Lazy OrdMap promotion (INV-FERR-072)
        AdaptiveStore enum: Positional(PositionalStore) | OrdMap(current Store)
        Cold-loaded stores start Positional; first transact() promotes to OrdMap.
        Snapshots work with either variant.

Step 5: bd-5zc4  Yoneda fusion (INV-FERR-073)
        Remove materialized AEVT/VAET/AVET OrdMaps from the OrdMap variant.
        When in OrdMap mode, use SortedVecIndexes for secondary indexes.
        When in Positional mode, use permutation arrays (already done).
```

### Step 3 Detail: Eliminate Redundant Primary OrdSet (bd-h2fz)

**Files to modify:**
- `ferratomic-core/src/store/mod.rs` — Store struct + constructors
- `ferratomic-core/src/store/query.rs` — LIVE queries + snapshot
- `ferratomic-core/src/store/merge.rs` — merge delegation
- `ferratomic-core/src/store/apply.rs` — transact + insert
- `ferratomic-core/src/store/checkpoint.rs` — serialization
- `ferratomic-core/src/store/tests.rs` — update assertions

**Strategy: dual representation during transition.** Don't rip out OrdSet in one shot. Instead:

1. Add `positional: Option<PositionalStore>` to Store struct
2. Make `from_datoms()` populate both `datoms` (OrdSet) AND `positional`
3. Add `datoms_positional()` method returning `&[Datom]` from positional
4. Run all tests — both representations agree
5. Switch `datoms()` to return from positional
6. Remove `datoms: OrdSet<Datom>` field
7. Run all tests — nothing broke

This incremental approach means every intermediate state compiles and passes tests.

**LIVE wiring:** The trickiest part. Store currently uses `live_causal` (nested OrdMap) for:
- `live_values(entity, attr)` → returns `OrdSet<Value>` of non-retracted values
- `live_resolve(entity, attr)` → returns single LWW value
- `live_apply()` during transact — incremental update

PositionalStore's `live_bits` bitvector answers "is this datom live?" but NOT "what are the live values for (e,a)?" — that requires scanning the bitvector and collecting values. For Step 3, keep `live_causal` alongside PositionalStore (it's needed for transact-time incremental updates). Step 5 can eliminate it once transact delegates fully.

**Acceptance criteria:**
1. All 83 ferratomic-core tests pass
2. All proptest suites pass (CRDT, index, schema, WAL, durability, clock, append-only, fault recovery, positional)
3. `Store::datoms()` returns from PositionalStore's canonical array
4. `Store::len()` agrees between OrdSet and PositionalStore (then OrdSet removed)
5. `Store::from_datoms()` builds PositionalStore
6. Merge uses `merge_positional` under the hood
7. Zero clippy warnings, zero lint suppressions

## Hard Constraints

- Zero `#[allow(...)]` anywhere — pre-commit hook enforces
- `#![forbid(unsafe_code)]` in all crates
- No `unwrap()` or `expect()` in production code
- `CARGO_TARGET_DIR=/data/cargo-target`
- Every public function references an INV-FERR in its doc comment
- All functions under 50 lines, all files under 500 LOC
- Pre-commit hook runs: fmt + clippy --all-targets + strict gate + zero-allow scan

## Stop Conditions

Stop and escalate to the user if:
- Removing `OrdSet<Datom>` breaks snapshot isolation (INV-FERR-006) — `im::OrdSet` provides O(1) clone; `Vec<Datom>` clone is O(n). The `Snapshot` struct may need to hold an `Arc<Vec<Datom>>` instead.
- `transact()` / `insert()` can't work with PositionalStore because positions shift on every insert — positions are NOT stable across mutations. Transact must rebuild PositionalStore or fall back to OrdMap (which is Step 4's job).
- The LIVE bitvector can't support incremental `live_apply()` during transact — the bitvector is a snapshot-time structure, not a transaction-time structure. Keep `live_causal` for transact; rebuild bitvector on snapshot.
- Any file exceeds 500 LOC after changes
- Any existing test fails and the fix is non-obvious (> 5 minutes to understand)
- You need to add a new crate dependency

## Key Insight: Transact vs Snapshot Split

PositionalStore is a **frozen snapshot representation**. It is optimal for:
- Cold start (arrays ARE the checkpoint)
- Read queries (binary search on contiguous memory)
- Merge (merge-sort)
- LIVE queries (bitvector)

It is NOT designed for incremental mutation. `transact()` adds datoms one at a time — rebuilding the entire PositionalStore per transaction is O(n log n), which is worse than OrdMap's O(log n) insert.

The correct architecture (implemented across Steps 3-4):
- **Mutable path (transact):** Keep OrdMap indexes + OrdSet primary + live_causal. This is the write-optimized representation.
- **Frozen path (snapshot/cold start):** Use PositionalStore. This is the read-optimized representation.
- **Transition:** First transact on a cold-loaded store promotes Positional → OrdMap (one-time O(n log n) cost). Snapshots can be taken from either representation.

Step 3 wires PositionalStore into the **construction and read paths** (`from_datoms`, `datoms()`, `merge`). It does NOT change the transact path. Step 4 adds the adaptive switching.

## Key Files

```
ferratomic-core/src/positional.rs    — PositionalStore (DONE, do not modify)
ferratomic-core/src/indexes.rs       — SortedVecBackend (DONE, do not modify)
ferratomic-core/src/store/mod.rs     — Store struct (MODIFY: add positional field)
ferratomic-core/src/store/query.rs   — LIVE + snapshot (MODIFY: dual-path reads)
ferratomic-core/src/store/merge.rs   — merge (MODIFY: use merge_positional)
ferratomic-core/src/store/apply.rs   — transact (DO NOT CHANGE in Step 3)
ferratomic-core/src/store/tests.rs   — tests (UPDATE assertions if APIs change)
ferratomic-core/src/store/checkpoint.rs — serialization (MAY need update)
```

## Performance Targets (verify after Step 3)

Step 3 should NOT degrade performance — it's adding a parallel representation, not replacing the hot path. After Step 5, verify:

| Metric | Current (im::OrdMap) | Target (Positional) |
|--------|---------------------|---------------------|
| Memory at 200K | 159 MB | 26 MB |
| Cold start 200K | 89s | <5ms (sort) |
| Point lookup | 300ns | 15-20ns |
| LIVE query | 200ns | 1ns |
| Merge 200K+200K | 89s | 50ms |
