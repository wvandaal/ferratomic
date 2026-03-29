# Ferratomic — Agent Guidelines

> Ferratomic is a formally verified, distributed embedded datom database engine.
> It is the storage foundation for braid and any system built on the datom model.

---

## True North

Ferratomic provides the universal substrate: an append-only datom store with
content-addressed identity, CRDT merge, indexed random access, and cloud-scale
distribution. It is to braid what PostgreSQL is to a web application — the
foundational infrastructure that everything else builds on.

**Store = (P(D), ∪)** — a G-Set CRDT semilattice. Writes are commutative,
associative, and idempotent by construction. No conflicts. No consensus protocol.
The data structure IS the consistency mechanism.

---

## Development Methodology: Spec-First TDD (Curry-Howard-Lambek)

**Non-negotiable phase ordering:**
```
Phase 0: Formal specification (spec/23-ferratomic.md in braid)  ← DONE
Phase 1: Lean 4 theorem statements + proofs
Phase 2: Test suite (Stateright, Kani, proptest) — ALL FAIL (red phase)
Phase 3: Type definitions (ferratom crate — types ARE propositions)
Phase 4: Implementation (ferratomic-core — programs ARE proofs)
Phase 5: Integration (braid-kernel migration)
```

**Phase gate**: Phase N+1 CANNOT begin until Phase N passes isomorphism check.
A gap between spec, algebra, and tests is a DEFECT, not technical debt.

---

## Specification

The canonical specification lives in braid's tree (colocated with the consumer):
- **Formal spec**: `../ddis-braid/spec/23-ferratomic.md` (36 INV, 7 ADR, 5 NEG)
- **Architecture**: `../ddis-braid/docs/design/FERRATOMIC_ARCHITECTURE.md`

Local documentation:
- **docs/spec/**: Symlinks to braid spec for locality
- **docs/design/**: Ferratomic-specific design decisions

---

## Crate Architecture

```
ferratomic/
├── ferratom/           # Leaf: core types (ZERO project deps)
├── ferratomic-core/    # Core: storage + concurrency engine
├── ferratomic-datalog/ # Facade: query engine
└── ferratomic-verify/  # Verification: Lean 4 + Stateright + Kani + proptest
```

Dependency direction: leaf → core → facade. No cycles.

---

## Hard Constraints

**C1: Append-only store.** Never delete or mutate datoms. Retractions are new datoms.
**C2: Content-addressed identity.** EntityId = BLAKE3(content).
**C4: CRDT merge = set union.** Commutative, associative, idempotent.
**INV-FERR-023: `#![forbid(unsafe_code)]`** in ALL crates. No exceptions.
**NEG-FERR-001: No panics.** No `unwrap()`, no `expect()` in production code.

---

## Build

```bash
CARGO_TARGET_DIR=/data/cargo-target cargo check --workspace
CARGO_TARGET_DIR=/data/cargo-target cargo clippy --workspace -- -D warnings
CARGO_TARGET_DIR=/data/cargo-target cargo test --workspace
```

Lean proofs:
```bash
cd ferratomic-verify/lean && lake build
```

---

## Quality Standard

`ms load rust-formal-engineering -m --full` — the standard methodology.
Every type encodes an invariant. Every function proves a property.
NASA-grade, zero-defect, cleanroom engineering. No shortcuts.
