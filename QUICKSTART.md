# Ferratomic — Quick Orientation

**What**: Formally verified, distributed embedded datom database engine.
**Core property**: `Store = (P(D), U)` — G-Set CRDT semilattice. Writes never conflict.
**Spec**: `spec/` — see `spec/README.md` for current invariant/ADR/NEG counts.

## Current Phase

Phases 0-3 COMPLETE. **Phase 4a (core implementation) — all work must complete before gate.**
bd-add has 35 direct dependencies (all open Phase 4a beads). Key categories:
- **bd-7fub** (Path to 10.0 EPIC) — captures all 11 tier EPICs + ~120 children
- **bd-4i6u** (perf EPIC) — 19 performance beads across 4 tiers
- **bd-flqz** (A+ gate EPIC) — quality vectors at 10.0
- **bd-7fub.22.10** (re-review, IN_PROGRESS) — cleanroom re-review + 10.0/A+ scoring
- **bd-y1w5** (tag v0.4.0-gate) — tag + gate closure document
- **29 standalone tasks** — testing, docs, bugs, code quality
- bd-cly9 (CLOSED) — 11-crate decomposition done
bd-add unblocks 21+ downstream beads (Phase 4b, 4a.5, prolly tree).
Run `br show bd-add` for the full dependency list.

| Phase | Status |
|-------|--------|
| 0: Specification | DONE |
| 1: Lean proofs (0 sorry) | DONE |
| 2: Tests (red phase) | DONE |
| 3: Type definitions | DONE |
| **4: Implementation** | **IN PROGRESS** |
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
