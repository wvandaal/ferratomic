/-
  Ferratomic Performance — LIVE view, genesis, and resolution proofs.

  Invariants proven:
    INV-FERR-029  LIVE view resolution (retraction semantics)
    INV-FERR-031  Genesis determinism (empty store is unique bottom)
    INV-FERR-032  LIVE resolution correctness (assert/retract algebra)

  Spec: spec/03-performance.md §23.3
-/

import Ferratomic.Store
import Mathlib.Data.Finset.Card

/-! ## INV-FERR-031: Genesis Determinism

  genesis() always returns the same store — the empty set.
  The empty set is the bottom element of the semilattice. -/

/-- Genesis is the empty store. -/
def genesis_model : DatomStore := ∅

/-- Genesis is the bottom element: every store is a superset. -/
theorem genesis_bottom (s : DatomStore) : genesis_model ⊆ s :=
  Finset.empty_subset s

/-- Merging with genesis is identity (left identity of merge). -/
theorem genesis_merge_left (s : DatomStore) : merge genesis_model s = s :=
  Finset.empty_union s

/-- Merging with genesis is identity (right identity of merge). -/
theorem genesis_merge_right (s : DatomStore) : merge s genesis_model = s :=
  Finset.union_empty s

/-- Genesis has zero cardinality. -/
theorem genesis_card : genesis_model.card = 0 :=
  Finset.card_empty

/-! ## INV-FERR-029: LIVE View Resolution

  The LIVE view is a derived projection: assertions add to live set,
  retractions remove. Modeled as operations on Finset (Nat × Nat × Nat)
  representing (entity, attribute, value) triples. -/

/-- Apply a datom's operation to the live set. -/
def apply_op (live : Finset (Nat × Nat × Nat)) (d : Datom) : Finset (Nat × Nat × Nat) :=
  let key := (d.e, d.a, d.v)
  if d.op then live ∪ {key}    -- assert: add to live set
  else live \ {key}             -- retract: remove from live set

/-- LIVE view: fold over datoms in order. -/
def live_view_model (datoms : List Datom) : Finset (Nat × Nat × Nat) :=
  datoms.foldl apply_op ∅

/-- Retraction removes the triple from the live set. -/
theorem retraction_removes (live : Finset (Nat × Nat × Nat)) (e a v : Nat) :
    (e, a, v) ∉ apply_op live ⟨e, a, v, 0, false⟩ := by
  unfold apply_op
  simp

/-- Assertion adds the triple to the live set. -/
theorem assertion_adds (live : Finset (Nat × Nat × Nat)) (e a v : Nat) :
    (e, a, v) ∈ apply_op live ⟨e, a, v, 0, true⟩ := by
  unfold apply_op
  simp

/-- Helper: each apply_op step changes cardinality by at most 1. -/
private theorem apply_op_card_le (live : Finset (Nat × Nat × Nat)) (d : Datom) :
    (apply_op live d).card ≤ live.card + 1 := by
  unfold apply_op
  split
  · -- assert: live ∪ {key}, card ≤ card + 1
    exact le_trans (Finset.card_union_le _ _) (by simp)
  · -- retract: live \ {key}, card ≤ card ≤ card + 1
    exact le_trans (Finset.card_le_card Finset.sdiff_subset) (Nat.le_add_right _ _)

/-- INV-FERR-029: Generalized bound — live view card bounded by init.card + list length. -/
private theorem live_bounded_aux (datoms : List Datom) (init : Finset (Nat × Nat × Nat)) :
    (datoms.foldl apply_op init).card ≤ init.card + datoms.length := by
  induction datoms generalizing init with
  | nil => simp
  | cons d rest ih =>
    simp only [List.foldl_cons, List.length_cons]
    have h := ih (apply_op init d)
    have hstep := apply_op_card_le init d
    omega

/-- INV-FERR-029: The live view cardinality is bounded by the number of datoms. -/
theorem live_bounded (datoms : List Datom) :
    (live_view_model datoms).card ≤ datoms.length := by
  unfold live_view_model
  have h := live_bounded_aux datoms ∅
  simp at h
  exact h

/-! ## INV-FERR-032: LIVE Resolution Correctness

  LIVE(S, e, a) = assertions(S, e, a) \ retractions(S, e, a)
  Values that are asserted but not retracted are live. -/

/-- Assertions for a given (entity, attribute) pair. -/
def assertions (datoms : Finset Datom) (e a : Nat) : Finset Nat :=
  (datoms.filter (fun d => d.e = e ∧ d.a = a ∧ d.op = true)).image (fun d => d.v)

/-- Retractions for a given (entity, attribute) pair. -/
def retractions (datoms : Finset Datom) (e a : Nat) : Finset Nat :=
  (datoms.filter (fun d => d.e = e ∧ d.a = a ∧ d.op = false)).image (fun d => d.v)

/-- Live values = assertions minus retractions. -/
def live_values (datoms : Finset Datom) (e a : Nat) : Finset Nat :=
  assertions datoms e a \ retractions datoms e a

/-- An asserted, non-retracted value is live. -/
theorem live_asserted_not_retracted (datoms : Finset Datom) (e a v : Nat)
    (h_in : v ∈ assertions datoms e a)
    (h_not : v ∉ retractions datoms e a) :
    v ∈ live_values datoms e a := by
  unfold live_values
  exact Finset.mem_sdiff.mpr ⟨h_in, h_not⟩

/-- A retracted value is not live. -/
theorem live_retracted_absent (datoms : Finset Datom) (e a v : Nat)
    (h_retracted : v ∈ retractions datoms e a) :
    v ∉ live_values datoms e a := by
  unfold live_values
  intro h
  exact absurd h_retracted (Finset.mem_sdiff.mp h).2
