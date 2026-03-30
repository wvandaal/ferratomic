#![forbid(unsafe_code)]

use std::collections::BTreeSet;

use stateright::{Model, Property};

/// INV-FERR-012: a datom's identity is its five-tuple content.
///
/// The Stateright model uses a finite synthetic datom domain derived from a
/// seed so the checker can explore all message orderings over a bounded space.
#[derive(Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct Datom {
    /// Content-addressed entity identifier.
    pub e: u64,
    /// Attribute identifier.
    pub a: u64,
    /// Abstract value identifier.
    pub v: u64,
    /// Transaction identifier.
    pub tx: u64,
    /// Operation bit: `true` is assert, `false` is retract.
    pub op: bool,
}

impl Datom {
    /// Builds a stable finite-domain datom for the Stateright model.
    pub const fn from_seed(seed: u64) -> Self {
        Self {
            e: seed,
            a: seed % 3,
            v: seed * 17 + 1,
            tx: seed / 2,
            op: seed % 2 == 0,
        }
    }
}

/// A merge message `(from, to, payload)` from spec §23.0.5.
pub type MergeMessage = (usize, usize, BTreeSet<Datom>);

/// CRDT state from spec §23.0.5.
///
/// INV-FERR-010: replicas converge when the same merge payloads are delivered,
/// regardless of delivery order.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct CrdtState {
    /// Replica-local G-Set state for each node.
    pub nodes: Vec<BTreeSet<Datom>>,
    /// In-flight merge snapshots `(from, to, payload)`.
    pub in_flight: Vec<MergeMessage>,
}

/// Actions available to the Stateright checker from spec §23.0.5.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub enum CrdtAction {
    /// Apply a local write to one node.
    Write(usize, Datom),
    /// Snapshot a node and queue a merge to a peer.
    InitMerge(usize, usize),
    /// Deliver the in-flight merge at the given index.
    DeliverMerge(usize),
}

/// Bounded Stateright model for the CRDT merge protocol.
#[derive(Clone, Debug)]
pub struct CrdtModel {
    /// Number of replicas in the cluster.
    pub node_count: usize,
    /// Finite datom domain size explored by the checker.
    pub max_datoms: u64,
    /// Bound on queued merge messages to keep the state space finite.
    pub max_in_flight: usize,
}

impl CrdtModel {
    /// Constructs a bounded CRDT model from the spec foundation state machine.
    pub const fn new(node_count: usize, max_datoms: u64, max_in_flight: usize) -> Self {
        Self {
            node_count,
            max_datoms,
            max_in_flight,
        }
    }

    /// INV-FERR-001, INV-FERR-002, INV-FERR-003: merge is plain set union.
    pub fn merge_sets(a: &BTreeSet<Datom>, b: &BTreeSet<Datom>) -> BTreeSet<Datom> {
        a.union(b).cloned().collect()
    }

    /// Returns `true` when every replica holds the same datom set.
    pub fn is_converged(state: &CrdtState) -> bool {
        match state.nodes.first() {
            Some(first) => state.nodes.iter().all(|node| node == first),
            None => true,
        }
    }

    fn datom_for(&self, seed: u64) -> Datom {
        Datom::from_seed(seed)
    }

    fn is_in_domain(&self, datom: &Datom) -> bool {
        datom.e < self.max_datoms
    }

    fn can_queue_snapshot(
        &self,
        state: &CrdtState,
        from: usize,
        to: usize,
    ) -> Option<BTreeSet<Datom>> {
        if from >= state.nodes.len() || to >= state.nodes.len() || from == to {
            return None;
        }
        if state.in_flight.len() >= self.max_in_flight {
            return None;
        }

        Some(state.nodes[from].clone())
    }

    fn canonical_nodes(nodes: &[BTreeSet<Datom>]) -> Vec<Vec<Datom>> {
        nodes
            .iter()
            .map(|node| node.iter().cloned().collect::<Vec<_>>())
            .collect()
    }
}

impl Default for CrdtModel {
    fn default() -> Self {
        Self::new(3, 3, 6)
    }
}

impl Model for CrdtModel {
    type State = CrdtState;
    type Action = CrdtAction;

    fn init_states(&self) -> Vec<Self::State> {
        vec![CrdtState {
            nodes: vec![BTreeSet::new(); self.node_count],
            in_flight: Vec::new(),
        }]
    }

    fn actions(&self, state: &Self::State, actions: &mut Vec<Self::Action>) {
        for node_idx in 0..self.node_count {
            for datom_id in 0..self.max_datoms {
                let datom = self.datom_for(datom_id);
                if !state.nodes[node_idx].contains(&datom) {
                    actions.push(CrdtAction::Write(node_idx, datom));
                }
            }

            for peer_idx in 0..self.node_count {
                if self.can_queue_snapshot(state, node_idx, peer_idx).is_some() {
                    actions.push(CrdtAction::InitMerge(node_idx, peer_idx));
                }
            }
        }

        for idx in 0..state.in_flight.len() {
            actions.push(CrdtAction::DeliverMerge(idx));
        }
    }

    fn next_state(&self, state: &Self::State, action: Self::Action) -> Option<Self::State> {
        let mut next = state.clone();
        match action {
            CrdtAction::Write(node, datom) => {
                if node >= next.nodes.len() || !self.is_in_domain(&datom) {
                    return None;
                }
                next.nodes[node].insert(datom);
            }
            CrdtAction::InitMerge(from, to) => {
                let payload = self.can_queue_snapshot(&next, from, to)?;
                next.in_flight.push((from, to, payload));
            }
            CrdtAction::DeliverMerge(index) => {
                if index >= next.in_flight.len() {
                    return None;
                }
                let (_, to, payload) = next.in_flight.remove(index);
                if to >= next.nodes.len() {
                    return None;
                }
                next.nodes[to] = Self::merge_sets(&next.nodes[to], &payload);
            }
        }
        Some(next)
    }

    fn within_boundary(&self, state: &Self::State) -> bool {
        state.in_flight.len() <= self.max_in_flight
            && state
                .nodes
                .iter()
                .all(|node| node.iter().all(|datom| self.is_in_domain(datom)))
            && state
                .in_flight
                .iter()
                .all(|(_, _, payload)| payload.iter().all(|datom| self.is_in_domain(datom)))
    }

    fn properties(&self) -> Vec<Property<Self>> {
        vec![
            Property::always(
                "inv_ferr_010_in_flight_payloads_stay_in_domain",
                |model: &CrdtModel, state: &CrdtState| {
                    state.in_flight.iter().all(|(_, _, payload)| {
                        payload.iter().all(|datom| model.is_in_domain(datom))
                    })
                },
            ),
            Property::sometimes(
                "inv_ferr_010_convergence",
                |_: &CrdtModel, state: &CrdtState| {
                    state.in_flight.is_empty() && CrdtModel::is_converged(state)
                },
            ),
        ]
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use stateright::{Checker, Model};

    use super::{CrdtAction, CrdtModel, CrdtState, Datom};

    fn datom(seed: u64) -> Datom {
        Datom::from_seed(seed)
    }

    fn set_of(seeds: &[u64]) -> BTreeSet<Datom> {
        seeds.iter().copied().map(datom).collect()
    }

    fn collect_final_orders(
        model: &CrdtModel,
        state: CrdtState,
        finals: &mut Vec<Vec<Vec<Datom>>>,
    ) {
        if state.in_flight.is_empty() {
            finals.push(CrdtModel::canonical_nodes(&state.nodes));
            return;
        }

        for index in 0..state.in_flight.len() {
            let next = model
                .next_state(&state, CrdtAction::DeliverMerge(index))
                .expect("INV-FERR-010: every queued merge must be deliverable");
            collect_final_orders(model, next, finals);
        }
    }

    fn seeded_convergence_state(model: &CrdtModel) -> CrdtState {
        let mut state = CrdtState {
            nodes: vec![set_of(&[0]), set_of(&[1]), set_of(&[2])],
            in_flight: Vec::new(),
        };

        for from in 0..3 {
            for to in 0..3 {
                if from == to {
                    continue;
                }
                state = model
                    .next_state(&state, CrdtAction::InitMerge(from, to))
                    .expect("INV-FERR-010: non-empty replica snapshots must queue");
            }
        }

        state
    }

    #[test]
    fn test_bug_bd_85j_2_4_duplicate_merge_messages_are_modeled() {
        let model = CrdtModel::default();
        let state0 = model.init_states().remove(0);
        let state1 = model
            .next_state(&state0, CrdtAction::Write(0, Datom::from_seed(0)))
            .expect("INV-FERR-003: local writes must remain reachable in the model");
        let state2 = model
            .next_state(&state1, CrdtAction::InitMerge(0, 1))
            .expect("INV-FERR-003: the first merge snapshot must queue");
        let duplicate = model.next_state(&state2, CrdtAction::InitMerge(0, 1));

        assert!(
            duplicate.is_some(),
            "INV-FERR-003 / INV-FERR-010: duplicate merge snapshots must remain in the \
             explored state space so replayed delivery stays modeled"
        );
    }

    #[test]
    fn inv_ferr_001_merge_commutativity_model() {
        let a = set_of(&[0, 2]);
        let b = set_of(&[1, 2]);

        assert_eq!(
            CrdtModel::merge_sets(&a, &b),
            CrdtModel::merge_sets(&b, &a),
            "INV-FERR-001: set-union merge must be commutative in the Stateright model"
        );
    }

    #[test]
    fn inv_ferr_002_merge_associativity_model() {
        let a = set_of(&[0]);
        let b = set_of(&[1]);
        let c = set_of(&[2]);

        assert_eq!(
            CrdtModel::merge_sets(&CrdtModel::merge_sets(&a, &b), &c),
            CrdtModel::merge_sets(&a, &CrdtModel::merge_sets(&b, &c)),
            "INV-FERR-002: regrouping merges must not change the final datom set"
        );
    }

    #[test]
    fn inv_ferr_003_merge_idempotency_model() {
        let store = set_of(&[0, 1, 2]);

        assert_eq!(
            CrdtModel::merge_sets(&store, &store),
            store,
            "INV-FERR-003: merging a replica with itself must be a no-op"
        );
    }

    #[test]
    fn inv_ferr_010_model_checker_finds_a_converged_state() {
        let checker = CrdtModel::new(2, 2, 2)
            .checker()
            .target_max_depth(4)
            .spawn_bfs()
            .join();

        checker.assert_no_discovery("inv_ferr_010_in_flight_payloads_stay_in_domain");
        checker.assert_any_discovery("inv_ferr_010_convergence");
    }

    #[test]
    fn inv_ferr_010_converges_for_all_delivery_orders_in_seeded_mesh() {
        let model = CrdtModel::default();
        let initial = seeded_convergence_state(&model);
        let mut finals = Vec::new();

        collect_final_orders(&model, initial, &mut finals);

        let expected = CrdtModel::canonical_nodes(&[
            set_of(&[0, 1, 2]),
            set_of(&[0, 1, 2]),
            set_of(&[0, 1, 2]),
        ]);

        assert!(
            !finals.is_empty(),
            "INV-FERR-010: the seeded mesh must have at least one delivery schedule"
        );
        assert!(
            finals.iter().all(|final_nodes| final_nodes == &expected),
            "INV-FERR-010: every delivery order over the same merge payloads must converge \
             to the identical replica state"
        );
    }
}
