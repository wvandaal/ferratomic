#![forbid(unsafe_code)]

//! Stateright bounded model for write linearizability (INV-FERR-007).
//!
//! INV-FERR-007: Committed writes appear in a strict total order defined by
//! their epoch numbers. The epoch sequence is strictly monotonically increasing
//! for committed writes. Combined with snapshot isolation (INV-FERR-006), every
//! committed write is visible to all subsequent snapshots.
//!
//! This model checks:
//! 1. **Safety**: committed_epochs is strictly monotonically increasing.
//! 2. **Safety**: recovery never produces an epoch less than the last committed.
//! 3. **Liveness**: at least one write commits successfully.
//!
//! The state machine mirrors the spec Level 2 contract from §01-core-invariants:
//! - A write lock serializes writers (Mutex).
//! - Epoch is assigned under the lock.
//! - WAL fsync precedes publication (INV-FERR-008).
//! - Crashes may occur before or after fsync, with recovery replaying the WAL.

use stateright::{Model, Property};

// ---------------------------------------------------------------------------
// Bounded parameters
// ---------------------------------------------------------------------------

/// Maximum epoch value explored by the checker.
const MAX_EPOCH: u64 = 3;

/// Maximum number of pending transactions in the model.
const MAX_PENDING: usize = 3;

// ---------------------------------------------------------------------------
// WriterLock
// ---------------------------------------------------------------------------

/// The state of the single-writer lock (spec Level 2: `self.write_lock.lock()`).
///
/// INV-FERR-007: only one writer holds the lock at a time, ensuring
/// epoch assignment is serialized.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum WriterLock {
    /// No writer currently holds the lock.
    Free,
    /// Writer `id` holds the lock. The `id` indexes into `pending_txns`.
    Held(usize),
}

// ---------------------------------------------------------------------------
// CrashMode
// ---------------------------------------------------------------------------

/// Whether the system is in normal operation or recovering from a crash.
///
/// INV-FERR-008: WAL fsync ordering interacts with crash recovery.
/// The model explores both crash-before-fsync and crash-after-fsync
/// to verify epoch monotonicity survives either scenario.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum CrashMode {
    /// Normal operation; writers can acquire the lock and commit.
    Normal,
    /// Recovering from a crash. No new writes until recovery completes.
    Recovering,
}

// ---------------------------------------------------------------------------
// PendingTxn
// ---------------------------------------------------------------------------

/// A pending transaction in the write pipeline.
///
/// Tracks each writer's progress through the commit protocol:
/// lock -> assign epoch -> fsync WAL -> release lock.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum PendingTxn {
    /// Waiting to acquire the write lock.
    Waiting,
    /// Lock acquired, epoch assigned but WAL not yet fsynced.
    EpochAssigned(u64),
    /// WAL fsynced, ready to release lock and publish.
    Fsynced(u64),
    /// Transaction completed and committed.
    Done(u64),
}

// ---------------------------------------------------------------------------
// WriteLinState
// ---------------------------------------------------------------------------

/// State of the write linearizability model.
///
/// INV-FERR-007: `committed_epochs` is the externally visible commit
/// history. The safety property asserts it is strictly monotonically
/// increasing at every reachable state.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct WriteLinState {
    /// Strictly ordered list of committed epoch numbers.
    pub committed_epochs: Vec<u64>,
    /// The next epoch to assign. Monotonically non-decreasing.
    pub current_epoch: u64,
    /// Single-writer lock state.
    pub writer_lock: WriterLock,
    /// Pending transaction states (bounded by `MAX_PENDING`).
    pub pending_txns: Vec<PendingTxn>,
    /// Normal operation or crash recovery.
    pub crash_mode: CrashMode,
    /// Epoch of the last WAL entry that was fsynced but not yet committed
    /// to `committed_epochs`. Used to model crash-after-fsync recovery.
    pub wal_fsynced_epoch: Option<u64>,
}

// ---------------------------------------------------------------------------
// WriteLinAction
// ---------------------------------------------------------------------------

/// Actions explored by the Stateright checker.
///
/// These mirror the spec Level 2 contract steps for `Store::transact`:
/// acquire lock -> assign epoch -> fsync WAL -> release lock,
/// plus crash and recovery actions.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum WriteLinAction {
    /// Writer `id` acquires the write lock (spec: `self.write_lock.lock()`).
    AcquireLock(usize),
    /// Writer `id` gets the next epoch under the lock (spec: `self.next_epoch()`).
    AssignEpoch(usize),
    /// Writer `id` fsyncs the WAL entry (spec: `self.wal.fsync()`).
    FsyncWal(usize),
    /// Writer `id` releases the lock and publishes the epoch.
    ReleaseLock(usize),
    /// Crash before the current writer's WAL fsync completes.
    CrashBeforeFsync,
    /// Crash after WAL fsync but before lock release / epoch publication.
    CrashAfterFsync,
    /// Recover from crash: replay WAL and restore consistent state.
    Recover,
}

// ---------------------------------------------------------------------------
// WriteLinModel
// ---------------------------------------------------------------------------

/// Bounded Stateright model for INV-FERR-007 (Write Linearizability).
///
/// Explores all interleavings of 2-3 concurrent writers with crash/recovery
/// to verify that committed epochs are always strictly monotonically increasing.
#[derive(Clone, Debug)]
pub struct WriteLinModel {
    /// Number of concurrent pending writers to explore.
    pub writer_count: usize,
}

impl WriteLinModel {
    /// Construct a model with the given number of writers.
    pub const fn new(writer_count: usize) -> Self {
        Self { writer_count }
    }
}

impl Default for WriteLinModel {
    fn default() -> Self {
        Self::new(3)
    }
}

/// INV-FERR-007: Acquire the exclusive writer lock for writer `id`.
fn apply_acquire_lock(next: &mut WriteLinState, id: usize) -> Option<()> {
    if next.writer_lock != WriterLock::Free
        || next.crash_mode != CrashMode::Normal
        || id >= next.pending_txns.len()
        || next.pending_txns[id] != PendingTxn::Waiting
    {
        return None;
    }
    next.writer_lock = WriterLock::Held(id);
    Some(())
}

/// INV-FERR-007: Assign the next epoch under the held lock.
fn apply_assign_epoch(next: &mut WriteLinState, id: usize) -> Option<()> {
    if next.writer_lock != WriterLock::Held(id) || next.pending_txns[id] != PendingTxn::Waiting {
        return None;
    }
    if next.current_epoch > MAX_EPOCH {
        return None;
    }
    let epoch = next.current_epoch;
    next.pending_txns[id] = PendingTxn::EpochAssigned(epoch);
    next.current_epoch = epoch + 1;
    Some(())
}

/// INV-FERR-008: Fsync the WAL entry for writer `id`.
fn apply_fsync_wal(next: &mut WriteLinState, id: usize) -> Option<()> {
    if let PendingTxn::EpochAssigned(epoch) = next.pending_txns[id] {
        next.pending_txns[id] = PendingTxn::Fsynced(epoch);
        next.wal_fsynced_epoch = Some(epoch);
        Some(())
    } else {
        None
    }
}

/// INV-FERR-007: Release lock and publish epoch for writer `id`.
fn apply_release_lock(next: &mut WriteLinState, id: usize) -> Option<()> {
    if next.writer_lock != WriterLock::Held(id) {
        return None;
    }
    if let PendingTxn::Fsynced(epoch) = next.pending_txns[id] {
        next.committed_epochs.push(epoch);
        next.pending_txns[id] = PendingTxn::Done(epoch);
        next.writer_lock = WriterLock::Free;
        next.wal_fsynced_epoch = None;
        Some(())
    } else {
        None
    }
}

/// Reset in-flight txns to Waiting based on which states should be discarded.
fn reset_inflight_txns(pending: &mut [PendingTxn], reset_fsynced: bool) {
    for txn in pending.iter_mut() {
        match txn {
            PendingTxn::EpochAssigned(_) => *txn = PendingTxn::Waiting,
            PendingTxn::Fsynced(_) if reset_fsynced => *txn = PendingTxn::Waiting,
            _ => {}
        }
    }
}

/// Crash before WAL fsync: in-flight write is lost.
fn apply_crash_before_fsync(next: &mut WriteLinState) {
    next.writer_lock = WriterLock::Free;
    next.wal_fsynced_epoch = None;
    reset_inflight_txns(&mut next.pending_txns, false);
    next.crash_mode = CrashMode::Recovering;
}

/// Crash after WAL fsync but before publication.
fn apply_crash_after_fsync(next: &mut WriteLinState) {
    next.writer_lock = WriterLock::Free;
    reset_inflight_txns(&mut next.pending_txns, true);
    next.crash_mode = CrashMode::Recovering;
}

/// INV-FERR-007: Recover from crash, replaying fsynced WAL entries.
fn apply_recover(next: &mut WriteLinState) -> Option<()> {
    if next.crash_mode != CrashMode::Recovering {
        return None;
    }
    if let Some(wal_epoch) = next.wal_fsynced_epoch.take() {
        let last_committed = next.committed_epochs.last().copied().unwrap_or(0);
        if wal_epoch > last_committed {
            next.committed_epochs.push(wal_epoch);
        }
    }
    let last = next.committed_epochs.last().copied().unwrap_or(0);
    if next.current_epoch <= last {
        next.current_epoch = last + 1;
    }
    next.crash_mode = CrashMode::Normal;
    Some(())
}

impl Model for WriteLinModel {
    type State = WriteLinState;
    type Action = WriteLinAction;

    fn init_states(&self) -> Vec<Self::State> {
        vec![WriteLinState {
            committed_epochs: Vec::new(),
            current_epoch: 1,
            writer_lock: WriterLock::Free,
            pending_txns: vec![PendingTxn::Waiting; self.writer_count],
            crash_mode: CrashMode::Normal,
            wal_fsynced_epoch: None,
        }]
    }

    fn actions(&self, state: &Self::State, actions: &mut Vec<Self::Action>) {
        match &state.crash_mode {
            CrashMode::Recovering => {
                // Only recovery action available while recovering.
                actions.push(WriteLinAction::Recover);
            }
            CrashMode::Normal => {
                for id in 0..state.pending_txns.len() {
                    match &state.pending_txns[id] {
                        PendingTxn::Waiting => {
                            if state.writer_lock == WriterLock::Free {
                                actions.push(WriteLinAction::AcquireLock(id));
                            }
                        }
                        PendingTxn::EpochAssigned(_) => {
                            actions.push(WriteLinAction::FsyncWal(id));
                            // Crash before fsync completes.
                            actions.push(WriteLinAction::CrashBeforeFsync);
                        }
                        PendingTxn::Fsynced(_) => {
                            actions.push(WriteLinAction::ReleaseLock(id));
                            // Crash after fsync but before publication.
                            actions.push(WriteLinAction::CrashAfterFsync);
                        }
                        PendingTxn::Done(_) => {
                            // This writer is finished; no actions.
                        }
                    }
                }

                // Assign epoch: only the lock holder, and only when in
                // the Waiting->Held transition hasn't assigned yet.
                // Actually, epoch assignment happens right after lock
                // acquisition. We model AcquireLock and AssignEpoch as
                // separate steps so crashes can interleave.
                if let WriterLock::Held(holder_id) = &state.writer_lock {
                    if let PendingTxn::Waiting = &state.pending_txns[*holder_id] {
                        actions.push(WriteLinAction::AssignEpoch(*holder_id));
                    }
                }
            }
        }
    }

    fn next_state(&self, state: &Self::State, action: Self::Action) -> Option<Self::State> {
        let mut next = state.clone();
        match action {
            WriteLinAction::AcquireLock(id) => {
                apply_acquire_lock(&mut next, id)?;
            }
            WriteLinAction::AssignEpoch(id) => {
                apply_assign_epoch(&mut next, id)?;
            }
            WriteLinAction::FsyncWal(id) => {
                apply_fsync_wal(&mut next, id)?;
            }
            WriteLinAction::ReleaseLock(id) => {
                apply_release_lock(&mut next, id)?;
            }
            WriteLinAction::CrashBeforeFsync => {
                apply_crash_before_fsync(&mut next);
            }
            WriteLinAction::CrashAfterFsync => {
                apply_crash_after_fsync(&mut next);
            }
            WriteLinAction::Recover => {
                apply_recover(&mut next)?;
            }
        }
        Some(next)
    }

    fn within_boundary(&self, state: &Self::State) -> bool {
        // Bound the state space: max epoch and max pending writers.
        state.current_epoch <= MAX_EPOCH + 1
            && state.committed_epochs.len() <= MAX_EPOCH as usize
            && state.pending_txns.len() <= MAX_PENDING
    }

    fn properties(&self) -> Vec<Property<Self>> {
        vec![
            // Safety 1: INV-FERR-007 — committed epochs are strictly
            // monotonically increasing. Each epoch > the previous.
            Property::always(
                "inv_ferr_007_epoch_strict_monotonicity",
                |_: &WriteLinModel, state: &WriteLinState| {
                    // INV-FERR-007: ∀ T₁, T₂: commit_order(T₁) < commit_order(T₂)
                    //   ⟹ epoch(T₁) < epoch(T₂)
                    state
                        .committed_epochs
                        .windows(2)
                        .all(|pair| pair[0] < pair[1])
                },
            ),
            // Safety 2: INV-FERR-007 — recovery never produces an epoch
            // less than the last committed epoch.
            Property::always(
                "inv_ferr_007_recovery_epoch_monotonicity",
                |_: &WriteLinModel, state: &WriteLinState| {
                    // INV-FERR-007: After recovery, current_epoch must be
                    // strictly greater than every committed epoch, ensuring
                    // the next write will have a higher epoch.
                    let last_committed = state.committed_epochs.last().copied().unwrap_or(0);
                    state.current_epoch > last_committed
                },
            ),
            // Liveness: at least one write commits successfully.
            // This ensures the model is non-vacuous — the commit path
            // is actually reachable, not just trivially safe because
            // nothing ever happens.
            Property::sometimes(
                "inv_ferr_007_at_least_one_commit",
                |_: &WriteLinModel, state: &WriteLinState| {
                    // INV-FERR-007: the commit path is reachable.
                    !state.committed_epochs.is_empty()
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
    use stateright::{Checker, Model};

    use super::*;

    // -- Unit tests for state transitions -----------------------------------

    #[test]
    fn test_inv_ferr_007_initial_state_has_no_commits() {
        let model = WriteLinModel::new(2);
        let states = model.init_states();
        assert_eq!(states.len(), 1);

        let state = &states[0];
        assert!(
            state.committed_epochs.is_empty(),
            "INV-FERR-007: initial state has no committed epochs"
        );
        assert_eq!(
            state.current_epoch, 1,
            "INV-FERR-007: first epoch starts at 1"
        );
        assert_eq!(state.writer_lock, WriterLock::Free);
        assert_eq!(state.crash_mode, CrashMode::Normal);
    }

    #[test]
    fn test_inv_ferr_007_single_write_commits() {
        let model = WriteLinModel::new(1);
        let s0 = model.init_states().remove(0);

        // AcquireLock(0)
        let s1 = model
            .next_state(&s0, WriteLinAction::AcquireLock(0))
            .expect("INV-FERR-007: acquire lock on free lock must succeed");
        assert_eq!(s1.writer_lock, WriterLock::Held(0));

        // AssignEpoch(0)
        let s2 = model
            .next_state(&s1, WriteLinAction::AssignEpoch(0))
            .expect("INV-FERR-007: assign epoch under held lock must succeed");
        assert_eq!(s2.pending_txns[0], PendingTxn::EpochAssigned(1));
        assert_eq!(s2.current_epoch, 2);

        // FsyncWal(0)
        let s3 = model
            .next_state(&s2, WriteLinAction::FsyncWal(0))
            .expect("INV-FERR-007: fsync after epoch assignment must succeed");
        assert_eq!(s3.pending_txns[0], PendingTxn::Fsynced(1));

        // ReleaseLock(0)
        let s4 = model
            .next_state(&s3, WriteLinAction::ReleaseLock(0))
            .expect("INV-FERR-007: release lock after fsync must succeed");
        assert_eq!(
            s4.committed_epochs,
            vec![1],
            "INV-FERR-007: epoch 1 must be committed"
        );
        assert_eq!(s4.writer_lock, WriterLock::Free);
        assert_eq!(s4.pending_txns[0], PendingTxn::Done(1));
    }

    #[test]
    fn test_inv_ferr_007_serialized_writes_produce_monotonic_epochs() {
        let model = WriteLinModel::new(2);
        let s0 = model.init_states().remove(0);

        // Writer 0: full commit cycle.
        let s1 = model
            .next_state(&s0, WriteLinAction::AcquireLock(0))
            .unwrap();
        let s2 = model
            .next_state(&s1, WriteLinAction::AssignEpoch(0))
            .unwrap();
        let s3 = model.next_state(&s2, WriteLinAction::FsyncWal(0)).unwrap();
        let s4 = model
            .next_state(&s3, WriteLinAction::ReleaseLock(0))
            .unwrap();

        // Writer 1: full commit cycle after writer 0 finished.
        let s5 = model
            .next_state(&s4, WriteLinAction::AcquireLock(1))
            .unwrap();
        let s6 = model
            .next_state(&s5, WriteLinAction::AssignEpoch(1))
            .unwrap();
        let s7 = model.next_state(&s6, WriteLinAction::FsyncWal(1)).unwrap();
        let s8 = model
            .next_state(&s7, WriteLinAction::ReleaseLock(1))
            .unwrap();

        assert_eq!(
            s8.committed_epochs,
            vec![1, 2],
            "INV-FERR-007: two serialized writes must produce epochs [1, 2]"
        );
        assert!(
            s8.committed_epochs[0] < s8.committed_epochs[1],
            "INV-FERR-007: epoch sequence must be strictly increasing"
        );
    }

    #[test]
    fn test_inv_ferr_007_lock_prevents_concurrent_acquire() {
        let model = WriteLinModel::new(2);
        let s0 = model.init_states().remove(0);

        // Writer 0 acquires the lock.
        let s1 = model
            .next_state(&s0, WriteLinAction::AcquireLock(0))
            .unwrap();

        // Writer 1 cannot acquire while writer 0 holds it.
        let actions = {
            let mut a = Vec::new();
            model.actions(&s1, &mut a);
            a
        };
        assert!(
            !actions.contains(&WriteLinAction::AcquireLock(1)),
            "INV-FERR-007: second writer must not acquire lock while first holds it"
        );
    }

    #[test]
    fn test_inv_ferr_007_crash_before_fsync_loses_write() {
        let model = WriteLinModel::new(1);
        let s0 = model.init_states().remove(0);

        let s1 = model
            .next_state(&s0, WriteLinAction::AcquireLock(0))
            .unwrap();
        let s2 = model
            .next_state(&s1, WriteLinAction::AssignEpoch(0))
            .unwrap();

        // Crash before fsync.
        let s3 = model
            .next_state(&s2, WriteLinAction::CrashBeforeFsync)
            .expect("INV-FERR-007: crash before fsync must be a valid transition");

        assert_eq!(s3.crash_mode, CrashMode::Recovering);
        assert!(
            s3.committed_epochs.is_empty(),
            "INV-FERR-007: crash before fsync must not commit the epoch"
        );

        // Recover.
        let s4 = model
            .next_state(&s3, WriteLinAction::Recover)
            .expect("INV-FERR-007: recovery must succeed");

        assert_eq!(s4.crash_mode, CrashMode::Normal);
        assert!(
            s4.committed_epochs.is_empty(),
            "INV-FERR-007: recovery after crash-before-fsync must not produce a committed epoch"
        );
    }

    #[test]
    fn test_inv_ferr_007_crash_after_fsync_replays_on_recovery() {
        let model = WriteLinModel::new(1);
        let s0 = model.init_states().remove(0);

        let s1 = model
            .next_state(&s0, WriteLinAction::AcquireLock(0))
            .unwrap();
        let s2 = model
            .next_state(&s1, WriteLinAction::AssignEpoch(0))
            .unwrap();
        let s3 = model.next_state(&s2, WriteLinAction::FsyncWal(0)).unwrap();

        // Crash after fsync but before release.
        let s4 = model
            .next_state(&s3, WriteLinAction::CrashAfterFsync)
            .expect("INV-FERR-007: crash after fsync must be a valid transition");

        assert_eq!(s4.crash_mode, CrashMode::Recovering);
        assert!(
            s4.committed_epochs.is_empty(),
            "INV-FERR-007: crash after fsync must not have published the epoch yet"
        );

        // Recover: WAL replay should commit the fsynced epoch.
        let s5 = model
            .next_state(&s4, WriteLinAction::Recover)
            .expect("INV-FERR-007: recovery must succeed");

        assert_eq!(s5.crash_mode, CrashMode::Normal);
        assert_eq!(
            s5.committed_epochs,
            vec![1],
            "INV-FERR-007: recovery must replay the fsynced WAL entry"
        );
        assert!(
            s5.current_epoch > 1,
            "INV-FERR-007: current_epoch after recovery must exceed the replayed epoch"
        );
    }

    #[test]
    fn test_inv_ferr_007_recovery_epoch_never_regresses() {
        let model = WriteLinModel::new(2);
        let s0 = model.init_states().remove(0);

        // Writer 0 commits epoch 1.
        let s1 = model
            .next_state(&s0, WriteLinAction::AcquireLock(0))
            .unwrap();
        let s2 = model
            .next_state(&s1, WriteLinAction::AssignEpoch(0))
            .unwrap();
        let s3 = model.next_state(&s2, WriteLinAction::FsyncWal(0)).unwrap();
        let s4 = model
            .next_state(&s3, WriteLinAction::ReleaseLock(0))
            .unwrap();

        assert_eq!(s4.committed_epochs, vec![1]);

        // Writer 1 gets epoch 2, fsyncs, then crashes.
        let s5 = model
            .next_state(&s4, WriteLinAction::AcquireLock(1))
            .unwrap();
        let s6 = model
            .next_state(&s5, WriteLinAction::AssignEpoch(1))
            .unwrap();
        let s7 = model.next_state(&s6, WriteLinAction::FsyncWal(1)).unwrap();
        let s8 = model
            .next_state(&s7, WriteLinAction::CrashAfterFsync)
            .unwrap();

        // Recover: should replay epoch 2, maintaining monotonicity.
        let s9 = model.next_state(&s8, WriteLinAction::Recover).unwrap();

        assert_eq!(
            s9.committed_epochs,
            vec![1, 2],
            "INV-FERR-007: recovery must replay epoch 2 after committed epoch 1"
        );
        assert!(
            s9.current_epoch > 2,
            "INV-FERR-007: current_epoch after recovery must exceed all committed epochs"
        );
    }

    // -- Model checker tests ------------------------------------------------

    #[test]
    fn test_inv_ferr_007_model_checker_2_writers() {
        let checker = WriteLinModel::new(2)
            .checker()
            .target_max_depth(12)
            .spawn_bfs()
            .join();

        checker.assert_no_discovery("inv_ferr_007_epoch_strict_monotonicity");
        checker.assert_no_discovery("inv_ferr_007_recovery_epoch_monotonicity");
        checker.assert_any_discovery("inv_ferr_007_at_least_one_commit");
    }

    #[test]
    fn test_inv_ferr_007_model_checker_3_writers() {
        let checker = WriteLinModel::new(3)
            .checker()
            .target_max_depth(14)
            .spawn_bfs()
            .join();

        checker.assert_no_discovery("inv_ferr_007_epoch_strict_monotonicity");
        checker.assert_no_discovery("inv_ferr_007_recovery_epoch_monotonicity");
        checker.assert_any_discovery("inv_ferr_007_at_least_one_commit");
    }

    #[test]
    fn test_inv_ferr_007_model_checker_single_writer_exhaustive() {
        // Single writer: smaller state space, deeper exploration.
        let checker = WriteLinModel::new(1)
            .checker()
            .target_max_depth(20)
            .spawn_bfs()
            .join();

        checker.assert_no_discovery("inv_ferr_007_epoch_strict_monotonicity");
        checker.assert_no_discovery("inv_ferr_007_recovery_epoch_monotonicity");
        checker.assert_any_discovery("inv_ferr_007_at_least_one_commit");
    }
}
