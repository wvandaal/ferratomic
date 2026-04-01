//! Transaction identifier and agent identity types.
//!
//! INV-FERR-015: HLC monotonicity — `TxId` ordering guarantees every
//! `tick()` output is strictly greater than the previous.
//!
//! INV-FERR-016: HLC causality — if e1 happens-before e2, then
//! `hlc(e1) < hlc(e2)`.

use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// AgentId
// ---------------------------------------------------------------------------

/// 16-byte agent identifier.
///
/// INV-FERR-015/016: Each agent in the distributed system has a unique
/// identity used to distinguish concurrent writers and break ties in the
/// hybrid logical clock.
///
/// Lexicographic byte comparison via derived `Ord`.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Debug, Serialize, Deserialize)]
pub struct AgentId([u8; 16]);

impl AgentId {
    /// Create an `AgentId` from raw bytes.
    ///
    /// INV-FERR-015: Every agent must have a unique 16-byte identifier.
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

    /// Create an `AgentId` from a `u16` seed by zero-extending into 16 bytes.
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
/// `(physical, logical, agent)` guarantees every `tick()` output is
/// strictly greater than the previous.
///
/// INV-FERR-016: HLC causality. `receive()` advances the local clock
/// past the remote timestamp, so causally-related events are ordered.
///
/// Total order: `(physical, logical, agent)` — physical time dominates,
/// logical breaks ties within the same millisecond, agent breaks ties
/// when two agents happen to share the same `(physical, logical)`.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
pub struct TxId {
    /// Wall-clock time in milliseconds since epoch.
    physical: u64,
    /// Logical counter within the same physical timestamp.
    logical: u32,
    /// Agent that created this transaction.
    agent: AgentId,
}

impl TxId {
    /// Create a `TxId` from compact components. **Testing only.**
    ///
    /// INV-FERR-015: The triple `(physical, logical, agent)` forms a
    /// totally ordered HLC timestamp.
    ///
    /// The `agent_seed` is zero-extended into a 16-byte `AgentId`. This
    /// constructor exists for ergonomic use in tests and generators; prefer
    /// [`TxId::with_agent`] in production code.
    #[cfg(any(test, feature = "test-utils"))]
    #[must_use]
    pub fn new(physical: u64, logical: u32, agent_seed: u16) -> Self {
        Self {
            physical,
            logical,
            agent: AgentId::from_seed(agent_seed),
        }
    }

    /// Create a `TxId` with an explicit [`AgentId`].
    ///
    /// INV-FERR-015: Production constructor for HLC timestamps produced
    /// by [`HybridClock::tick`](crate::HybridClock::tick).
    #[must_use]
    pub fn with_agent(physical: u64, logical: u32, agent: AgentId) -> Self {
        Self {
            physical,
            logical,
            agent,
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

    /// Agent that created this transaction.
    #[must_use]
    pub fn agent(&self) -> AgentId {
        self.agent
    }
}

/// Lexicographic ordering: `(physical, logical, agent)`.
///
/// INV-FERR-015: This ordering guarantees that monotonically increasing
/// physical time produces monotonically increasing `TxId`s, with logical
/// and agent as tiebreakers.
impl Ord for TxId {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.physical
            .cmp(&other.physical)
            .then_with(|| self.logical.cmp(&other.logical))
            .then_with(|| self.agent.cmp(&other.agent))
    }
}

impl PartialOrd for TxId {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
