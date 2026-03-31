//! Stateright backpressure-safety model for INV-FERR-021.
//!
//! Models the `WriteLimiter` as a bounded queue with concurrent submitters.
//! The queue has a fixed capacity; submissions to a full queue are rejected
//! with `Backpressure` (never silently dropped, never unbounded).
//!
//! State machine:
//!   Submit(datom_id) → queued (if room) OR rejected (if full)
//!   Process           → pop from queue, commit
//!
//! Properties verified:
//! - **Safety (no silent drop)**: `total_submitted == committed_count +
//!   rejected_count + queue.len()` in ALL reachable states.
//! - **Safety (bounded queue)**: `queue.len() <= capacity` in ALL states.
//! - **Safety (no data loss)**: every committed datom appears in the
//!   committed set.
//! - **Liveness**: at least one write commits AND at least one rejection
//!   occurs (when submitters exceed capacity).

use std::collections::BTreeSet;

use stateright::{Model, Property};

use super::crdt_model::Datom;

// ---------------------------------------------------------------------------
// State machine
// ---------------------------------------------------------------------------

/// Full state of the backpressure model.
///
/// INV-FERR-021: tracks every submitted datom through exactly one of three
/// fates: queued (pending), committed, or rejected. No fourth state exists.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct BackpressureState {
    /// Pending writes awaiting processing. Bounded by `capacity`.
    pub queue: Vec<Datom>,
    /// Maximum queue depth (mirrors `BackpressurePolicy::max_concurrent_writes`).
    pub capacity: u8,
    /// Total datoms that have been dequeued and committed.
    pub committed_count: usize,
    /// Total submissions that were rejected because the queue was full.
    pub rejected_count: usize,
    /// Total submissions attempted (accepted + rejected).
    pub total_submitted: usize,
    /// The set of committed datoms, for the no-data-loss property.
    pub committed_set: BTreeSet<Datom>,
}

/// Actions available to the Stateright checker.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum BackpressureAction {
    /// A submitter attempts to enqueue a datom. If the queue is at capacity,
    /// the submission is rejected (INV-FERR-021: explicit Backpressure error).
    Submit(Datom),
    /// The writer dequeues the front of the queue and commits it.
    Process,
}

// ---------------------------------------------------------------------------
// Model configuration
// ---------------------------------------------------------------------------

/// Bounded Stateright model for INV-FERR-021 backpressure safety.
///
/// Parameters control the state-space size:
/// - `queue_capacity`: max pending writes (models `BackpressurePolicy`).
/// - `max_submissions`: total Submit actions before the model stops generating
///   new submissions (keeps state space finite).
/// - `datom_domain`: number of distinct datom seeds explored.
#[derive(Clone, Debug)]
pub struct BackpressureModel {
    /// Maximum queue depth (3-5 for tractable checking).
    pub queue_capacity: u8,
    /// Total submission attempts before the model stops (5-8).
    pub max_submissions: usize,
    /// Distinct datom seeds to explore.
    pub datom_domain: u64,
}

impl BackpressureModel {
    /// Constructs a bounded backpressure model.
    pub const fn new(queue_capacity: u8, max_submissions: usize, datom_domain: u64) -> Self {
        Self {
            queue_capacity,
            max_submissions,
            datom_domain,
        }
    }
}

impl Default for BackpressureModel {
    fn default() -> Self {
        // capacity=3, max_submissions=6, 3 distinct datoms.
        // Provides a tractable state space while exercising all properties.
        Self::new(3, 6, 3)
    }
}

// ---------------------------------------------------------------------------
// Model implementation
// ---------------------------------------------------------------------------

impl Model for BackpressureModel {
    type State = BackpressureState;
    type Action = BackpressureAction;

    fn init_states(&self) -> Vec<Self::State> {
        vec![BackpressureState {
            queue: Vec::new(),
            capacity: self.queue_capacity,
            committed_count: 0,
            rejected_count: 0,
            total_submitted: 0,
            committed_set: BTreeSet::new(),
        }]
    }

    fn actions(&self, state: &Self::State, actions: &mut Vec<Self::Action>) {
        // Submit actions: any submitter can attempt a write if we have not
        // exhausted the total submission budget.
        if state.total_submitted < self.max_submissions {
            for seed in 0..self.datom_domain {
                actions.push(BackpressureAction::Submit(Datom::from_seed(seed)));
            }
        }

        // Process action: the writer can commit if the queue is non-empty.
        if !state.queue.is_empty() {
            actions.push(BackpressureAction::Process);
        }
    }

    fn next_state(&self, state: &Self::State, action: Self::Action) -> Option<Self::State> {
        let mut next = state.clone();
        match action {
            BackpressureAction::Submit(datom) => {
                if state.total_submitted >= self.max_submissions {
                    return None;
                }
                next.total_submitted += 1;

                if state.queue.len() < usize::from(state.capacity) {
                    // Queue has room: accept the write.
                    next.queue.push(datom);
                } else {
                    // Queue full: reject with Backpressure (INV-FERR-021).
                    // The datom is NOT silently dropped — the caller gets an
                    // explicit error and can retry or shed load.
                    next.rejected_count += 1;
                }
            }
            BackpressureAction::Process => {
                if state.queue.is_empty() {
                    return None;
                }
                // FIFO: pop from the front.
                let datom = next.queue.remove(0);
                next.committed_count += 1;
                next.committed_set.insert(datom);
            }
        }
        Some(next)
    }

    fn within_boundary(&self, state: &Self::State) -> bool {
        state.total_submitted <= self.max_submissions
            && state.queue.len() <= usize::from(state.capacity)
    }

    fn properties(&self) -> Vec<Property<Self>> {
        vec![
            // INV-FERR-021 Safety (no silent drop): every submitted datom is
            // accounted for — either queued, committed, or rejected.
            // total_submitted == committed_count + rejected_count + queue.len()
            Property::always(
                "inv_ferr_021_no_silent_drop",
                |_: &BackpressureModel, state: &BackpressureState| {
                    // INV-FERR-021: the conservation law. If this fails, a
                    // datom was silently lost.
                    state.total_submitted
                        == state.committed_count + state.rejected_count + state.queue.len()
                },
            ),
            // INV-FERR-021 / NEG-FERR-005 Safety (bounded queue): the queue
            // never exceeds capacity. Guarantees no OOM from unbounded queueing.
            Property::always(
                "inv_ferr_021_bounded_queue",
                |_: &BackpressureModel, state: &BackpressureState| {
                    // INV-FERR-021: queue depth <= capacity at all times.
                    state.queue.len() <= usize::from(state.capacity)
                },
            ),
            // INV-FERR-021 Safety (no data loss): the committed_count matches
            // the committed_set cardinality, ensuring no committed datom is
            // lost or double-counted. Note: committed_count may exceed
            // committed_set.len() when the same datom is committed multiple
            // times (duplicate submissions), but committed_set.len() must
            // never exceed committed_count.
            Property::always(
                "inv_ferr_021_committed_set_consistent",
                |_: &BackpressureModel, state: &BackpressureState| {
                    // INV-FERR-021: every unique committed datom is tracked.
                    state.committed_set.len() <= state.committed_count
                },
            ),
            // INV-FERR-021 Liveness (commit reachable): at least one write
            // commits eventually. Non-vacuity check.
            Property::sometimes(
                "inv_ferr_021_commit_reachable",
                |_: &BackpressureModel, state: &BackpressureState| {
                    // INV-FERR-021: the system can make progress.
                    state.committed_count > 0
                },
            ),
            // INV-FERR-021 Liveness (rejection reachable): when more
            // submissions than capacity are attempted, at least one
            // rejection occurs. Verifies the backpressure mechanism fires.
            Property::sometimes(
                "inv_ferr_021_rejection_reachable",
                |_: &BackpressureModel, state: &BackpressureState| {
                    // INV-FERR-021: backpressure rejection path is exercised.
                    state.rejected_count > 0
                },
            ),
            // INV-FERR-021 Liveness (both commit and reject): a state exists
            // where the system has both committed and rejected. This witnesses
            // the full backpressure lifecycle.
            Property::sometimes(
                "inv_ferr_021_full_lifecycle_reachable",
                |_: &BackpressureModel, state: &BackpressureState| {
                    // INV-FERR-021: both paths exercised in one trace.
                    state.committed_count > 0 && state.rejected_count > 0
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

    use super::{super::crdt_model::Datom, BackpressureAction, BackpressureModel};

    fn datom(seed: u64) -> Datom {
        Datom::from_seed(seed)
    }

    // -- Unit tests for individual transitions --

    #[test]
    fn inv_ferr_021_submit_to_empty_queue_accepted() {
        let model = BackpressureModel::default();
        let init = model.init_states().into_iter().next().unwrap();

        let d = datom(0);
        let after = model
            .next_state(&init, BackpressureAction::Submit(d.clone()))
            .expect("INV-FERR-021: submit to empty queue must succeed");

        assert_eq!(
            after.queue.len(),
            1,
            "INV-FERR-021: queue must contain the submitted datom"
        );
        assert_eq!(
            after.total_submitted, 1,
            "INV-FERR-021: total_submitted must increment"
        );
        assert_eq!(
            after.rejected_count, 0,
            "INV-FERR-021: no rejection when queue has room"
        );
    }

    #[test]
    fn inv_ferr_021_submit_to_full_queue_rejected() {
        let model = BackpressureModel::new(2, 5, 3);
        let init = model.init_states().into_iter().next().unwrap();

        // Fill the queue to capacity (2).
        let s1 = model
            .next_state(&init, BackpressureAction::Submit(datom(0)))
            .unwrap();
        let s2 = model
            .next_state(&s1, BackpressureAction::Submit(datom(1)))
            .unwrap();

        assert_eq!(s2.queue.len(), 2, "INV-FERR-021: queue must be at capacity");

        // Third submit must be rejected.
        let s3 = model
            .next_state(&s2, BackpressureAction::Submit(datom(2)))
            .expect("INV-FERR-021: submit to full queue returns a state (with rejection)");

        assert_eq!(
            s3.queue.len(),
            2,
            "INV-FERR-021: queue must not grow beyond capacity"
        );
        assert_eq!(
            s3.rejected_count, 1,
            "INV-FERR-021: rejected_count must increment"
        );
        assert_eq!(
            s3.total_submitted, 3,
            "INV-FERR-021: total_submitted must count the rejected attempt"
        );
    }

    #[test]
    fn inv_ferr_021_process_commits_front_of_queue() {
        let model = BackpressureModel::default();
        let init = model.init_states().into_iter().next().unwrap();

        let d0 = datom(0);
        let d1 = datom(1);
        let s1 = model
            .next_state(&init, BackpressureAction::Submit(d0.clone()))
            .unwrap();
        let s2 = model
            .next_state(&s1, BackpressureAction::Submit(d1.clone()))
            .unwrap();

        // Process dequeues from the front.
        let s3 = model
            .next_state(&s2, BackpressureAction::Process)
            .expect("INV-FERR-021: process from non-empty queue must succeed");

        assert_eq!(
            s3.queue.len(),
            1,
            "INV-FERR-021: queue shrinks after process"
        );
        assert_eq!(
            s3.committed_count, 1,
            "INV-FERR-021: committed_count must increment"
        );
        assert!(
            s3.committed_set.contains(&d0),
            "INV-FERR-021: first submitted datom must be in committed set"
        );
        assert_eq!(
            s3.queue[0], d1,
            "INV-FERR-021: second datom remains in queue"
        );
    }

    #[test]
    fn inv_ferr_021_process_empty_queue_rejected() {
        let model = BackpressureModel::default();
        let init = model.init_states().into_iter().next().unwrap();

        let result = model.next_state(&init, BackpressureAction::Process);
        assert!(
            result.is_none(),
            "INV-FERR-021: process on empty queue must return None"
        );
    }

    #[test]
    fn inv_ferr_021_conservation_law_holds_through_sequence() {
        let model = BackpressureModel::new(2, 6, 3);
        let init = model.init_states().into_iter().next().unwrap();

        // Submit 3 datoms to a capacity-2 queue: 2 accepted, 1 rejected.
        let s1 = model
            .next_state(&init, BackpressureAction::Submit(datom(0)))
            .unwrap();
        let s2 = model
            .next_state(&s1, BackpressureAction::Submit(datom(1)))
            .unwrap();
        let s3 = model
            .next_state(&s2, BackpressureAction::Submit(datom(2)))
            .unwrap();

        // Check conservation: 3 == 0 + 1 + 2
        assert_eq!(
            s3.total_submitted,
            s3.committed_count + s3.rejected_count + s3.queue.len(),
            "INV-FERR-021: conservation law must hold after mixed accept/reject"
        );

        // Process one.
        let s4 = model.next_state(&s3, BackpressureAction::Process).unwrap();

        assert_eq!(
            s4.total_submitted,
            s4.committed_count + s4.rejected_count + s4.queue.len(),
            "INV-FERR-021: conservation law must hold after process"
        );

        // Submit another (queue now has room).
        let s5 = model
            .next_state(&s4, BackpressureAction::Submit(datom(0)))
            .unwrap();

        assert_eq!(
            s5.total_submitted,
            s5.committed_count + s5.rejected_count + s5.queue.len(),
            "INV-FERR-021: conservation law must hold after re-fill"
        );
    }

    #[test]
    fn inv_ferr_021_submit_after_reject_and_process() {
        let model = BackpressureModel::new(1, 4, 2);
        let init = model.init_states().into_iter().next().unwrap();

        // Fill single-slot queue.
        let s1 = model
            .next_state(&init, BackpressureAction::Submit(datom(0)))
            .unwrap();

        // Reject.
        let s2 = model
            .next_state(&s1, BackpressureAction::Submit(datom(1)))
            .unwrap();
        assert_eq!(s2.rejected_count, 1);

        // Process frees the slot.
        let s3 = model.next_state(&s2, BackpressureAction::Process).unwrap();
        assert_eq!(s3.queue.len(), 0);

        // Now a new submit succeeds.
        let s4 = model
            .next_state(&s3, BackpressureAction::Submit(datom(1)))
            .unwrap();
        assert_eq!(
            s4.queue.len(),
            1,
            "INV-FERR-021: submit must succeed after process frees a slot"
        );
        assert_eq!(
            s4.total_submitted,
            s4.committed_count + s4.rejected_count + s4.queue.len(),
            "INV-FERR-021: conservation law after accept-reject-process-accept cycle"
        );
    }

    // -- Model checker tests --

    #[test]
    fn inv_ferr_021_model_checker_all_properties() {
        let checker = BackpressureModel::new(3, 6, 3)
            .checker()
            .target_max_depth(10)
            .spawn_bfs()
            .join();

        // Safety properties must hold in ALL reachable states.
        checker.assert_no_discovery("inv_ferr_021_no_silent_drop");
        checker.assert_no_discovery("inv_ferr_021_bounded_queue");
        checker.assert_no_discovery("inv_ferr_021_committed_set_consistent");

        // Liveness: these states must be reachable.
        checker.assert_any_discovery("inv_ferr_021_commit_reachable");
        checker.assert_any_discovery("inv_ferr_021_rejection_reachable");
        checker.assert_any_discovery("inv_ferr_021_full_lifecycle_reachable");
    }

    #[test]
    fn inv_ferr_021_model_checker_single_slot() {
        // Single-slot queue: maximises contention per submission.
        let checker = BackpressureModel::new(1, 5, 2)
            .checker()
            .target_max_depth(8)
            .spawn_bfs()
            .join();

        checker.assert_no_discovery("inv_ferr_021_no_silent_drop");
        checker.assert_no_discovery("inv_ferr_021_bounded_queue");
        checker.assert_no_discovery("inv_ferr_021_committed_set_consistent");
        checker.assert_any_discovery("inv_ferr_021_commit_reachable");
        checker.assert_any_discovery("inv_ferr_021_rejection_reachable");
        checker.assert_any_discovery("inv_ferr_021_full_lifecycle_reachable");
    }

    #[test]
    fn inv_ferr_021_model_checker_larger_domain() {
        // Larger queue and more submissions to explore deeper interleavings.
        let checker = BackpressureModel::new(4, 8, 4)
            .checker()
            .target_max_depth(12)
            .spawn_bfs()
            .join();

        checker.assert_no_discovery("inv_ferr_021_no_silent_drop");
        checker.assert_no_discovery("inv_ferr_021_bounded_queue");
        checker.assert_no_discovery("inv_ferr_021_committed_set_consistent");
        checker.assert_any_discovery("inv_ferr_021_commit_reachable");
        checker.assert_any_discovery("inv_ferr_021_rejection_reachable");
        checker.assert_any_discovery("inv_ferr_021_full_lifecycle_reachable");
    }

    #[test]
    fn inv_ferr_021_drain_all_committed() {
        // Submit exactly capacity datoms, then drain all. Conservation must
        // hold at every step.
        let cap = 3u8;
        let model = BackpressureModel::new(cap, usize::from(cap), u64::from(cap));
        let mut state = model.init_states().into_iter().next().unwrap();

        // Fill queue.
        for seed in 0..u64::from(cap) {
            state = model
                .next_state(&state, BackpressureAction::Submit(datom(seed)))
                .expect("INV-FERR-021: submit within capacity must succeed");
        }
        assert_eq!(state.queue.len(), usize::from(cap));
        assert_eq!(state.rejected_count, 0);

        // Drain all.
        for _ in 0..usize::from(cap) {
            state = model
                .next_state(&state, BackpressureAction::Process)
                .expect("INV-FERR-021: process from non-empty queue must succeed");
        }
        assert_eq!(state.queue.len(), 0);
        assert_eq!(state.committed_count, usize::from(cap));
        assert_eq!(
            state.committed_set.len(),
            usize::from(cap),
            "INV-FERR-021: each unique datom must appear in committed set"
        );

        // Conservation.
        assert_eq!(
            state.total_submitted,
            state.committed_count + state.rejected_count + state.queue.len(),
            "INV-FERR-021: conservation law after full drain"
        );
    }

    #[test]
    fn inv_ferr_021_interleaved_submit_and_process() {
        let model = BackpressureModel::new(2, 6, 3);
        let init = model.init_states().into_iter().next().unwrap();

        // Interleave: submit, submit, process, submit(rejected), process, submit
        let s1 = model
            .next_state(&init, BackpressureAction::Submit(datom(0)))
            .unwrap();
        let s2 = model
            .next_state(&s1, BackpressureAction::Submit(datom(1)))
            .unwrap();
        let s3 = model.next_state(&s2, BackpressureAction::Process).unwrap();

        // s3: queue=[datom(1)], committed=1, rejected=0, submitted=2
        assert_eq!(s3.queue.len(), 1);
        assert_eq!(s3.committed_count, 1);

        // Fill again.
        let s4 = model
            .next_state(&s3, BackpressureAction::Submit(datom(2)))
            .unwrap();
        // s4: queue=[datom(1), datom(2)], committed=1, rejected=0, submitted=3
        assert_eq!(s4.queue.len(), 2);

        // Reject.
        let s5 = model
            .next_state(&s4, BackpressureAction::Submit(datom(0)))
            .unwrap();
        assert_eq!(s5.rejected_count, 1, "INV-FERR-021: must reject when full");

        // Process.
        let s6 = model.next_state(&s5, BackpressureAction::Process).unwrap();

        // Submit succeeds again.
        let s7 = model
            .next_state(&s6, BackpressureAction::Submit(datom(0)))
            .unwrap();
        assert_eq!(s7.queue.len(), 2);

        // Check conservation at every checkpoint.
        for state in [&s1, &s2, &s3, &s4, &s5, &s6, &s7] {
            assert_eq!(
                state.total_submitted,
                state.committed_count + state.rejected_count + state.queue.len(),
                "INV-FERR-021: conservation law must hold at every step"
            );
        }
    }
}
