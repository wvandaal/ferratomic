//! Backpressure policy for bounded write queue depth.
//!
//! INV-FERR-021: No silent data loss under backpressure. When the write
//! pipeline is saturated, `transact()` returns `Err(Backpressure)` rather
//! than blocking indefinitely or dropping transactions silently.
//!
//! NEG-FERR-005: No unbounded memory growth. The backpressure mechanism
//! ensures that pending write attempts cannot exhaust memory.
//!
//! # Phase 4a Strategy
//!
//! The Phase 4a Mutex model provides inherent backpressure via `try_lock()`:
//! - At most 1 transaction executes at a time (the lock holder).
//! - All other `transact()` calls return `Err(Backpressure)` immediately.
//! - No write queue exists — transactions are accepted or rejected instantly.
//! - Memory is bounded: rejected transactions are dropped by the caller.
//!
//! This module adds an additional **concurrency limiter** that tracks the
//! number of concurrent `transact()` attempts (including the one holding
//! the lock). When the limit is reached, new attempts are rejected before
//! even trying the lock. This prevents thundering herd effects where many
//! threads simultaneously contend on the Mutex.

use std::sync::atomic::{AtomicUsize, Ordering};

/// Configuration for write backpressure policy.
///
/// INV-FERR-021: defines the bounds that prevent unbounded write queuing.
#[derive(Debug, Clone)]
pub struct BackpressurePolicy {
    /// Maximum number of concurrent `transact()` attempts.
    /// Includes the active writer and all threads waiting to `try_lock`.
    /// Defaults to 64.
    ///
    /// INV-FERR-021: this bound prevents unbounded write queue growth.
    /// NEG-FERR-005: ensures memory usage from pending writes is bounded.
    pub max_concurrent_writes: usize,
}

impl Default for BackpressurePolicy {
    fn default() -> Self {
        Self {
            max_concurrent_writes: 64,
        }
    }
}

/// Concurrency limiter for write backpressure.
///
/// INV-FERR-021: tracks the number of active `transact()` attempts.
/// When the count reaches `max_concurrent_writes`, new attempts are
/// rejected with `Err(Backpressure)`.
///
/// INV-FERR-021: the guard pattern ensures the active count is always
/// decremented, even on early returns or panics. This is a lightweight
/// semaphore implemented with `AtomicUsize`.
pub struct WriteGuard<'g> {
    limiter: &'g WriteLimiter,
}

impl Drop for WriteGuard<'_> {
    fn drop(&mut self) {
        self.limiter.active.fetch_sub(1, Ordering::Release);
    }
}

/// Atomic counter for concurrent write attempts.
///
/// INV-FERR-021: the limiter is lock-free and wait-free. It uses
/// `fetch_add` and `fetch_sub` with `Ordering::AcqRel` for correct
/// visibility across threads.
pub struct WriteLimiter {
    active: AtomicUsize,
    max: usize,
}

impl WriteLimiter {
    /// Create a new write limiter with the given policy.
    ///
    /// INV-FERR-021: initializes with zero active writes and the
    /// configured maximum from the policy.
    #[must_use]
    pub fn new(policy: &BackpressurePolicy) -> Self {
        Self {
            active: AtomicUsize::new(0),
            max: policy.max_concurrent_writes,
        }
    }

    /// Try to acquire a write slot. Returns a guard that releases
    /// the slot on drop, or `None` if the limit is reached.
    ///
    /// INV-FERR-021: when this returns `None`, the caller should
    /// return `Err(FerraError::Backpressure)`.
    pub fn try_acquire(&self) -> Option<WriteGuard<'_>> {
        let prev = self.active.fetch_add(1, Ordering::AcqRel);
        if prev >= self.max {
            self.active.fetch_sub(1, Ordering::Release);
            return None;
        }
        Some(WriteGuard { limiter: self })
    }

    /// Current number of active write attempts.
    ///
    /// INV-FERR-021: this value is bounded by `max_concurrent_writes`.
    /// Useful for monitoring and diagnostics.
    #[must_use]
    pub fn active_count(&self) -> usize {
        self.active.load(Ordering::Acquire)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_inv_ferr_021_default_policy() {
        let policy = BackpressurePolicy::default();
        assert_eq!(policy.max_concurrent_writes, 64);
    }

    #[test]
    fn test_inv_ferr_021_acquire_and_release() {
        let policy = BackpressurePolicy {
            max_concurrent_writes: 2,
        };
        let limiter = WriteLimiter::new(&policy);

        assert_eq!(limiter.active_count(), 0);

        let g1 = limiter.try_acquire();
        assert!(g1.is_some());
        assert_eq!(limiter.active_count(), 1);

        let g2 = limiter.try_acquire();
        assert!(g2.is_some());
        assert_eq!(limiter.active_count(), 2);

        // Third attempt should fail (limit is 2).
        let g3 = limiter.try_acquire();
        assert!(g3.is_none(), "INV-FERR-021: must reject when at capacity");
        assert_eq!(limiter.active_count(), 2);

        // Drop g1 — now there's room.
        drop(g1);
        assert_eq!(limiter.active_count(), 1);

        let g4 = limiter.try_acquire();
        assert!(g4.is_some(), "INV-FERR-021: should accept after release");
        assert_eq!(limiter.active_count(), 2);
    }

    #[test]
    fn test_inv_ferr_021_single_slot_limiter() {
        let policy = BackpressurePolicy {
            max_concurrent_writes: 1,
        };
        let limiter = WriteLimiter::new(&policy);

        let g1 = limiter.try_acquire();
        assert!(g1.is_some());

        let g2 = limiter.try_acquire();
        assert!(
            g2.is_none(),
            "INV-FERR-021: limit=1 must reject second attempt"
        );

        drop(g1);
        let g3 = limiter.try_acquire();
        assert!(g3.is_some());
    }
}
