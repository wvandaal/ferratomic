# Ferratomic Continuation — Session 025

> Generated: 2026-04-10
> Last commit: `418c7a8` "docs: session 024 closeout — QUICKSTART + continuation handoff"
> Branch: main (synced with master)

## Read First

1. `QUICKSTART.md` — project orientation (updated 2026-04-10)
2. `AGENTS.md` — guidelines, hard constraints, crate map
3. `spec/README.md` — 88 invariants, load only what you need
4. `GOALS.md` §7 — Six-Dimension Decision Framework (score every non-trivial decision)

## Session Summary

### Completed (session 024, 2026-04-09/10)

**Spec authoring (sessions 023.5→023.7, ~2200 lines in spec/06)**:
- INV-FERR-045c "Leaf Chunk Codec Conformance" — 802 lines, all 6 layers, composite 10.0
- INV-FERR-045a refactored as DatomPair Reference Codec — layered V1 format
- §23.9.0 trait-aware updates, §23.9.8 Codec Discriminator Registry authored
- DiffIterator full algorithm body (DiffStackEntry enum, merge_join_children, enumerate_subtree)
- Lean theorems for 047/048/050 + byte-level concretization precedent (axioms → concrete defs)
- Tier 1 inline integration annotations (sendfile, IBLT, BP+RMM)
- Edge-case hardening (empty/single/large chunks, codec dispatch, mixed-codec fingerprint)
- All 9 sorry instances closed → 0 sorry in spec/06

**Rust implementation (Phase 4b)**:
- `ferratomic-positional/src/codec.rs` (727 lines): LeafChunkCodec trait, DatomPairCodec,
  DatomPairChunk, LeafChunk enum, framework_fingerprint, conformance harness
- `ferratom/src/datom/mod.rs`: Datom::canonical_bytes() + from_canonical_bytes() per INV-FERR-086
  (all 11 Value variants, with parse helpers factored under 50 LOC each)
- 6 new FerraError variants (TruncatedChunk, TrailingBytes, NonCanonicalChunk, EmptyChunk,
  UnknownCodecTag, NotImplemented)
- content_hash() unified with canonical_bytes by the other agent (session 024 Phase 4a.5).
  **CRITICAL context**: the other agent REWROTE `Datom::content_hash()` in
  `ferratom/src/datom/mod.rs` to stream the INV-FERR-086 canonical format (u16 attr lengths,
  0x01-0x0B value tags) instead of the old ad-hoc format (u64 attr lengths, 0-10 tags).
  They also deleted 3 helper functions (hash_tagged_bytes, hash_tagged_fixed, hash_value)
  and replaced them with canonical_value_hash + hash_tag_u16_str + hash_tag_u32_bytes.
  The other agent's handoff is at `docs/prompts/sessions/2026-04-10-session-024-continuation.md`
  (if it exists) — read it for the full scope of their Phase 4a.5 changes

**Lean mechanization**:
- 7 new .lean files (807 lines): ProllyTreeCodec, DatomPair, Foundation, HistoryIndep,
  Diff, Transfer, Snapshot, Substrate
- 14 complete proofs, 0 sorry. lake build 768/768 green.

**Kani harnesses**:
- codec_conformance.rs: 4 bounded proofs (payload round-trip, determinism, dispatch, unknown tag)
- error_exhaustiveness.rs: updated 12→20 FerraError variants

**Fuzz targets**:
- fuzz_canonical_bytes — Datom::from_canonical_bytes crash + round-trip + content_hash oracle
- fuzz_codec_payload — DatomPairCodec::decode_payload crash + round-trip oracle

**Tests**: 23 codec (18 unit + 5 proptest at 10K cases) + 5 datom round-trip = 28 tests

**Audits performed**:
- 4 spec audits (lifecycle/17): 1 CRITICAL + 10 MAJOR + 11 MINOR = 22 findings, all fixed
- 1 verification audit (lifecycle/18): 2 P0 + 5 P1 + 4 P2 = 20 findings, all fixed
- 1 cleanroom review (lifecycle/06): 2 CRITICAL + 2 MAJOR + 2 MINOR + 1 STYLE = 7 findings, all fixed

### Decisions Made

- **D2 (enum dispatch)**: LeafChunkCodec uses closed-world enum, not static generics or trait objects.
  Matches Phase 4a AdaptiveIndexes precedent. Locked.
- **Fingerprint architecture**: framework_fingerprint XORs stored content_hash values directly
  (no re-hashing). DEFECT-001 from cleanroom review fixed the prior BLAKE3(key++value) bug.
- **canonical_bytes key encoding**: DatomPairChunk entries use canonical_bytes as key (reversible)
  instead of content_hash (one-way). Enables trait-level round-trip.
- **content_hash unification**: Other agent (session 024) unified content_hash() to stream the
  INV-FERR-086 canonical format. content_hash(d) == BLAKE3(canonical_bytes(d)) now holds.

### Bugs Found

- DEFECT-001 (CRITICAL): framework_fingerprint computed BLAKE3(key++value) instead of XOR'ing
  value directly. Would have broken store fingerprint agreement between prolly tree and direct
  INV-FERR-074 computation. Fixed in b7c2879.
- DEFECT-002 (CRITICAL): from_canonical_bytes silently accepted trailing bytes, enabling
  adversarial input normalization bypass. Fixed in b7c2879.
- DEFECT-004 (MAJOR): Bool decoder accepted 0x02-0xFF as true, creating 255 non-canonical
  encodings per value. Fixed in b7c2879.
- VERIFY-DRIFT-002 (P0): framework_fingerprint hashed only keys in the first implementation.
  Fixed in 563281a.
- Three-encoding divergence (content_hash vs canonical_bytes vs bincode): Found and unified
  by the other agent. See their handoff for details.

### Stopping Point

**Exactly where I stopped**: All code is committed and pushed at `418c7a8`. Working tree is
clean for MY files. The other agent has in-progress changes in 9+ files (ferratom/src/datom/*,
ferratom/src/error.rs, ferratomic-verify/integration/*, ferratomic-verify/kani/*) — treat
these as normal per the multi-agent rule (do NOT stash, revert, or overwrite). The codec
implementation is functionally complete: trait, reference impl, tests, proptests, Kani
harnesses, fuzz targets, Lean proofs.

**Last thing verified working**: `cargo test --package ferratomic-positional --lib codec` (23 pass),
`cargo test --package ferratom --lib datom::tests::test_inv_ferr_086` (5 pass), fuzz crate
compiles (`cd fuzz && cargo check`), lake build (768/768, 0 new sorry).

**Next thing to do**: Close bd-b7pfg (Attribute u16 length guard) OR pick from the ready queue.

## Next Execution Scope

### Primary Task

**bd-b7pfg** "Attribute u16 length guard" — the single remaining gap from 9.83 to 10.0.
Make `Attribute::new()` return `Result<Self, FerraError>`, rejecting attributes > 65535 bytes
at the type boundary. This makes the u16 length prefix in canonical_bytes structurally correct
for ALL representable inputs. ~80 call sites across ferratom, ferratomic-store, ferratomic-core,
ferratomic-verify. Mechanical but wide-reaching.

### Ready Queue
```bash
br ready          # Show unblocked issues
bv --robot-next   # Top pick with reasoning
```

### Dependency Context

- bd-b7pfg blocks literal 10.0 implementation composite but does NOT block any other work
- The other agent (Phase 4a.5) is at proptest/implementation stage — avoid touching files
  they're working on (ferratom/src/signing.rs, ferratom/src/bundle.rs, ferratom/src/filter.rs,
  ferratom/src/provenance.rs, ferratomic-store/src/schema_evolution.rs)
- Phase 4b prolly tree BUILDER (build_prolly_tree, split_at_boundaries) is the next
  implementation target after codec — uses the codec types as input

### Alternative Tracks (if user doesn't want bd-b7pfg)

1. **Phase 4b prolly tree builder** — implement `build_prolly_tree` in Rust using
   the `LeafChunkCodec` trait + `DatomPairChunk` types from codec.rs. The spec algorithm
   is fully authored in spec/06 (INV-FERR-046 Level 2).
2. **spec/09 perf audit** — per the True North Roadmap (memory: `roadmap_audit_to_implementation.md`),
   this was the original session 024 scope before the alien stack deep dive redirected us.
3. **Lean proof completion** — mechanize the remaining axioms in ProllyTreeDatomPair.lean
   (u32_le_roundtrip, datom_pair_roundtrip concrete proofs) and ProllyTreeDiff.lean
   (diff_correct, diff_chunk_loads_bound).
4. **Phase 4a.5 support** — the other agent may need help with federation implementation.
   Check their status via `bv --robot-triage`.

## Hard Constraints

- `#![forbid(unsafe_code)]` in all crates. Zero exceptions.
- No `unwrap()`, `expect()`, or `panic!()` in production code (strict clippy gate).
- Zero `#[allow(...)]` anywhere — fix root causes, not symptoms.
- `CARGO_TARGET_DIR=/data/cargo-target` — MUST set. Default fills tmpfs.
- `cargo fmt --all` BEFORE `git add` — prevents pre-commit hook failures.
- Phase N+1 cannot start until Phase N passes isomorphism check.
- GOALS.md §6 defensive engineering standards — all 11 gates must pass.
- GOALS.md §7 Six-Dimension scoring for non-trivial decisions.
- When implementing new serialization: check if existing encoding covers the same domain.
  UNIFY, never silently diverge.

## Stop Conditions

Stop and escalate to the user if:
- Any cargo gate fails and the fix requires changing another agent's in-progress files
- A spec invariant contradiction is discovered between spec/05 (federation) and spec/06 (prolly tree)
- The Attribute length guard change breaks more than 100 call sites (scope creep signal)
- content_hash() produces different values than BLAKE3(canonical_bytes()) for any input
  (the unification invariant is broken — this is a CRITICAL regression)
