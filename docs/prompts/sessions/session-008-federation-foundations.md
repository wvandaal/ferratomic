# Ferratomic Continuation — Session 008: Phase 4a.5 Federation Foundations

> Generated: 2026-04-03
> Last commit: 30a45b2 "docs: complete cleanroom review — 1 CRITICAL (fixed), 2 MAJOR (fixed), 4 MINOR (filed)"
> Branch: main

## Read First

1. `AGENTS.md` — project guidelines (note: `ferratom-clock` crate exists)
2. `spec/05-federation.md` lines 4945-6623 — **THE spec content written in session 005** (§23.8.5)
3. `spec/03-performance.md` lines 1007-1060 — INV-FERR-031 amendment (genesis 19→23)
4. `/home/ubuntu/.claude/plans/parsed-questing-moon.md` — 49-bead plan with full rationale
5. `docs/prompts/lifecycle/08-task-creation.md` — bead creation standards
6. `docs/prompts/lifecycle/14-bead-audit.md` — lab-grade bead template

## Load Before Starting

```bash
ms load spec-first-design -m --full
```

## Session 005 Summary

### Completed

- Deep codebase exploration (all 4 crates), all 6 ideas documents (000-005)
- Braid project archaeology: kernel code, spec, session history via cass
- FrankenSQLite verification pattern research
- 49-bead Phase 4a.5 plan (`/home/ubuntu/.claude/plans/parsed-questing-moon.md`)
- **B01 spec authoring COMPLETE** (bd-bdvf — all 10 acceptance criteria met, five-lens converged):
  - §23.8.5 scope section (5 design principles)
  - ADR-FERR-021..029 (9 ADRs, all audited, ID collisions fixed)
  - INV-FERR-060: Store Identity Persistence (all 6 verification layers)
  - INV-FERR-061: Causal Predecessor Completeness (all 6 layers)
  - INV-FERR-062: Merge Receipt Completeness (all 6 layers)
  - INV-FERR-063: Provenance Lattice Total Order (all 6 layers)
  - INV-FERR-025b: Universal Index Algebra & Graceful Degradation (Stage 1, all layers)
  - §23.8.5.1: Type definitions (DatomFilter, TxSignature, TxSigner, SignedTransactionBundle, ProvenanceType, Transport, LocalTransport, DatomIndex, TextIndex, VectorIndex, EmbeddingFn)
  - §23.8.5.2: Schema conventions (six-layer namespaces, observer-config, agent identity, verification evidence schema)
  - INV-FERR-039 staging note (positive-only DatomFilter)
  - INV-FERR-051 staging note (signing message with predecessors)
  - INV-FERR-031 amendment (genesis 19→23, determinism not cardinality)
  - spec/README.md updated (71 INV, 24 ADR)
  - Back-references added to INV-FERR-004, 014, 016, 039, 040, 051
- Spec audit (lifecycle/17): 23/23 findings resolved (3 CRITICAL, 9 MAJOR, 11 MINOR)
- Five-lens convergence: CONVERGED (Lens 1: 1 fix; Lens 2-5: 0; recheck: 0)
- Structural beads: bd-oiqr (epic), bd-r3um (gate), bd-bdvf (B01)
- Diamond topology wired: `bd-add → bd-r3um → bd-fzn`

### Decisions Made (14 total — ALL settled, do NOT re-derive)

- D1: Signatures as datoms (tx/signature, tx/signer metadata)
- D2: Per-transaction signing (explicit key, not per-Database)
- D3: Async Transport via std only (Pin<Box<dyn Future>>)
- D4: Positive-only DatomFilter (All, AttributeNamespace, FromAgents, Entities, And, Or)
- D5: Genesis 19→23 (tx/signature + tx/signer + tx/predecessor + tx/provenance)
- D6: Transaction-level federation (SignedTransactionBundle, braid insight)
- D7: Causal predecessors as datoms (Frontier → predecessor datoms at commit)
- D8: Store identity via self-signed first transaction (root of trust)
- D9: Universal Index Algebra (DatomIndex/TextIndex/VectorIndex/EmbeddingFn, Option<Box<dyn>>, graceful degradation)
- D10: Diamond phase topology (4a.5 parallel with 4b; 4c depends on both)
- D11: Observer-config as schema convention (NOT engine; deferred to 4c)
- D12: ProvenanceType lattice (Observed > Derived > Inferred > Hypothesized)
- D13: Merge receipts as datoms (queryable federation history)
- D14: Genesis determinism not cardinality (dynamic count)

### Stopping Point

B01 spec authoring is done and five-lens converged. The bead bd-bdvf cannot formally close because it depends on bd-add (Phase 4a gate), which is still open. The spec content IS complete and committed.

The remaining Phase 4a.5 work is: create implementation beads (B02-B17), verification beads (V01-V03), and research beads (R01-R27) from the plan, then execute them.

## Next Execution Scope

### Primary Task: Create Phase 4a.5 Beads

The plan at `/home/ubuntu/.claude/plans/parsed-questing-moon.md` contains 49 fully-specified beads. 3 structural beads exist (bd-oiqr epic, bd-r3um gate, bd-bdvf B01). The remaining 46 beads need to be created with lab-grade descriptions per `docs/prompts/lifecycle/08-task-creation.md` and `docs/prompts/lifecycle/14-bead-audit.md`.

**Create beads in this order:**

1. **Implementation beads B02-B17** (16 beads) — the core Phase 4a.5 work
2. **Correctness verification beads V01-V03** (3 beads) — braid archaeology findings
3. **Research/placeholder beads R01-R27** (27 beads) — future phase work

**Wire dependency edges per the plan's dependency table** (plan file section "Dependency Edge Summary").

**Wire phase gate edges:**
- All B-beads and V-beads depend on bd-bdvf (B01, which depends on bd-add)
- bd-r3um (gate) depends on all B-beads and V-beads
- R-beads depend on bd-r3um (gate) or bd-7ij (4b gate) per the plan

### Braid Correctness Findings (MUST be verification beads)

These are bugs braid discovered through implementation that ferratomic must verify against:

**V01**: LIVE view retraction handling + Op ordering invariant
- Op::Assert < Op::Retract in im::OrdSet is load-bearing for correctness
- Bare retractions must remove values from LIVE set
- Cardinality::Many must NOT be served by LIVE view
- Source: braid session `56ece7bd` (SOUND-LIVE-v2), session `6250045e`

**V02**: Schema bootstrap ordering in WAL recovery and checkpoint load
- Schema-defining datoms and user datoms may arrive in wrong order during recovery
- evolve_schema() must run before validation, or recovery must bypass validation
- Source: braid session `ed016be3`, `9c83906c`

**V03**: Implicit LWW via iteration-order assumptions
- Any code that iterates datoms and picks "the last one" for Cardinality::One is fragile
- Must use LIVE view or explicit max-by-TxId, never iterate-and-overwrite
- Source: braid session `c01bb082`

### Key Beads (from the plan)

| Level | Beads | Description |
|-------|-------|-------------|
| L1 (types) | B02-B06 | TxSignature/TxSigner, DatomFilter, genesis 23, error variants + ProvenanceType, SignedTransactionBundle |
| L2 (bridge) | B07-B09 | ReplicaFilter bridge, sign in transact + predecessors, Ed25519 sign/verify |
| L3 (integration) | B10-B13 | Filtered observers, selective merge + receipts, Transport trait, LocalTransport |
| L4 (identity) | B14-B15 | Store identity constructor, federation conventions docs |
| L5 (verification) | B16 | Integration test suite |
| L6 (bootstrap) | B17 | Self-verifying spec store (the capstone — stores Phase 4a.5 spec as signed datoms) |

### The Capstone: B17 Self-Verifying Spec Store

B17 is the single most important bead. It stores Phase 4a.5's own spec as signed datoms in a ferratomic store — making the store its own verification oracle. It composes ALL Phase 4a.5 features (signing, identity, predecessors, provenance, selective merge, merge receipts). It installs the `:verification/*` schema for Phase 4b evidence accumulation. Gate closure becomes a Datalog query. This seeds GOALS.md Level 2 ("bootstrap test") and replaces FrankenSQLite's const-array catalog with datoms.

Compound chain: B17 → R16 (FBW witnesses) → ADR-FERR-012 (Bayesian confidence) → ADR-FERR-014 (gate certificates) → M(S) ≅ S.

### Ready Queue

```bash
br show bd-bdvf    # B01 (complete, blocked on bd-add)
br show bd-r3um    # Phase 4a.5 gate
br show bd-oiqr    # Phase 4a.5 epic
br ready            # Other ready work
bv --robot-next     # Top pick
```

### Context Pointers

| What | Where |
|------|-------|
| 49-bead plan (full rationale + dependencies) | `/home/ubuntu/.claude/plans/parsed-questing-moon.md` |
| Phase 4a.5 spec content | `spec/05-federation.md` lines 4945-6623 |
| Genesis amendment | `spec/03-performance.md` lines 1007-1060 |
| Session 005 handoff memory | `~/.claude/projects/-data-projects-ddis-ferratomic/memory/project_session005_handoff.md` |
| Phase 4a.5 design decisions | `~/.claude/projects/-data-projects-ddis-ferratomic/memory/project_phase_4a5.md` |
| Semantic search = homomorphism insight | `~/.claude/projects/-data-projects-ddis-ferratomic/memory/feedback_semantic_homomorphism.md` |
| Braid archaeology findings | `/home/ubuntu/.claude/plans/parsed-questing-moon-agent-ad8608737e7ae4ed8.md` |
| Ideas documents 000-005 | `docs/ideas/` |
| FrankenSQLite verification patterns | `spec/08-verification-infrastructure.md` (ADR-FERR-011..014 in spec/08) |

## Hard Constraints

- `#![forbid(unsafe_code)]` in all crates (except ADR-FERR-020 localized mmap module)
- No `unwrap()` in production code
- `CARGO_TARGET_DIR=/data/cargo-target`
- Every bead follows lab-grade template from lifecycle/14-bead-audit.md
- The 14 design decisions listed above are SETTLED — do not re-derive
- No `ed25519-dalek` in ferratom (leaf crate stays minimal) — signing types are newtypes; ed25519-dalek is in ferratomic-core only
- Max 3 files per bead (scope atomicity)
- Zero lint escape hatches — no `#[allow(...)]` anywhere

## Stop Conditions

Stop and escalate to the user if:
- Any design decision from the 14 above seems wrong upon deeper analysis
- bd-add (Phase 4a gate) has blockers that prevent Phase 4a.5 from starting
- Another agent's compilation errors block bead creation or testing
- The bead dependency graph has cycles after wiring
- A bead requires touching more than 3 files (needs splitting)
- Any bead from the plan contradicts the spec content in §23.8.5

## Workspace State Warning

Other agents modified `ferratomic-core/src/positional.rs`, `store/apply.rs`, `store/mod.rs`, and several ferratomic-verify test files concurrently. Those changes have compilation errors (SortedVecBackend API breaking changes). Our spec-only commits are clean and pushed. The other agent's work is unstaged in the working tree. Investigate before running cargo commands.
