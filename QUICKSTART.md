# Ferratomic — Quick Orientation

**What**: Formally verified, distributed embedded datom database engine.
**Core property**: `Store = (P(D), U)` — G-Set CRDT semilattice. Writes never conflict.
**Spec**: `spec/` — see `spec/README.md` for current invariant/ADR/NEG counts.

## Current Phase

Phases 0-3 COMPLETE. **Phase 4a CLOSED 2026-04-08 at composite 9.55-9.57 / A+** within Phase 4a's closed scope. Tag `v0.4.0-gate` at commit `732c3aa`. Closure document at `docs/reviews/2026-04-08-phase-4a-gate-closure.md`. Two independent verification layers: lifecycle/13 deep-mode review (prior session) + bd-snnh empirical 100M validation (today). The 0.43 gap from literal 10.0 is structurally Phase 4b/4c implementation work, not Phase 4a defects.

**Phase 4a.5 + Phase 4b begin in parallel (diamond topology).** Bead audit COMPLETE 2026-04-08 (session 021). **Sessions 022 + 022.5 + 023** (2026-04-08/09): spec/05 §23.8.5 audit + Pattern F federation renumber + Pattern H DEFINITIVELY RESOLVED at the spec layer (§23.9.0 + INV-FERR-045a authored in spec/06; INV count 86→87; spec/06 2043→3295 lines). **Session 023 + alien stack deep dive (2026-04-09)**: produced `docs/ideas/014-the-alien-stack.md` (~2700 lines exploratory synthesis) + 30 lab-grade research beads (Tier 1 high-score actionable through Tier ΩΩΩ speculative) + 8 fail-fast experiment beads. **Codified to canonical sources**: GOALS.md §7 "The Six-Dimension Decision Evaluation Framework" (Performance, **Efficiency**, Accretiveness, Correctness Tier 1, Quality, Optimality — each scored 1-10, literal 10.0 requires all six at 10.0); AGENTS.md "Knowledge Organization Rule" (prescriptive content goes in canonical sources, never in docs/ideas/); README.md §6 (framework surfaced in design philosophy). Trait-based DI architectural correction: `LeafChunkCodec` trait + enum dispatch (matches Phase 4a `AdaptiveIndexes` precedent) becomes the load-bearing accretive lever for every alien artifact. Composite score: 7.9 → target literal 10.0 across 5 sessions (023.5 → 023.7 + Tier 1 implementation). **Phase A starts with session 023.5 Phase 1**: author `INV-FERR-045c "Leaf Chunk Codec Conformance"` in spec/06 (~450 lines, ~1.5-2 hours, full 6-layer template per gold standard INV-FERR-001). See: `docs/prompts/sessions/2026-04-09-session-023.5-continuation.md` (handoff prompt), `docs/ideas/014-the-alien-stack.md` (exploratory synthesis with status banner pointing to canonical sources), `GOALS.md §7` (canonical scoring framework), `AGENTS.md` "Knowledge Organization Rule" (canonical-vs-exploratory discipline), and the memory file `~/.claude/projects/-data-projects-ddis-ferratomic/memory/session023_alien_stack.md`.

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
