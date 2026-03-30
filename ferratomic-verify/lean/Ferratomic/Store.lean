/-
  Ferratomic Store — G-Set CRDT semilattice: formal model and proofs.

  Invariants proven:
    INV-FERR-001  Merge commutativity
    INV-FERR-002  Merge associativity
    INV-FERR-003  Merge idempotency
    INV-FERR-004  Monotonic growth
    INV-FERR-010  Merge convergence (strong eventual consistency)
    INV-FERR-012  Content-addressed identity
    INV-FERR-018  Append-only

  Spec: spec/01-core-invariants.md §23.1
  Foundation: spec/00-preamble.md §23.0.4
-/

import Mathlib.Data.Finset.Basic
import Mathlib.Data.Finset.Card
import Mathlib.Data.Finset.Lattice.Basic
import Mathlib.Data.Finset.Lattice.Lemmas

/-! ### Foundation Model (§23.0.4) -/

/-- A datom is a five-tuple [entity, attribute, value, tx, op].
    Entity and attribute are modeled as Nat for finiteness.
    op = true means assert, op = false means retract. -/
@[ext]
structure Datom where
  e  : Nat
  a  : Nat
  v  : Nat
  tx : Nat
  op : Bool
  deriving DecidableEq, Repr

/-- A datom store is a finite set of datoms.
    Using abbrev so Finset type class instances (Union, Membership, etc.)
    are inherited transparently. -/
abbrev DatomStore := Finset Datom

/-- Merge is set union — the ONLY merge operation.
    abbrev makes this definitionally transparent so Finset lemmas apply. -/
abbrev merge (a b : DatomStore) : DatomStore := a ∪ b

/-- Transact: add a datom to the store. -/
abbrev apply_tx (s : DatomStore) (d : Datom) : DatomStore := s ∪ {d}

/-! ## INV-FERR-001: Merge Commutativity

  ∀ A, B ∈ DatomStore: merge(A, B) = merge(B, A)
  Proof: set union is commutative. -/

theorem merge_comm (a b : DatomStore) : merge a b = merge b a :=
  Finset.union_comm a b

/-! ## INV-FERR-002: Merge Associativity

  ∀ A, B, C ∈ DatomStore: merge(merge(A, B), C) = merge(A, merge(B, C))
  Proof: set union is associative. -/

theorem merge_assoc (a b c : DatomStore) :
    merge (merge a b) c = merge a (merge b c) :=
  Finset.union_assoc a b c

/-! ## INV-FERR-003: Merge Idempotency

  ∀ A ∈ DatomStore: merge(A, A) = A
  Proof: set union is idempotent. -/

theorem merge_idemp (a : DatomStore) : merge a a = a :=
  Finset.union_self a

/-! ## INV-FERR-004: Monotonic Growth

  ∀ S, d: S ⊆ apply(S, d) and |apply(S, d)| ≥ |S|
  ∀ A, B: A ⊆ merge(A, B) and B ⊆ merge(A, B) -/

/-- Left input is preserved in merge. -/
theorem merge_mono_left (a b : DatomStore) : a ⊆ merge a b :=
  Finset.subset_union_left

/-- Right input is preserved in merge. -/
theorem merge_mono_right (a b : DatomStore) : b ⊆ merge a b :=
  Finset.subset_union_right

/-- Transact preserves all existing datoms. -/
theorem apply_superset (s : DatomStore) (d : Datom) : s ⊆ apply_tx s d :=
  Finset.subset_union_left

/-- Transact does not decrease cardinality. -/
theorem apply_monotone (s : DatomStore) (d : Datom) :
    s.card ≤ (apply_tx s d).card :=
  Finset.card_le_card (apply_superset s d)

/-! ## INV-FERR-010: Merge Convergence (Strong Eventual Consistency)

  If two replicas receive the same set of updates (in any order),
  their states are identical. Follows from commutativity + associativity. -/

/-- Merge order is irrelevant (direct corollary of commutativity). -/
theorem merge_convergence (a b : DatomStore) : merge a b = merge b a :=
  merge_comm a b

/-- Merging empty with updates yields the updates (genesis recovery). -/
theorem convergence_from_empty (updates : DatomStore) :
    merge ∅ updates = updates :=
  Finset.empty_union updates

/-- Two replicas starting from empty converge regardless of merge order. -/
theorem convergence_symmetric (a b : DatomStore) :
    merge (merge ∅ a) b = merge (merge ∅ b) a := by
  simp only [Finset.empty_union]
  exact Finset.union_comm a b

/-! ## INV-FERR-012: Content-Addressed Identity

  d₁ = d₂ ↔ all five fields match. Identity IS content.
  In Finset, identical datoms merge as one (deduplication by construction). -/

/-- Datom identity is content identity: d₁ = d₂ iff all five fields match. -/
theorem content_identity (d1 d2 : Datom) :
    d1 = d2 ↔ (d1.e = d2.e ∧ d1.a = d2.a ∧ d1.v = d2.v ∧
                d1.tx = d2.tx ∧ d1.op = d2.op) := by
  constructor
  · intro h; subst h; exact ⟨rfl, rfl, rfl, rfl, rfl⟩
  · rintro ⟨he, ha, hv, htx, hop⟩; exact Datom.ext he ha hv htx hop

/-- Adding a datom already present does not change the store. -/
theorem merge_dedup (a : DatomStore) (d : Datom) (h : d ∈ a) :
    merge a {d} = a := by
  ext x
  simp only [Finset.mem_union, Finset.mem_singleton]
  constructor
  · rintro (hx | rfl)
    · exact hx
    · exact h
  · intro hx; exact Or.inl hx

/-- Deduplication preserves cardinality. -/
theorem dedup_by_content (s : DatomStore) (d : Datom) (h : d ∈ s) :
    (s ∪ {d}).card = s.card := by
  have heq : s ∪ {d} = s := by
    ext x; simp only [Finset.mem_union, Finset.mem_singleton]
    constructor
    · rintro (hx | rfl)
      · exact hx
      · exact h
    · intro hx; exact Or.inl hx
  rw [heq]

/-! ## INV-FERR-018: Append-Only

  No operation removes datoms. The store is monotonically non-decreasing.
  Retractions are new datoms with op=false — they add, not remove. -/

/-- Transact never removes datoms. -/
theorem transact_mono (s new_datoms : DatomStore) : s ⊆ s ∪ new_datoms :=
  Finset.subset_union_left

/-- Merge never removes from left operand. -/
theorem append_only_merge_left (a b : DatomStore) : a ⊆ merge a b :=
  merge_mono_left a b

/-- Merge never removes from right operand. -/
theorem append_only_merge_right (a b : DatomStore) : b ⊆ merge a b :=
  merge_mono_right a b

/-- Cardinality never decreases under apply. -/
theorem append_only_card_apply (s : DatomStore) (d : Datom) :
    s.card ≤ (apply_tx s d).card :=
  Finset.card_le_card (apply_superset s d)

/-- Cardinality never decreases under merge. -/
theorem append_only_card_merge (a b : DatomStore) :
    a.card ≤ (merge a b).card :=
  Finset.card_le_card (merge_mono_left a b)
