# SEED.md Axiom Reference (Local Copy)

This document is a self-contained summary of the external SEED.md axioms referenced
throughout the Ferratomic specification. SEED.md is the foundational design document
that predates this repository. The spec references it via `SEED.md §N` notation as
provenance traces. This local copy makes the spec self-contained.

---

## §1: Foundational Epistemology

**Key phrase**: "verifiable coherence"

SEED.md §1 establishes the quality criterion for the entire system: knowledge must be
**verifiably coherent**, not merely stored. A datum is not knowledge until its
provenance can be traced, its integrity verified, and its relationship to other data
confirmed. This axiom drives the content-addressed identity model (C2), the causal
traceability constraint (C5), and the federation trust architecture (Merkle proofs,
signed digests, VKC verification).

**Referenced by**: Trust and verification invariants in spec/05-federation.md
(INV-FERR-051 through INV-FERR-055), ADR-FERR-009.

---

## §4: Core Abstraction

**Key phrases**: "datoms", "identity", "snapshots", "schema-as-data", "CRDT merge",
"calibrated policies are transferable", "append-only store", "substrate independence"

SEED.md §4 defines the datom as the atomic unit of knowledge: the five-tuple
`[entity, attribute, value, transaction, operation]`. It establishes the core design
commitments that become Ferratomic's constraints:

- **Axiom 1 (Append-only)**: Never delete or mutate datoms; retractions are new datoms (C1).
- **Axiom 2 (Store)**: The store is `(P(D), U)` -- a power set of datoms under set union. CRDT merge is pure set union: commutative, associative, idempotent (C4, L1-L5).
- **Axiom 3 (Snapshots)**: Point-in-time views are immutable under future writes (INV-FERR-006).
- **Design Commitment: Identity**: EntityId = BLAKE3(content). Content-addressed, coordination-free (C2).
- **Design Commitment: Schema-as-data**: Schema is defined by datoms, not hardcoded. Schema evolution is a transaction (C3).
- **Design Commitment: Traceability**: Every datom records provenance -- who, when, why, and what was known (C5).
- **Design Commitment: Substrate Independence**: The engine has no knowledge of application-layer concerns. Domain-neutral primitives only (C8).
- **Design Commitment: Temporal Ordering**: Hybrid logical clocks provide causal ordering across nodes.
- **Design Commitment: Implementation Architecture**: The crate decomposition and dependency DAG.
- **Design Commitment: CRDT merge scales**: "CRDT merge scales learning across organizations" -- federation is a structural consequence of the algebra, not a bolt-on feature.
- **Calibrated policies are transferable**: Trust policies established in one context carry across federation boundaries via the CRDT merge.

**Referenced by**: Nearly every invariant in the spec traces to §4. It is the root axiom.

---

## §5: Harvest/Seed Lifecycle

**Key phrase**: "durability", "recovery", "knowledge persistence across conversation boundaries"

SEED.md §5 defines the lifecycle model: knowledge is **harvested** (observed, ingested,
transacted) and **seeded** (persisted, recovered, replicated). The harvest/seed cycle is
the durability contract: once a transaction commits, its datoms survive process crashes,
disk failures, and conversation boundaries. This axiom drives the WAL-before-snapshot
discipline (INV-FERR-008), crash recovery (INV-FERR-014), and the append-only durability
guarantees.

**Referenced by**: Durability and recovery invariants in spec/02-concurrency.md
(INV-FERR-008, INV-FERR-014), spec/03-performance.md (checkpoint, WAL).

---

## §9.2: Central Finding

**Key phrase**: "substrate divergence", "the memory architecture bottleneck"

SEED.md §9.2 identifies the central finding that motivated Ferratomic: the memory
architecture bottleneck in agentic systems. When multiple agents operate on shared
knowledge, substrate divergence -- different agents using incompatible storage formats,
query models, or consistency guarantees -- becomes the primary scaling obstacle. The
solution is a single algebraic substrate (the datom store) that all agents share,
eliminating format translation, consistency negotiation, and knowledge fragmentation.

**Referenced by**: INV-FERR-024 (Substrate Agnosticism) in spec/02-concurrency.md.

---

## §10: The Bootstrap

**Key phrase**: "self-hosting", "genesis", "self-verifying systems", "epistemic foundation"

SEED.md §10 defines the bootstrap axiom: the system must be capable of describing itself.
Genesis is deterministic and self-describing -- the schema that defines datoms is itself
stored as datoms (C7). This creates a self-verifying foundation: the system's own
invariants are expressible and checkable within the system. The bootstrap axiom also
implies that durability mechanisms (WAL, checkpoints, recovery) must themselves be
verifiable from first principles, not assumed correct.

**Referenced by**: Genesis and self-bootstrap invariants in spec/03-performance.md
(INV-FERR-013, INV-FERR-014), spec/04-decisions-and-constraints.md (ADR-FERR-007),
spec/05-federation.md (federation bootstrap), spec/08-verification-infrastructure.md
(epistemic foundation for verification).
