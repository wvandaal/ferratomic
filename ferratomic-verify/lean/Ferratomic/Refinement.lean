/- 
  Ferratomic Refinement — concrete epoch/datoms coupling model.

  Invariants proven:
    CI-FERR-001  Lean-Rust coupling invariant (epoch/datoms projection)

  Spec: spec/07-refinement.md §23.11
-/

import Ferratomic.Store

/-! ## CI-FERR-001: Lean-Rust Coupling Invariant

The abstract `DatomStore` model in `Store.lean` tracks only the set of datoms.
The concrete Rust store also carries an epoch counter. This file introduces a
small Lean-side `ConcreteModel` that couples those two components so we can
prove the epoch/datoms relationship directly. -/

/-- Concrete state used for the refinement boundary: datom set + epoch. -/
structure ConcreteModel where
  datoms : DatomStore
  epoch : Nat

/-- Concrete genesis state: empty store at epoch 0. -/
def cm_genesis : ConcreteModel :=
  { datoms := ∅, epoch := 0 }

/-- Concrete transact step: insert one datom and advance epoch by exactly 1. -/
def cm_transact (model : ConcreteModel) (d : Datom) : ConcreteModel :=
  { datoms := apply_tx model.datoms d, epoch := model.epoch + 1 }

/-- Concrete merge step: union datoms and keep the maximum observed epoch. -/
def cm_merge (left right : ConcreteModel) : ConcreteModel :=
  { datoms := merge left.datoms right.datoms, epoch := max left.epoch right.epoch }

/-- Replay a transact-only history from genesis. -/
def cm_replay (history : List Datom) : ConcreteModel :=
  history.foldl cm_transact cm_genesis

/-- CI-FERR-001: genesis establishes the empty datom set at epoch 0. -/
theorem ci_genesis : cm_genesis.epoch = 0 ∧ cm_genesis.datoms = ∅ := by
  exact ⟨rfl, rfl⟩

/-- CI-FERR-001 + INV-FERR-004: transact advances epoch and preserves old datoms. -/
theorem ci_transact (model : ConcreteModel) (d : Datom) :
    (cm_transact model d).epoch = model.epoch + 1 ∧
      model.datoms ⊆ (cm_transact model d).datoms := by
  exact ⟨rfl, apply_superset model.datoms d⟩

/-- CI-FERR-001 + INV-FERR-007: merge keeps the max epoch and unions datoms. -/
theorem ci_merge_epoch (left right : ConcreteModel) :
    (cm_merge left right).epoch = max left.epoch right.epoch ∧
      (cm_merge left right).datoms = merge left.datoms right.datoms := by
  exact ⟨rfl, rfl⟩

/-- Helper: replayed epoch equals initial epoch plus history length. -/
private theorem cm_replay_epoch_from (history : List Datom) (init : ConcreteModel) :
    (history.foldl cm_transact init).epoch = init.epoch + history.length := by
  induction history generalizing init with
  | nil =>
      simp
  | cons d rest ih =>
      simp [cm_transact, ih, Nat.add_comm, Nat.add_left_comm]

/-- Replay from genesis advances epoch once per transaction in the history. -/
theorem cm_replay_epoch (history : List Datom) :
    (cm_replay history).epoch = history.length := by
  simpa [cm_replay, cm_genesis] using cm_replay_epoch_from history cm_genesis

/-- Helper: replayed datoms equal `apply_tx` folded from the initial datom set. -/
private theorem cm_replay_datoms_from (history : List Datom) (init : ConcreteModel) :
    (history.foldl cm_transact init).datoms = history.foldl apply_tx init.datoms := by
  induction history generalizing init with
  | nil =>
      rfl
  | cons d rest ih =>
      simp [cm_transact, ih]

/-- Replay from genesis produces the same datom set as the abstract store model. -/
theorem cm_replay_datoms (history : List Datom) :
    (cm_replay history).datoms = history.toFinset := by
  simpa [cm_replay, cm_genesis, apply_tx_foldl_eq_toFinset] using
    cm_replay_datoms_from history cm_genesis

/-- For transact-only histories with distinct datoms, epoch is bounded by card.

Duplicate datoms are excluded because `DatomStore` is a `Finset`: replaying the
same datom twice still advances epoch twice but does not increase cardinality
twice. The `Nodup` precondition captures the history shape where transaction
count and set cardinality stay aligned. -/
theorem epoch_bounds_card (history : List Datom) (h_nodup : history.Nodup) :
    (cm_replay history).epoch ≤ (cm_replay history).datoms.card := by
  rw [cm_replay_epoch, cm_replay_datoms, List.toFinset_card_of_nodup h_nodup]
