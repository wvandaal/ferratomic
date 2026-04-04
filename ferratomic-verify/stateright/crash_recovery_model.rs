#![forbid(unsafe_code)]

//! Stateright crash-recovery model for INV-FERR-014 (Recovery Correctness).
//!
//! Models the state machine:
//!   Idle → Writing → FsyncWal → Fsynced → Commit → Idle
//!                  ↘ CrashBeforeFsync       ↘ CrashAfterFsync
//!                     Crashed{fsynced:false}    Crashed{fsynced:true}
//!                                    ↘ Recover ↙
//!                                     Recovered
//!
//! Properties verified:
//! - **Safety**: `recover(crash(S)) ⊇ last_committed(S)` — every committed
//!   transaction's datoms survive recovery. No phantom datoms appear.
//! - **Idempotency**: double-recovery produces the same store.
//! - **Liveness**: a recovered state is always reachable from any crash.
//! - **Index bijection** (INV-FERR-005): secondary index equals committed
//!   store in every stable state.
//! - **WAL fsync ordering** (INV-FERR-008): un-fsynced pending datoms are
//!   never in the WAL.
//! - **Checkpoint equivalence** (INV-FERR-013): load(checkpoint(S)) = S,
//!   aliased to recovery idempotency at model level.
//! - **Append-only recovery** (INV-FERR-018): committed data survives
//!   recovery (committed_store ⊆ recovered_store).

use std::collections::BTreeSet;

use stateright::{Model, Property};

use super::crdt_model::Datom;

// ---------------------------------------------------------------------------
// State machine
// ---------------------------------------------------------------------------

/// Phase of the crash-recovery lifecycle.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum Phase {
    /// No operation in progress.
    Idle,
    /// A transaction is being written (WAL entry prepared, not yet fsynced).
    Writing {
        /// Datoms in the pending transaction.
        pending: BTreeSet<Datom>,
    },
    /// WAL fsync completed but commit (pointer swap) has not. The pending
    /// datoms are durable in the WAL but NOT yet in `committed_store`.
    /// This models the window between steps 3 (fsync) and 4 (ArcSwap::store)
    /// in the real `Database::transact` implementation.
    Fsynced {
        /// Datoms that are durable in the WAL but not yet committed.
        pending: BTreeSet<Datom>,
    },
    /// Process crashed. `fsynced` indicates whether the WAL entry was durable.
    Crashed {
        /// Whether the WAL fsync completed before the crash.
        fsynced: bool,
        /// Datoms that were in the pending transaction at crash time.
        pending: BTreeSet<Datom>,
    },
    /// Recovery is in progress (checkpoint loaded, WAL being replayed).
    Recovering,
    /// Recovery complete — store is fully functional.
    Recovered,
}

/// Full state of the crash-recovery model.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct CrashRecoveryState {
    /// Current lifecycle phase.
    pub phase: Phase,
    /// The durable store: datoms from all committed transactions AND
    /// datoms promoted from fsynced WAL entries during recovery.
    /// This is the "last_committed(S)" projection from the spec.
    pub committed_store: BTreeSet<Datom>,
    /// The store visible after recovery. Populated during the Recovering
    /// transition and checked against `committed_store` in properties.
    pub recovered_store: BTreeSet<Datom>,
    /// WAL contents: datoms that were fsynced to the WAL. Recovery replays
    /// these on top of the checkpoint (committed_store).
    pub wal: BTreeSet<Datom>,
    /// INV-FERR-005: Abstract secondary index. Must equal committed_store in
    /// every stable state (Idle, Recovered). May diverge transiently during
    /// Writing or after Crash (recovery restores the bijection).
    pub index_set: BTreeSet<Datom>,
    /// Tracks how many transactions have been committed (bounds the model).
    pub committed_count: usize,
}

/// Actions in the crash-recovery state machine.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum CrashRecoveryAction {
    /// Begin writing a transaction containing the given datom.
    BeginWrite(Datom),
    /// The WAL fsync completes — transaction is now durable.
    FsyncWal,
    /// Commit: apply fsynced WAL entry to the store.
    Commit,
    /// Crash during a write (WAL not yet fsynced).
    CrashBeforeFsync,
    /// Crash after WAL fsync but before commit published.
    CrashAfterFsync,
    /// Crash while idle (clean crash, no in-flight transaction).
    CrashIdle,
    /// Run the recovery procedure.
    Recover,
}

// ---------------------------------------------------------------------------
// Model configuration
// ---------------------------------------------------------------------------

/// Bounded Stateright model for INV-FERR-014 crash-recovery correctness.
#[derive(Clone, Debug)]
pub struct CrashRecoveryModel {
    /// Finite datom domain size explored by the checker.
    pub max_datoms: u64,
    /// Maximum number of committed transactions before the model stops
    /// generating new writes (keeps state space finite).
    pub max_commits: usize,
}

impl CrashRecoveryModel {
    /// Constructs a bounded crash-recovery model.
    pub const fn new(max_datoms: u64, max_commits: usize) -> Self {
        Self {
            max_datoms,
            max_commits,
        }
    }
}

impl Default for CrashRecoveryModel {
    fn default() -> Self {
        Self::new(3, 3)
    }
}

// ---------------------------------------------------------------------------
// Model implementation
// ---------------------------------------------------------------------------

/// Generate write actions for a state that can accept new writes.
fn generate_write_actions(
    max_datoms: u64,
    max_commits: usize,
    state: &CrashRecoveryState,
    actions: &mut Vec<CrashRecoveryAction>,
) {
    if state.committed_count < max_commits {
        for seed in 0..max_datoms {
            let datom = Datom::from_seed(seed);
            if !state.committed_store.contains(&datom) {
                actions.push(CrashRecoveryAction::BeginWrite(datom));
            }
        }
    }
}

/// Apply the FsyncWal transition: make pending datoms durable in the WAL.
///
/// INV-FERR-008: after this step, the pending datoms are in the WAL and
/// will survive a crash. But `committed_store` is NOT yet updated — that
/// happens in the `Commit` step. This models the real implementation's
/// window between WAL fsync and ArcSwap pointer publication.
fn apply_fsync_wal_transition(
    next: &mut CrashRecoveryState,
    state: &CrashRecoveryState,
) -> Option<()> {
    let pending = match &state.phase {
        Phase::Writing { pending } => pending.clone(),
        _ => return None,
    };
    next.wal = next.wal.union(&pending).cloned().collect();
    next.phase = Phase::Fsynced { pending };
    Some(())
}

/// Apply the Commit transition: publish fsynced datoms to committed_store.
///
/// INV-FERR-014: this is the point where the transaction becomes visible.
/// INV-FERR-005: index_set is updated atomically with committed_store.
/// INV-FERR-007: committed_count advances (epoch monotonicity).
fn apply_commit_transition(
    next: &mut CrashRecoveryState,
    state: &CrashRecoveryState,
) -> Option<()> {
    let pending = match &state.phase {
        Phase::Fsynced { pending } => pending.clone(),
        _ => return None,
    };
    next.committed_store = next.committed_store.union(&pending).cloned().collect();
    next.index_set = next.committed_store.clone();
    next.committed_count += 1;
    next.phase = Phase::Idle;
    Some(())
}

/// Apply the recovery procedure to a crashed state.
///
/// INV-FERR-014 Level 1: Load checkpoint, replay fsynced WAL entries,
/// truncate incomplete entries.
///
/// INV-FERR-005: rebuilds the index_set from the recovered store to restore
/// the bijection invariant.
fn apply_recovery(next: &mut CrashRecoveryState, fsynced: bool, pending: &BTreeSet<Datom>) {
    let mut recovered = next.committed_store.clone();
    if fsynced {
        // Recovery replays fsynced WAL entries into committed_store.
        // The pending datoms are already in next.wal (added during FsyncWal),
        // so we do NOT modify wal here — recovery reads the WAL, it does
        // not write to it.
        recovered = recovered.union(pending).cloned().collect();
        next.committed_store = recovered.clone();
        // The fsynced transaction was committed via WAL replay — increment
        // committed_count to match the normal Commit path.
        next.committed_count += 1;
    }
    next.recovered_store = recovered;
    next.index_set = next.recovered_store.clone();
    next.phase = Phase::Recovered;
}

impl Model for CrashRecoveryModel {
    type State = CrashRecoveryState;
    type Action = CrashRecoveryAction;

    fn init_states(&self) -> Vec<Self::State> {
        vec![CrashRecoveryState {
            phase: Phase::Idle,
            committed_store: BTreeSet::new(),
            recovered_store: BTreeSet::new(),
            wal: BTreeSet::new(),
            index_set: BTreeSet::new(),
            committed_count: 0,
        }]
    }

    fn actions(&self, state: &Self::State, actions: &mut Vec<Self::Action>) {
        match &state.phase {
            Phase::Idle => {
                generate_write_actions(self.max_datoms, self.max_commits, state, actions);
                actions.push(CrashRecoveryAction::CrashIdle);
            }
            Phase::Writing { .. } => {
                actions.push(CrashRecoveryAction::FsyncWal);
                actions.push(CrashRecoveryAction::CrashBeforeFsync);
            }
            Phase::Fsynced { .. } => {
                actions.push(CrashRecoveryAction::Commit);
                actions.push(CrashRecoveryAction::CrashAfterFsync);
            }
            Phase::Crashed { .. } => {
                actions.push(CrashRecoveryAction::Recover);
            }
            Phase::Recovering => {}
            Phase::Recovered => {
                generate_write_actions(self.max_datoms, self.max_commits, state, actions);
                actions.push(CrashRecoveryAction::CrashIdle);
            }
        }
    }

    fn next_state(&self, state: &Self::State, action: Self::Action) -> Option<Self::State> {
        let mut next = state.clone();
        match action {
            CrashRecoveryAction::BeginWrite(datom) => {
                match &state.phase {
                    Phase::Idle | Phase::Recovered => {}
                    _ => return None,
                }
                next.phase = Phase::Writing {
                    pending: BTreeSet::from([datom]),
                };
            }
            CrashRecoveryAction::FsyncWal => {
                apply_fsync_wal_transition(&mut next, state)?;
            }
            CrashRecoveryAction::Commit => {
                apply_commit_transition(&mut next, state)?;
            }
            CrashRecoveryAction::CrashAfterFsync => {
                let pending = match &state.phase {
                    Phase::Fsynced { pending } => pending.clone(),
                    _ => return None,
                };
                next.phase = Phase::Crashed {
                    fsynced: true,
                    pending,
                };
            }
            CrashRecoveryAction::CrashBeforeFsync => {
                let pending = match &state.phase {
                    Phase::Writing { pending } => pending.clone(),
                    _ => return None,
                };
                next.phase = Phase::Crashed {
                    fsynced: false,
                    pending,
                };
            }
            CrashRecoveryAction::CrashIdle => {
                match &state.phase {
                    Phase::Idle | Phase::Recovered => {}
                    _ => return None,
                }
                next.phase = Phase::Crashed {
                    fsynced: false,
                    pending: BTreeSet::new(),
                };
            }
            CrashRecoveryAction::Recover => {
                let (fsynced, pending) = match &state.phase {
                    Phase::Crashed { fsynced, pending } => (*fsynced, pending.clone()),
                    _ => return None,
                };
                apply_recovery(&mut next, fsynced, &pending);
            }
        }
        Some(next)
    }

    fn within_boundary(&self, state: &Self::State) -> bool {
        state.committed_count <= self.max_commits
    }

    fn properties(&self) -> Vec<Property<Self>> {
        vec![
            // INV-FERR-014 Safety: recover(crash(S)) ⊇ last_committed(S).
            // After recovery, the recovered store must contain ALL datoms
            // from the committed store (no committed data lost).
            Property::always(
                "inv_ferr_014_recovery_preserves_committed",
                |_: &CrashRecoveryModel, state: &CrashRecoveryState| {
                    match &state.phase {
                        Phase::Recovered => state.committed_store.is_subset(&state.recovered_store),
                        _ => true, // Property only meaningful in Recovered phase
                    }
                },
            ),
            // INV-FERR-014 No-phantom safety: no datom appears in the
            // recovered store that was never written to any transaction.
            // recovered_store ⊆ committed_store ∪ fsynced_pending.
            // Since our model folds fsynced pending into committed_store
            // during Recover, this simplifies to recovered == committed.
            Property::always(
                "inv_ferr_014_no_phantom_datoms",
                |_: &CrashRecoveryModel, state: &CrashRecoveryState| match &state.phase {
                    Phase::Recovered => state.recovered_store.is_subset(&state.committed_store),
                    _ => true,
                },
            ),
            // INV-FERR-014 Idempotency: recovered store equals committed
            // store exactly (since the model applies fsynced WAL entries
            // to committed_store during recovery).
            Property::always(
                "inv_ferr_014_recovery_idempotent",
                |_: &CrashRecoveryModel, state: &CrashRecoveryState| match &state.phase {
                    Phase::Recovered => state.recovered_store == state.committed_store,
                    _ => true,
                },
            ),
            // Liveness: a recovered state is reachable.
            Property::sometimes(
                "inv_ferr_014_recovery_reachable",
                |_: &CrashRecoveryModel, state: &CrashRecoveryState| {
                    matches!(&state.phase, Phase::Recovered)
                },
            ),
            // Liveness: recovery after a real write-then-crash is reachable.
            Property::sometimes(
                "inv_ferr_014_write_crash_recovery_reachable",
                |_: &CrashRecoveryModel, state: &CrashRecoveryState| {
                    matches!(&state.phase, Phase::Recovered) && !state.committed_store.is_empty()
                },
            ),
            // Liveness: a Crashed{fsynced:true} state is reachable. This
            // confirms the BFS explores the crash-after-fsync window — the
            // critical durability scenario where data is in the WAL but not
            // yet committed.
            Property::sometimes(
                "inv_ferr_014_crash_after_fsync_reachable",
                |_: &CrashRecoveryModel, state: &CrashRecoveryState| {
                    matches!(&state.phase, Phase::Crashed { fsynced: true, .. })
                },
            ),
            // INV-FERR-005 Safety: Index bijection holds in every stable state.
            // In Idle and Recovered phases, the abstract secondary index must
            // equal the committed store. During Writing, Fsynced, and Crashed
            // phases, the bijection may be transiently broken.
            //
            // NOTE: This model uses a SINGLE abstract index_set. The real
            // implementation has four indexes (EAVT, AEVT, VAET, AVET).
            // Inter-index consistency (e.g., EAVT has entry but AEVT does not)
            // is verified by proptest/integration tests, not this model.
            Property::always(
                "inv_ferr_005_index_bijection",
                |_: &CrashRecoveryModel, state: &CrashRecoveryState| match &state.phase {
                    Phase::Idle | Phase::Recovered => state.committed_store == state.index_set,
                    _ => true,
                },
            ),
            // INV-FERR-008 Safety: WAL fsync ordering (unfsynced direction).
            // If we crashed BEFORE fsync, the pending datoms must NOT be in
            // the WAL — they were never made durable.
            Property::always(
                "inv_ferr_008_no_unfsynced_in_recovery",
                |_: &CrashRecoveryModel, state: &CrashRecoveryState| {
                    if let Phase::Crashed {
                        fsynced: false,
                        pending,
                    } = &state.phase
                    {
                        pending.iter().all(|d| !state.wal.contains(d))
                    } else {
                        true
                    }
                },
            ),
            // INV-FERR-008 Safety: WAL fsync ordering (fsynced direction).
            // If we crashed AFTER fsync, the pending datoms MUST be in the
            // WAL — they were made durable before the crash. Recovery will
            // replay them to restore committed state.
            Property::always(
                "inv_ferr_008_fsynced_pending_in_wal",
                |_: &CrashRecoveryModel, state: &CrashRecoveryState| {
                    if let Phase::Crashed {
                        fsynced: true,
                        pending,
                    } = &state.phase
                    {
                        pending.iter().all(|d| state.wal.contains(d))
                    } else {
                        true
                    }
                },
            ),
            // INV-FERR-013: Checkpoint equivalence — load(checkpoint(S)) = S.
            // At the Stateright model level, this is identical to INV-FERR-014's
            // recovery idempotency property because the model abstracts away the
            // serialization format. The checkpoint round-trip identity reduces to
            // recovered_store == committed_store. The byte-level round-trip is
            // verified separately by proptest (durability_properties.rs).
            // This named alias exists for spec traceability.
            Property::always(
                "inv_ferr_013_checkpoint_equivalence",
                |_: &CrashRecoveryModel, state: &CrashRecoveryState| match &state.phase {
                    Phase::Recovered => state.recovered_store == state.committed_store,
                    _ => true,
                },
            ),
            // INV-FERR-018: Append-only (recovery dimension).
            // ∀ S, op: ∀ d ∈ S: d ∈ op(S, args). No operation removes a datom.
            // For recovery: committed_store ⊆ recovered_store. This is
            // algebraically equivalent to inv_ferr_014_recovery_preserves_committed
            // but named separately for INV-FERR-018 spec traceability.
            // The CRDT dimension of append-only is verified on crdt_model.rs.
            Property::always(
                "inv_ferr_018_append_only_recovery",
                |_: &CrashRecoveryModel, state: &CrashRecoveryState| match &state.phase {
                    Phase::Recovered => state.committed_store.is_subset(&state.recovered_store),
                    _ => true,
                },
            ),
        ]
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use stateright::{Checker, Model};

    use super::{super::crdt_model::Datom, CrashRecoveryAction, CrashRecoveryModel, Phase};

    fn datom(seed: u64) -> Datom {
        Datom::from_seed(seed)
    }

    /// Helper: commit a datom through the full BeginWrite → FsyncWal → Commit
    /// sequence. Returns the state after Commit (Idle phase).
    fn commit_datom(
        model: &CrashRecoveryModel,
        state: &super::CrashRecoveryState,
        d: Datom,
    ) -> super::CrashRecoveryState {
        let s1 = model
            .next_state(state, CrashRecoveryAction::BeginWrite(d))
            .expect("INV-FERR-014: BeginWrite must succeed");
        let s2 = model
            .next_state(&s1, CrashRecoveryAction::FsyncWal)
            .expect("INV-FERR-008: FsyncWal must succeed");
        model
            .next_state(&s2, CrashRecoveryAction::Commit)
            .expect("INV-FERR-014: Commit must succeed")
    }

    // -- Unit tests for individual transitions --

    #[test]
    fn inv_ferr_014_write_fsync_commit_preserves_datom() {
        let model = CrashRecoveryModel::default();
        let init = model.init_states().into_iter().next().unwrap();

        let d = datom(0);
        let after_write = model
            .next_state(&init, CrashRecoveryAction::BeginWrite(d.clone()))
            .expect("INV-FERR-014: write from idle must succeed");

        let after_fsync = model
            .next_state(&after_write, CrashRecoveryAction::FsyncWal)
            .expect("INV-FERR-008: fsync from writing must succeed");

        // After fsync: datom is in WAL but NOT yet in committed_store.
        assert!(
            after_fsync.wal.contains(&d),
            "INV-FERR-008: datom must be in WAL after fsync"
        );
        assert!(
            !after_fsync.committed_store.contains(&d),
            "INV-FERR-008: datom must NOT be in committed_store before commit"
        );
        assert!(
            matches!(after_fsync.phase, Phase::Fsynced { .. }),
            "INV-FERR-008: phase must be Fsynced after fsync"
        );

        let after_commit = model
            .next_state(&after_fsync, CrashRecoveryAction::Commit)
            .expect("INV-FERR-014: commit from fsynced must succeed");

        assert!(
            after_commit.committed_store.contains(&d),
            "INV-FERR-014: datom must be in committed_store after commit"
        );
        assert!(
            matches!(after_commit.phase, Phase::Idle),
            "INV-FERR-014: phase must return to Idle after commit"
        );
    }

    #[test]
    fn inv_ferr_014_crash_before_fsync_loses_pending() {
        let model = CrashRecoveryModel::default();
        let init = model.init_states().into_iter().next().unwrap();

        let d = datom(1);
        let after_write = model
            .next_state(&init, CrashRecoveryAction::BeginWrite(d.clone()))
            .unwrap();

        let after_crash = model
            .next_state(&after_write, CrashRecoveryAction::CrashBeforeFsync)
            .expect("INV-FERR-014: crash before fsync must succeed");

        let after_recover = model
            .next_state(&after_crash, CrashRecoveryAction::Recover)
            .expect("INV-FERR-014: recovery from crash must succeed");

        assert!(
            !after_recover.recovered_store.contains(&d),
            "INV-FERR-014: un-fsynced datom must NOT survive recovery"
        );
        assert!(
            matches!(after_recover.phase, Phase::Recovered),
            "INV-FERR-014: phase must be Recovered after recovery"
        );
    }

    #[test]
    fn inv_ferr_014_committed_data_survives_crash() {
        let model = CrashRecoveryModel::default();
        let init = model.init_states().into_iter().next().unwrap();

        // Commit a datom (full cycle: write → fsync → commit).
        let d = datom(0);
        let s2 = commit_datom(&model, &init, d.clone());

        // Crash from idle.
        let crashed = model
            .next_state(&s2, CrashRecoveryAction::CrashIdle)
            .expect("INV-FERR-014: crash from idle must succeed");

        // Recover.
        let recovered = model
            .next_state(&crashed, CrashRecoveryAction::Recover)
            .expect("INV-FERR-014: recovery must succeed");

        assert!(
            recovered.recovered_store.contains(&d),
            "INV-FERR-014: committed datom must survive crash + recovery"
        );
        assert_eq!(
            recovered.recovered_store, recovered.committed_store,
            "INV-FERR-014: recovered store must equal committed store"
        );
    }

    #[test]
    fn inv_ferr_014_recovery_idempotent_unit() {
        let model = CrashRecoveryModel::default();
        let init = model.init_states().into_iter().next().unwrap();

        // Commit, crash, recover.
        let d = datom(2);
        let s2 = commit_datom(&model, &init, d.clone());
        let crashed = model
            .next_state(&s2, CrashRecoveryAction::CrashIdle)
            .unwrap();
        let recovered = model
            .next_state(&crashed, CrashRecoveryAction::Recover)
            .unwrap();

        // Crash again from recovered state, recover again.
        let crashed2 = model
            .next_state(&recovered, CrashRecoveryAction::CrashIdle)
            .unwrap();
        let recovered2 = model
            .next_state(&crashed2, CrashRecoveryAction::Recover)
            .unwrap();

        assert_eq!(
            recovered.recovered_store, recovered2.recovered_store,
            "INV-FERR-014: double recovery must produce the same store"
        );
    }

    #[test]
    fn inv_ferr_014_multiple_commits_then_crash() {
        let model = CrashRecoveryModel::default();
        let init = model.init_states().into_iter().next().unwrap();

        // Commit two datoms (full cycle each).
        let d0 = datom(0);
        let d1 = datom(1);

        let s2 = commit_datom(&model, &init, d0.clone());
        let s4 = commit_datom(&model, &s2, d1.clone());

        // Crash and recover.
        let crashed = model
            .next_state(&s4, CrashRecoveryAction::CrashIdle)
            .unwrap();
        let recovered = model
            .next_state(&crashed, CrashRecoveryAction::Recover)
            .unwrap();

        assert!(
            recovered.recovered_store.contains(&d0),
            "INV-FERR-014: first committed datom must survive"
        );
        assert!(
            recovered.recovered_store.contains(&d1),
            "INV-FERR-014: second committed datom must survive"
        );
    }

    #[test]
    fn inv_ferr_014_no_phantom_from_uncommitted() {
        let model = CrashRecoveryModel::default();
        let init = model.init_states().into_iter().next().unwrap();

        // Write but crash before fsync.
        let d = datom(0);
        let s1 = model
            .next_state(&init, CrashRecoveryAction::BeginWrite(d.clone()))
            .unwrap();
        let crashed = model
            .next_state(&s1, CrashRecoveryAction::CrashBeforeFsync)
            .unwrap();
        let recovered = model
            .next_state(&crashed, CrashRecoveryAction::Recover)
            .unwrap();

        // Recovered store must be exactly the committed store (empty).
        assert_eq!(
            recovered.recovered_store,
            BTreeSet::new(),
            "INV-FERR-014: no phantom datoms after recovering from un-fsynced crash"
        );
    }

    /// INV-FERR-005: Index bijection — write, fsync, commit, verify index_set
    /// equals committed_store; crash, recover, verify bijection restored.
    #[test]
    fn inv_ferr_005_index_bijection_unit() {
        let model = CrashRecoveryModel::default();
        let init = model.init_states().into_iter().next().unwrap();

        // Initial state: both empty, bijection holds.
        assert_eq!(
            init.committed_store, init.index_set,
            "INV-FERR-005: index_set must equal committed_store in initial Idle state"
        );

        // Write, fsync, and commit a datom.
        let d = datom(0);
        let after_commit = commit_datom(&model, &init, d.clone());

        // After commit (Idle): bijection must hold.
        assert_eq!(
            after_commit.committed_store, after_commit.index_set,
            "INV-FERR-005: index_set must equal committed_store after commit"
        );
        assert!(
            after_commit.index_set.contains(&d),
            "INV-FERR-005: committed datom must be in index_set"
        );

        // Crash and recover: bijection must hold in Recovered state.
        let crashed = model
            .next_state(&after_commit, CrashRecoveryAction::CrashIdle)
            .unwrap();
        let recovered = model
            .next_state(&crashed, CrashRecoveryAction::Recover)
            .unwrap();

        assert_eq!(
            recovered.recovered_store, recovered.index_set,
            "INV-FERR-005: index_set must equal recovered_store after recovery"
        );
        assert!(
            recovered.index_set.contains(&d),
            "INV-FERR-005: committed datom must be in index_set after recovery"
        );
    }

    /// INV-FERR-008: WAL fsync ordering — pending datoms from an un-fsynced
    /// write must NOT appear in the WAL after a crash-before-fsync.
    #[test]
    fn inv_ferr_008_unfsynced_pending_not_in_wal() {
        let model = CrashRecoveryModel::default();
        let init = model.init_states().into_iter().next().unwrap();

        // Write a datom but crash before fsync.
        let d = datom(0);
        let after_write = model
            .next_state(&init, CrashRecoveryAction::BeginWrite(d.clone()))
            .expect("INV-FERR-008: BeginWrite must succeed from Idle");

        let crashed = model
            .next_state(&after_write, CrashRecoveryAction::CrashBeforeFsync)
            .expect("INV-FERR-008: CrashBeforeFsync must succeed from Writing");

        // The pending datom must NOT be in the WAL.
        assert!(
            !crashed.wal.contains(&d),
            "INV-FERR-008: un-fsynced pending datom must not be in WAL after crash"
        );

        // Verify the Crashed phase state is correct.
        assert!(
            matches!(&crashed.phase, super::Phase::Crashed { fsynced: false, .. }),
            "INV-FERR-008: phase must be Crashed with fsynced=false"
        );
    }

    /// INV-FERR-014 + INV-FERR-008: Crash AFTER fsync but BEFORE commit.
    /// The fsynced datom is in the WAL but NOT in committed_store. Recovery
    /// must replay the WAL entry to restore the datom.
    ///
    /// This is the critical crash window that was previously dead code in
    /// the model. It exercises the `apply_recovery(fsynced: true)` path.
    #[test]
    fn inv_ferr_014_crash_after_fsync_recovers_data() {
        let model = CrashRecoveryModel::default();
        let init = model.init_states().into_iter().next().unwrap();

        let d = datom(0);
        let after_write = model
            .next_state(&init, CrashRecoveryAction::BeginWrite(d.clone()))
            .expect("INV-FERR-014: BeginWrite must succeed");
        let after_fsync = model
            .next_state(&after_write, CrashRecoveryAction::FsyncWal)
            .expect("INV-FERR-008: FsyncWal must succeed");

        // Crash AFTER fsync but BEFORE commit.
        let crashed = model
            .next_state(&after_fsync, CrashRecoveryAction::CrashAfterFsync)
            .expect("INV-FERR-014: CrashAfterFsync must succeed from Fsynced");

        // The datom is NOT in committed_store (commit never happened).
        assert!(
            !crashed.committed_store.contains(&d),
            "INV-FERR-014: datom must NOT be in committed_store (commit was interrupted)"
        );
        // But it IS in the WAL (fsync completed).
        assert!(
            crashed.wal.contains(&d),
            "INV-FERR-008: fsynced datom must be in WAL after crash"
        );
        assert!(
            matches!(&crashed.phase, Phase::Crashed { fsynced: true, .. }),
            "INV-FERR-014: phase must be Crashed with fsynced=true"
        );

        // Recovery must replay the fsynced WAL entry.
        let recovered = model
            .next_state(&crashed, CrashRecoveryAction::Recover)
            .expect("INV-FERR-014: recovery from fsynced crash must succeed");

        assert!(
            recovered.recovered_store.contains(&d),
            "INV-FERR-014: fsynced datom must survive recovery via WAL replay"
        );
        assert!(
            recovered.committed_store.contains(&d),
            "INV-FERR-014: recovered committed_store must include fsynced datom"
        );
        assert!(
            recovered.wal.contains(&d),
            "INV-FERR-008: fsynced datom must remain in WAL after recovery"
        );
        assert_eq!(
            recovered.index_set, recovered.committed_store,
            "INV-FERR-005: index_set must equal committed_store after fsynced recovery"
        );
        assert_eq!(
            recovered.recovered_store, recovered.committed_store,
            "INV-FERR-014: recovered store must equal committed store after fsynced recovery"
        );
    }

    // -- Model checker tests --

    #[test]
    fn inv_ferr_014_model_checker_all_properties() {
        let checker = CrashRecoveryModel::new(2, 2)
            .checker()
            .target_max_depth(8)
            .spawn_bfs()
            .join();

        // Safety properties must hold in ALL reachable states.
        checker.assert_no_discovery("inv_ferr_014_recovery_preserves_committed");
        checker.assert_no_discovery("inv_ferr_014_no_phantom_datoms");
        checker.assert_no_discovery("inv_ferr_014_recovery_idempotent");
        checker.assert_no_discovery("inv_ferr_005_index_bijection");
        checker.assert_no_discovery("inv_ferr_008_no_unfsynced_in_recovery");
        checker.assert_no_discovery("inv_ferr_008_fsynced_pending_in_wal");
        checker.assert_no_discovery("inv_ferr_013_checkpoint_equivalence");
        checker.assert_no_discovery("inv_ferr_018_append_only_recovery");

        // Liveness: these states must be reachable.
        checker.assert_any_discovery("inv_ferr_014_recovery_reachable");
        checker.assert_any_discovery("inv_ferr_014_write_crash_recovery_reachable");
        checker.assert_any_discovery("inv_ferr_014_crash_after_fsync_reachable");
    }

    #[test]
    fn inv_ferr_014_model_checker_larger_domain() {
        // Slightly larger domain to explore more interleavings.
        let checker = CrashRecoveryModel::new(3, 3)
            .checker()
            .target_max_depth(12)
            .spawn_bfs()
            .join();

        checker.assert_no_discovery("inv_ferr_014_recovery_preserves_committed");
        checker.assert_no_discovery("inv_ferr_014_no_phantom_datoms");
        checker.assert_no_discovery("inv_ferr_014_recovery_idempotent");
        checker.assert_no_discovery("inv_ferr_005_index_bijection");
        checker.assert_no_discovery("inv_ferr_008_no_unfsynced_in_recovery");
        checker.assert_no_discovery("inv_ferr_008_fsynced_pending_in_wal");
        checker.assert_no_discovery("inv_ferr_013_checkpoint_equivalence");
        checker.assert_no_discovery("inv_ferr_018_append_only_recovery");
        checker.assert_any_discovery("inv_ferr_014_recovery_reachable");
        checker.assert_any_discovery("inv_ferr_014_write_crash_recovery_reachable");
        checker.assert_any_discovery("inv_ferr_014_crash_after_fsync_reachable");
    }
}
