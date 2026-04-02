# Ferratomic Continuation — Session 007: Performance Architecture Implementation

> Generated: 2026-04-02
> Last commit: (see git log — multiple commits this session)
> Branch: main

## Read First

1. `QUICKSTART.md` — project orientation
2. `AGENTS.md` — guidelines and constraints
3. `spec/09-performance-architecture.md` — **THE spec for this session** (INV-FERR-070-076, ADR-FERR-020)
4. `spec/01-core-invariants.md` — INV-FERR-005 (index bijection), INV-FERR-006 (snapshot isolation)
5. `spec/03-performance.md` — INV-FERR-025 (index backend interchangeability)

## The Big Idea: Positional Content Addressing

This session implements a fundamental architectural change. Read INV-FERR-076 in
`spec/09-performance-architecture.md` completely before writing any code.

**Core insight**: Every datom in a sorted canonical array has a unique position
`p : u32` in `[0, n)`. That position is deterministic (same datom set = same positions),
content-derived (sort order is defined by datom fields), and 4 bytes instead of 32.
This position replaces EntityId hashes for ALL internal references:

- **Index entries**: 4-byte position offsets instead of 32-byte hash keys
- **LIVE view**: bitvector `live_bits[p]` instead of nested OrdMap
- **Merge**: merge-sort on contiguous arrays instead of tree insertion
- **Cold start**: read file = done (arrays ARE the runtime structures)

Current state at 200K datoms:
```
OrdSet + 4 OrdMaps + LIVE OrdMap = 159 MB, 89s cold start
```

Target state at 200K datoms:
```
Vec<Datom> + 3 Vec<u32> + BitVec + [u8;32] = 26 MB, <5ms cold start
```

## Session 006 Summary

### Completed
- Deep progress review: 8.84 → ~9.1
- Zero lint escape hatches enforced everywhere (pre-commit hook)
- Kani harnesses compile under normal cargo (7 API drift bugs fixed)
- 6 Kani harnesses verified by CBMC
- Kani toolchain installed (cargo-kani 0.67.0)
- FerraError::Io preserves ErrorKind
- 200K scale benchmarks passing (WA<5x, P99<100us in release)
- spec/09-performance-architecture.md authored + audited twice (0 findings remaining)
- INV-FERR-076 (Positional Content Addressing) specified with full Level 0/1/2
- 12 beads closed, 6 phantom edges removed, bead audit completed
- Cross-cutting longitudinal audit: 0 contradictions across full spec surface

### Decisions Made
- ALL performance beads are Phase 4a gate requirements
- ADR-FERR-020: Localized unsafe for mmap (one function, BLAKE3-guarded)
- Positional content addressing is the architectural foundation for everything

### Stopping Point
All spec and bead work done. Zero implementation exists. This session is pure
implementation.

## Execution Plan

### Build Order (strict — each step depends on the previous)

```
Step 1: bd-1c5r  SortedVecBackend
        ~150 LOC in ferratomic-core/src/indexes.rs
        Implements IndexBackend<K,V> with Vec<(K,V)> + binary search

Step 2: bd-vpca  PositionalStore (INV-FERR-076)
        ~300 LOC new file: ferratomic-core/src/positional.rs
        Vec<Datom> + BitVec + 3x Vec<u32> + fingerprint
        Replaces Store as the primary store representation

Step 3: bd-h2fz  Eliminate redundant primary OrdSet
        EAVT index IS the canonical array — remove OrdSet<Datom>
        Wire existing Store API to delegate to PositionalStore

Step 4: bd-bkff  Lazy OrdMap promotion (INV-FERR-072)
        AdaptiveIndexes enum: Positional | OrdMap
        Cold-loaded stores use Positional; first write promotes

Step 5: bd-5zc4  Yoneda fusion (INV-FERR-073)
        Remove materialized AEVT/VAET/AVET OrdMaps
        Replace with permutation arrays (already in PositionalStore)

Step 6: bd-ndok  ClockSource trait injection
        Make HybridClock generic over clock source
        Enables Kani verification of HLC harnesses

Step 7: bd-erfj  ADR-FERR-020 localized unsafe boundary
        Create ferratomic-core/src/mmap.rs with validate_and_cast

Step 8: bd-a2vf  Checkpoint V3 (pre-sorted arrays)
        Serialize PositionalStore directly — arrays ARE the format

Step 9: bd-ta8c  mmap zero-copy cold start (INV-FERR-070)
        Memory-map V3 checkpoint — zero construction
```

### Step 1 Detail: SortedVecBackend (bd-1c5r)

**File**: `ferratomic-core/src/indexes.rs`

Add alongside existing `impl IndexBackend<K,V> for OrdMap<K,V>`:

```rust
#[derive(Clone, Debug)]
pub struct SortedVecBackend<K: Ord, V> {
    entries: Vec<(K, V)>,
    sorted: bool,
}

impl<K: Ord + Clone + Debug, V: Clone + Debug> IndexBackend<K, V>
    for SortedVecBackend<K, V>
{
    fn backend_insert(&mut self, key: K, value: V) {
        self.entries.push((key, value));
        self.sorted = false;
    }

    fn backend_get(&self, key: &K) -> Option<&V> {
        debug_assert!(self.sorted, "INV-FERR-071: lookup on unsorted backend");
        self.entries
            .binary_search_by(|(k, _)| k.cmp(key))
            .ok()
            .map(|i| &self.entries[i].1)
    }

    fn backend_len(&self) -> usize { self.entries.len() }

    fn backend_values(&self) -> Box<dyn Iterator<Item = &V> + '_> {
        Box::new(self.entries.iter().map(|(_, v)| v))
    }
}
```

Plus `sort()` with dedup (map semantics) and `Default` impl.

**Acceptance criteria**:
1. Proptest: SortedVecBackend and OrdMap identical for all operations
2. All existing tests pass (SortedVecBackend doesn't replace OrdMap yet)
3. `cargo clippy --workspace --all-targets -- -D warnings` zero warnings

### Step 2 Detail: PositionalStore (bd-vpca)

**File**: NEW `ferratomic-core/src/positional.rs`

This is the core data structure. Everything else wraps or delegates to it.

Key methods:
- `from_datoms(iter)` — sort + dedup + build permutations + build live bitvector
- `position_of(datom)` — binary search, returns `Option<u32>`
- `is_live(position)` — bit test, O(1)
- `eavt_get(key)` — binary search on canonical array
- `aevt_get(key)` — binary search on permuted view
- `merge_positional(a, b)` — merge-sort + rebuild

**Acceptance criteria**:
1. Proptest: PositionalStore.datoms() == Store.datoms() for same input
2. Proptest: PositionalStore.live_view() == Store.live_view() for same input
3. Proptest: merge_positional(a,b).datoms() == merge(a,b).datoms()
4. LIVE bitvector length == canonical array length
5. All permutation arrays are valid permutations of [0, n)

## Hard Constraints

- Zero `#[allow(...)]` anywhere — pre-commit hook enforces
- `#![forbid(unsafe_code)]` in all crates until Step 7 (ADR-FERR-020)
- No `unwrap()` or `expect()` in production code
- `CARGO_TARGET_DIR=/data/cargo-target`
- Every public function references an INV-FERR in its doc comment
- All functions under 50 lines, all files under 500 LOC
- Pre-commit hook runs: fmt + clippy --all-targets + strict gate + zero-allow scan

## Stop Conditions

Stop and escalate to the user if:
- SortedVecBackend requires changing the `IndexBackend` trait signature
- The `sort()` dedup strategy conflicts with `Store::transact` semantics
- Any existing test fails after changes
- The LIVE bitvector construction requires information not available in EAVT order
  (e.g., needs to see all datoms for an (e,a) pair before determining liveness,
  but EAVT groups by entity then attribute — this SHOULD work but verify)
- PositionalStore `merge_positional` produces different results from `Store::merge`
  for any input
- You need to add a new crate dependency
- Any file exceeds 500 LOC

## Key Files

```
ferratomic-core/src/indexes.rs       — SortedVecBackend (Step 1)
ferratomic-core/src/positional.rs    — PositionalStore (Step 2, NEW)
ferratomic-core/src/store/mod.rs     — Wire delegation (Steps 3-5)
ferratomic-core/src/store/query.rs   — LIVE bitvector integration
ferratomic-core/src/store/merge.rs   — merge-sort path
ferratomic-verify/proptest/          — New proptest suites for 076
ferratomic-verify/kani/              — Kani harnesses for 076
```

## Performance Targets (verify with benchmarks after implementation)

| Metric | Current (im::OrdMap) | Target (Positional) | Improvement |
|--------|---------------------|---------------------|-------------|
| Memory at 200K | 159 MB | 26 MB | 6x |
| Cold start 200K | 89s | <5ms (sort) | 17,800x |
| Point lookup | 300ns | 15-20ns | 15-20x |
| LIVE query | 200ns | 1ns | 200x |
| Merge 200K+200K | 89s | 50ms | 1,780x |
| LIVE merge | seconds | 1 microsecond | 1,000,000x |
