/-
  Ferratomic Concurrency — checkpoint, HLC, sharding, snapshot, and transaction proofs.

  Invariants proven:
    INV-FERR-006  Snapshot isolation (snapshot ⊆ store, epoch-bounded)
    INV-FERR-007  Write linearizability (strictly monotonic epochs)
    INV-FERR-008  WAL ordering (fsync happens-before snapshot publish)
    INV-FERR-011  Observer monotonicity (epochs never regress)
    INV-FERR-013  Checkpoint equivalence (serialization roundtrip)
    INV-FERR-014  Crash recovery (monotone prefix: recover(prefix(WAL)) ⊆ recover(WAL))
    INV-FERR-015  HLC monotonicity (tick always advances)
    INV-FERR-016  HLC causality (receive > both local and remote)
    INV-FERR-017  Shard equivalence (partition union = original)
    INV-FERR-020  Transaction atomicity (all-or-nothing visibility)

  Spec: spec/01-core-invariants.md, spec/02-concurrency.md
-/

import Ferratomic.Store
import Mathlib.Data.Finset.Union
import Mathlib.Data.Fintype.Fin

/-! ## INV-FERR-013: Checkpoint Equivalence

  load(checkpoint(S)) = S — serialization is a faithful roundtrip.
  Modeled through an intermediate tuple representation to force the proof
  to demonstrate that field decomposition → reconstruction preserves all data. -/

/-- Encode a datom as a tuple of its five fields. -/
def Datom.toTuple (d : Datom) : Nat × Nat × Nat × Nat × Bool :=
  (d.e, d.a, d.v, d.tx, d.op)

/-- Decode a tuple back to a datom. -/
def Datom.ofTuple (t : Nat × Nat × Nat × Nat × Bool) : Datom :=
  ⟨t.1, t.2.1, t.2.2.1, t.2.2.2.1, t.2.2.2.2⟩

/-- Tuple roundtrip: ofTuple(toTuple(d)) = d. -/
theorem Datom.tuple_roundtrip (d : Datom) : Datom.ofTuple (Datom.toTuple d) = d := by
  cases d; rfl

/-- Checkpoint serialize: encode each datom to its tuple representation. -/
def checkpoint_serialize (s : DatomStore) : Finset (Nat × Nat × Nat × Nat × Bool) :=
  s.image Datom.toTuple

/-- Checkpoint deserialize: decode tuples back to datoms. -/
def checkpoint_deserialize (t : Finset (Nat × Nat × Nat × Nat × Bool)) : DatomStore :=
  t.image Datom.ofTuple

/-- INV-FERR-013: Checkpoint roundtrip preserves the store exactly.
    The proof forces decomposition through tuples and reconstruction,
    demonstrating that all five fields survive the encode/decode cycle. -/
theorem checkpoint_roundtrip (s : DatomStore) :
    checkpoint_deserialize (checkpoint_serialize s) = s := by
  unfold checkpoint_deserialize checkpoint_serialize
  simp only [Finset.image_image]
  have : (Datom.ofTuple ∘ Datom.toTuple) = id := by
    funext d; exact Datom.tuple_roundtrip d
  rw [this]; exact Finset.image_id

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

instance : LT HLC := ⟨HLC.lt⟩

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

/-! ## INV-FERR-006: Snapshot Isolation

  snapshot(S, e).datoms ⊆ store(S).datoms
  A snapshot at epoch e sees only datoms with tx epoch ≤ e.
  No reader sees partial transactions or future writes. -/

/-- A snapshot is the subset of datoms with tx epoch ≤ the snapshot epoch. -/
def snapshot (s : DatomStore) (epoch : Nat) : DatomStore :=
  s.filter (fun d => d.tx ≤ epoch)

/-- INV-FERR-006: Snapshot datoms are a subset of the full store. -/
theorem snapshot_subset (s : DatomStore) (epoch : Nat) :
    snapshot s epoch ⊆ s :=
  Finset.filter_subset _ s

/-- INV-FERR-006: Snapshot cardinality never exceeds the store. -/
theorem snapshot_card_le (s : DatomStore) (epoch : Nat) :
    (snapshot s epoch).card ≤ s.card :=
  Finset.card_le_card (snapshot_subset s epoch)

/-- INV-FERR-006: A datom in the snapshot must have tx ≤ epoch. -/
theorem snapshot_epoch_bound (s : DatomStore) (epoch : Nat) (d : Datom)
    (hd : d ∈ snapshot s epoch) : d.tx ≤ epoch := by
  simp only [snapshot, Finset.mem_filter] at hd
  exact hd.2

/-- INV-FERR-006: A datom with tx > epoch is NOT in the snapshot. -/
theorem snapshot_excludes_future (s : DatomStore) (epoch : Nat) (d : Datom)
    (_hd : d ∈ s) (hfuture : epoch < d.tx) : d ∉ snapshot s epoch := by
  simp only [snapshot, Finset.mem_filter, not_and]
  intro _
  omega

/-- INV-FERR-006: Snapshots are monotone — later epoch sees a superset. -/
theorem snapshot_monotone (s : DatomStore) (e₁ e₂ : Nat) (hle : e₁ ≤ e₂) :
    snapshot s e₁ ⊆ snapshot s e₂ := by
  intro d hd
  simp only [snapshot, Finset.mem_filter] at hd ⊢
  exact ⟨hd.1, Nat.le_trans hd.2 hle⟩

/-- INV-FERR-006: Adding a datom with tx > epoch does not change the snapshot. -/
theorem snapshot_stable_under_future_write (s : DatomStore) (epoch : Nat)
    (d : Datom) (hfuture : epoch < d.tx) :
    snapshot (apply_tx s d) epoch = snapshot s epoch := by
  ext x
  simp only [snapshot, apply_tx, Finset.mem_filter, Finset.mem_union,
             Finset.mem_singleton]
  constructor
  · rintro ⟨hx | rfl, hle⟩
    · exact ⟨hx, hle⟩
    · omega
  · rintro ⟨hx, hle⟩
    exact ⟨Or.inl hx, hle⟩

/-! ## INV-FERR-007: Write Linearizability

  For a commit sequence, epochs are strictly monotonic:
  epoch(tx_i) < epoch(tx_{i+1}).
  We model this as a property on lists of epochs produced by sequential commits. -/

/-- INV-FERR-007: The next_epoch function always produces a strictly greater value. -/
theorem next_epoch_strict (prev : Nat) : prev < prev + 1 := by omega

/-- INV-FERR-007: A mapped range [start+1, start+2, ...] has pairwise strict ordering.
    This is the List.Pairwise formulation, which composes well with Lean's list lemmas. -/
theorem linear_epoch_pairwise_lt (start n : Nat) :
    (List.range n |>.map (· + start + 1)).Pairwise (· < ·) := by
  rw [List.pairwise_map]
  exact List.Pairwise.imp (fun hab => by omega) List.pairwise_lt_range

/-- INV-FERR-007 (direct formulation): If two transactions commit sequentially
    and each gets epoch = prev + 1, their epochs are strictly ordered. -/
theorem write_linear_pair (e₁ e₂ : Nat) (h : e₂ = e₁ + 1) : e₁ < e₂ := by omega

/-- INV-FERR-007: Transitivity of strict ordering for commit chains. -/
theorem write_linear_chain (e₁ e₂ e₃ : Nat)
    (h₁₂ : e₁ < e₂) (h₂₃ : e₂ < e₃) : e₁ < e₃ := by omega

/-- INV-FERR-007: No two transactions share the same epoch. -/
theorem epoch_uniqueness (e₁ e₂ : Nat) (h : e₁ < e₂) : e₁ ≠ e₂ := by omega

/-- INV-FERR-007: Epoch sequence from sequential next_epoch calls.
    Defined as List.map (· + start + 1) (List.range n), producing
    [start+1, start+2, ..., start+n]. -/
def epoch_sequence (start n : Nat) : List Nat :=
  (List.range n).map (· + start + 1)

/-- INV-FERR-007: Elements of epoch_sequence are start+1, ..., start+n. -/
theorem epoch_sequence_mem (start n : Nat) (a : Nat) :
    a ∈ epoch_sequence start n ↔ ∃ i, i < n ∧ a = i + start + 1 := by
  simp only [epoch_sequence, List.mem_map, List.mem_range]
  constructor
  · rintro ⟨i, hi, rfl⟩; exact ⟨i, hi, rfl⟩
  · rintro ⟨i, hi, rfl⟩; exact ⟨i, hi, rfl⟩

/-- INV-FERR-007: Consecutive elements in epoch_sequence are strictly ordered.
    For all pairs in the list, earlier < later. -/
theorem epoch_sequence_pairwise_lt (start n : Nat) :
    (epoch_sequence start n).Pairwise (· < ·) := by
  unfold epoch_sequence
  rw [List.pairwise_map]
  exact List.Pairwise.imp (fun hab => by omega) List.pairwise_lt_range

/-! ## INV-FERR-008: WAL Ordering

  WAL write happens-before snapshot publication.
  We model this as an ordering constraint: for any committed transaction,
  durable(WAL(T)) BEFORE visible(SNAP(e)). -/

/-- Events in the commit protocol. -/
inductive CommitEvent where
  | wal_append (epoch : Nat) : CommitEvent
  | wal_fsync (epoch : Nat) : CommitEvent
  | index_apply (epoch : Nat) : CommitEvent
  | snapshot_publish (epoch : Nat) : CommitEvent
  deriving DecidableEq, Repr

/-- Extract the epoch from a commit event. -/
def CommitEvent.epoch : CommitEvent → Nat
  | wal_append e => e
  | wal_fsync e => e
  | index_apply e => e
  | snapshot_publish e => e

/-- A valid commit sequence for a transaction at epoch e follows the
    strict ordering: append → fsync → apply → publish. -/
def valid_commit_order (events : List CommitEvent) (e : Nat) : Prop :=
  events = [CommitEvent.wal_append e,
            CommitEvent.wal_fsync e,
            CommitEvent.index_apply e,
            CommitEvent.snapshot_publish e]

/-- The canonical commit sequence for epoch e. -/
def commit_sequence (e : Nat) : List CommitEvent :=
  [CommitEvent.wal_append e,
   CommitEvent.wal_fsync e,
   CommitEvent.index_apply e,
   CommitEvent.snapshot_publish e]

/-- INV-FERR-008: The canonical commit sequence satisfies the ordering. -/
theorem commit_sequence_valid (e : Nat) :
    valid_commit_order (commit_sequence e) e := by
  unfold valid_commit_order commit_sequence
  rfl

/-- The commit sequence after wal_append is [wal_fsync, index_apply, snapshot_publish]. -/
theorem commit_after_append (e : Nat) :
    (commit_sequence e).tail = [CommitEvent.wal_fsync e,
                                CommitEvent.index_apply e,
                                CommitEvent.snapshot_publish e] := by
  unfold commit_sequence; rfl

/-- INV-FERR-008: WAL fsync is in the commit sequence. -/
theorem wal_fsync_in_sequence (e : Nat) :
    CommitEvent.wal_fsync e ∈ commit_sequence e := by
  unfold commit_sequence
  simp

/-- INV-FERR-008: Snapshot publish is in the commit sequence. -/
theorem snapshot_publish_in_sequence (e : Nat) :
    CommitEvent.snapshot_publish e ∈ commit_sequence e := by
  unfold commit_sequence
  simp

/-- INV-FERR-008: WAL fsync precedes snapshot publish — fsync is not in
    the suffix that starts with snapshot_publish.
    We prove the contrapositive: if we drop the first two elements
    (wal_append, wal_fsync), snapshot_publish is still present,
    meaning it comes after wal_fsync. -/
theorem wal_fsync_before_publish (e : Nat) :
    CommitEvent.snapshot_publish e ∈ (commit_sequence e).drop 2 := by
  unfold commit_sequence
  simp

/-- INV-FERR-008: WAL fsync is NOT in the part of the sequence after
    snapshot_publish. Proved by showing wal_fsync is in the first half. -/
theorem wal_fsync_not_after_publish (e : Nat) :
    CommitEvent.wal_fsync e ∉ (commit_sequence e).drop 3 := by
  unfold commit_sequence
  simp

/-- INV-FERR-008: WAL append precedes WAL fsync — append is element 0,
    fsync is element 1. -/
theorem wal_append_before_fsync (e : Nat) :
    CommitEvent.wal_fsync e ∈ (commit_sequence e).drop 1 := by
  unfold commit_sequence
  simp

/-- INV-FERR-008: If process crashes before fsync (prefix ≤ 1 element),
    the transaction is not published. Empty prefix case. -/
theorem crash_before_fsync_no_publish_zero (e : Nat) :
    CommitEvent.snapshot_publish e ∉ (commit_sequence e).take 0 := by
  simp [commit_sequence]

/-- INV-FERR-008: Prefix of length 1 (only wal_append) does not contain
    snapshot_publish. -/
theorem crash_before_fsync_no_publish_one (e : Nat) :
    CommitEvent.snapshot_publish e ∉ (commit_sequence e).take 1 := by
  unfold commit_sequence
  simp [List.take]

/-! ## INV-FERR-011: Observer Monotonicity

  For an observer's epoch stream [e₁, e₂, ...], epoch(eᵢ) ≤ epoch(eᵢ₊₁).
  Observations never regress. -/

/-- INV-FERR-011: fetch_max ensures non-regression. Given a previous observed
    epoch and a current store epoch, the observer sees max(prev, current) ≥ prev. -/
theorem observer_fetch_max_ge_prev (prev current : Nat) :
    prev ≤ max prev current :=
  Nat.le_max_left prev current

/-- INV-FERR-011: fetch_max ensures non-regression relative to current. -/
theorem observer_fetch_max_ge_current (prev current : Nat) :
    current ≤ max prev current :=
  Nat.le_max_right prev current

/-- Observer state: tracks the last seen epoch. -/
structure Observer where
  last_epoch : Nat

/-- Observe: update the observer and return the effective epoch (max of last and current). -/
def Observer.observe (obs : Observer) (store_epoch : Nat) : Observer × Nat :=
  let effective := max obs.last_epoch store_epoch
  (⟨effective⟩, effective)

/-- INV-FERR-011: Observation never regresses the observer's epoch. -/
theorem observer_monotone (obs : Observer) (store_epoch : Nat) :
    obs.last_epoch ≤ (obs.observe store_epoch).2 := by
  unfold Observer.observe
  simp only
  exact Nat.le_max_left obs.last_epoch store_epoch

/-- INV-FERR-011: Sequential observations produce a non-decreasing epoch sequence.
    If obs₁ = observe(obs₀, e₁) and obs₂ = observe(obs₁, e₂), then
    obs₁.epoch ≤ obs₂.epoch. -/
theorem observer_sequential_monotone (obs : Observer) (e₁ e₂ : Nat) :
    let (obs₁, epoch₁) := obs.observe e₁
    let (_, epoch₂) := obs₁.observe e₂
    epoch₁ ≤ epoch₂ := by
  unfold Observer.observe
  simp only
  exact Nat.le_max_left _ _

/-- INV-FERR-011: Snapshot monotonicity — if observer epochs are non-decreasing,
    then snapshot sets are non-decreasing (each is subset of the next). -/
theorem observer_snapshot_monotone (s : DatomStore) (e₁ e₂ : Nat) (hle : e₁ ≤ e₂) :
    snapshot s e₁ ⊆ snapshot s e₂ :=
  snapshot_monotone s e₁ e₂ hle

/-! ## INV-FERR-020: Transaction Atomicity

  All datoms in a transaction receive the same epoch.
  A transaction is either fully visible or fully invisible at any snapshot. -/

/-- A transaction is a list of datoms, all assigned the same epoch. -/
def stamp_transaction (tx_datoms : List Datom) (epoch : Nat) : List Datom :=
  tx_datoms.map (fun d => ⟨d.e, d.a, d.v, epoch, d.op⟩)

/-- Apply a stamped transaction to the store. -/
def apply_transaction (s : DatomStore) (tx_datoms : List Datom) (epoch : Nat) :
    DatomStore :=
  s ∪ (stamp_transaction tx_datoms epoch).toFinset

/-- INV-FERR-020: All datoms in a stamped transaction have the same epoch. -/
theorem transaction_epoch_uniform (tx_datoms : List Datom) (epoch : Nat) :
    ∀ d ∈ stamp_transaction tx_datoms epoch, d.tx = epoch := by
  intro d hd
  simp only [stamp_transaction, List.mem_map] at hd
  obtain ⟨_, _, rfl⟩ := hd
  rfl

/-- INV-FERR-020: At a snapshot epoch ≥ tx epoch, ALL transaction datoms are visible. -/
theorem transaction_all_visible (s : DatomStore) (tx_datoms : List Datom)
    (tx_epoch snap_epoch : Nat) (hle : tx_epoch ≤ snap_epoch) :
    ∀ d ∈ stamp_transaction tx_datoms tx_epoch,
      d ∈ snapshot (apply_transaction s tx_datoms tx_epoch) snap_epoch := by
  intro d hd
  simp only [snapshot, Finset.mem_filter, apply_transaction]
  constructor
  · exact Finset.mem_union_right s (List.mem_toFinset.mpr hd)
  · rw [transaction_epoch_uniform tx_datoms tx_epoch d hd]
    exact hle

/-- INV-FERR-020: At a snapshot epoch < tx epoch, NO transaction datoms are visible. -/
theorem transaction_all_invisible (s : DatomStore) (tx_datoms : List Datom)
    (tx_epoch snap_epoch : Nat) (hlt : snap_epoch < tx_epoch)
    (_hfresh : ∀ d ∈ stamp_transaction tx_datoms tx_epoch, d ∉ s) :
    ∀ d ∈ stamp_transaction tx_datoms tx_epoch,
      d ∉ snapshot (apply_transaction s tx_datoms tx_epoch) snap_epoch := by
  intro d hd
  simp only [snapshot, apply_transaction, Finset.mem_filter, Finset.mem_union, not_and]
  intro _
  rw [transaction_epoch_uniform tx_datoms tx_epoch d hd]
  omega

/-- INV-FERR-020: Atomicity — either all datoms visible or none visible.
    This is the key theorem: there is no snapshot epoch where a strict
    subset of the transaction's datoms is visible. -/
theorem transaction_atomic_visibility (s : DatomStore) (tx_datoms : List Datom)
    (tx_epoch snap_epoch : Nat)
    (hfresh : ∀ d ∈ stamp_transaction tx_datoms tx_epoch, d ∉ s) :
    (∀ d ∈ stamp_transaction tx_datoms tx_epoch,
       d ∈ snapshot (apply_transaction s tx_datoms tx_epoch) snap_epoch) ∨
    (∀ d ∈ stamp_transaction tx_datoms tx_epoch,
       d ∉ snapshot (apply_transaction s tx_datoms tx_epoch) snap_epoch) := by
  by_cases hle : tx_epoch ≤ snap_epoch
  · left; exact transaction_all_visible s tx_datoms tx_epoch snap_epoch hle
  · right
    have hlt : snap_epoch < tx_epoch := Nat.lt_of_not_le hle
    exact transaction_all_invisible s tx_datoms tx_epoch snap_epoch hlt hfresh

/-! ## INV-FERR-014: Crash Recovery

  Two complementary models:

  **Point recovery**: `recover(committed, wal) = committed ∪ wal`.
  Three algebraic facts:
  1. `committed ⊆ recover(committed, wal)` — no committed data lost
  2. `d ∈ recover(committed, wal) → d ∈ committed ∨ d ∈ wal` — no phantoms
  3. `recover(committed, ∅) = committed` — clean recovery is identity

  **WAL prefix**: `recover_wal(prefix(WAL)) ⊆ recover_wal(WAL)`.
  Crash truncates the WAL to a prefix; recovery via fold-union on the
  prefix produces a subset of recovery on the full WAL.

  INV-FERR-014 is tagged V:MODEL, not V:LEAN. These proofs capture the
  algebraic core — recovery is set union, and union is monotonic. The crash
  non-determinism (which WAL entries survived) is verified by Stateright. -/

/-- Point recovery: merge checkpoint with WAL delta via set union. -/
abbrev recover_point (committed wal_delta : DatomStore) : DatomStore :=
  committed ∪ wal_delta

/-- INV-FERR-014: Recovery preserves all committed data.
    `committed ⊆ committed ∪ wal` — no committed datom is lost. -/
theorem recovery_preserves_committed (committed wal_delta : DatomStore) :
    committed ⊆ recover_point committed wal_delta :=
  Finset.subset_union_left

/-- INV-FERR-014: Recovery introduces no phantom datoms.
    Every datom in the recovered store was either committed or in the WAL. -/
theorem recovery_no_phantoms (committed wal_delta : DatomStore) (d : Datom) :
    d ∈ recover_point committed wal_delta → d ∈ committed ∨ d ∈ wal_delta :=
  Finset.mem_union.mp

/-- INV-FERR-014: Clean recovery (empty WAL) is the identity.
    `recover(committed, ∅) = committed`. -/
theorem recovery_idempotent_clean (committed : DatomStore) :
    recover_point committed ∅ = committed :=
  Finset.union_empty committed

/-! ### WAL Prefix Theorem

  The WAL is a free monoid over transactions. Recovery folds the WAL with
  set-union. A crash truncates the WAL to a prefix. The algebraic guarantee:

    `recover(prefix(WAL)) ⊆ recover(WAL)`

  This is the universal crash-recovery correctness theorem. It holds for
  ALL WAL lengths, ALL crash timings, ALL datom types — because set-union
  is monotone on subsets. The Stateright model verifies a bounded instance;
  this Lean theorem proves the universal property. -/

/-- Recover a WAL by folding transactions via set-union.
    Each transaction is a finite set of datoms. -/
def recover_wal (wal : List DatomStore) : DatomStore :=
  wal.foldl (· ∪ ·) ∅

/-- Lemma: the accumulator is always a subset of foldl union's result.
    `acc ⊆ foldl (· ∪ ·) acc xs` for any list of stores.
    This is the key monotonicity lemma: folding with union only grows. -/
private theorem foldl_union_acc_subset (acc : DatomStore) :
    ∀ (xs : List DatomStore), acc ⊆ xs.foldl (· ∪ ·) acc := by
  intro xs
  induction xs generalizing acc with
  | nil => exact Finset.Subset.refl acc
  | cons x xs ih =>
    simp only [List.foldl_cons]
    exact (Finset.subset_union_left).trans (ih (acc ∪ x))

/-- INV-FERR-014: Crash recovery monotone prefix theorem.

    A crash truncates the WAL to a prefix (the fsynced entries).
    Recovery via set-union fold on the prefix produces a subset of
    recovery on the full WAL:

      `recover(prefix(WAL)) ⊆ recover(WAL)`

    Proof: decompose `wal = take n ++ drop n`, then:
      `foldl ∅ wal = foldl (foldl ∅ (take n)) (drop n)`
    By `foldl_union_acc_subset`:
      `foldl ∅ (take n) ⊆ foldl (foldl ∅ (take n)) (drop n)`

    This is the algebraic core of crash-recovery correctness. It holds
    for ALL WAL lengths, ALL crash timings, ALL datom types — because
    set-union is monotone. The Stateright model verifies bounded instances;
    this theorem proves the universal property. -/
theorem crash_recovery_monotone_prefix
    (wal : List DatomStore) (n : Nat) :
    recover_wal (wal.take n) ⊆ recover_wal wal := by
  unfold recover_wal
  conv_rhs => rw [← List.take_append_drop n wal]
  rw [List.foldl_append]
  exact foldl_union_acc_subset _ _
