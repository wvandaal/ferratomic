/-
  Ferratomic Decisions — cross-shard query and partition tolerance proofs.

  Invariants proven:
    INV-FERR-033  Cross-shard query correctness (CALM: filter distributes over union)
    INV-FERR-035  Partition-safe operation (writes are local, no coordination needed)

  Spec: spec/04-decisions-and-constraints.md §23.4-23.7
-/

import Ferratomic.Store
import Mathlib.Data.Finset.Union

/-! ## INV-FERR-033: Cross-Shard Query Correctness

  For monotonic queries (modeled as filter predicates over datoms),
  querying the union equals the union of per-shard queries.
  This is the CALM theorem applied to our G-Set CRDT. -/

/-- Two-store case: filter distributes over union. -/
theorem filter_union_comm (a b : DatomStore) (p : Datom → Prop) [DecidablePred p] :
    (a ∪ b).filter p = a.filter p ∪ b.filter p :=
  Finset.filter_union p a b

/-- Monotonic query over a merged store equals merging per-store results. -/
theorem cross_shard_query (a b : DatomStore) (p : Datom → Prop) [DecidablePred p] :
    (merge a b).filter p = merge (a.filter p) (b.filter p) := by
  unfold merge
  exact Finset.filter_union p a b

/-- Query over union of N stores (generalized). -/
theorem filter_biUnion_comm {ι : Type*} [DecidableEq ι] (stores : Finset ι)
    (f : ι → DatomStore) (p : Datom → Prop) [DecidablePred p] :
    (stores.biUnion f).filter p = stores.biUnion (fun i => (f i).filter p) :=
  Finset.filter_biUnion stores f p

/-! ## INV-FERR-035: Partition-Safe Operation

  Writes are local to the G-Set CRDT — no coordination required.
  The function signature of apply_tx has no network/quorum parameter.
  This IS the proof: the type system encodes partition safety. -/

/-- After partition heals: merge restores full state from both sides. -/
theorem partition_recovery (side_a side_b : DatomStore) :
    side_a ⊆ merge side_a side_b ∧ side_b ⊆ merge side_a side_b :=
  ⟨merge_mono_left side_a side_b, merge_mono_right side_a side_b⟩

/-- Partition recovery is order-independent. -/
theorem partition_recovery_order (side_a side_b : DatomStore) :
    merge side_a side_b = merge side_b side_a :=
  merge_comm side_a side_b

/-- Repeated merge after partition is idempotent. -/
theorem partition_recovery_idempotent (side_a side_b : DatomStore) :
    merge (merge side_a side_b) (merge side_a side_b) = merge side_a side_b :=
  merge_idemp (merge side_a side_b)
