//! Vector clock frontier tracking per-agent progress.
//!
//! INV-FERR-016: The frontier records the latest `TxId` observed from each
//! agent. This enables peers to compute the delta (new datoms) needed to
//! bring a lagging replica up to date.

use std::collections::HashMap;

use crate::{AgentId, TxId};

/// Vector clock tracking per-agent progress.
///
/// INV-FERR-016: The frontier records the latest `TxId` observed from each
/// agent. This enables peers to compute the delta (new datoms) needed to
/// bring a lagging replica up to date.
///
/// Merge semantics: per-agent max. If two frontiers disagree on agent A's
/// latest transaction, the greater `TxId` wins. This mirrors the join
/// operation on the product lattice of per-agent HLC chains.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frontier {
    /// Per-agent latest observed `TxId`.
    map: HashMap<AgentId, TxId>,
}

impl Frontier {
    /// Create an empty frontier (no agents observed).
    #[must_use]
    pub fn new() -> Self {
        Self {
            map: HashMap::new(),
        }
    }

    /// Record that `agent` has progressed to at least `tx_id`.
    ///
    /// INV-FERR-016: The frontier only advances — if the existing entry
    /// for `agent` is already greater than or equal to `tx_id`, this is
    /// a no-op.
    pub fn advance(&mut self, agent: AgentId, tx_id: TxId) {
        let entry = self.map.entry(agent).or_insert(tx_id);
        if tx_id > *entry {
            *entry = tx_id;
        }
    }

    /// Return the latest `TxId` observed for `agent`, if any.
    #[must_use]
    pub fn get(&self, agent: &AgentId) -> Option<&TxId> {
        self.map.get(agent)
    }

    /// Merge another frontier into this one (per-agent max).
    ///
    /// INV-FERR-016: The merged frontier dominates both inputs on every
    /// agent dimension. This is the join (least upper bound) in the
    /// product lattice.
    pub fn merge(&mut self, other: &Frontier) {
        for (&agent, &tx_id) in &other.map {
            self.advance(agent, tx_id);
        }
    }

    /// Return the number of agents tracked by this frontier.
    #[must_use]
    pub fn len(&self) -> usize {
        self.map.len()
    }

    /// Return `true` if no agents are tracked.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    /// Iterate over `(agent, tx_id)` pairs.
    pub fn iter(&self) -> impl Iterator<Item = (&AgentId, &TxId)> {
        self.map.iter()
    }
}

impl Default for Frontier {
    fn default() -> Self {
        Self::new()
    }
}
