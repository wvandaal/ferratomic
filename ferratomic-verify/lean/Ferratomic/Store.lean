/-
  Ferratomic Store: Formal model of the datom store as a G-Set CRDT semilattice.

  This file defines the core algebraic structure and proves the CRDT properties
  that are the foundation of Ferratomic's correctness guarantees.

  Corresponds to: spec/23-ferratomic.md §23.1 INV-FERR-001..004
  Traces to: SEED.md §4 Axiom 2

  Development order: These theorems are written BEFORE the Rust implementation.
  The Rust code must satisfy these properties — the proofs define the contract.
-/

import Mathlib.Data.Finset.Basic
import Mathlib.Data.Finset.Lattice

-- Simplified datom for proof purposes.
-- The full Datom structure (with ByteArray entity, String attribute, etc.)
-- is modeled here as an opaque type with decidable equality.
variable {Datom : Type} [DecidableEq Datom]

/-- A DatomStore is a finite set of datoms. -/
def DatomStore (Datom : Type) [DecidableEq Datom] := Finset Datom

namespace DatomStore

/-- Merge is set union. This is the ONLY merge operation. -/
def merge (a b : Finset Datom) : Finset Datom := a ∪ b

/-- INV-FERR-001: Merge commutativity.
    ∀ A B: merge(A, B) = merge(B, A) -/
theorem merge_comm (a b : Finset Datom) : merge a b = merge b a :=
  Finset.union_comm a b

/-- INV-FERR-002: Merge associativity.
    ∀ A B C: merge(merge(A, B), C) = merge(A, merge(B, C)) -/
theorem merge_assoc (a b c : Finset Datom) :
    merge (merge a b) c = merge a (merge b c) :=
  Finset.union_assoc a b c

/-- INV-FERR-003: Merge idempotency.
    ∀ A: merge(A, A) = A -/
theorem merge_idemp (a : Finset Datom) : merge a a = a :=
  Finset.union_self a

/-- INV-FERR-004: Monotonic growth (merge direction).
    ∀ A B: A ⊆ merge(A, B) -/
theorem merge_mono_left (a b : Finset Datom) : a ⊆ merge a b :=
  Finset.subset_union_left

/-- INV-FERR-004: Monotonic growth (both inputs preserved).
    ∀ A B: B ⊆ merge(A, B) -/
theorem merge_mono_right (a b : Finset Datom) : b ⊆ merge a b :=
  Finset.subset_union_right

/-- INV-FERR-018: Append-only (transact never removes).
    ∀ S new: S ⊆ S ∪ new -/
theorem transact_mono (s new : Finset Datom) : s ⊆ s ∪ new :=
  Finset.subset_union_left

/-- INV-FERR-010: Merge convergence.
    If two stores have merged all of each other's data, they are equal. -/
theorem merge_convergence (a b : Finset Datom) :
    merge a b = merge b a :=
  merge_comm a b

end DatomStore
