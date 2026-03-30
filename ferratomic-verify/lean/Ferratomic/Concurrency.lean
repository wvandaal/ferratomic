/-
  Ferratomic Concurrency — checkpoint, HLC, and sharding proofs.

  Invariants proven:
    INV-FERR-013  Checkpoint equivalence (serialization roundtrip)
    INV-FERR-015  HLC monotonicity (tick always advances)
    INV-FERR-016  HLC causality (receive > both local and remote)
    INV-FERR-017  Shard equivalence (partition union = original)

  Spec: spec/02-concurrency.md §23.2
-/

import Ferratomic.Store
import Mathlib.Data.Finset.Union
import Mathlib.Data.Fintype.Fin

/-! ## INV-FERR-013: Checkpoint Equivalence

  load(checkpoint(S)) = S — serialization is a faithful roundtrip.
  Modeled as identity functions since the mathematical content is preserved;
  only the physical representation changes. -/

def checkpoint_serialize (s : DatomStore) : DatomStore := s
def checkpoint_deserialize (s : DatomStore) : DatomStore := s

/-- Checkpoint roundtrip is identity. -/
theorem checkpoint_roundtrip (s : DatomStore) :
    checkpoint_deserialize (checkpoint_serialize s) = s := rfl

/-- Checkpoint preserves cardinality. -/
theorem checkpoint_preserves_card (s : DatomStore) :
    (checkpoint_deserialize (checkpoint_serialize s)).card = s.card := by
  rw [checkpoint_roundtrip]

/-- Checkpoint preserves membership. -/
theorem checkpoint_preserves_mem (s : DatomStore) (d : Datom) :
    d ∈ checkpoint_deserialize (checkpoint_serialize s) ↔ d ∈ s := by
  rw [checkpoint_roundtrip]

/-! ## INV-FERR-015: HLC Monotonicity

  Every tick produces a strictly greater HLC value in lexicographic order.
  If wall clock advanced: new physical > old physical.
  If wall clock stale: same physical, logical incremented. -/

/-- Hybrid Logical Clock: (physical, logical) pair. -/
structure HLC where
  physical : Nat
  logical  : Nat
  deriving DecidableEq, Repr

/-- Strict lexicographic ordering on HLC timestamps. -/
def HLC.lt (a b : HLC) : Prop :=
  a.physical < b.physical ∨ (a.physical = b.physical ∧ a.logical < b.logical)

/-- Local tick: advance the HLC given current wall clock. -/
def hlc_tick (prev : HLC) (wall : Nat) : HLC :=
  if wall > prev.physical then ⟨wall, 0⟩
  else ⟨prev.physical, prev.logical + 1⟩

/-- INV-FERR-015: tick always produces a strictly greater HLC value. -/
theorem hlc_tick_monotone (prev : HLC) (wall : Nat) :
    HLC.lt prev (hlc_tick prev wall) := by
  unfold HLC.lt hlc_tick
  split
  · left; omega
  · right; exact ⟨rfl, by simp only []; omega⟩

/-! ## INV-FERR-016: HLC Causality

  happens_before(e₁, e₂) ⟹ hlc(e₁) < hlc(e₂)
  Receive merges local and remote HLC, producing value > both. -/

/-- Receive: merge local and remote HLC. Result is strictly greater than both. -/
def hlc_receive (loc rem : HLC) (wall : Nat) : HLC :=
  let p := max wall (max loc.physical rem.physical)
  if p > loc.physical ∧ p > rem.physical then ⟨p, 0⟩
  else if loc.physical = rem.physical then ⟨p, max loc.logical rem.logical + 1⟩
  else if p = loc.physical then ⟨p, loc.logical + 1⟩
  else ⟨p, rem.logical + 1⟩

/-- Physical component of receive is always the max of all three. -/
private theorem hlc_receive_physical (loc rem : HLC) (wall : Nat) :
    (hlc_receive loc rem wall).physical = max wall (max loc.physical rem.physical) := by
  simp only [hlc_receive]
  split
  · rfl
  · split
    · rfl
    · split <;> rfl

/-- When physical equals remote's, logical is strictly greater. -/
private theorem hlc_receive_logical_gt (loc rem : HLC) (wall : Nat)
    (heq : max wall (max loc.physical rem.physical) = rem.physical) :
    rem.logical < (hlc_receive loc rem wall).logical := by
  simp only [hlc_receive]
  -- heq implies wall ≤ rem.physical and loc.physical ≤ rem.physical
  split
  · -- Branch 1: p > loc ∧ p > rem — impossible since p = rem.physical
    rename_i h; omega
  · split
    · -- Branch 2: loc.physical = rem.physical → max(loc.log, rem.log) + 1
      simp only; omega
    · split
      · -- Branch 3: p = loc.physical — but then loc = rem, contradiction
        rename_i h1 h2 h3; omega
      · -- Branch 4: logical = rem.logical + 1
        simp only; omega

/-- INV-FERR-016: receive produces value strictly greater than remote. -/
theorem hlc_receive_gt_remote (loc rem : HLC) (wall : Nat) :
    HLC.lt rem (hlc_receive loc rem wall) := by
  unfold HLC.lt
  rw [hlc_receive_physical]
  by_cases hgt : rem.physical < max wall (max loc.physical rem.physical)
  · left; exact hgt
  · right
    push Not at hgt
    have hge : rem.physical ≤ max wall (max loc.physical rem.physical) := by omega
    have heq : max wall (max loc.physical rem.physical) = rem.physical := by omega
    exact ⟨heq.symm, hlc_receive_logical_gt loc rem wall heq⟩

/-- INV-FERR-016: receive produces value strictly greater than local. -/
theorem hlc_receive_gt_local (loc rem : HLC) (wall : Nat) :
    HLC.lt loc (hlc_receive loc rem wall) := by
  unfold HLC.lt
  rw [hlc_receive_physical]
  by_cases hgt : loc.physical < max wall (max loc.physical rem.physical)
  · left; exact hgt
  · right
    push Not at hgt
    have heq : max wall (max loc.physical rem.physical) = loc.physical := by omega
    constructor
    · exact heq.symm
    · -- Need: loc.logical < (hlc_receive loc rem wall).logical
      show loc.logical < (hlc_receive loc rem wall).logical
      unfold hlc_receive
      simp only
      split
      · omega
      · split
        · simp only []; omega
        · -- After two splits: ¬(p > loc ∧ p > rem) and ¬(loc = rem).
          -- simp already resolved the third if (p = loc.physical) via heq.
          simp only []; omega

/-- Transitivity of HLC ordering (required for causal chains). -/
theorem hlc_lt_trans (a b c : HLC) (hab : HLC.lt a b) (hbc : HLC.lt b c) :
    HLC.lt a c := by
  unfold HLC.lt at *
  rcases hab with h1 | ⟨h1, h2⟩ <;> rcases hbc with h3 | ⟨h3, h4⟩
  · left; omega
  · left; omega
  · left; omega
  · right; exact ⟨h1.trans h3, by omega⟩

/-! ## INV-FERR-017: Shard Equivalence

  The union of all shards equals the original store.
  Sharding is a partition: coverage, disjointness, union. -/

/-- Partition a store by a shard function. -/
def shard_partition {n : Nat} (s : DatomStore) (f : Datom → Fin n) (i : Fin n) : DatomStore :=
  s.filter (fun d => f d = i)

/-- Coverage + union: the union of all shards equals the original store. -/
theorem shard_union {n : Nat} (s : DatomStore) (f : Datom → Fin n) (_hn : 0 < n) :
    Finset.univ.biUnion (shard_partition s f) = s := by
  ext d
  simp only [Finset.mem_biUnion, Finset.mem_univ, true_and, shard_partition,
             Finset.mem_filter]
  constructor
  · rintro ⟨_, hd, _⟩; exact hd
  · intro hd; exact ⟨f d, hd, rfl⟩

/-- Disjointness: no datom belongs to two different shards. -/
theorem shard_disjoint {n : Nat} (s : DatomStore) (f : Datom → Fin n)
    (i j : Fin n) (h : i ≠ j) :
    shard_partition s f i ∩ shard_partition s f j = ∅ := by
  ext d
  simp only [shard_partition, Finset.mem_inter, Finset.mem_filter]
  constructor
  · rintro ⟨⟨_, hi⟩, _, hj⟩
    exact absurd (hi.symm.trans hj) h
  · simp

/-- Merging all shards recovers the store (operational form). -/
theorem shard_merge_recovery {n : Nat} (s : DatomStore) (f : Datom → Fin n) (_hn : 0 < n) :
    ∀ d ∈ s, ∃ i : Fin n, d ∈ shard_partition s f i := by
  intro d hd
  exact ⟨f d, Finset.mem_filter.mpr ⟨hd, rfl⟩⟩
