//! Hybrid Logical Clock (HLC) for causal ordering.
//!
//! INV-FERR-015: HLC monotonicity — `tick()` always produces a `TxId`
//! strictly greater than the previous one, even under NTP clock regression.
//!
//! INV-FERR-016: HLC causality — if e1 happens-before e2, then
//! `hlc(e1) < hlc(e2)`. Causality is defined by the predecessor graph,
//! NOT by HLC comparison (see SEED.md section 4, INV-STORE-010).
//!
//! # Types
//!
//! - [`AgentId`]: 16-byte agent identifier (newtype over `[u8; 16]`).
//! - [`TxId`]: Transaction identifier — `(physical, logical, agent)` triple.
//! - [`HybridClock`]: Stateful clock that produces monotonically increasing `TxId`s.
//! - [`Frontier`]: Vector clock tracking per-agent progress.

use std::collections::HashMap;

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
    /// by [`HybridClock::tick`].
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

// ---------------------------------------------------------------------------
// HybridClock
// ---------------------------------------------------------------------------

/// Hybrid Logical Clock producing causally ordered [`TxId`]s.
///
/// INV-FERR-015: `tick()` always produces a `TxId` strictly greater than
/// any previously produced or received timestamp. Even if the wall clock
/// regresses (NTP correction), the logical counter advances to maintain
/// monotonicity.
///
/// INV-FERR-016: `receive()` merges a remote timestamp into the local
/// clock state, ensuring that subsequent `tick()`s produce timestamps
/// ordered after the remote event. This establishes happens-before
/// ordering across agents.
#[derive(Clone, Debug)]
pub struct HybridClock {
    /// Last known physical time (wall clock ms).
    physical: u64,
    /// Logical counter within the current physical timestamp.
    logical: u32,
    /// Identity of the agent owning this clock.
    agent: AgentId,
}

impl HybridClock {
    /// Create a new `HybridClock` for the given agent.
    ///
    /// INV-FERR-015: The clock starts at `(0, 0)` — the first `tick()`
    /// will advance to at least the current wall clock time.
    #[must_use]
    pub fn new(agent: AgentId) -> Self {
        Self {
            physical: 0,
            logical: 0,
            agent,
        }
    }

    /// Advance the clock and return a new, strictly greater [`TxId`].
    ///
    /// INV-FERR-015: If the wall clock has advanced past our recorded
    /// physical time, we adopt the new time and reset the logical counter.
    /// If the wall clock has NOT advanced (or regressed), we increment
    /// the logical counter to ensure strict monotonicity.
    ///
    /// The returned `TxId` is guaranteed to be strictly greater than any
    /// previously returned or received timestamp.
    pub fn tick(&mut self) -> TxId {
        let now = Self::wall_clock();

        if now > self.physical {
            self.physical = now;
            self.logical = 0;
        } else if let Some(next_logical) = self.logical.checked_add(1) {
            // Wall clock did not advance — increment logical.
            self.logical = next_logical;
        } else {
            // INV-FERR-015 / INV-FERR-021: logical counter overflow.
            // Backpressure: busy-wait until wall clock advances.
            // This caps throughput at u32::MAX events per millisecond
            // (~4.3 billion/ms) — physically unreachable.
            loop {
                std::thread::yield_now();
                let updated = Self::wall_clock();
                if updated > self.physical {
                    self.physical = updated;
                    self.logical = 0;
                    break;
                }
            }
        }

        TxId::with_agent(self.physical, self.logical, self.agent)
    }

    /// Merge a remote timestamp into the local clock state.
    ///
    /// INV-FERR-016: After `receive(remote)`, the local clock state is
    /// at least as large as `remote` AND at least as large as the previous
    /// local state. The next `tick()` will produce a `TxId` strictly
    /// greater than both.
    ///
    /// Algorithm:
    /// 1. `new_physical = max(self.physical, remote.physical, now)`
    /// 2. If all three physical values are equal, `logical = max(self.logical, remote.logical)`
    /// 3. If `self.physical` wins, keep `self.logical`
    /// 4. If `remote.physical` wins, adopt `remote.logical`
    /// 5. If `now` wins (or ties with the max), reset `logical = 0`
    pub fn receive(&mut self, remote: &TxId) {
        let now = Self::wall_clock();

        let new_physical = now.max(self.physical).max(remote.physical);

        let new_logical = if new_physical == self.physical && new_physical == remote.physical {
            // All three tied — take max of the two logical counters.
            self.logical.max(remote.logical)
        } else if new_physical == self.physical {
            // Local physical is the max (and strictly > remote physical).
            self.logical
        } else if new_physical == remote.physical {
            // Remote physical is the max (and strictly > local physical).
            remote.logical
        } else {
            // Wall clock is strictly the max — reset logical.
            0
        };

        self.physical = new_physical;
        self.logical = new_logical;
    }

    /// Current wall-clock time in milliseconds since the Unix epoch.
    ///
    /// Extracted as a separate method so that future test harnesses can
    /// override it (e.g., via a clock trait or feature flag).
    #[must_use]
    #[allow(clippy::cast_possible_truncation)]
    fn wall_clock() -> u64 {
        // as_millis() returns u128 but u64 millis covers 584 million years
        // from epoch — truncation is physically unreachable.
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            // duration_since only fails if UNIX_EPOCH is in the future,
            // which is physically impossible on any real system. Fallback
            // to 0 rather than panicking (NEG-FERR-001: no panics).
            .unwrap_or_default()
            .as_millis() as u64
    }
}

// ---------------------------------------------------------------------------
// Frontier
// ---------------------------------------------------------------------------

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

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // -- AgentId tests --

    #[test]
    fn agent_id_roundtrip() {
        let bytes = [1u8, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16];
        let id = AgentId::from_bytes(bytes);
        assert_eq!(*id.as_bytes(), bytes);
    }

    #[test]
    fn agent_id_from_seed_deterministic() {
        let a = AgentId::from_seed(42);
        let b = AgentId::from_seed(42);
        assert_eq!(a, b, "Same seed must produce same AgentId");
    }

    #[test]
    fn agent_id_from_seed_distinct() {
        let a = AgentId::from_seed(1);
        let b = AgentId::from_seed(2);
        assert_ne!(a, b, "Different seeds must produce different AgentIds");
    }

    #[test]
    fn agent_id_ord_is_lexicographic() {
        let a = AgentId::from_bytes([0u8; 16]);
        let b = AgentId::from_bytes([1u8; 16]);
        assert!(a < b);
    }

    // -- TxId tests --

    #[test]
    fn tx_id_new_accessors() {
        let tx = TxId::new(100, 5, 7);
        assert_eq!(tx.physical(), 100);
        assert_eq!(tx.logical(), 5);
        assert_eq!(tx.agent(), AgentId::from_seed(7));
    }

    #[test]
    fn tx_id_with_agent_accessors() {
        let agent = AgentId::from_bytes([0xAA; 16]);
        let tx = TxId::with_agent(200, 10, agent);
        assert_eq!(tx.physical(), 200);
        assert_eq!(tx.logical(), 10);
        assert_eq!(tx.agent(), agent);
    }

    #[test]
    fn tx_id_ord_physical_dominates() {
        let a = TxId::new(1, 100, 100);
        let b = TxId::new(2, 0, 0);
        assert!(a < b, "Higher physical must dominate");
    }

    #[test]
    fn tx_id_ord_logical_tiebreaks() {
        let a = TxId::new(5, 0, 0);
        let b = TxId::new(5, 1, 0);
        assert!(a < b, "Higher logical must break physical tie");
    }

    #[test]
    fn tx_id_ord_agent_tiebreaks() {
        let a = TxId::new(5, 5, 0);
        let b = TxId::new(5, 5, 1);
        assert!(a < b, "Higher agent must break physical+logical tie");
    }

    #[test]
    fn tx_id_equality() {
        let a = TxId::new(1, 2, 3);
        let b = TxId::new(1, 2, 3);
        assert_eq!(a, b);
    }

    // -- HybridClock tests --

    #[test]
    fn inv_ferr_015_tick_monotonicity() {
        let agent = AgentId::from_bytes([1u8; 16]);
        let mut clock = HybridClock::new(agent);

        let t1 = clock.tick();
        let t2 = clock.tick();
        let t3 = clock.tick();

        assert!(
            t2 > t1,
            "INV-FERR-015: second tick must be greater than first"
        );
        assert!(
            t3 > t2,
            "INV-FERR-015: third tick must be greater than second"
        );
    }

    #[test]
    fn inv_ferr_016_receive_advances_past_remote() {
        let mut sender_clock = HybridClock::new(AgentId::from_bytes([1u8; 16]));
        let mut receiver_clock = HybridClock::new(AgentId::from_bytes([2u8; 16]));

        let sent = sender_clock.tick();
        receiver_clock.receive(&sent);
        let received = receiver_clock.tick();

        assert!(
            received > sent,
            "INV-FERR-016: receiver tick must exceed sender's timestamp"
        );
    }

    #[test]
    fn inv_ferr_016_receive_preserves_local_progress() {
        let mut clock = HybridClock::new(AgentId::from_bytes([1u8; 16]));

        // Advance the local clock significantly.
        let local = clock.tick();

        // Receive a remote timestamp from the past.
        let old_remote = TxId::new(0, 0, 5);
        clock.receive(&old_remote);

        let after = clock.tick();
        assert!(
            after > local,
            "INV-FERR-016: receiving old remote must not regress local clock"
        );
    }

    // -- Frontier tests --

    #[test]
    fn frontier_advance_and_get() {
        let agent = AgentId::from_bytes([1u8; 16]);
        let tx = TxId::new(10, 0, 0);

        let mut frontier = Frontier::new();
        frontier.advance(agent, tx);

        assert_eq!(frontier.get(&agent), Some(&tx));
        assert_eq!(frontier.len(), 1);
    }

    #[test]
    fn frontier_advance_only_moves_forward() {
        let agent = AgentId::from_bytes([1u8; 16]);
        let tx_old = TxId::new(1, 0, 0);
        let tx_new = TxId::new(10, 0, 0);

        let mut frontier = Frontier::new();
        frontier.advance(agent, tx_new);
        frontier.advance(agent, tx_old); // should be a no-op

        assert_eq!(
            frontier.get(&agent),
            Some(&tx_new),
            "Frontier must not regress"
        );
    }

    #[test]
    fn frontier_merge_takes_per_agent_max() {
        let a1 = AgentId::from_bytes([1u8; 16]);
        let a2 = AgentId::from_bytes([2u8; 16]);

        let mut f1 = Frontier::new();
        f1.advance(a1, TxId::new(10, 0, 0));
        f1.advance(a2, TxId::new(5, 0, 0));

        let mut f2 = Frontier::new();
        f2.advance(a1, TxId::new(5, 0, 0));
        f2.advance(a2, TxId::new(10, 0, 0));

        f1.merge(&f2);

        assert_eq!(f1.get(&a1), Some(&TxId::new(10, 0, 0)));
        assert_eq!(f1.get(&a2), Some(&TxId::new(10, 0, 0)));
    }

    #[test]
    fn frontier_empty_default() {
        let f = Frontier::default();
        assert!(f.is_empty());
        assert_eq!(f.len(), 0);
    }
}
