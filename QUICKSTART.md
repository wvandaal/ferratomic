# Ferratomic — Quick Orientation

**What**: Formally verified, distributed embedded datom database engine.
**Core property**: `Store = (P(D), U)` — G-Set CRDT semilattice. Writes never conflict.
**Spec**: `spec/` — see `spec/README.md` for current invariant/ADR/NEG counts.

## Current Phase

Phases 0-3 COMPLETE. **Phase 4a CLOSED 2026-04-08 at composite 9.55-9.57 / A+** within Phase 4a's closed scope. Tag `v0.4.0-gate` at commit `732c3aa`. Closure document at `docs/reviews/2026-04-08-phase-4a-gate-closure.md`. Two independent verification layers: lifecycle/13 deep-mode review (prior session) + bd-snnh empirical 100M validation (today). The 0.43 gap from literal 10.0 is structurally Phase 4b/4c implementation work, not Phase 4a defects.

**Phase 4a.5 + Phase 4b begin in parallel (diamond topology).** Bead audit COMPLETE 2026-04-08 (session 021): all 112 beads (27 4a.5 + 85 4b) audited at lab-grade depth. 170 findings, 8 cross-phase patterns. Pattern H DEFINITIVELY RESOLVED via cass + git log + grep triple-confirmation. Roadmap codified in `docs/reviews/2026-04-08-phase-4a5-4b-audit.md` §16-20. **Sessions 022 + 022.5 progress (2026-04-08/09)**: 7 hidden Phase 4b orphans audited (Pattern I 9th NEW pattern discovered: 2 of 4 Session 015 cleanroom-finding orphans were already-fixed-but-unclosed); spec/05 §23.8.5 spec audit complete (18 elements, 26 findings 200-225); Phase 4 remediation executed in two waves: (1) session 022 main pass — Pattern F federation-side renumber (ADR-031/032/033 → 034/035/036 in spec/05; spec/09 perf ADRs preserved at original numbers) + 5 CRITICAL/MAJOR finding fixes inline, (2) session 022.5 immediate follow-up — both deferred CRITICAL findings RESOLVED: FINDING-219 (Transport scope leak; replaced INV-038's stale `#[async_trait]` Transport with canonical `Pin<Box<dyn Future>>` version + removed INV-025b duplicate) and FINDING-206+207 (spec/03 INV-FERR-029 Level 0 tie-breaking amendment + Level 2 three-path equivalence + bd-l64y regression proptest). **All 5 §23.8.5 CRITICAL findings resolved.** **Next phase**: session 023 = spec audit Section 7 (spec/06 prolly tree) — AUTHOR INV-FERR-045a + §23.9.0 to resolve Pattern H per audit doc §18.1. Then session 024 spec/09 perf, session 025 Phase 3 reconciliation, session 027+ implementation in parallel diamond. See roadmap memory `~/.claude/projects/-data-projects-ddis-ferratomic/memory/roadmap_audit_to_implementation.md` and session 023 handoff at `docs/prompts/sessions/2026-04-08-session-022-continuation.md`.

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
