# Ferratomic Continuation — Session 008: Phase 4a.5 Federation Foundations Spec

> Generated: 2026-04-02
> Last commit: c0017c7 "feat: PositionalStore — positional content addressing core (INV-FERR-076)"
> Branch: main

## Read First

1. `AGENTS.md` — guidelines and constraints (note: `ferratom-clock` crate exists now)
2. `spec/05-federation.md` — **THE target file** — read lines 4922-5760 (§23.8.5, written in session 005)
3. `spec/01-core-invariants.md` — INV-FERR-031 (genesis determinism) needs amendment
4. `spec/README.md` — counts need updating after new invariants
5. `docs/prompts/lifecycle/16-spec-authoring.md` — **THE methodology** — follow exactly
6. `docs/prompts/lifecycle/17-spec-audit.md` — run AFTER spec authoring completes
7. `/home/ubuntu/.claude/plans/parsed-questing-moon.md` — comprehensive 49-bead plan

## Load Before Starting

```bash
ms load spec-first-design -m --full    # Formal specification methodology
```

## The Big Idea: Self-Verifying Spec Store

Phase 4a.5 adds federation foundations: transaction signing, causal predecessors,
store identity, provenance typing, selective merge, filtered observers, and
LocalTransport. The capstone (B17) stores the Phase 4a.5 spec AS SIGNED DATOMS
in a ferratomic store — making the store its own verification oracle.

**Why this matters**: Every day of signed transactions is provenance history.
Signed transactions + causal predecessors + CRDT merge = decentralized trustless
knowledge chain WITHOUT consensus. The store that stores its own spec can query
"am I correct?" by checking its own datoms.

## Session 005 Summary

### Completed
- Deep exploration of all 4 crates, all 6 ideas documents, braid archaeology
- 49-bead plan with 14 design decisions (`/home/ubuntu/.claude/plans/parsed-questing-moon.md`)
- Structural beads: bd-oiqr (epic), bd-r3um (gate), bd-bdvf (B01 spec authoring)
- Diamond topology wired: `bd-add → bd-r3um → bd-fzn` (4a.5 parallel with 4b)
- **Spec authoring ~40% complete** in `spec/05-federation.md` lines 4922-5760:
  - §23.8.5 scope section with 5 design principles
  - ADR-FERR-011 through ADR-FERR-019 (9 ADRs, all complete)
  - INV-FERR-060: Store Identity Persistence (all 6 verification layers)
  - INV-FERR-061: Causal Predecessor Completeness (all 6 verification layers)
  - INV-FERR-062: Merge Receipt Completeness (all 6 verification layers)

### Decisions Made (14 total — ALL settled, do NOT re-derive)
- D1: Signatures as datoms (tx/signature, tx/signer metadata datoms)
- D2: Per-transaction signing (explicit key, not per-Database)
- D3: Async Transport via std only (Pin<Box<dyn Future>>, zero runtime deps)
- D4: Positive-only DatomFilter (All, AttributeNamespace, FromAgents, Entities, And, Or)
- D5: Genesis 19→23 (tx/signature + tx/signer + tx/predecessor + tx/provenance)
- D6: Transaction-level federation (SignedTransactionBundle preserves signing boundary)
- D7: Causal predecessors as datoms (Frontier → predecessor datoms at commit, ~30 LOC)
- D8: Store identity via self-signed first transaction (root of trust)
- D9: Universal Index Algebra (DatomIndex trait; Text/Vector/Spatial/Temporal/Graph; Option<Box<dyn>>; graceful degradation; index config as datoms)
- D10: Diamond phase topology (4a.5 parallel with 4b; 4c depends on both)
- D11: Observer-config as schema convention (NOT engine concept — defer to 4c)
- D12: ProvenanceType lattice (Observed > Derived > Inferred > Hypothesized)
- D13: Merge receipts as datoms (queryable federation history)
- D14: Genesis determinism not cardinality (dynamic count, not hardcoded)

### Stopping Point
`spec/05-federation.md` line 5760 — after INV-FERR-062 closing `---` marker.
The ADRs and three core invariants are complete. The REMAINING items below have
NOT been written yet.

## Next Execution Scope

### Primary Task: Complete B01 (bd-bdvf) — Spec Authoring

**This is the single critical path item.** Everything else is blocked on it.
Follow `docs/prompts/lifecycle/16-spec-authoring.md` exactly.

### What Remains (append after line 5760 of spec/05-federation.md)

**1. INV-FERR-025b: Universal Index Algebra & Graceful Degradation (Stage 1)**

Write with all 6 verification layers. Key properties:
- Universal `DatomIndex` trait: `observe(&mut self, datom, schema)`, `retract(...)`, `rebuild(...)`, `name() -> &str`
- Homomorphism contract: `I(S₁ ∪ S₂) = I(S₁) ⊕ I(S₂)` for all conforming indexes
- Five index families: sort-order (EAVT/AEVT/VAET/AVET — required), LIVE (required), Text (optional), Vector (optional), Extensible (optional)
- Optional indexes as `Option<Box<dyn Trait>>` with NullDefaults — zero overhead when None
- Graceful degradation: text falls back to O(n) scan when absent; vector returns empty (engine doesn't claim semantic understanding without app-provided embeddings)
- `TextIndex` trait: insert, remove, search, rebuild. Fully specifiable tokenization.
- `VectorIndex` trait: insert, remove, search, rebuild. Requires app-provided `EmbeddingFn`
- `EmbeddingFn` trait: `embed(&self, value: &Value) -> Option<Vec<f32>>`, `dimension() -> usize`
- **Key algebraic insight**: Per-datom embedding IS a homomorphism (distributes over union). The distinction from FTS is specifiability (model-dependent), not algebra. Both IN as injectable traits.
- Index config as datoms: `:index/*` namespace (Phase 4b impl)
- Stage 1: spec now, implementation deferred to Phase 4b

**2. INV-FERR-031 amendment (in spec/01-core-invariants.md)**

Find the existing INV-FERR-031 (Genesis Determinism) and amend:
- Change "19 axiomatic meta-schema attributes" → assert DETERMINISM not CARDINALITY
- Document 4 new genesis attributes: tx/signature (Bytes, One, LWW), tx/signer (Bytes, One, LWW), tx/predecessor (Ref, Many, MultiValue), tx/provenance (Keyword, One, LWW)
- Braid lesson: compute count dynamically (no hardcoded constant)
- Update Level 0, Level 1, Level 2, proptest, Lean to reflect new attributes

**3. INV-FERR-039 staging note (in spec/05-federation.md)**

Find existing INV-FERR-039 (Selective Merge) and add a staging note:
"Phase 4a.5 implements selective merge with positive-only DatomFilter (ADR-FERR-012).
Not/Custom/AfterEpoch variants deferred to Phase 4c."

**4. INV-FERR-051 staging note (in spec/05-federation.md)**

Find existing INV-FERR-051 (Signed Transactions) and add a staging note:
"Phase 4a.5 implements Ed25519 signing without Merkle proof binding (needs prolly tree).
Signing message = blake3(sorted_user_datoms ∥ tx_id ∥ sorted_predecessor_tx_ids ∥ signer_public_key).
Metadata datoms (tx/signature, tx/signer, tx/predecessor, tx/provenance, tx/time, tx/agent) are
excluded from the signing message per ADR-FERR-011."

**5. Level 2 type definitions (within §23.8.5)**

Specify as Rust contracts (conceptual, using BTreeSet per spec convention):
- `DatomFilter` enum — 6 variants + `matches(&self, &Datom) -> bool`
- `TxSignature([u8; 64])` — opaque newtype, Serialize/Eq/Ord/Hash/Clone/Copy
- `TxSigner([u8; 32])` — opaque newtype, same derives
- `SignedTransactionBundle { tx_id: TxId, datoms: Vec<Datom>, signature: Option<TxSignature>, signer: Option<TxSigner>, predecessors: Vec<TxId>, provenance: Option<ProvenanceType> }`
- `ProvenanceType` enum — Observed(1.0)/Derived(0.8)/Inferred(0.5)/Hypothesized(0.2) with Ord + confidence()
- `Transport` trait — fetch_datoms, fetch_signed_transactions, schema, frontier, ping — all returning `Pin<Box<dyn Future<...> + Send + '_>>`
- `LocalTransport { db: Arc<Database> }` implementing Transport

**6. Schema conventions (documentation within §23.8.5)**

- Six-layer namespace conventions (from doc 005):
  `:world/*` (Layer 1), `:structure/*` (Layer 2), `:cognition/*` (Layer 3),
  `:conversation/*` (Layer 4), `:interface/*` (Layer 5), `:policy/*` (Layer 6)
- Observer-config convention: `:observer-config/*` namespace
- Agent identity convention: `:agent/*` namespace (public-key, namespace, name, role)
- Verification evidence schema: `:verification/*` (lean-status, proptest-passes, proptest-failures, confidence, kani-status, stateright-status, evidence-hash) and `:gate/*` (verdict, blocking-invariants)

**7. spec/README.md update**

Update INV-FERR count (was 59, now 62 + 025b = 63), ADR count (was 10, now 19).

**8. Five-lens convergence protocol**

After ALL content written, run 5 sequential single-lens passes over the entire
new §23.8.5 section (including the 3 already-written invariants):
1. **Completeness**: What fields are missing?
2. **Soundness**: Are proof sketches correct?
3. **Simplicity**: Simplest mathematical structure that works?
4. **Adversarial**: How would an adversary break each invariant?
5. **Traceability**: Does every thread trace through every layer?

Converged when a pass produces zero structural changes.

### After B01 Completes

1. Run spec audit per `docs/prompts/lifecycle/17-spec-audit.md` on amended spec/05-federation.md
2. Create remaining 46 beads (B02-B17, V01-V03, R01-R27) per plan file
3. Execute implementation beads in dependency order (L0→L6)

### Ready Queue
```bash
br show bd-bdvf    # B01 — the spec authoring bead (PRIMARY TASK)
br ready            # Other ready work (Phase 4a gate closure, performance work)
bv --robot-next     # Top pick with reasoning
```

### Key Beads
- **bd-bdvf**: B01 spec authoring (THIS SESSION'S TARGET)
- **bd-r3um**: Phase 4a.5 gate (depends on all Phase 4a.5 beads)
- **bd-oiqr**: Phase 4a.5 epic (container)
- **bd-add**: Phase 4a gate (bd-bdvf depends on this — check if closed)

## Hard Constraints

- `#![forbid(unsafe_code)]` in all crates (except ADR-FERR-020 localized mmap module)
- No `unwrap()` in production code
- `CARGO_TARGET_DIR=/data/cargo-target`
- Every INV-FERR needs all 6 verification layers (Level 0 + Level 1 + Level 2 + falsification + proptest + Lean)
- Every ADR follows the template: Problem/Options/Decision/Rejected/Consequence/Source
- All cross-references bidirectional
- Spec-first: NO implementation before spec converges
- The 14 design decisions listed above are SETTLED — do not re-derive or re-debate

## Stop Conditions

Stop and escalate to the user if:
- Any design decision from the 14 listed above seems wrong upon deeper analysis
- An invariant's Level 0 proof has a gap that can't be resolved
- Two invariants contradict each other
- The spec exceeds 7000 lines (may need splitting into 05a/05b)
- You discover a dependency on Phase 4b infrastructure that wasn't anticipated
- The five-lens convergence doesn't converge within 3 passes

## Context Pointers

These files contain the full reasoning behind every decision. Read them if you
need to understand WHY a decision was made, not just WHAT was decided:

- `/home/ubuntu/.claude/plans/parsed-questing-moon.md` — 49-bead plan with all rationale
- `/home/ubuntu/.claude/projects/-data-projects-ddis-ferratomic/memory/project_phase_4a5.md` — design decision summary
- `/home/ubuntu/.claude/projects/-data-projects-ddis-ferratomic/memory/project_session005_handoff.md` — detailed continuation state
- `/home/ubuntu/.claude/projects/-data-projects-ddis-ferratomic/memory/feedback_semantic_homomorphism.md` — why VectorIndex IS a homomorphism
- `docs/ideas/000-agentic-systems-algebra.md` — agent decomposition, associate mechanism
- `docs/ideas/003-ferratomic-distributed-cognition.md` — store-messaging unification
- `docs/ideas/005-everything-is-datoms.md` — six-layer stack, bilateral Y-combinator
