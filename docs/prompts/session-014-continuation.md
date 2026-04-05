# Ferratomic Continuation — Session 014

> Generated: 2026-04-04
> Last commit: `720bc60` "fix: session 013 R4 audit fixes — schema deep equality, doc corrections"
> Branch: main

## Read First

1. `QUICKSTART.md` — project orientation
2. `AGENTS.md` — guidelines and constraints
3. `spec/README.md` — load only the spec modules you need
4. `docs/prompts/lifecycle/06-cleanroom-review.md` — the 9-phase review protocol

## Session 013 Summary

### Delivered

**Two features:**
- **CHD Perfect Hash** (`ferratomic-core/src/mph.rs`, NEW): O(1) entity existence + position lookup via 3-hash Compress-Hash-Displace. `MphBackend` trait abstracts backend swaps. Zero-copy verification against canonical array (no keys duplication). ~9 bytes/entity. Integrated into `PositionalStore::entity_lookup`.
- **LIVE-first V3 Checkpoint** (`ferratomic-core/src/checkpoint/v3.rs`): Version 0x0103 stores LIVE datoms first, historical second. `PartialStore` with `live_store()` accessor for partial cold start. Version dispatch in `deserialize_checkpoint_bytes`. O(n) all paths via `from_checkpoint_v3`.

**Four cleanroom audit rounds:**
- R1: 21 findings, all fixed
- R2: 20 findings, 13 fixed, 7 closed (wontfix/deferred)
- R3: 10 findings, 7 fixed, 3 closed
- R4: 5 findings, 4 fixed, 1 deferred (OnceLock poisoning)
- **Convergence**: 21 → 20 → 10 → 5. Zero CRITICALs × 4 rounds.

**Spec corrected:**
- ADR-FERR-030: "MMPH" → "O(1) monotone rank computation" with PtrHash Phase 4c+ target
- INV-FERR-075: fingerprint deferral noted, partition refinement documented

**Test coverage added:**
- 6 MPH unit tests, 5 MPH proptests (incl. post-merge, binary search fallback)
- 10 LIVE-first unit tests (incl. error paths, LIVE-only query, mixed groups)
- 2 LIVE-first proptests with randomized retraction fractions
- Schema deep equality in ALL checkpoint proptests

### Architecture Decisions

- **CHD not MMPH**: The hash function is non-monotone. Monotone rank comes from sorted verification. True MMPH (PtrHash, 2.0 bits/key) is Phase 4c+ optimization. `MphBackend` trait is the swap point.
- **Rank-space reduction**: ADR-FERR-030 wavelet matrix needs O(1) rank. Current CHD provides it. PtrHash eliminates the 32n-byte verification table when ready.
- **Bitvector-OR CRDT**: The information-theoretic convergence target is per-column wavelet matrices where merge = level-wise bitwise OR. This is the "alien artifact" — store, index, merge, and query are the same structure.

### Bugs Found / Known Gaps

- **entity_lookup fallback dispatch**: Line 456 in positional.rs has zero coverage through entity_lookup itself. The fallback (binary search when MPH build fails) is tested via `first_datom_position_for_entity` directly, not through the actual dispatch in `entity_lookup`. Requires OnceLock poisoning infrastructure to test properly.

### Stopping Point

All session 013 work committed and pushed. 4 commits on main. 122 tests passing. Zero clippy warnings. All pre-commit gates pass. Lean proofs untouched (no Lean changes this session).

The codebase compiles, passes all gates, and has been through 4 cleanroom review rounds with convergent findings.

## Next Execution Scope

### Primary: Phase 4a Gate Closure

The gate chain:
```
remaining pre-session-013 audit beads → bd-7fub.22.10 (re-review at 10.0) → bd-y1w5 (tag) → bd-add (Phase 4a gate closes) → 17+ Phase 4b beads unblocked
```

Check what's blocking the gate:
```bash
br show bd-add          # What blocks Phase 4a closure
br show bd-7fub.22.10   # The re-review gatekeeper
br ready                # What's actionable now
bv --robot-triage       # Full ranked recommendations
```

### Alternative: More Feature Work

If the gate has other blockers beyond audit beads:
```bash
br ready | grep -v AUDIT   # Non-audit ready work
```

Key Phase 4a features still open:
- bd-0zfw: Chunk Fingerprint Array (INV-FERR-079)
- bd-218b: Cuckoo filter pre-filter (shares unique_entity_ids with MPH)
- bd-gkln: Full Kani harness overnight run

### Alternative: Phase 4b Prep

If Phase 4a gate is close to closing, start Phase 4b spec expansion:
- bd-3gk: EPIC: Phase 4b specification expansion
- Prolly tree (INV-FERR-045..050)
- WriterActor with group commit
- Value pooling (bd-kt98) for wavelet matrix prerequisite

## Hard Constraints

- `#![forbid(unsafe_code)]` in all crates except ferratomic-core (ADR-FERR-020)
- No `unwrap()` in production code
- `CARGO_TARGET_DIR=/data/cargo-target` — NOT auto-configured
- Zero lint suppressions (`#[allow(...)]`)
- Phase N+1 cannot start until Phase N passes isomorphism check
- Every fix must pass cleanroom review before bead closure
- Subagents must read GOALS.md and lifecycle/05 in full before writing code

## Stop Conditions

Stop and escalate to the user if:
- A fix requires modifying a spec invariant's Level 0 algebraic law
- A dependency edge removal would bypass a verification gate
- A test failure suggests an algebraic correctness issue
- Any proposed change conflicts with GOALS.md Tier 1 values
- Uncertainty about whether a change is correct — ask rather than guess
