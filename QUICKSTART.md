# Ferratomic — Quick Orientation

**What**: Formally verified, distributed embedded datom database engine.
**Core property**: `Store = (P(D), U)` — G-Set CRDT semilattice. Writes never conflict.
**Spec**: `spec/` — see `spec/README.md` for current invariant/ADR/NEG counts.

## Current Phase

Phases 0-3 COMPLETE. **Phase 4a CLOSED 2026-04-08 at composite 9.55-9.57 / A+.** Tag `v0.4.0-gate` at commit `732c3aa`.

**Session 024 (2026-04-09/10)**: Completed the entire Phase A path (sessions 023.5→023.7) + Phase 4b codec implementation + Lean mechanization + verification audit + cleanroom review. spec/06 grew 3295→5500 lines (+2200 lines spec). codec.rs: 727 lines Rust (LeafChunkCodec trait, DatomPairCodec, 23 tests + 5 proptests at 10K cases). 7 new .lean files (807 lines, 14 complete proofs). Datom::canonical_bytes + from_canonical_bytes (INV-FERR-086). 2 fuzz targets. 4 audit rounds (22 spec findings + 20 verification drift findings + 7 cleanroom defects, all fixed). Spec composite 10.0, implementation composite 9.83 (single remaining gap: bd-b7pfg Attribute u16 length guard). INV count 88.

| Phase | Status |
|-------|--------|
| 0: Specification | DONE |
| 1: Lean proofs (0 sorry) | DONE |
| 2: Tests (red phase) | DONE |
| 3: Type definitions | DONE |
| **4a: Core implementation** | **CLOSED at A+ 2026-04-08 (v0.4.0-gate)** |
| **4a.5: Federation foundations** | **NEXT (diamond track 1)** |
| **4b: Performance + canonical spec form** | **NEXT (diamond track 2)** |
| 4c: Federation/transport | — |
| 4d: Datalog query engine | — |
| 5: Integration | — |

## Where to Start

1. Read `AGENTS.md` — build commands, hard constraints, quality gates, crate map
2. Read `GOALS.md` — value hierarchy, success criteria, defensive engineering standards (§6)
3. Read `spec/README.md` — spec module index (load only what you need)
4. Check project state:

```bash
export CARGO_TARGET_DIR=/data/cargo-target  # CRITICAL — omitting fills /tmp
br ready          # Actionable tasks (no blockers)
bv --robot-next   # Top-priority pick with claim command
```

## Key Documents

| Document | What It Contains |
|----------|-----------------|
| `AGENTS.md` | Build commands, hard constraints, CI gates, code discipline, agentic rules |
| `GOALS.md` | Purpose, value hierarchy, success criteria, defensive engineering standards (§6) |
| `spec/README.md` | Spec module index (canonical invariant/ADR/NEG counts) |
| `docs/prompts/lifecycle/` | One prompt per cognitive phase (17 prompts) |
| `docs/design/` | Migration path, architectural influences, refinement chains |
