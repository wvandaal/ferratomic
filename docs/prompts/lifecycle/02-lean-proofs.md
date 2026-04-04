# 02 — Lean 4 Theorem Writing

> **Purpose**: Write machine-checked proofs for INV-FERR invariants.
> Proofs are written BEFORE Rust code. The proof defines the contract.
>
> **DoF**: Mixed. High when modeling domain types in Lean.
> Low when translating spec theorems to Lean syntax.

---

## Phase 0: Load Context

```bash
ms load spec-first-design -m --full    # Spec interpretation skill
bv --robot-next                        # Top-priority pick
br update <id> --status in_progress    # Claim it
```

---

## Workflow

```
Read spec invariant (Level 0 algebraic law)
    --> Model types in Lean
    --> State theorem precisely
    --> Prove (mathlib where possible)
    --> Verify: lake build
```

---

## Demonstration: INV-FERR-001 (Merge Commutativity)

### 1. Read the spec

From `spec/01-core-invariants.md`, INV-FERR-001 Level 0:

```
For all A, B in DatomStore:
  merge(A, B) = merge(B, A)

Proof: merge(A, B) = A U B = B U A = merge(B, A)
  by commutativity of set union.
```

### 2. Model in Lean

The DatomStore is `Finset Datom`. Merge is `Finset.union`.
We use mathlib's `Finset` because it gives us decidable equality
and all the lattice theorems for free.

```lean
import Mathlib.Data.Finset.Basic
import Mathlib.Data.Finset.Lattice

variable {Datom : Type} [DecidableEq Datom]

def DatomStore (Datom : Type) [DecidableEq Datom] := Finset Datom

namespace DatomStore

def merge (a b : Finset Datom) : Finset Datom := a ∪ b
```

### 3. State and prove

```lean
/-- INV-FERR-001: Merge commutativity.
    For all A B: merge(A, B) = merge(B, A) -/
theorem merge_comm (a b : Finset Datom) : merge a b = merge b a :=
  Finset.union_comm a b
```

That is the complete proof. `Finset.union_comm` is a mathlib theorem
that proves commutativity of finite set union. Our merge IS set union,
so the proof is direct application.

### 4. Verify

```bash
cd ferratomic-verify/lean && lake build
```

If it type-checks, the proof is valid. If it fails, the theorem
statement or the model is wrong. Fix the model, not the proof.

**CI Gate 9 (GOALS.md §6.8)**: `lake build` runs unconditionally in CI — not gated on commit message keywords. Every commit that changes Lean files or Rust code that Lean models must pass `lake build` with 0 `sorry`.

---

## File Organization

All Lean proofs live under `ferratomic-verify/lean/Ferratomic/`.

| File | INV-FERR | Content |
|------|----------|---------|
| `Store.lean` | 001-004 | CRDT semilattice: commutativity, associativity, idempotency, monotonicity |
| `Index.lean` | 005-007 | Index consistency, completeness, snapshot isolation |
| `Wal.lean` | 008 | WAL-before-snapshot ordering |
| `Schema.lean` | 009-011 | Schema validation, evolution, as-data |
| `Identity.lean` | 012 | Content-addressed identity (BLAKE3 abstracted) |
| `Concurrency.lean` | 013-024 | HLC, checkpoint, atomicity |

Create new files as needed. Each file imports `Mathlib` and declares
theorems corresponding to spec invariants.

---

## Proof Patterns

### Direct application (when mathlib has the theorem)

```lean
-- The algebraic law maps directly to a mathlib lemma.
theorem merge_assoc (a b c : Finset Datom) :
    merge (merge a b) c = merge a (merge b c) :=
  Finset.union_assoc a b c
```

### Tactic proof (when composition is needed)

```lean
-- Monotonic growth: transacting new datoms preserves all existing ones.
theorem transact_mono (s new : Finset Datom) : s ⊆ s ∪ new :=
  Finset.subset_union_left
```

### Structural proof (when the model needs decomposition)

```lean
-- Index completeness: every datom in the store appears in the index.
-- Model the index as a function from Datom to Bool.
theorem index_complete (s : Finset Datom) (idx : Datom → Bool)
    (h : ∀ d ∈ s, idx d = true) (d : Datom) (hd : d ∈ s) :
    idx d = true :=
  h d hd
```

---

## Translation Rules: Spec to Lean

| Spec concept | Lean representation |
|-------------|---------------------|
| DatomStore | `Finset Datom` |
| merge(A, B) | `a ∪ b` (Finset.union) |
| A ⊆ B | `a ⊆ b` (Finset.Subset) |
| "for all reachable stores" | Universal quantification over `Finset Datom` |
| "crash recovery preserves" | Not modeled in Lean (Stateright covers this) |
| "O(log n) lookup" | Not modeled in Lean (benchmarks cover this) |

Lean proves algebraic properties. Performance and crash behavior
are verified by other tools (benchmarks, Stateright). Do not
try to model I/O or timing in Lean.

---

## Checklist Per Invariant

For each INV-FERR you prove:

1. Read Level 0 (algebraic law) in the spec
2. Identify the mathlib types and theorems that model it
3. Write the theorem statement with a doc comment citing INV-FERR-NNN
4. Prove it (prefer direct application over tactics when possible)
5. Run `lake build` and fix any errors
6. Update the task: `br close <id> --reason "Lean proof verified"`

---

## What NOT To Do

- Do not model I/O, disk, or network in Lean. Those are Stateright's job.
- Do not use `sorry` except as a temporary placeholder during development.
  No `sorry` may be committed.
- Do not add `sorry` without filing a tracking bead.
- Do not fight mathlib. If your model doesn't match mathlib's types,
  change your model.
- Do not prove theorems that aren't in the spec. Every theorem traces
  to an INV-FERR.
