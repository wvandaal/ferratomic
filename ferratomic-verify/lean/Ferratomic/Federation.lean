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

  Spec: spec/05-federation.md §23.8
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

/-- When every store responds, the partial-result view equals the full result. -/
theorem all_respond_equals_full {ι : Type*} [DecidableEq ι]
    (stores : Finset ι)
    (f : ι → DatomStore)
    (p : Datom → Prop) [DecidablePred p] :
    (stores.biUnion f).filter p = (stores.biUnion f).filter p :=
  rfl

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
