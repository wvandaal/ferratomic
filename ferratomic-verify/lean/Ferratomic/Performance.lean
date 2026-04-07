/-
  Ferratomic Performance — LIVE view, genesis, resolution, and search proofs.

  Invariants proven:
    INV-FERR-029  LIVE view resolution (retraction semantics)
    INV-FERR-031  Genesis determinism (empty store is unique bottom)
    INV-FERR-032  LIVE resolution correctness (assert/retract algebra)
    INV-FERR-072  Lazy representation promotion/demotion (identity at abstract level)
    INV-FERR-077  Interpolation search equivalence (search strategy irrelevance)

  Spec: spec/03-performance.md §23.3, spec/09-performance-architecture.md
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

/-! ## INV-FERR-022: Anti-Entropy Convergence (Null Identity)

  The null anti-entropy implementation satisfies the identity property:
  for any store, apply_diff(store, diff(store)) = store. Modeled as the
  identity function on DatomStore. -/

/-- Null anti-entropy diff: returns the empty set (no changes needed). -/
def null_ae_diff (_s : DatomStore) : DatomStore := ∅

/-- Null anti-entropy apply: merges diff into local store. -/
def null_ae_apply (local_ diff : DatomStore) : DatomStore := local_ ∪ diff

/-- INV-FERR-022: null diff is empty for any store. -/
theorem null_ae_diff_empty (s : DatomStore) : null_ae_diff s = ∅ := rfl

/-- INV-FERR-022: applying empty diff is identity. -/
theorem null_ae_apply_identity (s : DatomStore) :
    null_ae_apply s (null_ae_diff s) = s := by
  unfold null_ae_apply null_ae_diff
  exact Finset.union_empty s

/-! ## INV-FERR-024/025: Backend Parametricity

  Two index backends produce identical query results for the same store
  content. Modeled as: any function f applied to a set S depends only
  on S, not on the representation. This is trivially true in Lean because
  Finset equality is extensional. -/

/-- INV-FERR-024/025: Any query on a store depends only on its content.
    Two stores with identical datom sets produce identical query results
    for any pure query function f. -/
theorem backend_parametricity (s1 s2 : DatomStore)
    (h : s1 = s2) (f : DatomStore → DatomStore) :
    f s1 = f s2 := by
  rw [h]

/-- INV-FERR-025: Index view is a function of the datom set alone.
    Identical datom sets produce identical index projections. -/
theorem index_view_deterministic (s1 s2 : DatomStore)
    (h : s1 = s2) (proj : Datom → Nat) :
    s1.image proj = s2.image proj := by
  rw [h]

/-! ## INV-FERR-030: Read Replica Subset

  A read replica stores a subset of the full store, determined by a
  filter predicate. The accept-all filter returns the entire store. -/

/-- Replica filter: keep only datoms matching a predicate. -/
def replica_filter (s : DatomStore) (p : Datom → Prop) [DecidablePred p] : DatomStore :=
  s.filter p

/-- INV-FERR-030: accept-all filter returns the entire store. -/
theorem accept_all_identity (s : DatomStore) :
    replica_filter s (fun _ => True) = s :=
  Finset.filter_true_of_mem (fun _ _ => trivial)

/-- INV-FERR-030: any replica filter produces a subset of the store. -/
theorem replica_subset (s : DatomStore) (p : Datom → Prop) [DecidablePred p] :
    replica_filter s p ⊆ s :=
  Finset.filter_subset p s

/-- INV-FERR-030: replica filter distributes over merge. -/
theorem replica_filter_merge_mono (a b : DatomStore) (p : Datom → Prop) [DecidablePred p] :
    replica_filter a p ⊆ replica_filter (merge a b) p := by
  intro d hd
  unfold replica_filter at hd ⊢
  unfold merge
  rw [Finset.mem_filter] at hd ⊢
  exact ⟨Finset.mem_union_left _ hd.1, hd.2⟩

/-! ## INV-FERR-072: Lazy Representation Promotion / Demotion

  At the Lean abstraction level, both SortedVec and OrdMap represent the same
  abstract DatomStore (Finset Datom). Promotion and demotion are identity
  functions on the abstract type — the representation change is invisible.
  Concrete representation fidelity is verified by proptest in Rust. -/

/-- Construct a SortedVec-backed store (identity at the abstract level). -/
def sorted_vec_of (s : DatomStore) : DatomStore := s

/-- Promote from SortedVec to OrdMap (identity at the abstract level). -/
def promote (s : DatomStore) : DatomStore := s

/-- Demote from OrdMap back to SortedVec (identity at the abstract level). -/
def demote (s : DatomStore) : DatomStore := s

/-- V:LEAN-ABSTRACT: Proves identity on Finset. Does not verify concrete sorted-array implementation.

    INV-FERR-072: Promotion preserves the abstract datom set.
    `promote(sorted_vec_of(S)) = S` — no algebraic content is introduced
    or lost by the representation change. -/
theorem promote_preserves_content (s : DatomStore) :
    promote (sorted_vec_of s) = s := rfl

/-- V:LEAN-ABSTRACT: Proves identity on Finset. Does not verify concrete sorted-array implementation.

    INV-FERR-072: Demotion round-trips through promotion.
    `demote(promote(S)) = S` — the promote/demote cycle is the identity. -/
theorem demote_preserves_content (s : DatomStore) :
    demote (promote s) = s := rfl

/-! ## INV-FERR-077: Interpolation Search Equivalence

  At the Finset abstraction level, the choice of probe position within a
  sorted representation does not affect the membership answer. Membership
  in a Finset is independent of any search strategy over the sorted form.
  The O(log log N) complexity claim is a performance property verified
  empirically by proptest benchmarks. -/

/-- V:LEAN-ABSTRACT placeholder: proves Finset membership reflexivity.
    Concrete interpolation search correctness is verified by proptest
    (INV-FERR-077). A concrete Lean proof connecting sorted-array binary
    search to Finset membership is deferred to Phase 4b.

    INV-FERR-077: Interpolation search lookup equivalence.
    Membership in the Finset is equivalent to membership in any sorted
    representation of that Finset. The search strategy (interpolation vs
    binary) is irrelevant at this abstraction level. -/
theorem interpolation_search_equiv (S : DatomStore) (d : Datom) :
    d ∈ S ↔ d ∈ S := Iff.rfl

/-- V:LEAN-ABSTRACT: Proves identity on Finset. Does not verify concrete sorted-array implementation.

    INV-FERR-077: Lookup in equal stores is deterministic.
    If two stores have the same datom set, any lookup query produces
    the same result. This holds regardless of how the search algorithm
    chooses its probe sequence. -/
theorem sorted_lookup_deterministic (S₁ S₂ : DatomStore)
    (h : S₁ = S₂) (d : Datom) :
    (d ∈ S₁) = (d ∈ S₂) := by
  rw [h]
