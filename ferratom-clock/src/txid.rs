//! Transaction identifier and node identity types.
//!
//! INV-FERR-015: HLC monotonicity — `TxId` ordering guarantees every
//! `tick()` output is strictly greater than the previous.
//!
//! INV-FERR-016: HLC causality — if e1 happens-before e2, then
//! `hlc(e1) < hlc(e2)`.
//!
//! C8 (Substrate Independence): The engine-level identifier for the
//! distributed writer is `NodeId`, not `AgentId`. "Node" is the
//! domain-neutral name shared with HLC literature, CRDT literature, and
//! Datomic's "peer" terminology. Application-layer code is free to use
//! `:agent/*` namespace conventions on top of this primitive.

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// NodeId
// ---------------------------------------------------------------------------

/// 16-byte node identifier.
///
/// INV-FERR-015/016: Each node in the distributed system has a unique
/// identity used to distinguish concurrent writers and break ties in the
/// hybrid logical clock.
///
/// Lexicographic byte comparison via derived `Ord`.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug, Serialize, Deserialize)]
pub struct NodeId([u8; 16]);

impl NodeId {
    /// Create a `NodeId` from raw bytes.
    ///
    /// INV-FERR-015: Every node must have a unique 16-byte identifier.
    /// The caller is responsible for uniqueness (typically via UUID v4 or
    /// BLAKE3 truncation).
    #[must_use]
    pub fn from_bytes(bytes: [u8; 16]) -> Self {
        Self(bytes)
    }

    /// Return a reference to the underlying 16-byte identifier.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8; 16] {
        &self.0
    }

    /// Create a `NodeId` from a `u16` seed by zero-extending into 16 bytes.
    ///
    /// Intended for tests and generators where a compact seed is more
    /// convenient than a full 16-byte array.
    #[cfg(any(test, feature = "test-utils"))]
    #[must_use]
    pub fn from_seed(seed: u16) -> Self {
        let mut bytes = [0u8; 16];
        let le = seed.to_le_bytes();
        bytes[0] = le[0];
        bytes[1] = le[1];
        Self(bytes)
    }
}

// ---------------------------------------------------------------------------
// TxId
// ---------------------------------------------------------------------------

/// Hybrid Logical Clock transaction identifier.
///
/// INV-FERR-015: HLC monotonicity. Lexicographic ordering on
/// `(physical, logical, node)` guarantees every `tick()` output is
/// strictly greater than the previous.
///
/// INV-FERR-016: HLC causality. `receive()` advances the local clock
/// past the remote timestamp, so causally-related events are ordered.
///
/// Total order: `(physical, logical, node)` — physical time dominates,
/// logical breaks ties within the same millisecond, node breaks ties
/// when two nodes happen to share the same `(physical, logical)`.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
pub struct TxId {
    /// Wall-clock time in milliseconds since epoch.
    physical: u64,
    /// Logical counter within the same physical timestamp.
    logical: u32,
    /// Node that originated this transaction.
    node: NodeId,
}

impl TxId {
    /// Create a `TxId` from compact components. **Testing only.**
    ///
    /// INV-FERR-015: The triple `(physical, logical, node)` forms a
    /// totally ordered HLC timestamp.
    ///
    /// The `node_seed` is zero-extended into a 16-byte `NodeId`. This
    /// constructor exists for ergonomic use in tests and generators; prefer
    /// [`TxId::with_node`] in production code.
    #[cfg(any(test, feature = "test-utils"))]
    #[must_use]
    pub fn new(physical: u64, logical: u32, node_seed: u16) -> Self {
        Self {
            physical,
            logical,
            node: NodeId::from_seed(node_seed),
        }
    }

    /// Create a `TxId` with an explicit [`NodeId`].
    ///
    /// INV-FERR-015: Production constructor for HLC timestamps produced
    /// by [`HybridClock::tick`](crate::HybridClock::tick).
    #[must_use]
    pub fn with_node(physical: u64, logical: u32, node: NodeId) -> Self {
        Self {
            physical,
            logical,
            node,
        }
    }

    /// Wall-clock milliseconds since epoch.
    #[must_use]
    pub fn physical(&self) -> u64 {
        self.physical
    }

    /// Logical counter within the same physical timestamp.
    #[must_use]
    pub fn logical(&self) -> u32 {
        self.logical
    }

    /// Node that originated this transaction.
    #[must_use]
    pub fn node(&self) -> NodeId {
        self.node
    }
}

/// Lexicographic ordering: `(physical, logical, node)`.
///
/// INV-FERR-015: This ordering guarantees that monotonically increasing
/// physical time produces monotonically increasing `TxId`s, with logical
/// and node as tiebreakers.
impl Ord for TxId {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.physical
            .cmp(&other.physical)
            .then_with(|| self.logical.cmp(&other.logical))
            .then_with(|| self.node.cmp(&other.node))
    }
}

impl PartialOrd for TxId {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
