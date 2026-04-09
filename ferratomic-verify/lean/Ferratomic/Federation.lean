/-
  Ferratomic Federation — federated query, selective merge, provenance proofs.

  Invariants proven:
    INV-FERR-037  Federated query correctness (CALM over N stores)
    INV-FERR-038  Federation substrate transparency (transport wrappers are faithful)
    INV-FERR-039  Selective merge (filter + union preserves CRDT laws)
    INV-FERR-040  Merge provenance preservation (union preserves identity)
    INV-FERR-041  Transport latency tolerance (partial results are subsets)
    INV-FERR-043  Schema compatibility symmetry
    INV-FERR-044  Namespace isolation (filter soundness)

  Phase 4a.5 (spec/05 §23.8.5):
    INV-FERR-060  Store identity persistence (identity tx survives merge)
    INV-FERR-061  Causal predecessor completeness (1 sorry — bd-aqg9h)
    INV-FERR-062  Merge receipt completeness (receipt datoms in result)
    INV-FERR-063  Provenance lattice total order (0 sorry)

  Spec: spec/05-federation.md §23.8, §23.8.5
-/

import Ferratomic.Store
import Ferratomic.Decisions  -- reuses filter_union_comm

/-! ## INV-FERR-037: Federated Query Correctness

  For monotonic queries, querying the federation (union of stores)
  equals merging per-store results. Generalizes INV-FERR-033 from
  shards to federated stores. -/

/-- Two-store federated query. -/
theorem federated_query_two (s1 s2 : DatomStore) (p : Datom → Prop) [DecidablePred p] :
    (s1 ∪ s2).filter p = s1.filter p ∪ s2.filter p :=
  Finset.filter_union p s1 s2

/-- N-store federated query (same as filter_biUnion_comm from Decisions). -/
theorem federated_query_n {ι : Type*} [DecidableEq ι] (stores : Finset ι)
    (f : ι → DatomStore) (p : Datom → Prop) [DecidablePred p] :
    (stores.biUnion f).filter p = stores.biUnion (fun i => (f i).filter p) :=
  filter_biUnion_comm stores f p

/-- Result merge is commutative (order of federation doesn't matter). -/
theorem federated_result_comm (r1 r2 : DatomStore) : r1 ∪ r2 = r2 ∪ r1 :=
  Finset.union_comm r1 r2

/-- Result merge is associative (grouping of federation doesn't matter). -/
theorem federated_result_assoc (r1 r2 r3 : DatomStore) :
    (r1 ∪ r2) ∪ r3 = r1 ∪ (r2 ∪ r3) :=
  Finset.union_assoc r1 r2 r3

/-! ## INV-FERR-038: Federation Substrate Transparency

  Local and remote transports are faithful wrappers over the same store.
  They may change latency/metadata, but not query results. -/

/-- Local transport wrapper: identity on the algebraic store model. -/
def local_transport (s : DatomStore) : DatomStore := s

/-- Remote transport wrapper: identity on the algebraic store model. -/
def remote_transport (s : DatomStore) : DatomStore := s

/-- Any store observer sees identical data through either transport wrapper. -/
theorem transport_transparency {α : Sort _} (s : DatomStore) (f : DatomStore → α) :
    f (local_transport s) = f (remote_transport s) :=
  rfl

/-- Applied to monotonic queries (modeled as filters), local and remote agree. -/
theorem transport_query_equiv (s : DatomStore) (p : Datom → Prop) [DecidablePred p] :
    (local_transport s).filter p = (remote_transport s).filter p :=
  rfl

/-! ## INV-FERR-039: Selective Merge

  selective_merge(local, remote, filter) = local ∪ filter(remote)
  Preserves CRDT monotonicity: local is always a subset of the result. -/

/-- Selective merge: local ∪ filter(remote). -/
def selective_merge (local_ remote : DatomStore) (p : Datom → Prop) [DecidablePred p] :
    DatomStore :=
  local_ ∪ remote.filter p

/-- Monotonicity: local is always preserved. -/
theorem selective_merge_mono (local_ remote : DatomStore) (p : Datom → Prop)
    [DecidablePred p] :
    local_ ⊆ selective_merge local_ remote p :=
  Finset.subset_union_left

/-- Bounded: result is a subset of local ∪ remote. -/
theorem selective_merge_bounded (local_ remote : DatomStore) (p : Datom → Prop)
    [DecidablePred p] :
    selective_merge local_ remote p ⊆ local_ ∪ remote := by
  unfold selective_merge
  exact Finset.union_subset_union_right (Finset.filter_subset p remote)

/-- filter = True reduces to full merge. -/
theorem selective_merge_all (local_ remote : DatomStore) :
    selective_merge local_ remote (fun _ => True) = local_ ∪ remote := by
  unfold selective_merge
  congr 1
  exact Finset.filter_true_of_mem (fun _ _ => trivial)

/-- filter = False is identity on local. -/
theorem selective_merge_none (local_ remote : DatomStore) :
    selective_merge local_ remote (fun _ => False) = local_ := by
  unfold selective_merge
  have : remote.filter (fun _ => False) = ∅ := by ext; simp
  rw [this, Finset.union_empty]

/-- INV-FERR-039: Selective merge is idempotent — repeating it is a no-op. -/
theorem selective_merge_idemp (local_ remote : DatomStore) (p : Datom → Prop)
    [DecidablePred p] :
    selective_merge (selective_merge local_ remote p) remote p =
    selective_merge local_ remote p := by
  unfold selective_merge
  rw [Finset.union_assoc, Finset.union_self]

/-! ## INV-FERR-040: Merge Provenance Preservation

  Set union does not modify datom fields. Every datom in merge(A,B)
  came from A or B with fields unchanged. Provenance is structural. -/

/-- Union preserves membership: every element came from one of the inputs. -/
theorem merge_provenance (a b : DatomStore) (d : Datom) (h : d ∈ merge a b) :
    d ∈ a ∨ d ∈ b :=
  Finset.mem_union.mp h

/-- Union does not invent elements: result is bounded by inputs. -/
theorem merge_no_invention (a b : DatomStore) :
    ∀ d ∈ merge a b, d ∈ a ∨ d ∈ b :=
  fun _d hd => Finset.mem_union.mp hd

/-! ## INV-FERR-041: Transport Latency Tolerance

  Partial federated results are unions over the responding-store subset,
  so they are always subsets of the full federated result. -/

/-- Results from responding stores are a subset of the full federation result. -/
theorem partial_subset_full {ι : Type*} [DecidableEq ι]
    (stores responding : Finset ι)
    (h_sub : responding ⊆ stores)
    (f : ι → DatomStore)
    (p : Datom → Prop) [DecidablePred p] :
    (responding.biUnion f).filter p ⊆ (stores.biUnion f).filter p := by
  intro d hd
  rcases Finset.mem_filter.mp hd with ⟨hd_resp, hp⟩
  rcases Finset.mem_biUnion.mp hd_resp with ⟨i, hi_resp, hd_i⟩
  exact Finset.mem_filter.mpr ⟨Finset.mem_biUnion.mpr ⟨i, h_sub hi_resp, hd_i⟩, hp⟩

/-! ## INV-FERR-043: Schema Compatibility Check

  Schema compatibility is symmetric: if A is compatible with B,
  then B is compatible with A. Shared attributes must have identical types. -/

/-- Schema compatibility: all shared attributes have matching types.
    Modeled as: for all common attribute IDs, types and cardinalities agree. -/
def schema_compatible (s1 s2 : Finset (Nat × Nat × Nat)) : Prop :=
  ∀ a t1 c1 t2 c2,
    (a, t1, c1) ∈ s1 → (a, t2, c2) ∈ s2 → t1 = t2 ∧ c1 = c2

/-- Schema compatibility is symmetric. -/
theorem schema_compat_symmetric (s1 s2 : Finset (Nat × Nat × Nat)) :
    schema_compatible s1 s2 → schema_compatible s2 s1 := by
  intro h a t2 c2 t1 c1 h2 h1
  have ⟨ht, hc⟩ := h a t1 c1 t2 c2 h1 h2
  exact ⟨ht.symm, hc.symm⟩

/-! ## INV-FERR-044: Namespace Isolation

  Filtering by namespace prefix ensures only matching datoms are transferred.
  No datom outside the namespace enters the result. -/

/-- Namespace filter: keep only datoms matching a namespace predicate. -/
def ns_filter (s : DatomStore) (ns : Datom → Prop) [DecidablePred ns] : DatomStore :=
  s.filter ns

/-- Every datom in the filtered result satisfies the namespace predicate. -/
theorem ns_filter_sound (s : DatomStore) (ns : Datom → Prop) [DecidablePred ns] :
    ∀ d ∈ ns_filter s ns, ns d := by
  intro d hd
  exact (Finset.mem_filter.mp hd).2

/-- The filtered result is a subset of the original store. -/
theorem ns_filter_subset (s : DatomStore) (ns : Datom → Prop) [DecidablePred ns] :
    ns_filter s ns ⊆ s :=
  Finset.filter_subset ns s

/-- Namespace filter preserves merge monotonicity. -/
theorem ns_filter_merge_mono (a b : DatomStore) (ns : Datom → Prop) [DecidablePred ns] :
    ns_filter a ns ⊆ ns_filter (merge a b) ns := by
  unfold ns_filter merge
  intro d hd
  rw [Finset.mem_filter] at hd ⊢
  exact ⟨Finset.mem_union_left _ hd.1, hd.2⟩

/-! ═══════════════════════════════════════════════════════════════════════
    Phase 4a.5: Federation Foundations (spec/05 §23.8.5)

    INV-FERR-060  Store identity persistence
    INV-FERR-061  Causal predecessor completeness
    INV-FERR-062  Merge receipt completeness
    INV-FERR-063  Provenance lattice total order
    ═══════════════════════════════════════════════════════════════════════ -/

/-! ## INV-FERR-060: Store Identity Persistence

  The identity transaction (a self-signed datom asserting the store's
  public key) persists through merge, recovery, and selective_merge
  (when the filter accepts it). By INV-FERR-004 (monotonic growth),
  any datom in a store survives set union. -/

/-- Identity transaction survives merge with any other store. -/
theorem identity_persists_merge (S S' : DatomStore) (identity_datom : Datom)
    (h : identity_datom ∈ S) :
    identity_datom ∈ merge S S' :=
  Finset.mem_union_left S' h

/-- After merging two signed stores, BOTH identity transactions coexist. -/
theorem both_identities_survive_merge
    (S₁ S₂ : DatomStore) (id₁ id₂ : Datom)
    (h₁ : id₁ ∈ S₁) (h₂ : id₂ ∈ S₂) :
    id₁ ∈ merge S₁ S₂ ∧ id₂ ∈ merge S₁ S₂ :=
  ⟨Finset.mem_union_left S₂ h₁, Finset.mem_union_right S₁ h₂⟩

/-- Identity uniqueness per signing key: if two identity datoms in the
    same store have the same entity (derived from the same public key),
    they are the same datom. Modeled as: datom identity is structural
    (Datom.ext from the @[ext] attribute on the Datom structure). -/
theorem identity_unique_per_key (_S : DatomStore) (id₁ id₂ : Datom)
    (_h₁ : id₁ ∈ _S) (_h₂ : id₂ ∈ _S)
    (h_same : id₁.e = id₂.e ∧ id₁.a = id₂.a ∧ id₁.v = id₂.v
            ∧ id₁.tx = id₂.tx ∧ id₁.op = id₂.op) :
    id₁ = id₂ := by
  exact Datom.ext h_same.1 h_same.2.1 h_same.2.2.1 h_same.2.2.2.1 h_same.2.2.2.2

/-! ## INV-FERR-061: Causal Predecessor Completeness

  emit_predecessors maps each frontier entry to exactly one predecessor
  datom. The mapping is injective (different nodes produce different Ref
  values). The predecessor relation is acyclic because predecessor TxIds
  are strictly less than the new TxId (INV-FERR-015 monotonicity). -/

/-- Model: emit_predecessors as an injective map from frontier entries
    to predecessor datoms. The function creates one datom per (NodeId, TxId)
    pair in the frontier, with the entity = tx_entity and value = Ref(pred). -/
def emit_predecessors_model (frontier : Finset (Nat × Nat))
    (tx_entity : Nat) : Finset Datom :=
  frontier.image (fun ⟨_node, pred_tx⟩ =>
    { e := tx_entity, a := 0 /- :tx/predecessor -/, v := pred_tx,
      tx := 0 /- filled later -/, op := true })

/-- INV-FERR-061 (a): Predecessor count equals frontier size when
    frontier entries have distinct predecessor TxIds. -/
theorem predecessor_complete
    (frontier : Finset (Nat × Nat))
    (tx_entity : Nat)
    (h_inj : ∀ a b, a ∈ frontier → b ∈ frontier →
      (fun ⟨_, t⟩ => t : Nat × Nat → Nat) a =
      (fun ⟨_, t⟩ => t : Nat × Nat → Nat) b → a = b) :
    (emit_predecessors_model frontier tx_entity).card = frontier.card := by
  unfold emit_predecessors_model
  apply Finset.card_image_of_injective
  intro ⟨n1, t1⟩ ⟨n2, t2⟩ h
  simp [Datom.ext_iff] at h
  have := h_inj ⟨n1, t1⟩ ⟨n2, t2⟩
  sorry -- Requires: (n1, t1) ∈ frontier → ... → t1 = t2 → (n1,t1) = (n2,t2)
         -- This needs the full frontier membership context; tracked as bd-aqg9h

/-- INV-FERR-061 (b): Acyclicity — if T₂ is a predecessor of T₁, then
    T₂.tx < T₁.tx. Modeled as: for any datom in the predecessor set,
    its value (the predecessor's TxId) is strictly less than the new TxId. -/
theorem predecessor_acyclic
    (frontier : Finset (Nat × Nat))
    (tx_entity new_tx : Nat)
    (h_lt : ∀ pair ∈ frontier, pair.2 < new_tx) :
    ∀ d ∈ emit_predecessors_model frontier tx_entity,
      d.v < new_tx := by
  intro d hd
  unfold emit_predecessors_model at hd
  rw [Finset.mem_image] at hd
  obtain ⟨⟨n, t⟩, hmem, heq⟩ := hd
  subst heq
  exact h_lt ⟨n, t⟩ hmem

/-- INV-FERR-061 (c): Predecessor datoms survive merge. They are
    ordinary datoms in the G-Set; set union preserves them. -/
theorem predecessor_dag_merge
    (G₁ G₂ : DatomStore) (d : Datom) (h : d ∈ G₁) :
    d ∈ merge G₁ G₂ :=
  Finset.mem_union_left G₂ h

/-! ## INV-FERR-062: Merge Receipt Completeness

  selective_merge emits 4 receipt datoms (source, filter, transferred,
  timestamp) as a proper transaction. The receipt datoms are added to the
  local store via set union, so they persist by INV-FERR-004. -/

/-- Model: merge receipt as 4 datoms added to the result. -/
def selective_merge_with_receipt
    (local_ remote : DatomStore)
    (f : Datom → Prop) [DecidablePred f]
    (receipt : Finset Datom) : DatomStore :=
  local_ ∪ remote.filter f ∪ receipt

/-- INV-FERR-062 (a): All 4 receipt datoms are present in the result. -/
theorem merge_receipt_present
    (local_ remote : DatomStore)
    (f : Datom → Prop) [DecidablePred f]
    (receipt : Finset Datom) :
    receipt ⊆ selective_merge_with_receipt local_ remote f receipt :=
  Finset.subset_union_right

/-- INV-FERR-062 (b): Receipt datoms persist through subsequent merges
    (they are ordinary datoms, preserved by INV-FERR-004). -/
theorem merge_receipt_persists
    (result other : DatomStore)
    (r : Datom) (h : r ∈ result) :
    r ∈ merge result other :=
  Finset.mem_union_left other h

/-- INV-FERR-062 (c): Local datoms are preserved through selective merge
    with receipt (monotonicity). -/
theorem selective_merge_receipt_mono
    (local_ remote : DatomStore)
    (f : Datom → Prop) [DecidablePred f]
    (receipt : Finset Datom) :
    local_ ⊆ selective_merge_with_receipt local_ remote f receipt := by
  unfold selective_merge_with_receipt
  exact Finset.subset_union_left.trans Finset.subset_union_left

/-! ## INV-FERR-063: Provenance Lattice Total Order

  ProvenanceType = {Hypothesized, Inferred, Derived, Observed} with
  weights (0.2, 0.5, 0.8, 1.0). This forms a total order (linearly
  ordered finite set). Weight is monotone with the ordering. -/

/-- Provenance type as an inductive with 4 constructors. -/
inductive ProvenanceType where
  | hypothesized : ProvenanceType
  | inferred     : ProvenanceType
  | derived      : ProvenanceType
  | observed     : ProvenanceType
  deriving DecidableEq, Repr

/-- Weight assignment (modeled as Nat × 10 to avoid rationals). -/
def ProvenanceType.weight : ProvenanceType → Nat
  | .hypothesized => 2
  | .inferred     => 5
  | .derived      => 8
  | .observed     => 10

/-- Ordering by weight. -/
instance : LE ProvenanceType where
  le a b := a.weight ≤ b.weight

instance : LT ProvenanceType where
  lt a b := a.weight < b.weight

instance : DecidableRel (· ≤ · : ProvenanceType → ProvenanceType → Prop) :=
  fun a b => Nat.decLe a.weight b.weight

/-- Simp lemma: unfold ≤ on ProvenanceType to Nat comparison on weights. -/
@[simp] lemma provenance_le_iff (a b : ProvenanceType) :
    (a ≤ b) ↔ (a.weight ≤ b.weight) := Iff.rfl

/-- INV-FERR-063 (a): Total order — every pair is comparable. -/
theorem provenance_total (a b : ProvenanceType) : a ≤ b ∨ b ≤ a := by
  show a.weight ≤ b.weight ∨ b.weight ≤ a.weight
  exact Nat.le_total a.weight b.weight

/-- INV-FERR-063 (b): Weight is monotone with ordering (tautological
    by definition, but stated explicitly for the spec). -/
theorem weight_monotone (a b : ProvenanceType) (h : a ≤ b) :
    a.weight ≤ b.weight :=
  h

/-- INV-FERR-063 (c): Transitivity. -/
theorem provenance_trans (a b c : ProvenanceType) (h1 : a ≤ b) (h2 : b ≤ c) :
    a ≤ c :=
  Nat.le_trans h1 h2

/-- INV-FERR-063 (d): Antisymmetry — equal weights implies equal type.
    Since all four weights are distinct (2,5,8,10), equal weights ⟺ equal variant. -/
theorem provenance_antisymm (a b : ProvenanceType)
    (h1 : a ≤ b) (h2 : b ≤ a) : a = b := by
  have h1' : a.weight ≤ b.weight := h1
  have h2' : b.weight ≤ a.weight := h2
  have heq : a.weight = b.weight := Nat.le_antisymm h1' h2'
  cases a <;> cases b <;> simp_all [ProvenanceType.weight]

/-- INV-FERR-063 (e): The lattice has a bottom (Hypothesized) and top (Observed). -/
theorem provenance_bottom (a : ProvenanceType) :
    ProvenanceType.hypothesized ≤ a := by
  cases a <;> (show (2 : Nat) ≤ _; simp [ProvenanceType.weight])

theorem provenance_top (a : ProvenanceType) :
    a ≤ ProvenanceType.observed := by
  cases a <;> (show _ ≤ (10 : Nat); simp [ProvenanceType.weight])
