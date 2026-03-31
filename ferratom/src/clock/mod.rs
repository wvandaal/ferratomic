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

mod frontier;
mod txid;

pub use frontier::Frontier;
pub use txid::{AgentId, TxId};

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

        let new_physical = now.max(self.physical).max(remote.physical());

        let new_logical = if new_physical == self.physical && new_physical == remote.physical() {
            // All three tied — take max of the two logical counters.
            self.logical.max(remote.logical())
        } else if new_physical == self.physical {
            // Local physical is the max (and strictly > remote physical).
            self.logical
        } else if new_physical == remote.physical() {
            // Remote physical is the max (and strictly > local physical).
            remote.logical()
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
