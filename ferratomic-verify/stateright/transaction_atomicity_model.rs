#![forbid(unsafe_code)]

//! Stateright bounded model checker for INV-FERR-020: Transaction Atomicity.
//!
//! Models the all-or-nothing semantics of transaction commit, snapshot
//! visibility, and crash recovery. The state machine explores:
//! - Starting a transaction with a set of datom identifiers
//! - Committing the transaction (assigns epoch, makes durable)
//! - Taking snapshots at various epochs
//! - Crashing before or after commit
//! - Recovery (replays durable WAL entries, discards incomplete ones)
//!
//! Properties verified:
//! 1. Epoch uniformity: all datoms in a committed tx share the same epoch
//! 2. All-or-nothing: at any snapshot, a tx is fully visible or fully invisible
//! 3. Crash atomicity: crash-before-commit discards; crash-after preserves
//! 4. Liveness: a committed-and-snapshotted state is reachable

use std::collections::BTreeSet;

use stateright::{Model, Property};

/// Maximum number of transactions the model explores.
const MAX_TRANSACTIONS: usize = 3;
/// Maximum number of datoms per transaction.
const MAX_DATOMS_PER_TX: usize = 3;
/// Maximum epoch value (bounds the state space).
const MAX_EPOCH: u64 = 3;
/// Maximum number of snapshots to take.
const MAX_SNAPSHOTS: usize = 3;

/// A committed transaction: epoch plus the set of datom identifiers.
///
/// INV-FERR-020: all datoms share the assigned epoch.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct CommittedTx {
    /// The epoch assigned at commit time.
    pub epoch: u64,
    /// The set of datom identifiers in this transaction.
    pub datom_ids: BTreeSet<u64>,
}

/// A snapshot taken at a given epoch, recording which datom ids are visible.
///
/// INV-FERR-006, INV-FERR-020: a snapshot at epoch `e` sees all datoms
/// from transactions with `epoch <= e` and none from `epoch > e`.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct SnapshotView {
    /// The epoch at which this snapshot was taken.
    pub at_epoch: u64,
    /// The datom identifiers visible in this snapshot.
    pub visible_datom_ids: BTreeSet<u64>,
}

/// WAL durability state for a pending transaction.
///
/// INV-FERR-008: a WAL entry is either fully fsynced or not.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum WalState {
    /// Transaction is being built but not yet written to WAL.
    NotWritten,
    /// WAL entry written and fsynced — durable across crashes.
    /// In the current model, commit is atomic (NotWritten -> committed),
    /// so this variant is not constructed. It exists for completeness
    /// and future models that split WAL write from fsync.
    Fsynced,
}

/// A transaction in progress (not yet committed to the store).
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct PendingTx {
    /// The datom identifiers in this pending transaction.
    pub datom_ids: BTreeSet<u64>,
    /// The epoch that will be assigned on commit.
    pub assigned_epoch: u64,
    /// Whether the WAL entry has been durably written.
    pub wal_state: WalState,
}

/// Full state of the transaction atomicity model.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct TxAtomicityState {
    /// Transactions that have been fully committed to the store.
    pub committed_txns: Vec<CommittedTx>,
    /// Snapshots taken at various epochs.
    pub snapshots: Vec<SnapshotView>,
    /// The current epoch counter (monotonically increasing).
    pub current_epoch: u64,
    /// A pending transaction, if one is in progress.
    pub pending_tx: Option<PendingTx>,
    /// Whether the system has crashed and needs recovery.
    pub crashed: bool,
    /// WAL entries that survived the crash (fsynced before crash).
    /// Used during recovery to determine which txns to replay.
    pub durable_wal: Vec<CommittedTx>,
}

/// Actions the model checker can take.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum TxAction {
    /// Begin a new transaction with the given set of datom ids.
    StartTx(BTreeSet<u64>),
    /// Commit the pending transaction: write WAL, fsync, apply to store.
    CommitTx,
    /// Take a snapshot at the current epoch.
    TakeSnapshot,
    /// Crash before the pending tx is committed (WAL not fsynced).
    CrashBeforeCommit,
    /// Crash after the pending tx is committed (WAL fsynced, applied).
    CrashAfterCommit,
    /// Recover from a crash: replay durable WAL, discard incomplete.
    Recover,
}

/// Bounded Stateright model for INV-FERR-020 transaction atomicity.
#[derive(Clone, Debug)]
pub struct TxAtomicityModel {
    /// The finite set of datom id values to explore.
    pub datom_domain: Vec<BTreeSet<u64>>,
}

impl TxAtomicityModel {
    /// Construct the model with pre-computed transaction payloads.
    ///
    /// Generates all non-empty subsets of `{0, 1, 2}` up to
    /// `MAX_DATOMS_PER_TX` elements as candidate transaction payloads.
    pub fn new() -> Self {
        let mut datom_domain = Vec::new();
        // Generate non-empty subsets of {0..MAX_DATOMS_PER_TX-1}
        let base_ids: Vec<u64> = (0..MAX_DATOMS_PER_TX as u64).collect();
        for mask in 1..(1u64 << base_ids.len()) {
            let subset: BTreeSet<u64> = base_ids
                .iter()
                .enumerate()
                .filter(|(i, _)| mask & (1 << i) != 0)
                .map(|(_, &id)| id)
                .collect();
            datom_domain.push(subset);
        }
        Self { datom_domain }
    }

    /// Compute visible datom ids at a given snapshot epoch.
    ///
    /// INV-FERR-020: a datom is visible iff its transaction's epoch <= snapshot epoch.
    fn visible_at_epoch(committed: &[CommittedTx], snap_epoch: u64) -> BTreeSet<u64> {
        committed
            .iter()
            .filter(|tx| tx.epoch <= snap_epoch)
            .flat_map(|tx| tx.datom_ids.iter().copied())
            .collect()
    }
}

impl Default for TxAtomicityModel {
    fn default() -> Self {
        Self::new()
    }
}

/// Start a new transaction with the given datom ids.
fn apply_start_tx(next: &mut TxAtomicityState, datom_ids: BTreeSet<u64>) -> Option<()> {
    if next.pending_tx.is_some() || next.crashed || next.current_epoch > MAX_EPOCH {
        return None;
    }
    next.pending_tx = Some(PendingTx {
        datom_ids,
        assigned_epoch: next.current_epoch,
        wal_state: WalState::NotWritten,
    });
    Some(())
}

/// Commit the pending transaction: write WAL, fsync, apply to store.
fn apply_commit_tx(next: &mut TxAtomicityState) -> Option<()> {
    let pending = next.pending_tx.take()?;
    if next.crashed {
        return None;
    }
    let committed = CommittedTx {
        epoch: pending.assigned_epoch,
        datom_ids: pending.datom_ids,
    };
    next.durable_wal.push(committed.clone());
    next.committed_txns.push(committed);
    next.current_epoch = next
        .current_epoch
        .checked_add(1)
        .unwrap_or(next.current_epoch);
    Some(())
}

/// Take a snapshot at the current epoch.
fn apply_take_snapshot(next: &mut TxAtomicityState) -> Option<()> {
    if next.crashed || next.committed_txns.is_empty() {
        return None;
    }
    let snap_epoch = next.current_epoch.saturating_sub(1);
    let visible = TxAtomicityModel::visible_at_epoch(&next.committed_txns, snap_epoch);
    next.snapshots.push(SnapshotView {
        at_epoch: snap_epoch,
        visible_datom_ids: visible,
    });
    Some(())
}

/// Crash before the pending tx is committed.
fn apply_crash_before_commit(next: &mut TxAtomicityState) -> Option<()> {
    let pending = next.pending_tx.as_ref()?;
    if pending.wal_state != WalState::NotWritten {
        return None;
    }
    next.pending_tx = None;
    apply_crash(next);
    Some(())
}

/// Crash after the pending tx has been committed.
fn apply_crash_after_commit(next: &mut TxAtomicityState) -> Option<()> {
    if next.pending_tx.is_some() || next.crashed {
        return None;
    }
    apply_crash(next);
    Some(())
}

/// INV-FERR-020: Recover from crash by replaying durable WAL entries.
fn apply_tx_recover(next: &mut TxAtomicityState) -> Option<()> {
    if !next.crashed {
        return None;
    }
    next.committed_txns = next.durable_wal.clone();
    next.crashed = false;
    let max_epoch = next
        .durable_wal
        .iter()
        .map(|tx| tx.epoch)
        .max()
        .unwrap_or(0);
    next.current_epoch = max_epoch + 1;
    Some(())
}

/// Apply a crash: clear in-memory state, mark as crashed.
fn apply_crash(next: &mut TxAtomicityState) {
    next.crashed = true;
    next.committed_txns.clear();
    next.snapshots.clear();
}

/// INV-FERR-020: Epoch uniformity -- all committed txns have valid, unique epochs.
fn check_epoch_uniformity(state: &TxAtomicityState) -> bool {
    for tx in &state.committed_txns {
        if tx.epoch == 0 || tx.epoch > MAX_EPOCH {
            return false;
        }
    }
    let epochs: BTreeSet<u64> = state.committed_txns.iter().map(|tx| tx.epoch).collect();
    epochs.len() == state.committed_txns.len()
}

/// INV-FERR-020: All-or-nothing visibility at every snapshot.
fn check_all_or_nothing_visibility(state: &TxAtomicityState) -> bool {
    for snap in &state.snapshots {
        for tx in &state.committed_txns {
            if tx.epoch <= snap.at_epoch {
                for id in &tx.datom_ids {
                    if !snap.visible_datom_ids.contains(id) {
                        return false;
                    }
                }
            }
        }
    }
    true
}

/// INV-FERR-020: Crash atomicity -- committed_txns matches durable_wal.
fn check_crash_atomicity(state: &TxAtomicityState) -> bool {
    if state.crashed {
        return state.committed_txns.is_empty();
    }
    let committed_set: BTreeSet<&CommittedTx> = state.committed_txns.iter().collect();
    let wal_set: BTreeSet<&CommittedTx> = state.durable_wal.iter().collect();
    committed_set == wal_set
}

impl Model for TxAtomicityModel {
    type State = TxAtomicityState;
    type Action = TxAction;

    fn init_states(&self) -> Vec<Self::State> {
        vec![TxAtomicityState {
            committed_txns: Vec::new(),
            snapshots: Vec::new(),
            current_epoch: 1,
            pending_tx: None,
            crashed: false,
            durable_wal: Vec::new(),
        }]
    }

    fn actions(&self, state: &Self::State, actions: &mut Vec<Self::Action>) {
        if state.crashed {
            actions.push(TxAction::Recover);
            return;
        }
        if state.pending_tx.is_none()
            && state.committed_txns.len() < MAX_TRANSACTIONS
            && state.current_epoch <= MAX_EPOCH
        {
            for datom_set in &self.datom_domain {
                actions.push(TxAction::StartTx(datom_set.clone()));
            }
        }
        if state.pending_tx.is_some() {
            actions.push(TxAction::CommitTx);
        }
        if !state.committed_txns.is_empty() && state.snapshots.len() < MAX_SNAPSHOTS {
            actions.push(TxAction::TakeSnapshot);
        }
        if let Some(ref pending) = state.pending_tx {
            if pending.wal_state == WalState::NotWritten {
                actions.push(TxAction::CrashBeforeCommit);
            }
        }
        if state.pending_tx.is_none() && !state.committed_txns.is_empty() {
            actions.push(TxAction::CrashAfterCommit);
        }
    }

    fn next_state(&self, state: &Self::State, action: Self::Action) -> Option<Self::State> {
        let mut next = state.clone();
        match action {
            TxAction::StartTx(datom_ids) => apply_start_tx(&mut next, datom_ids)?,
            TxAction::CommitTx => apply_commit_tx(&mut next)?,
            TxAction::TakeSnapshot => apply_take_snapshot(&mut next)?,
            TxAction::CrashBeforeCommit => apply_crash_before_commit(&mut next)?,
            TxAction::CrashAfterCommit => apply_crash_after_commit(&mut next)?,
            TxAction::Recover => apply_tx_recover(&mut next)?,
        }
        Some(next)
    }

    fn within_boundary(&self, state: &Self::State) -> bool {
        state.committed_txns.len() <= MAX_TRANSACTIONS
            && state.snapshots.len() <= MAX_SNAPSHOTS
            && state.current_epoch <= MAX_EPOCH + 2
            && state.durable_wal.len() <= MAX_TRANSACTIONS
    }

    fn properties(&self) -> Vec<Property<Self>> {
        vec![
            Property::always(
                "inv_ferr_020_epoch_uniformity",
                |_: &TxAtomicityModel, state: &TxAtomicityState| check_epoch_uniformity(state),
            ),
            Property::always(
                "inv_ferr_020_all_or_nothing_visibility",
                |_: &TxAtomicityModel, state: &TxAtomicityState| {
                    check_all_or_nothing_visibility(state)
                },
            ),
            Property::always(
                "inv_ferr_020_crash_atomicity",
                |_: &TxAtomicityModel, state: &TxAtomicityState| check_crash_atomicity(state),
            ),
            Property::sometimes(
                "inv_ferr_020_committed_snapshot_reachable",
                |_: &TxAtomicityModel, state: &TxAtomicityState| {
                    !state.committed_txns.is_empty()
                        && !state.snapshots.is_empty()
                        && state
                            .snapshots
                            .iter()
                            .any(|s| !s.visible_datom_ids.is_empty())
                },
            ),
            Property::sometimes(
                "inv_ferr_020_crash_recovery_reachable",
                |_: &TxAtomicityModel, state: &TxAtomicityState| {
                    !state.crashed
                        && !state.durable_wal.is_empty()
                        && !state.committed_txns.is_empty()
                        && state.committed_txns == state.durable_wal
                },
            ),
        ]
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use stateright::{Checker, Model};

    use super::{TxAction, TxAtomicityModel};

    fn datom_set(ids: &[u64]) -> BTreeSet<u64> {
        ids.iter().copied().collect()
    }

    // -----------------------------------------------------------------------
    // Unit tests: epoch uniformity
    // -----------------------------------------------------------------------

    #[test]
    fn inv_ferr_020_commit_assigns_single_epoch() {
        let model = TxAtomicityModel::new();
        let state = model.init_states().remove(0);

        // Start a tx with datoms {0, 1, 2}.
        let state = model
            .next_state(&state, TxAction::StartTx(datom_set(&[0, 1, 2])))
            .expect("INV-FERR-020: StartTx must succeed on empty state");

        // Commit.
        let state = model
            .next_state(&state, TxAction::CommitTx)
            .expect("INV-FERR-020: CommitTx must succeed with pending tx");

        assert_eq!(
            state.committed_txns.len(),
            1,
            "INV-FERR-020: exactly one tx committed"
        );
        let tx = &state.committed_txns[0];
        assert_eq!(tx.epoch, 1, "INV-FERR-020: first tx gets epoch 1");
        assert_eq!(
            tx.datom_ids,
            datom_set(&[0, 1, 2]),
            "INV-FERR-020: all datoms present in committed tx"
        );
    }

    #[test]
    fn inv_ferr_020_successive_txns_get_distinct_epochs() {
        let model = TxAtomicityModel::new();
        let mut state = model.init_states().remove(0);

        // Commit two transactions.
        for i in 0..2 {
            state = model
                .next_state(&state, TxAction::StartTx(datom_set(&[i])))
                .expect("INV-FERR-020: StartTx must succeed");
            state = model
                .next_state(&state, TxAction::CommitTx)
                .expect("INV-FERR-020: CommitTx must succeed");
        }

        let epochs: Vec<u64> = state.committed_txns.iter().map(|t| t.epoch).collect();
        assert_eq!(
            epochs,
            vec![1, 2],
            "INV-FERR-020: distinct monotonic epochs"
        );
    }

    // -----------------------------------------------------------------------
    // Unit tests: all-or-nothing snapshot visibility
    // -----------------------------------------------------------------------

    #[test]
    fn inv_ferr_020_snapshot_sees_all_datoms_of_committed_tx() {
        let model = TxAtomicityModel::new();
        let mut state = model.init_states().remove(0);

        // Commit tx with {0, 1, 2} at epoch 1.
        state = model
            .next_state(&state, TxAction::StartTx(datom_set(&[0, 1, 2])))
            .expect("INV-FERR-020: StartTx");
        state = model
            .next_state(&state, TxAction::CommitTx)
            .expect("INV-FERR-020: CommitTx");

        // Take snapshot (at epoch 1, which is current_epoch - 1 = 2 - 1 = 1).
        state = model
            .next_state(&state, TxAction::TakeSnapshot)
            .expect("INV-FERR-020: TakeSnapshot");

        let snap = &state.snapshots[0];
        assert_eq!(
            snap.at_epoch, 1,
            "INV-FERR-020: snapshot at committed epoch"
        );
        assert_eq!(
            snap.visible_datom_ids,
            datom_set(&[0, 1, 2]),
            "INV-FERR-020: all datoms from tx visible in snapshot"
        );
    }

    #[test]
    fn inv_ferr_020_snapshot_sees_none_before_commit_epoch() {
        let model = TxAtomicityModel::new();
        let mut state = model.init_states().remove(0);

        // Commit first tx at epoch 1 (to have something snapshotable).
        state = model
            .next_state(&state, TxAction::StartTx(datom_set(&[0])))
            .expect("INV-FERR-020: StartTx");
        state = model
            .next_state(&state, TxAction::CommitTx)
            .expect("INV-FERR-020: CommitTx");

        // Take snapshot at epoch 1 (current_epoch=2, snap at 1).
        state = model
            .next_state(&state, TxAction::TakeSnapshot)
            .expect("INV-FERR-020: TakeSnapshot");

        // Commit second tx at epoch 2.
        state = model
            .next_state(&state, TxAction::StartTx(datom_set(&[1, 2])))
            .expect("INV-FERR-020: StartTx");
        state = model
            .next_state(&state, TxAction::CommitTx)
            .expect("INV-FERR-020: CommitTx");

        // The snapshot taken at epoch 1 must NOT see tx at epoch 2.
        let snap = &state.snapshots[0];
        assert!(
            !snap.visible_datom_ids.contains(&1),
            "INV-FERR-020: datom 1 from later tx must not be visible in earlier snapshot"
        );
        assert!(
            !snap.visible_datom_ids.contains(&2),
            "INV-FERR-020: datom 2 from later tx must not be visible in earlier snapshot"
        );
    }

    // -----------------------------------------------------------------------
    // Unit tests: crash atomicity
    // -----------------------------------------------------------------------

    #[test]
    fn inv_ferr_020_crash_before_commit_discards_pending() {
        let model = TxAtomicityModel::new();
        let mut state = model.init_states().remove(0);

        // Start a tx but don't commit.
        state = model
            .next_state(&state, TxAction::StartTx(datom_set(&[0, 1])))
            .expect("INV-FERR-020: StartTx");

        // Crash before commit.
        state = model
            .next_state(&state, TxAction::CrashBeforeCommit)
            .expect("INV-FERR-020: CrashBeforeCommit must succeed");

        assert!(state.crashed, "INV-FERR-020: system is crashed");
        assert!(
            state.committed_txns.is_empty(),
            "INV-FERR-020: no committed txns after crash-before-commit"
        );
        assert!(
            state.pending_tx.is_none(),
            "INV-FERR-020: pending tx discarded after crash"
        );

        // Recover.
        state = model
            .next_state(&state, TxAction::Recover)
            .expect("INV-FERR-020: Recover must succeed");

        assert!(!state.crashed, "INV-FERR-020: system recovered");
        assert!(
            state.committed_txns.is_empty(),
            "INV-FERR-020: no txns restored (none were durable)"
        );
    }

    #[test]
    fn inv_ferr_020_crash_after_commit_preserves_tx() {
        let model = TxAtomicityModel::new();
        let mut state = model.init_states().remove(0);

        // Commit a tx.
        state = model
            .next_state(&state, TxAction::StartTx(datom_set(&[0, 1])))
            .expect("INV-FERR-020: StartTx");
        state = model
            .next_state(&state, TxAction::CommitTx)
            .expect("INV-FERR-020: CommitTx");

        let committed_before = state.committed_txns.clone();

        // Crash after commit.
        state = model
            .next_state(&state, TxAction::CrashAfterCommit)
            .expect("INV-FERR-020: CrashAfterCommit must succeed");

        assert!(state.crashed, "INV-FERR-020: system is crashed");
        assert!(
            state.committed_txns.is_empty(),
            "INV-FERR-020: in-memory state lost after crash"
        );
        assert_eq!(
            state.durable_wal.len(),
            1,
            "INV-FERR-020: durable WAL has the committed tx"
        );

        // Recover.
        state = model
            .next_state(&state, TxAction::Recover)
            .expect("INV-FERR-020: Recover must succeed");

        assert!(!state.crashed, "INV-FERR-020: system recovered");
        assert_eq!(
            state.committed_txns, committed_before,
            "INV-FERR-020: committed tx restored from WAL"
        );
    }

    #[test]
    fn inv_ferr_020_crash_after_two_commits_preserves_both() {
        let model = TxAtomicityModel::new();
        let mut state = model.init_states().remove(0);

        // Commit two transactions.
        state = model
            .next_state(&state, TxAction::StartTx(datom_set(&[0])))
            .expect("INV-FERR-020: StartTx");
        state = model
            .next_state(&state, TxAction::CommitTx)
            .expect("INV-FERR-020: CommitTx");
        state = model
            .next_state(&state, TxAction::StartTx(datom_set(&[1, 2])))
            .expect("INV-FERR-020: StartTx");
        state = model
            .next_state(&state, TxAction::CommitTx)
            .expect("INV-FERR-020: CommitTx");

        let committed_before = state.committed_txns.clone();

        // Crash and recover.
        state = model
            .next_state(&state, TxAction::CrashAfterCommit)
            .expect("INV-FERR-020: CrashAfterCommit");
        state = model
            .next_state(&state, TxAction::Recover)
            .expect("INV-FERR-020: Recover");

        assert_eq!(
            state.committed_txns, committed_before,
            "INV-FERR-020: both committed txns restored from WAL"
        );
    }

    // -----------------------------------------------------------------------
    // Unit tests: no-op and guard transitions
    // -----------------------------------------------------------------------

    #[test]
    fn inv_ferr_020_cannot_start_tx_while_pending() {
        let model = TxAtomicityModel::new();
        let mut state = model.init_states().remove(0);

        state = model
            .next_state(&state, TxAction::StartTx(datom_set(&[0])))
            .expect("INV-FERR-020: first StartTx");

        let result = model.next_state(&state, TxAction::StartTx(datom_set(&[1])));
        assert!(
            result.is_none(),
            "INV-FERR-020: cannot start second tx while first is pending"
        );
    }

    #[test]
    fn inv_ferr_020_cannot_commit_without_pending() {
        let model = TxAtomicityModel::new();
        let state = model.init_states().remove(0);

        let result = model.next_state(&state, TxAction::CommitTx);
        assert!(
            result.is_none(),
            "INV-FERR-020: CommitTx requires a pending tx"
        );
    }

    #[test]
    fn inv_ferr_020_cannot_recover_when_not_crashed() {
        let model = TxAtomicityModel::new();
        let state = model.init_states().remove(0);

        let result = model.next_state(&state, TxAction::Recover);
        assert!(
            result.is_none(),
            "INV-FERR-020: Recover requires crashed state"
        );
    }

    // -----------------------------------------------------------------------
    // Model checker: exhaustive bounded verification
    // -----------------------------------------------------------------------

    #[test]
    fn inv_ferr_020_model_checker_no_safety_violations() {
        let checker = TxAtomicityModel::new()
            .checker()
            .target_max_depth(10)
            .spawn_bfs()
            .join();

        checker.assert_no_discovery("inv_ferr_020_epoch_uniformity");
        checker.assert_no_discovery("inv_ferr_020_all_or_nothing_visibility");
        checker.assert_no_discovery("inv_ferr_020_crash_atomicity");
    }

    #[test]
    fn inv_ferr_020_model_checker_liveness() {
        let checker = TxAtomicityModel::new()
            .checker()
            .target_max_depth(10)
            .spawn_bfs()
            .join();

        checker.assert_any_discovery("inv_ferr_020_committed_snapshot_reachable");
        checker.assert_any_discovery("inv_ferr_020_crash_recovery_reachable");
    }
}
