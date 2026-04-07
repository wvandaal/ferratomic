//! # ferratom-clock — Hybrid Logical Clock for causal ordering
//!
//! INV-FERR-015: HLC monotonicity — `tick()` always produces a `TxId`
//! strictly greater than the previous one, even under NTP clock regression.
//!
//! INV-FERR-016: HLC causality — if e1 happens-before e2, then
//! `hlc(e1) < hlc(e2)`. Causality is defined by the predecessor graph,
//! NOT by HLC comparison.
//!
//! # Types
//!
//! - [`AgentId`]: 16-byte agent identifier (newtype over `[u8; 16]`).
//! - [`TxId`]: Transaction identifier — `(physical, logical, agent)` triple.
//! - [`HybridClock`]: Stateful clock that produces monotonically increasing `TxId`s.
//! - [`Frontier`]: Vector clock tracking per-agent progress.

// INV-FERR-023: No unsafe code permitted. Compiler-enforced.
#![forbid(unsafe_code)]
#![deny(missing_docs, clippy::all)]
#![warn(clippy::pedantic)]

mod frontier;
mod txid;

pub use frontier::Frontier;
pub use txid::{AgentId, TxId};

// ---------------------------------------------------------------------------
// ClockError
// ---------------------------------------------------------------------------

/// Errors from clock operations.
///
/// INV-FERR-021: Bounded backpressure — clock operations that cannot
/// complete within a bounded number of retries return an error instead
/// of busy-waiting indefinitely.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClockError {
    /// Logical counter overflow after bounded retry exhaustion.
    ///
    /// The wall clock has not advanced after `MAX_BUSY_WAIT_RETRIES`
    /// iterations. This indicates a frozen or mocked clock source.
    LogicalOverflow,
}

impl core::fmt::Display for ClockError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::LogicalOverflow => write!(
                f,
                "HLC logical counter overflow: wall clock stuck after bounded retry (INV-FERR-021)"
            ),
        }
    }
}

impl std::error::Error for ClockError {}

/// Maximum busy-wait iterations before returning [`ClockError::LogicalOverflow`].
///
/// 1 million iterations of `yield_now()` is ~1-10ms on modern hardware,
/// well beyond any realistic wall-clock stall.
const MAX_BUSY_WAIT_RETRIES: u32 = 1_000_000;

// ---------------------------------------------------------------------------
// ClockSource trait
// ---------------------------------------------------------------------------

/// Wall clock source for HLC (INV-FERR-015).
///
/// Default: [`SystemClock`] (real time). Kani verification uses a
/// deterministic `KaniClock` (lives in `ferratomic-verify`).
pub trait ClockSource: Send + Sync + 'static {
    /// Current wall clock time in milliseconds since Unix epoch.
    fn now(&self) -> u64;
}

// ---------------------------------------------------------------------------
// SystemClock
// ---------------------------------------------------------------------------

/// Real wall clock via [`std::time::SystemTime`].
///
/// INV-FERR-015: Returns milliseconds since the Unix epoch. The
/// `unwrap_or(0)` fallback is intentional — `SystemTime::now()` returning
/// a time before the Unix epoch is a platform-level impossibility on
/// modern systems; falling back to 0 lets the HLC logical counter
/// advance (safe degradation).
#[derive(Debug, Clone, Default)]
pub struct SystemClock;

impl ClockSource for SystemClock {
    fn now(&self) -> u64 {
        let millis_u128 = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        u64::try_from(millis_u128).unwrap_or(u64::MAX)
    }
}

// ---------------------------------------------------------------------------
// HybridClock
// ---------------------------------------------------------------------------

/// Hybrid Logical Clock producing causally ordered [`TxId`]s.
///
/// Generic over [`ClockSource`] with a default of [`SystemClock`], so
/// existing call sites that use `HybridClock` (without a type parameter)
/// resolve to `HybridClock<SystemClock>` and compile without changes.
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
pub struct HybridClock<C: ClockSource = SystemClock> {
    /// Wall clock source.
    clock: C,
    /// Last known physical time (wall clock ms).
    physical: u64,
    /// Logical counter within the current physical timestamp.
    logical: u32,
    /// Identity of the agent owning this clock.
    agent: AgentId,
}

impl<C: ClockSource> HybridClock<C> {
    /// Create a new `HybridClock` for the given agent with a custom clock source.
    ///
    /// INV-FERR-015: The clock starts at `(0, 0)` — the first `tick()`
    /// will advance to at least the current wall clock time.
    #[must_use]
    pub fn with_clock(agent: AgentId, clock: C) -> Self {
        Self {
            clock,
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
    ///
    /// # Errors
    ///
    /// Returns [`ClockError::LogicalOverflow`] if the logical counter
    /// overflows and the wall clock does not advance within
    /// [`MAX_BUSY_WAIT_RETRIES`] iterations (INV-FERR-021).
    pub fn tick(&mut self) -> Result<TxId, ClockError> {
        let now = self.clock.now();

        if now > self.physical {
            self.physical = now;
            self.logical = 0;
        } else if let Some(next_logical) = self.logical.checked_add(1) {
            self.logical = next_logical;
        } else {
            // INV-FERR-015 / INV-FERR-021: logical counter overflow.
            // Bounded backpressure: busy-wait until wall clock advances.
            let mut retries = 0u32;
            loop {
                std::thread::yield_now();
                let updated = self.clock.now();
                if updated > self.physical {
                    self.physical = updated;
                    self.logical = 0;
                    break;
                }
                retries += 1;
                if retries >= MAX_BUSY_WAIT_RETRIES {
                    return Err(ClockError::LogicalOverflow);
                }
            }
        }

        Ok(TxId::with_agent(self.physical, self.logical, self.agent))
    }

    /// Merge a remote timestamp into the local clock state.
    ///
    /// INV-FERR-016: After `receive(remote)`, the local clock state is
    /// at least as large as `remote` AND at least as large as the previous
    /// local state. The next `tick()` will produce a `TxId` strictly
    /// greater than both.
    pub fn receive(&mut self, remote: &TxId) {
        let now = self.clock.now();
        let new_physical = now.max(self.physical).max(remote.physical());

        let new_logical = if new_physical == self.physical && new_physical == remote.physical() {
            self.logical.max(remote.logical())
        } else if new_physical == self.physical {
            self.logical
        } else if new_physical == remote.physical() {
            remote.logical()
        } else {
            0
        };

        self.physical = new_physical;
        self.logical = new_logical;
    }

    /// Set the logical counter directly for testing overflow behavior.
    ///
    /// Only available in test builds and when the `test-utils` feature is
    /// enabled. Used to reach `u32::MAX` without 4 billion ticks.
    #[cfg(any(test, feature = "test-utils"))]
    pub fn set_logical_for_test(&mut self, logical: u32) {
        self.logical = logical;
    }
}

impl HybridClock<SystemClock> {
    /// Create a new `HybridClock` for the given agent using the real wall clock.
    ///
    /// This is a convenience constructor equivalent to
    /// `HybridClock::with_clock(agent, SystemClock)`.
    ///
    /// INV-FERR-015: The clock starts at `(0, 0)` — the first `tick()`
    /// will advance to at least the current wall clock time.
    #[must_use]
    pub fn new(agent: AgentId) -> Self {
        Self::with_clock(agent, SystemClock)
    }

    /// Alias for [`HybridClock::new`] — explicitly names the clock source.
    #[must_use]
    pub fn with_system_clock(agent: AgentId) -> Self {
        Self::new(agent)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn inv_ferr_015_tick_monotonicity() {
        let agent = AgentId::from_bytes([1u8; 16]);
        let mut clock = HybridClock::with_system_clock(agent);
        let t1 = clock.tick().unwrap();
        let t2 = clock.tick().unwrap();
        let t3 = clock.tick().unwrap();
        assert!(t2 > t1, "INV-FERR-015: second > first");
        assert!(t3 > t2, "INV-FERR-015: third > second");
    }

    #[test]
    fn inv_ferr_016_receive_advances_past_remote() {
        let mut sender = HybridClock::with_system_clock(AgentId::from_bytes([1u8; 16]));
        let mut recv_clock = HybridClock::with_system_clock(AgentId::from_bytes([2u8; 16]));
        let sent = sender.tick().unwrap();
        recv_clock.receive(&sent);
        let after_recv = recv_clock.tick().unwrap();
        assert!(after_recv > sent, "INV-FERR-016: receiver > sender");
    }

    #[test]
    fn inv_ferr_016_receive_preserves_local_progress() {
        let mut clock = HybridClock::with_system_clock(AgentId::from_bytes([1u8; 16]));
        let local = clock.tick().unwrap();
        let old_remote = TxId::new(0, 0, 5);
        clock.receive(&old_remote);
        let after = clock.tick().unwrap();
        assert!(after > local, "INV-FERR-016: old remote must not regress");
    }

    #[test]
    fn frontier_advance_and_get() {
        let agent = AgentId::from_bytes([1u8; 16]);
        let tx = TxId::new(10, 0, 0);
        let mut frontier = Frontier::new();
        frontier.advance(agent, tx);
        assert_eq!(frontier.get(&agent), Some(&tx));
    }

    #[test]
    fn frontier_advance_only_moves_forward() {
        let agent = AgentId::from_bytes([1u8; 16]);
        let old = TxId::new(1, 0, 0);
        let new = TxId::new(10, 0, 0);
        let mut frontier = Frontier::new();
        frontier.advance(agent, new);
        frontier.advance(agent, old);
        assert_eq!(frontier.get(&agent), Some(&new));
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
    }

    /// A clock source that always returns the same value, simulating a
    /// frozen/stuck wall clock for overflow testing.
    struct FrozenClock {
        fixed_ms: u64,
    }

    impl ClockSource for FrozenClock {
        fn now(&self) -> u64 {
            self.fixed_ms
        }
    }

    /// INV-FERR-021: When the logical counter is at `u32::MAX` and the wall
    /// clock is frozen, `tick()` enters the bounded retry loop, exhausts
    /// `MAX_BUSY_WAIT_RETRIES`, and returns `Err(LogicalOverflow)`.
    #[test]
    fn test_inv_ferr_021_logical_overflow_under_frozen_clock() {
        let agent = AgentId::from_bytes([7u8; 16]);
        let mut clock = HybridClock::with_clock(agent, FrozenClock { fixed_ms: 42 });

        // First tick: physical=42, logical=0.
        let first = clock.tick().expect("first tick must succeed");
        assert_eq!(first.physical(), 42);
        assert_eq!(first.logical(), 0);

        // Force logical to u32::MAX via test helper.
        clock.set_logical_for_test(u32::MAX);

        // Next tick: checked_add(1) overflows, enters busy-wait loop.
        // FrozenClock never advances, so after MAX_BUSY_WAIT_RETRIES
        // iterations the clock returns Err(LogicalOverflow).
        let result = clock.tick();
        assert!(
            matches!(result, Err(ClockError::LogicalOverflow)),
            "INV-FERR-021: frozen clock at logical=u32::MAX must overflow, got {result:?}"
        );
    }
}
