# Ferratomic Continuation — Session 014

> Generated: 2026-04-04
> Last commit: `b62f3eb` "fix: SortedVecBackend::sort doc comment matches dedup_by implementation"
> Branch: main

## Read First

1. `QUICKSTART.md` — project orientation
2. `AGENTS.md` — guidelines and constraints
3. `spec/README.md` — load only the spec modules you need

## Session 013 Summary

### Completed

**Two features:**
- **CHD Perfect Hash** (`ferratomic-core/src/mph.rs`, NEW): O(1) entity existence + position lookup via 3-hash Compress-Hash-Displace. `MphBackend` trait (build/lookup/reverse_lookup) abstracts backend swaps. Zero-copy verification against canonical array. ~9 bytes/entity. Integrated into `PositionalStore::entity_lookup`.
- **LIVE-first V3 Checkpoint** (`ferratomic-core/src/checkpoint/v3.rs`): Version 0x0103 stores LIVE datoms first, historical second. `PartialStore` with `live_store()` accessor for partial cold start. Version dispatch in `deserialize_checkpoint_bytes`. O(n) all paths via `from_checkpoint_v3`.

**Five cleanroom audit rounds (R1-R5) with convergent findings:**
- R1: 21 findings, all fixed
- R2: 20 findings, 13 fixed, 7 closed (wontfix/deferred)
- R3: 10 findings, 7 fixed, 3 closed
- R4: 5 findings, 4 fixed, 1 deferred (OnceLock poisoning)
- R5: 1 finding, fixed (doc mismatch)
- **Convergence**: 21 → 20 → 10 → 5 → 1. Zero CRITICALs × 5 rounds.

**13 prior audit beads closed** (from session 012 cleanroom review):
- 2 P0 CRITICALs: INV-023 proptest forbid/deny mismatch, verify_bijection cfg-gating
- 3 P1: merge_sort_dedup regression tests, ferratom-clock INV-023 coverage, HLC sentinel doc
- 8 P2: coverage_by_layer type_level, catalog documentation, observer clone fix, ADR-020 spec cross-ref, SortedVecBackend sort/get guards, StorageBackend Send+Sync, SharedBufferWriter flush/Drop, power-cut WAL delta assertion

**64 total beads closed this session. 128 tests (was 111). 10 commits pushed.**

### Decisions Made

- **CHD not MMPH**: The hash function is non-monotone. Monotone rank comes from sorted verification table. True MMPH (PtrHash, 2.0 bits/key) is Phase 4c+ optimization. ADR-FERR-030 corrected in spec/09.
- **Rank-space reduction**: ADR-FERR-030 wavelet matrix needs O(1) rank. Current CHD provides it via `entity_position`. PtrHash eliminates the verification table when ready. `MphBackend` trait is the swap point.
- **debug_assert policy clarified**: Production-critical preconditions (u32::MAX bounds, sorted-order for binary search) promoted to production checks. Cosmetic preconditions (sort-order on construction from trusted internal data) remain debug_assert with documented BLAKE3 defense per ADR-FERR-010.
- **API visibility pattern**: Internal modules (`pub(crate) mod v3`) with types re-exported via parent module (`pub use v3::PartialStore`). Functions wrapped as pub delegates.

### Bugs Found / Known Gaps

- **entity_lookup fallback dispatch**: Line 456 in positional.rs has zero coverage through `entity_lookup` itself. The fallback (binary search when MPH build fails) is tested via `first_datom_position_for_entity` directly. Requires OnceLock poisoning to test the actual dispatch path.

### Stopping Point

All work committed and pushed. 10 commits on main. 128 tests passing. Zero clippy warnings. All pre-commit gates pass (fmt, clippy all-targets, clippy strict --lib, zero #[allow]). Lean proofs untouched (no Lean changes this session).

Zero audit/defect beads remaining. All AUDIT-* and DEFECT-* beads from sessions 012 and 013 are closed.

## Next Execution Scope

### Primary: Phase 4a A+ Gate Features

The Phase 4a gate (bd-add) is blocked by the A+ EPIC (bd-flqz), which requires these open features:

```
bd-gkln (P1) — Run Kani harnesses overnight (--unwind 2)
bd-83j4 (P2) — XOR homomorphic store fingerprint (INV-FERR-074)
bd-218b (P2) — Cuckoo filter pre-filter for O(1) negative lookups
bd-a7s1 (P2) — Parallel index sort via rayon
    ↓
bd-flqz (P0 EPIC) — Phase 4a A+ gate
    ↓
bd-7fub.22.10 (P0) — Re-review at 10.0
    ↓
bd-y1w5 (P0) — Tag Phase 4a
    ↓
bd-add (P1) — Gate closes → 17 Phase 4b beads unblocked
```

Plus two P1 verification investigations:
- bd-4pna: Verify schema bootstrap ordering in WAL recovery and checkpoint
- bd-u5vi: Verify LIVE view retraction handling and Op ordering invariant

### Recommended Order

1. **bd-gkln**: Run Kani harnesses (low effort, just run + review results)
2. **bd-4pna + bd-u5vi**: Verification investigations (medium, may find bugs)
3. **bd-83j4**: XOR fingerprint (INV-FERR-074) — medium feature, spec already authored
4. **bd-218b**: Cuckoo filter — shares `unique_entity_ids` with MPH, medium feature
5. **bd-a7s1**: Parallel index sort via rayon — medium, new dependency

### Ready Queue
```bash
br ready          # Show unblocked issues
bv --robot-next   # Top pick with reasoning
```

## Hard Constraints

- `#![forbid(unsafe_code)]` in all crates except ferratomic-core (ADR-FERR-020, `#![deny(unsafe_code)]`)
- No `unwrap()` in production code (test code may use `expect()` with descriptive messages)
- `CARGO_TARGET_DIR=/data/cargo-target` — NOT auto-configured
- Zero lint suppressions (`#[allow(...)]`) anywhere, including tests
- No `#[cfg(...)]` hiding production code from the type checker
- Phase N+1 cannot start until Phase N passes isomorphism check
- Every fix must pass cleanroom review (lifecycle/06) before bead closure
- Do NOT close quality-score EPICs (10.0 targets) without independent progress review
- Subagents must read GOALS.md and lifecycle/05 in full before writing code
- Worktrees FORBIDDEN (corrupts .beads/ and .cass/)

## Stop Conditions

Stop and escalate to the user if:
- A fix requires modifying a spec invariant's Level 0 algebraic law
- A dependency edge removal would bypass a verification gate
- A test failure suggests an algebraic correctness issue (not just a test gap)
- The progress review scores below 8.0 on any vector
- Any proposed change conflicts with GOALS.md Tier 1 values (algebraic correctness, append-only durability, safety)
- Uncertainty about whether a change is correct — ask rather than guess
