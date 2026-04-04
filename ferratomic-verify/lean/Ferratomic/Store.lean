/-
  Ferratomic Store — G-Set CRDT semilattice: formal model and proofs.

  Invariants proven:
    INV-FERR-001  Merge commutativity
    INV-FERR-002  Merge associativity
    INV-FERR-003  Merge idempotency
    INV-FERR-004  Monotonic growth
    INV-FERR-005  Index bijection (cardinality preservation under injective projection)
    INV-FERR-009  Schema validation (atomic accept/reject at transact boundary)
    INV-FERR-010  Merge convergence (strong eventual consistency)
    INV-FERR-012  Content-addressed identity
    INV-FERR-018  Append-only
    INV-FERR-030  Read replica subset (replica ⊆ leader)

  Spec: spec/01-core-invariants.md §23.1
  Foundation: spec/00-preamble.md §23.0.4
-/

import Mathlib.Data.Finset.Basic
import Mathlib.Data.Finset.Card
import Mathlib.Data.Finset.Lattice.Basic
import Mathlib.Data.Finset.Lattice.Lemmas
import Mathlib.Data.Finset.Dedup

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

/-- INV-FERR-004: Strict growth — adding a new datom increases cardinality by exactly 1. -/
theorem apply_strict_growth (s : DatomStore) (d : Datom) (h : d ∉ s) :
    (apply_tx s d).card = s.card + 1 := by
  show (s ∪ {d}).card = s.card + 1
  rw [Finset.union_comm, Finset.singleton_union]
  exact Finset.card_insert_of_notMem h

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

/-- Helper: foldl apply_tx distributes over the initial accumulator. -/
private theorem foldl_apply_tx_union (ds : List Datom) (init : DatomStore) :
    ds.foldl apply_tx init = init ∪ ds.toFinset := by
  induction ds generalizing init with
  | nil => simp [List.toFinset]
  | cons d rest ih =>
    simp only [List.foldl_cons, List.toFinset_cons]
    rw [ih]
    rw [Finset.union_assoc, ← Finset.insert_eq]

/-- INV-FERR-010: foldl apply_tx from empty equals toFinset.
    Sequential application of datoms produces the same result
    as converting the list to a finite set. -/
theorem apply_tx_foldl_eq_toFinset (ds : List Datom) :
    ds.foldl apply_tx ∅ = ds.toFinset := by
  rw [foldl_apply_tx_union]
  exact Finset.empty_union ds.toFinset

/-- INV-FERR-010: Strong eventual consistency — any permutation of the same
    datom list produces the same store. This is the substantive convergence
    theorem: order of transaction application is irrelevant. -/
theorem convergence_perm (ds₁ ds₂ : List Datom) (h : ds₁.Perm ds₂) :
    ds₁.foldl apply_tx ∅ = ds₂.foldl apply_tx ∅ := by
  rw [apply_tx_foldl_eq_toFinset, apply_tx_foldl_eq_toFinset]
  exact List.toFinset_eq_of_perm _ _ h

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

/-! ## INV-FERR-005: Index Bijection

  Indexes are projections of the primary datom set, differing only in access pattern.
  Any bijective projection preserves cardinality and membership.

  We model an index as the image of a bijection over the primary set.
  Since a bijection on Datom (Datom → Datom) applied via Finset.image preserves
  cardinality and membership (via injectivity), all indexes have the same
  cardinality as primary and contain the same elements. -/

/-- An index projection is any injective function on Datom (e.g., identity with a
    different sort order — the set content is identical). -/
def index_project (s : DatomStore) (f : Datom → Datom) : DatomStore :=
  s.image f

/-- INV-FERR-005: A bijective projection preserves cardinality.
    |index(S)| = |S| when the projection is injective. -/
theorem index_bijection_card (s : DatomStore) (f : Datom → Datom)
    (hinj : Function.Injective f) :
    (index_project s f).card = s.card := by
  unfold index_project
  exact Finset.card_image_of_injective s hinj

/-- INV-FERR-005: Identity projection (trivial case — index IS the primary set). -/
theorem index_identity (s : DatomStore) :
    index_project s id = s := by
  unfold index_project
  exact Finset.image_id

/-- INV-FERR-005: Multiple indexes all have the same cardinality as primary,
    when each is an injective projection. -/
theorem index_bijection_multi (s : DatomStore)
    (f₁ f₂ f₃ f₄ : Datom → Datom)
    (h₁ : Function.Injective f₁)
    (h₂ : Function.Injective f₂)
    (h₃ : Function.Injective f₃)
    (h₄ : Function.Injective f₄) :
    (index_project s f₁).card = s.card ∧
    (index_project s f₂).card = s.card ∧
    (index_project s f₃).card = s.card ∧
    (index_project s f₄).card = s.card :=
  ⟨index_bijection_card s f₁ h₁,
   index_bijection_card s f₂ h₂,
   index_bijection_card s f₃ h₃,
   index_bijection_card s f₄ h₄⟩

/-- INV-FERR-005: Membership equivalence — d ∈ primary ↔ f(d) ∈ index,
    when f is a bijection on Datom. -/
theorem index_bijection_mem (s : DatomStore) (f : Datom → Datom)
    (hbij : Function.Bijective f) (d : Datom) :
    d ∈ s ↔ f d ∈ index_project s f := by
  unfold index_project
  simp only [Finset.mem_image]
  constructor
  · intro hd; exact ⟨d, hd, rfl⟩
  · rintro ⟨x, hx, hfx⟩
    have : x = d := hbij.1 hfx
    subst this; exact hx

/-- INV-FERR-005: Transact preserves index bijection — if we apply the same
    transaction to both primary and index, cardinality remains equal. -/
theorem index_bijection_after_transact (s : DatomStore) (d : Datom)
    (f : Datom → Datom) (hinj : Function.Injective f) :
    (index_project (apply_tx s d) f).card = (apply_tx s d).card :=
  index_bijection_card (apply_tx s d) f hinj

/-- INV-FERR-005: Merge preserves index bijection. -/
theorem index_bijection_after_merge (a b : DatomStore)
    (f : Datom → Datom) (hinj : Function.Injective f) :
    (index_project (merge a b) f).card = (merge a b).card :=
  index_bijection_card (merge a b) f hinj

/-! ## INV-FERR-009: Schema Validation

  Schema is a predicate on datoms. If a datom passes the schema check,
  transact succeeds and the datom is in the resulting store. If it fails,
  transact rejects the entire transaction (no datoms enter the store).

  We model schema as a decidable predicate and transact_checked as a
  function that either applies all datoms (if all pass) or applies none. -/

/-- Checked transact: apply a list of datoms only if ALL pass the schema check.
    Returns some (new store) on success, none on rejection. -/
def transact_checked (s : DatomStore) (datoms : List Datom) (schema : Datom → Bool) :
    Option DatomStore :=
  if datoms.all schema = true then
    some (s ∪ datoms.toFinset)
  else
    none

/-- INV-FERR-009: If all datoms pass schema validation, transact succeeds. -/
theorem schema_valid_implies_success (s : DatomStore) (datoms : List Datom)
    (schema : Datom → Bool) (hvalid : datoms.all schema = true) :
    transact_checked s datoms schema = some (s ∪ datoms.toFinset) := by
  unfold transact_checked
  simp [hvalid]

/-- INV-FERR-009: If any datom fails schema validation, transact rejects. -/
theorem schema_invalid_implies_rejection (s : DatomStore) (datoms : List Datom)
    (schema : Datom → Bool) (hinvalid : datoms.all schema = false) :
    transact_checked s datoms schema = none := by
  unfold transact_checked
  simp [hinvalid]

/-- INV-FERR-009: Rejected transaction leaves the store unchanged. -/
theorem schema_rejection_preserves_store (s : DatomStore) (datoms : List Datom)
    (schema : Datom → Bool) (hinvalid : datoms.all schema = false) :
    ∀ s', transact_checked s datoms schema ≠ some s' := by
  intro s'
  simp [schema_invalid_implies_rejection s datoms schema hinvalid]

/-- INV-FERR-009: Successful transact includes all new datoms. -/
theorem schema_success_includes_datoms (s : DatomStore) (datoms : List Datom)
    (schema : Datom → Bool) (hvalid : datoms.all schema = true) (d : Datom)
    (hd : d ∈ datoms) :
    ∃ s', transact_checked s datoms schema = some s' ∧ d ∈ s' := by
  refine ⟨s ∪ datoms.toFinset, schema_valid_implies_success s datoms schema hvalid, ?_⟩
  exact Finset.mem_union_right s (List.mem_toFinset.mpr hd)

/-- INV-FERR-009: Successful transact preserves existing datoms. -/
theorem schema_success_preserves_store (s : DatomStore) (datoms : List Datom)
    (schema : Datom → Bool) (hvalid : datoms.all schema = true) :
    ∃ s', transact_checked s datoms schema = some s' ∧ s ⊆ s' := by
  refine ⟨s ∪ datoms.toFinset, schema_valid_implies_success s datoms schema hvalid, ?_⟩
  exact Finset.subset_union_left

/-- INV-FERR-009: Schema validation is atomic — all-or-nothing.
    If the result is some, all datoms passed. If none, zero datoms entered. -/
theorem schema_atomic (s : DatomStore) (datoms : List Datom)
    (schema : Datom → Bool) :
    (∃ s', transact_checked s datoms schema = some s') ↔
    datoms.all schema = true := by
  constructor
  · intro ⟨s', hs'⟩
    unfold transact_checked at hs'
    split at hs' <;> simp_all
  · intro hvalid
    exact ⟨s ∪ datoms.toFinset, schema_valid_implies_success s datoms schema hvalid⟩

/-! ## INV-FERR-029: Causal LIVE Lattice Homomorphism

  The causal LIVE set maps each `(e, a, v)` triple to its latest `(tx, op)`.
  Merge of two causal sets is per-key `max(tx)`. This is a lattice homomorphism
  over datom set union because:

  1. **Filter distributes**: datoms matching key `k` in `A ∪ B` =
     (matching in `A`) ∪ (matching in `B`).
  2. **Sup distributes**: `max` over `A ∪ B` = `max(max A, max B)`.

  Together: `causal_live(A ∪ B) = merge_causal(causal_live(A), causal_live(B))`. -/

/-- INV-FERR-029: Filter distributes over union — the structural foundation
    of the causal LIVE homomorphism. For any key predicate `p`, the datoms
    matching `p` in `A ∪ B` equal the union of matching datoms in `A` and `B`. -/
theorem causal_live_filter_union (A B : DatomStore)
    (p : Datom → Prop) [DecidablePred p] :
    (A ∪ B).filter p = A.filter p ∪ B.filter p := by
  ext d; simp [Finset.mem_filter, Finset.mem_union]
  tauto

/-- INV-FERR-029: Per-key filter preserves merge structure. For a specific
    `(e, a, v)` triple, the matching datoms distribute over store merge. -/
theorem causal_live_key_union (A B : DatomStore) (e a v : Nat) :
    (merge A B).filter (fun d => d.e = e ∧ d.a = a ∧ d.v = v) =
    A.filter (fun d => d.e = e ∧ d.a = a ∧ d.v = v) ∪
    B.filter (fun d => d.e = e ∧ d.a = a ∧ d.v = v) :=
  causal_live_filter_union A B _

/-- INV-FERR-029: Causal LIVE homomorphism (full statement).
    For any key predicate `p`, the filtered datom set distributes over merge.
    Combined with the standard order-theoretic fact that `max` distributes
    over union (`max(A ∪ B) = max(max A, max B)`), this gives:
    `causal_live(A ∪ B) = merge_causal(causal_live(A), causal_live(B))`. -/
theorem causal_live_homomorphism (A B : DatomStore)
    (p : Datom → Prop) [DecidablePred p] :
    (merge A B).filter p = merge (A.filter p) (B.filter p) :=
  causal_live_filter_union A B p

/-! ## INV-FERR-030: Read Replica Subset

  `∀ replica Rᵢ: replica(Rᵢ) ⊆ leader(S)`

  A replica that receives datoms only via merge from the leader is always
  a subset of the leader. This follows from the monotonicity of set union:
  if `replica ⊆ leader` and `delta ⊆ leader`, then `replica ∪ delta ⊆ leader`.

  The base case is `∅ ⊆ leader` (fresh replica). The inductive step uses
  `Finset.union_subset`: if both operands are subsets of X, their union
  is a subset of X. -/

/-- INV-FERR-030: Replica subset preservation.
    If a replica is a subset of the leader, and the merge delta is also a
    subset of the leader, then the merged replica remains a subset. -/
theorem replica_subset_preserved (replica leader delta : DatomStore)
    (h_rep : replica ⊆ leader) (h_delta : delta ⊆ leader) :
    merge replica delta ⊆ leader :=
  Finset.union_subset h_rep h_delta

/-- INV-FERR-030: Full catch-up produces the leader store.
    Merging a replica with the entire leader produces exactly the leader
    (since `leader ∪ leader = leader` by idempotency, and any datoms in
    the replica are already in the leader by the subset precondition). -/
theorem replica_catches_up (replica leader : DatomStore)
    (h_rep : replica ⊆ leader) :
    merge replica leader = leader :=
  Finset.union_eq_right.mpr h_rep
