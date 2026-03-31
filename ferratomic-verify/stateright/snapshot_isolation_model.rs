//! Stateright snapshot isolation model for ferratomic verification.
use std::collections::{BTreeMap, BTreeSet};

use stateright::{Model, Property};

/// Maximum number of epochs the model explores.
const MAX_EPOCHS: u8 = 3;

/// Maximum number of distinct datom identifiers in the model domain.
const MAX_DATOMS: u8 = 3;

/// Maximum number of reader snapshots that can be outstanding.
const MAX_READERS: usize = 2;

/// Maximum number of write transactions the model explores.
const MAX_WRITES: usize = 2;

/// INV-FERR-006: Snapshot isolation state machine.
///
/// Models a single-node store with epoch-based snapshot isolation.
/// Readers capture a snapshot at the current epoch and see exactly
/// the datoms committed at or before that epoch. Writers advance
/// the epoch upon commit. The safety property: no reader snapshot
/// contains datoms from an epoch higher than its captured epoch.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct SnapshotIsolationState {
    /// Monotonically increasing epoch counter for the store.
    pub current_epoch: u8,
    /// Datoms committed at each epoch. Key = epoch, value = set of datom ids.
    pub datoms_by_epoch: BTreeMap<u8, BTreeSet<u8>>,
    /// Outstanding reader snapshots: `(captured_epoch, visible_datoms)`.
    /// `visible_datoms` is the union of all datoms at epochs <= captured_epoch,
    /// frozen at the time the snapshot was taken.
    pub reader_snapshots: Vec<(u8, BTreeSet<u8>)>,
    /// Pending write transaction: datom ids being written but not yet committed.
    pub pending_write: Option<Vec<u8>>,
    /// Whether a write has been committed (used to track write count).
    pub total_writes: u8,
    /// Whether at least one snapshot has been verified (for liveness).
    pub snapshot_verified: bool,
}

/// Actions available to the Stateright checker for INV-FERR-006.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum SnapshotAction {
    /// A reader captures a snapshot at the current epoch.
    StartRead,
    /// Begin a write transaction with specific datom ids.
    StartWrite(Vec<u8>),
    /// Commit the pending write: advance epoch, make datoms visible.
    CommitWrite,
    /// Verify a specific reader's snapshot against INV-FERR-006.
    VerifySnapshot(usize),
}

/// Bounded Stateright model for INV-FERR-006: Snapshot Isolation.
///
/// Explores interleavings of readers capturing snapshots and writers
/// committing transactions. Verifies that no reader ever sees datoms
/// from a future epoch.
#[derive(Clone, Debug)]
pub struct SnapshotIsolationModel {
    /// Maximum epochs the model will explore.
    pub max_epochs: u8,
    /// Maximum datom ids in the domain.
    pub max_datoms: u8,
    /// Maximum outstanding reader snapshots.
    pub max_readers: usize,
    /// Maximum write transactions.
    pub max_writes: usize,
}

impl SnapshotIsolationModel {
    /// Constructs a bounded snapshot isolation model.
    pub const fn new(
        max_epochs: u8,
        max_datoms: u8,
        max_readers: usize,
        max_writes: usize,
    ) -> Self {
        Self {
            max_epochs,
            max_datoms,
            max_readers,
            max_writes,
        }
    }

    /// Computes the set of all datoms visible at a given epoch.
    ///
    /// INV-FERR-006: snapshot(S, e) = union {T.datoms | T committed at epoch <= e}
    pub fn visible_datoms_at_epoch(
        datoms_by_epoch: &BTreeMap<u8, BTreeSet<u8>>,
        epoch: u8,
    ) -> BTreeSet<u8> {
        let mut result = BTreeSet::new();
        for (&committed_epoch, datom_ids) in datoms_by_epoch {
            if committed_epoch <= epoch {
                for &id in datom_ids {
                    result.insert(id);
                }
            }
        }
        result
    }

    /// Checks whether a reader snapshot satisfies INV-FERR-006.
    ///
    /// A snapshot at epoch `e` must contain no datoms from any epoch `e' > e`.
    pub fn snapshot_satisfies_isolation(
        datoms_by_epoch: &BTreeMap<u8, BTreeSet<u8>>,
        captured_epoch: u8,
        visible_datoms: &BTreeSet<u8>,
    ) -> bool {
        // Collect all datoms committed strictly after the captured epoch.
        let mut future_datoms = BTreeSet::new();
        for (&committed_epoch, datom_ids) in datoms_by_epoch {
            if committed_epoch > captured_epoch {
                for &id in datom_ids {
                    future_datoms.insert(id);
                }
            }
        }
        // The snapshot must not contain any future datoms.
        // Note: a datom id could appear in both past and future epochs
        // (e.g., same datom id written at epoch 1 and epoch 3). We only
        // flag a violation if the id is EXCLUSIVELY from future epochs.
        for &datom_id in visible_datoms {
            let in_past = datoms_by_epoch.iter().any(|(&epoch, ids)| {
                epoch <= captured_epoch && ids.contains(&datom_id)
            });
            if !in_past && future_datoms.contains(&datom_id) {
                return false;
            }
        }
        true
    }

    /// Generates all non-empty subsets of datom ids within the domain.
    fn datom_subsets(&self) -> Vec<Vec<u8>> {
        let mut subsets = Vec::new();
        let domain_size = self.max_datoms;
        // Enumerate all non-empty subsets using bitmask.
        for mask in 1u16..(1u16 << domain_size) {
            let mut subset = Vec::new();
            for bit in 0..domain_size {
                if mask & (1 << bit) != 0 {
                    subset.push(bit);
                }
            }
            subsets.push(subset);
        }
        subsets
    }
}

impl Default for SnapshotIsolationModel {
    fn default() -> Self {
        Self::new(MAX_EPOCHS, MAX_DATOMS, MAX_READERS, MAX_WRITES)
    }
}

impl Model for SnapshotIsolationModel {
    type State = SnapshotIsolationState;
    type Action = SnapshotAction;

    fn init_states(&self) -> Vec<Self::State> {
        vec![SnapshotIsolationState {
            current_epoch: 0,
            datoms_by_epoch: BTreeMap::new(),
            reader_snapshots: Vec::new(),
            pending_write: None,
            total_writes: 0,
            snapshot_verified: false,
        }]
    }

    fn actions(&self, state: &Self::State, actions: &mut Vec<Self::Action>) {
        // StartRead: a reader captures a snapshot if we haven't hit max readers.
        if state.reader_snapshots.len() < self.max_readers {
            actions.push(SnapshotAction::StartRead);
        }

        // StartWrite: begin a write transaction if none is pending and
        // we haven't exceeded the write budget.
        if state.pending_write.is_none()
            && (state.total_writes as usize) < self.max_writes
            && state.current_epoch < self.max_epochs
        {
            for subset in self.datom_subsets() {
                actions.push(SnapshotAction::StartWrite(subset));
            }
        }

        // CommitWrite: commit the pending write transaction.
        if state.pending_write.is_some() {
            actions.push(SnapshotAction::CommitWrite);
        }

        // VerifySnapshot: verify any outstanding reader snapshot.
        for idx in 0..state.reader_snapshots.len() {
            actions.push(SnapshotAction::VerifySnapshot(idx));
        }
    }

    fn next_state(&self, state: &Self::State, action: Self::Action) -> Option<Self::State> {
        let mut next = state.clone();
        match action {
            SnapshotAction::StartRead => {
                if next.reader_snapshots.len() >= self.max_readers {
                    return None;
                }
                // INV-FERR-006: reader captures the current epoch and the
                // datoms visible at that epoch. This snapshot is immutable.
                let epoch = next.current_epoch;
                let visible = Self::visible_datoms_at_epoch(&next.datoms_by_epoch, epoch);
                next.reader_snapshots.push((epoch, visible));
            }
            SnapshotAction::StartWrite(datom_ids) => {
                if next.pending_write.is_some() {
                    return None;
                }
                if (next.total_writes as usize) >= self.max_writes {
                    return None;
                }
                if next.current_epoch >= self.max_epochs {
                    return None;
                }
                next.pending_write = Some(datom_ids);
            }
            SnapshotAction::CommitWrite => {
                let datom_ids = next.pending_write.take()?;
                // INV-FERR-007: writes are linearizable. Advance epoch.
                next.current_epoch = next.current_epoch.checked_add(1)?;
                let epoch = next.current_epoch;
                let datom_set: BTreeSet<u8> = datom_ids.into_iter().collect();
                next.datoms_by_epoch.insert(epoch, datom_set);
                next.total_writes = next.total_writes.checked_add(1)?;
            }
            SnapshotAction::VerifySnapshot(idx) => {
                if idx >= next.reader_snapshots.len() {
                    return None;
                }
                // Mark that at least one snapshot has been verified.
                next.snapshot_verified = true;
            }
        }
        Some(next)
    }

    fn within_boundary(&self, state: &Self::State) -> bool {
        state.current_epoch <= self.max_epochs
            && state.reader_snapshots.len() <= self.max_readers
            && (state.total_writes as usize) <= self.max_writes
            && state.datoms_by_epoch.values().all(|ids| {
                ids.iter().all(|&id| id < self.max_datoms)
            })
    }

    fn properties(&self) -> Vec<Property<Self>> {
        vec![
            // INV-FERR-006 Safety: No reader snapshot contains datoms from
            // an epoch higher than its captured epoch. This must hold in
            // EVERY reachable state (Property::always).
            //
            // The snapshot was frozen at capture time, so even after new
            // writes commit, the snapshot's visible_datoms set must not
            // contain any datom id that is exclusively from a future epoch.
            Property::always(
                "inv_ferr_006_snapshot_isolation_safety",
                |_: &SnapshotIsolationModel, state: &SnapshotIsolationState| {
                    // INV-FERR-006: for every outstanding reader snapshot,
                    // verify it contains no datoms from epochs after capture.
                    for (captured_epoch, visible_datoms) in &state.reader_snapshots {
                        if !SnapshotIsolationModel::snapshot_satisfies_isolation(
                            &state.datoms_by_epoch,
                            *captured_epoch,
                            visible_datoms,
                        ) {
                            return false;
                        }
                    }
                    true
                },
            ),
            // INV-FERR-006 Snapshot consistency: each snapshot equals exactly
            // the union of all datoms committed at or before the captured epoch.
            // This checks that the snapshot captured the CORRECT set, not just
            // that it excluded future datoms.
            Property::always(
                "inv_ferr_006_snapshot_completeness",
                |_: &SnapshotIsolationModel, state: &SnapshotIsolationState| {
                    // INV-FERR-006: snapshot(S, e) = union {T.datoms | T committed at epoch <= e}
                    for (captured_epoch, visible_datoms) in &state.reader_snapshots {
                        let expected = SnapshotIsolationModel::visible_datoms_at_epoch(
                            &state.datoms_by_epoch,
                            *captured_epoch,
                        );
                        if *visible_datoms != expected {
                            return false;
                        }
                    }
                    true
                },
            ),
            // INV-FERR-006 Liveness: at least one snapshot is created and
            // verified. This ensures the model explores meaningful states
            // rather than vacuously satisfying safety on empty runs.
            Property::sometimes(
                "inv_ferr_006_snapshot_verified_reachable",
                |_: &SnapshotIsolationModel, state: &SnapshotIsolationState| {
                    // INV-FERR-006: a state where a snapshot has been taken,
                    // a write committed, and the snapshot verified is reachable.
                    state.snapshot_verified
                        && !state.reader_snapshots.is_empty()
                        && state.total_writes > 0
                },
            ),
        ]
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};

    use stateright::{Checker, Model};

    use super::{SnapshotAction, SnapshotIsolationModel};

    /// Helper: construct a `BTreeSet<u8>` from a slice.
    fn id_set(ids: &[u8]) -> BTreeSet<u8> {
        ids.iter().copied().collect()
    }

    /// Helper: construct a `datoms_by_epoch` map from `(epoch, &[datom_ids])` pairs.
    fn epoch_map(entries: &[(u8, &[u8])]) -> BTreeMap<u8, BTreeSet<u8>> {
        entries
            .iter()
            .map(|&(epoch, ids)| (epoch, id_set(ids)))
            .collect()
    }

    // -----------------------------------------------------------------------
    // Unit tests for helper functions
    // -----------------------------------------------------------------------

    #[test]
    fn test_inv_ferr_006_visible_datoms_at_epoch_empty_store() {
        let datoms = BTreeMap::new();
        let visible = SnapshotIsolationModel::visible_datoms_at_epoch(&datoms, 0);
        assert!(
            visible.is_empty(),
            "INV-FERR-006: an empty store has no visible datoms at any epoch"
        );
    }

    #[test]
    fn test_inv_ferr_006_visible_datoms_at_epoch_single_commit() {
        let datoms = epoch_map(&[(1, &[0, 1])]);
        // At epoch 0 (before the commit), nothing is visible.
        let at_0 = SnapshotIsolationModel::visible_datoms_at_epoch(&datoms, 0);
        assert!(
            at_0.is_empty(),
            "INV-FERR-006: datoms committed at epoch 1 must not be visible at epoch 0"
        );
        // At epoch 1 (the commit epoch), the datoms are visible.
        let at_1 = SnapshotIsolationModel::visible_datoms_at_epoch(&datoms, 1);
        assert_eq!(
            at_1,
            id_set(&[0, 1]),
            "INV-FERR-006: datoms committed at epoch 1 must be visible at epoch 1"
        );
        // At epoch 2 (after the commit), still visible.
        let at_2 = SnapshotIsolationModel::visible_datoms_at_epoch(&datoms, 2);
        assert_eq!(
            at_2,
            id_set(&[0, 1]),
            "INV-FERR-006: datoms committed at epoch 1 must remain visible at epoch 2"
        );
    }

    #[test]
    fn test_inv_ferr_006_visible_datoms_at_epoch_multiple_commits() {
        let datoms = epoch_map(&[(1, &[0]), (2, &[1]), (3, &[2])]);
        let at_2 = SnapshotIsolationModel::visible_datoms_at_epoch(&datoms, 2);
        assert_eq!(
            at_2,
            id_set(&[0, 1]),
            "INV-FERR-006: snapshot at epoch 2 sees datoms from epochs 1 and 2, not epoch 3"
        );
    }

    #[test]
    fn test_inv_ferr_006_snapshot_satisfies_isolation_valid() {
        let datoms = epoch_map(&[(1, &[0]), (2, &[1])]);
        let visible = id_set(&[0]); // snapshot at epoch 1
        assert!(
            SnapshotIsolationModel::snapshot_satisfies_isolation(&datoms, 1, &visible),
            "INV-FERR-006: snapshot at epoch 1 with datom 0 satisfies isolation"
        );
    }

    #[test]
    fn test_inv_ferr_006_snapshot_satisfies_isolation_violation() {
        let datoms = epoch_map(&[(1, &[0]), (2, &[1])]);
        // A snapshot at epoch 1 that contains datom 1 (from epoch 2) is a violation.
        let visible = id_set(&[0, 1]);
        assert!(
            !SnapshotIsolationModel::snapshot_satisfies_isolation(&datoms, 1, &visible),
            "INV-FERR-006: snapshot at epoch 1 must not contain datom 1 from epoch 2"
        );
    }

    #[test]
    fn test_inv_ferr_006_snapshot_completeness_check() {
        let datoms = epoch_map(&[(1, &[0, 1]), (2, &[2])]);
        let expected_at_1 = SnapshotIsolationModel::visible_datoms_at_epoch(&datoms, 1);
        assert_eq!(
            expected_at_1,
            id_set(&[0, 1]),
            "INV-FERR-006: snapshot at epoch 1 must contain exactly datoms 0 and 1"
        );
        let expected_at_2 = SnapshotIsolationModel::visible_datoms_at_epoch(&datoms, 2);
        assert_eq!(
            expected_at_2,
            id_set(&[0, 1, 2]),
            "INV-FERR-006: snapshot at epoch 2 must contain datoms 0, 1, and 2"
        );
    }

    // -----------------------------------------------------------------------
    // State machine transition tests
    // -----------------------------------------------------------------------

    #[test]
    fn test_inv_ferr_006_start_read_captures_current_epoch() {
        let model = SnapshotIsolationModel::default();
        let state0 = model.init_states().remove(0);
        // Write and commit a transaction first.
        let state1 = model
            .next_state(&state0, SnapshotAction::StartWrite(vec![0]))
            .expect("INV-FERR-006: starting a write must succeed from genesis");
        let state2 = model
            .next_state(&state1, SnapshotAction::CommitWrite)
            .expect("INV-FERR-006: committing a pending write must succeed");
        assert_eq!(
            state2.current_epoch, 1,
            "INV-FERR-006: epoch must advance to 1 after first commit"
        );
        // Now take a snapshot.
        let state3 = model
            .next_state(&state2, SnapshotAction::StartRead)
            .expect("INV-FERR-006: starting a read must succeed");
        assert_eq!(
            state3.reader_snapshots.len(),
            1,
            "INV-FERR-006: one reader snapshot must be outstanding"
        );
        let (epoch, ref datoms) = state3.reader_snapshots[0];
        assert_eq!(
            epoch, 1,
            "INV-FERR-006: captured epoch must be the current epoch"
        );
        assert_eq!(
            *datoms,
            id_set(&[0]),
            "INV-FERR-006: snapshot must contain datom 0 from epoch 1"
        );
    }

    #[test]
    fn test_inv_ferr_006_commit_write_advances_epoch() {
        let model = SnapshotIsolationModel::default();
        let state0 = model.init_states().remove(0);
        let state1 = model
            .next_state(&state0, SnapshotAction::StartWrite(vec![0, 1]))
            .expect("INV-FERR-006: starting a write must succeed");
        let state2 = model
            .next_state(&state1, SnapshotAction::CommitWrite)
            .expect("INV-FERR-006: commit must succeed");
        assert_eq!(
            state2.current_epoch, 1,
            "INV-FERR-006: epoch must advance to 1"
        );
        assert_eq!(
            state2.datoms_by_epoch.get(&1),
            Some(&id_set(&[0, 1])),
            "INV-FERR-006: datoms 0 and 1 must be recorded at epoch 1"
        );
    }

    #[test]
    fn test_inv_ferr_006_snapshot_not_affected_by_later_write() {
        let model = SnapshotIsolationModel::default();
        let state0 = model.init_states().remove(0);
        // Take a snapshot at epoch 0 (no datoms yet).
        let state1 = model
            .next_state(&state0, SnapshotAction::StartRead)
            .expect("INV-FERR-006: reading empty store must succeed");
        // Now write and commit.
        let state2 = model
            .next_state(&state1, SnapshotAction::StartWrite(vec![0]))
            .expect("INV-FERR-006: starting a write must succeed");
        let state3 = model
            .next_state(&state2, SnapshotAction::CommitWrite)
            .expect("INV-FERR-006: committing must succeed");
        // The snapshot taken at epoch 0 must still see no datoms.
        let (epoch, ref visible) = state3.reader_snapshots[0];
        assert_eq!(epoch, 0, "INV-FERR-006: captured epoch must be 0");
        assert!(
            visible.is_empty(),
            "INV-FERR-006: snapshot at epoch 0 must not see datoms committed at epoch 1"
        );
        // Verify isolation holds.
        assert!(
            SnapshotIsolationModel::snapshot_satisfies_isolation(
                &state3.datoms_by_epoch,
                epoch,
                visible,
            ),
            "INV-FERR-006: snapshot isolation must hold for the epoch-0 snapshot"
        );
    }

    #[test]
    fn test_inv_ferr_006_double_commit_no_pending_returns_none() {
        let model = SnapshotIsolationModel::default();
        let state0 = model.init_states().remove(0);
        let result = model.next_state(&state0, SnapshotAction::CommitWrite);
        assert!(
            result.is_none(),
            "INV-FERR-006: committing without a pending write must return None"
        );
    }

    #[test]
    fn test_inv_ferr_006_verify_snapshot_marks_verified() {
        let model = SnapshotIsolationModel::default();
        let state0 = model.init_states().remove(0);
        let state1 = model
            .next_state(&state0, SnapshotAction::StartRead)
            .expect("INV-FERR-006: reading must succeed");
        assert!(
            !state1.snapshot_verified,
            "INV-FERR-006: snapshot_verified must be false before verification"
        );
        let state2 = model
            .next_state(&state1, SnapshotAction::VerifySnapshot(0))
            .expect("INV-FERR-006: verifying an existing snapshot must succeed");
        assert!(
            state2.snapshot_verified,
            "INV-FERR-006: snapshot_verified must be true after verification"
        );
    }

    #[test]
    fn test_inv_ferr_006_verify_out_of_bounds_returns_none() {
        let model = SnapshotIsolationModel::default();
        let state0 = model.init_states().remove(0);
        let result = model.next_state(&state0, SnapshotAction::VerifySnapshot(0));
        assert!(
            result.is_none(),
            "INV-FERR-006: verifying a non-existent snapshot must return None"
        );
    }

    // -----------------------------------------------------------------------
    // Checker tests — exhaustive model checking
    // -----------------------------------------------------------------------

    #[test]
    fn test_snapshot_isolation_safety() {
        // INV-FERR-006: exhaustively check that no reachable state
        // violates snapshot isolation. The checker explores all
        // interleavings of reads, writes, commits, and verifications
        // within the bounded domain.
        let checker = SnapshotIsolationModel::default()
            .checker()
            .spawn_bfs()
            .join();

        // Safety: no snapshot ever contains future-epoch datoms.
        checker.assert_no_discovery("inv_ferr_006_snapshot_isolation_safety");
        // Completeness: every snapshot contains exactly the right datoms.
        checker.assert_no_discovery("inv_ferr_006_snapshot_completeness");
    }

    #[test]
    fn test_snapshot_isolation_liveness() {
        // INV-FERR-006: verify that a state where a snapshot has been
        // taken, a write committed, and the snapshot verified is reachable.
        // This ensures the model is non-vacuous.
        let checker = SnapshotIsolationModel::default()
            .checker()
            .spawn_bfs()
            .join();

        checker.assert_any_discovery("inv_ferr_006_snapshot_verified_reachable");
    }

    #[test]
    fn test_snapshot_isolation_safety_minimal() {
        // INV-FERR-006: minimal configuration (1 epoch, 1 datom, 1 reader,
        // 1 write) as a sanity check that the model works at small scale.
        let checker = SnapshotIsolationModel::new(1, 1, 1, 1)
            .checker()
            .spawn_bfs()
            .join();

        checker.assert_no_discovery("inv_ferr_006_snapshot_isolation_safety");
        checker.assert_no_discovery("inv_ferr_006_snapshot_completeness");
        checker.assert_any_discovery("inv_ferr_006_snapshot_verified_reachable");
    }

    #[test]
    fn test_snapshot_isolation_multiple_readers_see_different_epochs() {
        // INV-FERR-006: two readers at different epochs see different
        // datom sets. Reader at epoch 1 sees {0}, reader at epoch 2 sees {0, 1}.
        let model = SnapshotIsolationModel::default();
        let state0 = model.init_states().remove(0);

        // First write + commit: datom 0 at epoch 1.
        let s1 = model
            .next_state(&state0, SnapshotAction::StartWrite(vec![0]))
            .unwrap();
        let s2 = model
            .next_state(&s1, SnapshotAction::CommitWrite)
            .unwrap();
        // Reader 1 captures at epoch 1.
        let s3 = model
            .next_state(&s2, SnapshotAction::StartRead)
            .unwrap();

        // Second write + commit: datom 1 at epoch 2.
        let s4 = model
            .next_state(&s3, SnapshotAction::StartWrite(vec![1]))
            .unwrap();
        let s5 = model
            .next_state(&s4, SnapshotAction::CommitWrite)
            .unwrap();
        // Reader 2 captures at epoch 2.
        let s6 = model
            .next_state(&s5, SnapshotAction::StartRead)
            .unwrap();

        let (epoch_r1, ref datoms_r1) = s6.reader_snapshots[0];
        let (epoch_r2, ref datoms_r2) = s6.reader_snapshots[1];

        assert_eq!(epoch_r1, 1, "INV-FERR-006: first reader captured at epoch 1");
        assert_eq!(
            *datoms_r1,
            id_set(&[0]),
            "INV-FERR-006: first reader sees only datom 0"
        );
        assert_eq!(epoch_r2, 2, "INV-FERR-006: second reader captured at epoch 2");
        assert_eq!(
            *datoms_r2,
            id_set(&[0, 1]),
            "INV-FERR-006: second reader sees datoms 0 and 1"
        );
    }
}
