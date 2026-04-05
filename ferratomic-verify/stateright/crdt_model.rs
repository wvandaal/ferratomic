#![forbid(unsafe_code)]

use std::collections::BTreeSet;

use stateright::{Model, Property};

/// INV-FERR-012: a datom's identity is its five-tuple content.
///
/// The Stateright model uses a finite synthetic datom domain derived from a
/// seed so the checker can explore all message orderings over a bounded space.
///
/// All fields are `pub` intentionally: this is a simplified verification model
/// type, not the production `Datom`. The model needs direct field access for
/// `from_seed` construction and `BTreeSet` ordering. The production `Datom`
/// in `ferratom/` enforces encapsulation through content-addressed constructors.
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
    ///
    /// Fields are derived independently so the model can explore:
    /// - Same entity with different attributes (e=0,a=0 vs e=0,a=1)
    /// - Assert and retract on the same entity (op decoupled from e)
    /// - Multiple values per entity-attribute pair
    ///
    /// The seed encodes: bits[0] = op, bits[1] = attribute, bits[2..] = entity.
    /// Value and tx are fixed (identity in a G-Set is the full 5-tuple).
    pub const fn from_seed(seed: u64) -> Self {
        Self {
            e: seed >> 2,       // entity: independent of op and attr
            a: (seed >> 1) & 1, // attribute: 2 values, independent of entity
            v: 0,               // fixed value — identity comes from (e, a, op)
            tx: 0,              // fixed tx — G-Set identity is content, not time
            op: seed & 1 == 0,  // op: assert (even) / retract (odd)
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
    /// Per-node causal write history: which original writes each node has
    /// received (directly or via merge). This tracks the UPDATE SET
    /// independently of the node's state, enabling a non-vacuous SEC check.
    pub received_writes: Vec<BTreeSet<Datom>>,
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
        // Domain check: entity must be within max_datoms/4 (since each
        // entity generates 4 variants: 2 attributes × 2 ops).
        let max_entity = self.max_datoms / 4 + 1;
        datom.e < max_entity && datom.a <= 1
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
            received_writes: vec![BTreeSet::new(); self.node_count],
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
                next.nodes[node].insert(datom.clone());
                next.received_writes[node].insert(datom);
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
                // Track received writes: all datoms in the payload are
                // now causally received by the target node.
                for d in &payload {
                    next.received_writes[to].insert(d.clone());
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
            // INV-FERR-010 Safety (SEC): at quiescence, if two nodes have
            // received the same set of original writes (tracked independently
            // in received_writes), their states must be identical.
            // This is NON-VACUOUS: received_writes tracks causal history,
            // not final state. A broken merge could produce different states
            // from the same received writes, and this property would catch it.
            Property::always(
                "inv_ferr_010_sec_convergence",
                |_: &CrdtModel, state: &CrdtState| {
                    if !state.in_flight.is_empty() {
                        return true; // SEC only applies at quiescence
                    }
                    // For every pair of nodes: if they have the same
                    // received_writes set, their states must be equal.
                    for i in 0..state.nodes.len() {
                        for j in (i + 1)..state.nodes.len() {
                            if state.received_writes[i] == state.received_writes[j]
                                && state.nodes[i] != state.nodes[j]
                            {
                                return false; // SEC violation
                            }
                        }
                    }
                    true
                },
            ),
            // INV-FERR-004: Monotonic Growth.
            // ∀ S, d: S ⊆ apply(S, d) — no datom is ever lost.
            // Verified by checking: every datom that a node has received
            // (via direct write or merge delivery) is still present in the
            // node's state. received_writes tracks causal history independently
            // of node state, so a broken merge that loses datoms would cause
            // received_writes[i] ⊄ nodes[i].
            Property::always(
                "inv_ferr_004_monotonic_growth",
                |_: &CrdtModel, state: &CrdtState| {
                    state
                        .nodes
                        .iter()
                        .zip(state.received_writes.iter())
                        .all(|(node, received)| received.is_subset(node))
                },
            ),
            // INV-FERR-018: Append-Only (CRDT dimension — traceability alias).
            // ∀ S, op: ∀ d ∈ S: d ∈ op(S, args). No operation removes a datom.
            // For G-Set CRDTs, append-only (INV-FERR-018) and monotonic growth
            // (INV-FERR-004) are algebraically equivalent: both reduce to
            // S ⊆ S' after any operation. This predicate is IDENTICAL to
            // inv_ferr_004_monotonic_growth — it adds zero independent
            // falsification power. It exists solely for spec traceability.
            // The meaningful INV-FERR-018 Stateright verification is
            // inv_ferr_018_append_only_recovery on crash_recovery_model.rs,
            // which verifies committed data survives recovery.
            Property::always(
                "inv_ferr_018_append_only_crdt_alias",
                |_: &CrdtModel, state: &CrdtState| {
                    state
                        .nodes
                        .iter()
                        .zip(state.received_writes.iter())
                        .all(|(node, received)| received.is_subset(node))
                },
            ),
            // Liveness: a converged quiescent state is reachable.
            Property::sometimes(
                "inv_ferr_010_convergence_reachable",
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

    fn canonical_nodes(nodes: &[BTreeSet<Datom>]) -> Vec<Vec<Datom>> {
        nodes
            .iter()
            .map(|node| node.iter().cloned().collect::<Vec<_>>())
            .collect()
    }

    fn collect_final_orders(
        model: &CrdtModel,
        state: CrdtState,
        finals: &mut Vec<Vec<Vec<Datom>>>,
    ) {
        if state.in_flight.is_empty() {
            finals.push(canonical_nodes(&state.nodes));
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
            received_writes: vec![set_of(&[0]), set_of(&[1]), set_of(&[2])],
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
        checker.assert_no_discovery("inv_ferr_010_sec_convergence");
        checker.assert_no_discovery("inv_ferr_004_monotonic_growth");
        checker.assert_no_discovery("inv_ferr_018_append_only_crdt_alias");
        checker.assert_any_discovery("inv_ferr_010_convergence_reachable");
    }

    #[test]
    fn inv_ferr_004_received_datoms_always_present_after_merge() {
        // Write datom to node 0, merge to node 1, verify received ⊆ nodes.
        let model = CrdtModel::default();
        let s0 = model.init_states().remove(0);
        let d = datom(0);
        let s1 = model
            .next_state(&s0, CrdtAction::Write(0, d.clone()))
            .expect("INV-FERR-004: write must succeed");
        // received_writes[0] must contain the datom
        assert!(
            s1.received_writes[0].contains(&d),
            "INV-FERR-004: received_writes must track the write"
        );
        // nodes[0] must contain the datom
        assert!(
            s1.nodes[0].contains(&d),
            "INV-FERR-004: node must contain the written datom"
        );
        // Merge from 0 to 1
        let s2 = model
            .next_state(&s1, CrdtAction::InitMerge(0, 1))
            .expect("INV-FERR-004: merge init must succeed");
        let s3 = model
            .next_state(&s2, CrdtAction::DeliverMerge(0))
            .expect("INV-FERR-004: merge delivery must succeed");
        // After merge, node 1 must contain the datom
        assert!(
            s3.nodes[1].contains(&d),
            "INV-FERR-004: merged datom must be in target node"
        );
        assert!(
            s3.received_writes[1].is_subset(&s3.nodes[1]),
            "INV-FERR-004: received_writes must be subset of nodes after merge"
        );
    }

    #[test]
    fn inv_ferr_010_converges_for_all_delivery_orders_in_seeded_mesh() {
        let model = CrdtModel::default();
        let initial = seeded_convergence_state(&model);
        let mut finals = Vec::new();

        collect_final_orders(&model, initial, &mut finals);

        let expected =
            canonical_nodes(&[set_of(&[0, 1, 2]), set_of(&[0, 1, 2]), set_of(&[0, 1, 2])]);

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
