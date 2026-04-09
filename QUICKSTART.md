# Ferratomic — Quick Orientation

**What**: Formally verified, distributed embedded datom database engine.
**Core property**: `Store = (P(D), U)` — G-Set CRDT semilattice. Writes never conflict.
**Spec**: `spec/` — see `spec/README.md` for current invariant/ADR/NEG counts.

## Current Phase

Phases 0-3 COMPLETE. **Phase 4a CLOSED 2026-04-08 at composite 9.55-9.57 / A+.** Tag `v0.4.0-gate` at commit `732c3aa`.

**Session 023.5 COMPLETE (2026-04-09)**: INV-FERR-045c "Leaf Chunk Codec Conformance" authored (802 lines, composite 9.50, all 6 verification layers) + INV-FERR-045a refactored as DatomPair reference codec + §23.9.0 trait-aware updates + §23.9.8 Codec Discriminator Registry authored + helper definitions + performance budgets. INV count 87→88. Three lifecycle/17 self-audits caught 5 MAJOR + 7 MINOR findings, all remediated. Six commits (`73b4b63`→`2921a32`). Lean concretization bead filed (`bd-he332`). **Next: session 023.5.5** (trait edge-case hardening).

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
