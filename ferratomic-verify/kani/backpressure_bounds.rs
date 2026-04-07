//! Backpressure bounds Kani harnesses.
//!
//! Covers INV-FERR-021: WriteLimiter enforces capacity bounds.
//! Acquiring up to max succeeds; the next attempt returns None.

use ferratomic_db::backpressure::{BackpressurePolicy, WriteLimiter};

#[cfg(not(kani))]
use super::kani;

/// INV-FERR-021: WriteLimiter enforces capacity — acquire up to max
/// succeeds, then the next attempt returns None.
///
/// Bounded to max_concurrent_writes = 3 for Kani tractability.
#[cfg_attr(kani, kani::proof)]
#[cfg_attr(kani, kani::unwind(6))]
#[cfg_attr(not(kani), test)]
#[cfg_attr(not(kani), ignore = "requires Kani verifier")]
fn write_limiter_capacity_enforcement() {
    let policy = BackpressurePolicy {
        max_concurrent_writes: 3,
    };
    let limiter = WriteLimiter::new(&policy);

    // Acquire 3 slots — all must succeed.
    let g1 = limiter.try_acquire();
    assert!(g1.is_some(), "INV-FERR-021: first acquire must succeed");
    let g2 = limiter.try_acquire();
    assert!(g2.is_some(), "INV-FERR-021: second acquire must succeed");
    let g3 = limiter.try_acquire();
    assert!(
        g3.is_some(),
        "INV-FERR-021: third acquire (at max) must succeed"
    );

    // Fourth acquire must fail — capacity reached.
    let g4 = limiter.try_acquire();
    assert!(
        g4.is_none(),
        "INV-FERR-021: acquire beyond max must return None"
    );
    assert_eq!(
        limiter.active_count(),
        3,
        "INV-FERR-021: active count must equal max"
    );
}

/// INV-FERR-021: WriteLimiter releases slots on guard drop.
///
/// After dropping a guard, a new acquire succeeds.
#[cfg_attr(kani, kani::proof)]
#[cfg_attr(kani, kani::unwind(4))]
#[cfg_attr(not(kani), test)]
#[cfg_attr(not(kani), ignore = "requires Kani verifier")]
fn write_limiter_release_on_drop() {
    let policy = BackpressurePolicy {
        max_concurrent_writes: 1,
    };
    let limiter = WriteLimiter::new(&policy);

    // Acquire the single slot.
    let g1 = limiter.try_acquire();
    assert!(g1.is_some(), "INV-FERR-021: single slot must be acquirable");

    // Second attempt must fail.
    let g2 = limiter.try_acquire();
    assert!(
        g2.is_none(),
        "INV-FERR-021: limit=1 must reject second attempt"
    );

    // Drop the first guard.
    drop(g1);
    assert_eq!(
        limiter.active_count(),
        0,
        "INV-FERR-021: active count must be 0 after drop"
    );

    // Now acquire should succeed again.
    let g3 = limiter.try_acquire();
    assert!(
        g3.is_some(),
        "INV-FERR-021: acquire must succeed after release"
    );
}

/// INV-FERR-021: symbolic capacity — for any capacity 1..=3 and
/// any number of acquires, active_count never exceeds max.
#[cfg_attr(kani, kani::proof)]
#[cfg_attr(kani, kani::unwind(6))]
#[cfg_attr(not(kani), test)]
#[cfg_attr(not(kani), ignore = "requires Kani verifier")]
fn write_limiter_active_count_bounded() {
    let max: usize = kani::any();
    kani::assume((1..=3).contains(&max));

    let policy = BackpressurePolicy {
        max_concurrent_writes: max,
    };
    let limiter = WriteLimiter::new(&policy);

    // Try to acquire max+1 times. The first `max` must succeed;
    // any beyond must fail. Active count must never exceed max.
    let mut guards = Vec::new();
    for _ in 0..=max {
        if let Some(g) = limiter.try_acquire() {
            guards.push(g);
        }
    }

    assert!(
        limiter.active_count() <= max,
        "INV-FERR-021: active_count must never exceed max_concurrent_writes"
    );
    assert!(
        guards.len() <= max,
        "INV-FERR-021: number of acquired guards must not exceed max"
    );
}
