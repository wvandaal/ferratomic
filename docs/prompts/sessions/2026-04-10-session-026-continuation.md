# Ferratomic Continuation — Session 026

> Generated: 2026-04-10
> Last commit: `f0e6100` "test(prolly): strengthen 2 ADEQUATE assertions to STRONG"
> Branch: main (synced with master)

## Read First

1. `QUICKSTART.md` — project orientation (updated 2026-04-10)
2. `AGENTS.md` — guidelines, hard constraints, crate map
3. `spec/README.md` — 93 invariants, load only what you need
4. `GOALS.md` §5 — Curry-Howard-Lambek computational trinitarianism (updated this session)
5. `GOALS.md` §7 — Six-Dimension Decision Framework

## Session Summary

### Completed (9 commits, 15 beads closed)

**GOALS.md Curry-Howard-Lambek upgrade**:
- §5 fourth axiom: "Types are propositions" → "Computational trinitarianism" with
  correspondence table, functor chain, categorical coproduct, index functors
- §3 Tier 2: "Spec-implementation alignment" → "(functoriality)"

**Wavelet matrix research (bd-obo8, CLOSED at composite 10.0)**:
- spec/09 §Wavelet section (~550 lines): rank/select contract (6 algebraic laws),
  symbol encoding (5 columns, order-preserving + round-trip laws), effective-alphabet
  per-chunk encoding with per-chunk dictionaries (~0.22 bytes/datom), HQwt construction
  algorithm, per-chunk alphabet analysis (h_chunk≈10-17 → ~50-85ns, competitive with
  PositionalStore), 3 query algorithms (Access/Rank_c/Select_c), query composition
  table, per-chunk composition model (prefix-sum + EntityRLE routing), operational
  integration (merge via prolly diff, WAL two-tier read, concurrent r/w with ArcSwap
  + C4 commutativity + backpressure)
- ADR-FERR-030 expanded: Phase 4c+→4b, 3-scale projections, `qwt` HQwt recommended,
  risk register, PtrHash→Phase 4b prerequisite, three-layer scale strategy

**Phase 4b spec EPIC (bd-3gk, CLOSED)**:
- 5 new INV-FERRs: 046a (Gear hash), 050b (manifest CAS), 050c (journal),
  050d (GC safety), 050e (recovery roundtrip). INV count 88→93.
- 5 beads verified already fixed from sessions 023/024 (bd-132, bd-14b, bd-t9h,
  bd-f74, bd-r2u)
- 4 federation spec fixes (bd-2rq, bd-26x, bd-2ac, bd-xopd)

**Three lifecycle/17 spec audits**:
- spec/09 §Wavelet: 1 MAJOR (chunk-local encoding unspecified) + 5 MINOR, all fixed
- spec/06 new INVs: 2 CRITICAL (050b dir fsync, 050d ArcSwap race) + 13 MAJOR
  (proptests: concurrent, adversarial, idempotent, edge cases, cross-invariant
  dependency matrix) + 3 MINOR, all fixed
- spec/05 federation: 3 HIGH (V1 boundary variants, FederatedResult dup), all fixed

**Prolly tree foundation (bd-85j.13, IN PROGRESS)**:
- `ferratomic-core/src/prolly/chunk.rs`: Chunk type (content-addressed, `Arc<[u8]>`),
  `ChunkStore` trait (5 methods), `MemoryChunkStore`. 5 tests.
- `ferratomic-core/src/prolly/boundary.rs`: `gear_hash` (BLAKE3-derived table, seed
  `b"ferratomic-gear-hash-table"`), `is_boundary` with CDF bounds (min=32, max=1024).
  7 tests.
- `ferratomic-core/src/prolly/build.rs`: `build_prolly_tree`, `serialize_leaf_chunk` /
  `deserialize_leaf_chunk` with `len_u32` checked conversion (GOALS.md §5: overflow is
  type-checked error), `serialize_internal_chunk`, `decode_child_addrs`. Level parameter
  tracks recursion depth (cleanroom DEFECT-001 fixed). 13 tests.
- Cleanroom review (lifecycle/06): 2 MAJOR + 1 MINOR fixed (level hardcode, unreachable
  unwrap_or_default, missing error path tests)
- Test suite audit (lifecycle/19): 2 ADEQUATE → STRONG (empty/single-entry edge cases,
  tighter boundary rate tolerance with 100K trials)
- 25/25 tests pass. Zero clippy errors (strict gate clean).

### Decisions Made

- `qwt` crate (`HQwt256Pfs`) recommended as wavelet matrix library over `sucds`
  (Huffman-shaped quad wavelet tree with prefetching — SOTA per Kurpicz et al. 2025)
- PtrHash reclassified from Phase 4c+ to Phase 4b prerequisite (CHD verification
  table is 3.2 GB at 100M entities; PtrHash: 25 MB)
- Per-chunk effective-alphabet encoding with per-chunk dictionaries (~0.22 bytes/datom)
- Gear hash with BLAKE3-derived table seed `b"ferratomic-gear-hash-table"` (INV-FERR-046a)
- Chunk format: 0x01=leaf, 0x02=internal, u32 LE length-prefixed entries
- `len_u32` helper for all usize→u32 conversions: overflow returns
  `FerraError::InvariantViolation` (GOALS.md §5 Curry-Howard)
- `pattern_width` is a store-wide constant; changing after creation is breaking

### Bugs Found

- DEFECT-001 (MAJOR): `build_internal_nodes` hardcoded `level = 1u8` for all recursion
  depths. Fixed: level parameter increments per recursive call.
- DEFECT-002 (MAJOR): Unreachable `unwrap_or_default` on separator key in
  `build_prolly_tree`. Fixed: direct `group[0]` access (group guaranteed non-empty).
- spec/06 FINDING-4 (CRITICAL): INV-FERR-050b missing directory-level fsync after
  rename. Fixed: added `dir.sync_all()` after rename in L2 contract.
- spec/06 FINDING-8 (CRITICAL): INV-FERR-050d ArcSwap reader-GC race condition
  not formalized. Fixed: added synchronization requirement to L1.
- spec/05 FINDING-1/5 (HIGH): V1 remote query boundary listed wrong variant names
  (7 WireQueryExpr vs actual 6 DatomFilter). Fixed.

### Stopping Point

**Exactly where I stopped**: All code committed and pushed at `f0e6100`. The prolly
tree foundation layer is complete: `Chunk`, `ChunkStore`, `MemoryChunkStore`,
`gear_hash`, `is_boundary`, `build_prolly_tree`, leaf/internal serialization,
`decode_child_addrs`. 25 tests, all STRONG assertions, cleanroom reviewed.

**Last thing verified working**: `cargo test -p ferratomic-db --lib -- prolly` → 25/25
pass. `cargo clippy -p ferratomic-db --lib -- -D warnings` → 0 errors in prolly module.
All pre-commit gates pass.

**Next thing to do**: Implement `read_prolly_tree` (walk from root hash to leaves,
deserialize, collect key-value pairs) and `DiffIterator` (INV-FERR-047). The spec
pseudocode is in spec/06-prolly-tree.md lines 3131-3474 (DiffIterator) — fully
authored with `DiffStackEntry` enum, `merge_join_children`, `diff_sorted_entries`.

## Next Execution Scope

### Primary Task

Continue **bd-85j.13** (prolly tree block store, IN PROGRESS). The foundation layer
is done (chunk, boundary, build). The remaining modules are:

| Module | INV-FERRs | Spec reference | Priority |
|--------|-----------|----------------|----------|
| `prolly/read.rs` | 049 | spec/06 lines 4229-4378 (Snapshot) | HIGH — needed for roundtrip tests |
| `prolly/diff.rs` | 047 | spec/06 lines 3131-3474 (DiffIterator) | HIGH — core federation primitive |
| `prolly/transfer.rs` | 048 | spec/06 lines 3847-3957 (RecursiveTransfer) | MEDIUM |
| `prolly/snapshot.rs` | 049, 050b | spec/06 lines 4229-4378 + 5128-5265 | MEDIUM |
| `prolly/journal.rs` | 050c, 050d, 050e | spec/06 lines 5265-5900 | LOW — needed for full recovery |

Start with `read_prolly_tree` (inverse of `build_prolly_tree`) — this enables
the snapshot roundtrip test (INV-FERR-049) that validates the entire build→read path.

### Ready Queue
```bash
br ready          # Show unblocked issues
bv --robot-next   # Top pick with reasoning
```

### Dependency Context

- Another agent is working on **Phase 4a.5** (federation impl in ferratom/src/*,
  ferratomic-core/src/db/*, ferratomic-core/src/signing.rs). Do NOT touch those files.
- The prolly tree files (`ferratomic-core/src/prolly/*`) are exclusively ours.
- The wavelet formal INV-FERRs (gvil.2-4: bd-vhgn, bd-lkdh, bd-8uck) are now
  unblocked by bd-obo8's closure. They can run in parallel with prolly tree impl.

## Hard Constraints

- `#![forbid(unsafe_code)]` in all crates. Zero exceptions.
- No `unwrap()`, `expect()`, or `panic!()` in production code (strict clippy gate).
- Zero `#[allow(...)]` anywhere.
- `CARGO_TARGET_DIR=/data/cargo-target` — MUST set.
- `cargo fmt --all` BEFORE `git add`.
- All `usize→u32` conversions use `len_u32()` helper (returns `FerraError::InvariantViolation`).
- Chunk format tags: 0x01=leaf, 0x02=internal. Do NOT add new tags without updating
  `decode_child_addrs` match arms.
- GOALS.md §5 Curry-Howard-Lambek: invalid states must be unrepresentable in the
  type system. Overflow is a checked error path, not a silent truncation.
- GOALS.md §6 defensive engineering standards — all 11 gates must pass.
- GOALS.md §7 Six-Dimension scoring for non-trivial decisions.

## Stop Conditions

Stop and escalate to the user if:
- Any cargo gate fails and the fix requires changing the other agent's files
- A spec invariant contradiction is discovered between spec/06 (prolly tree) and
  spec/09 (wavelet matrix / performance architecture)
- The `DiffIterator` implementation diverges from the spec pseudocode at
  spec/06 lines 3131-3474 (the spec was authored and triple-audited this session)
- `build_prolly_tree` root hash changes for the same input (history independence
  regression — INV-FERR-046 violation)
- Any test in the existing 25-test suite starts failing (regression)
