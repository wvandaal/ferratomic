//! Vector clock frontier tracking per-node progress.
//!
//! INV-FERR-016: The frontier records the latest `TxId` observed from each
//! node. This enables peers to compute the delta (new datoms) needed to
//! bring a lagging replica up to date.
//!
//! C8 (Substrate Independence): The engine-level writer identifier is
//! `NodeId`, not `AgentId`. Application-layer code may still use
//! `:agent/*` namespace conventions on top of this primitive.

use std::collections::BTreeMap;

use crate::{NodeId, TxId};

/// Vector clock tracking per-node progress.
///
/// INV-FERR-016: The frontier records the latest `TxId` observed from each
/// node. This enables peers to compute the delta (new datoms) needed to
/// bring a lagging replica up to date.
///
/// Merge semantics: per-node max. If two frontiers disagree on node A's
/// latest transaction, the greater `TxId` wins. This mirrors the join
/// operation on the product lattice of per-node HLC chains.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frontier {
    /// Per-node latest observed `TxId`.
    map: BTreeMap<NodeId, TxId>,
}

impl Frontier {
    /// Create an empty frontier (no nodes observed).
    #[must_use]
    pub fn new() -> Self {
        Self {
            map: BTreeMap::new(),
        }
    }

    /// Record that `node` has progressed to at least `tx_id`.
    ///
    /// INV-FERR-016: The frontier only advances — if the existing entry
    /// for `node` is already greater than or equal to `tx_id`, this is
    /// a no-op.
    pub fn advance(&mut self, node: NodeId, tx_id: TxId) {
        let entry = self.map.entry(node).or_insert(tx_id);
        if tx_id > *entry {
            *entry = tx_id;
        }
    }

    /// Return the latest `TxId` observed for `node`, if any.
    #[must_use]
    pub fn get(&self, node: &NodeId) -> Option<&TxId> {
        self.map.get(node)
    }

    /// Merge another frontier into this one (per-node max).
    ///
    /// INV-FERR-016: The merged frontier dominates both inputs on every
    /// node dimension. This is the join (least upper bound) in the
    /// product lattice.
    pub fn merge(&mut self, other: &Frontier) {
        for (&node, &tx_id) in &other.map {
            self.advance(node, tx_id);
        }
    }

    /// Return the number of nodes tracked by this frontier.
    #[must_use]
    pub fn len(&self) -> usize {
        self.map.len()
    }

    /// Return `true` if no nodes are tracked.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    /// Iterate over `(node, tx_id)` pairs in this frontier.
    ///
    /// INV-FERR-016: Exposes per-node progress tracking for delta
    /// computation during causal replication.
    pub fn iter(&self) -> impl Iterator<Item = (&NodeId, &TxId)> {
        self.map.iter()
    }
}

impl Default for Frontier {
    fn default() -> Self {
        Self::new()
    }
}
