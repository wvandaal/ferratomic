//! Hybrid clock Kani harnesses.
//!
//! Covers INV-FERR-015 and INV-FERR-016.
//!
//! The harnesses inject a deterministic verification clock rather than using
//! `SystemClock`, so Kani proves the real HLC transition logic without
//! reaching host syscalls such as `clock_gettime`.

use ferratom::{HybridClock, NodeId};

use super::helpers::KaniClock;
#[cfg(not(kani))]
use super::kani;

/// INV-FERR-015: every tick must advance the local HLC.
#[cfg_attr(kani, kani::proof)]
#[cfg_attr(kani, kani::unwind(10))]
#[cfg_attr(not(kani), test)]
#[cfg_attr(not(kani), ignore = "requires Kani verifier")]
fn hlc_monotonicity() {
    let mut hlc = HybridClock::with_clock(
        NodeId::from_bytes([1u8; 16]),
        KaniClock::new([
            kani::any(),
            kani::any(),
            kani::any(),
            kani::any(),
            kani::any(),
        ]),
    );
    let mut prev = hlc.tick().unwrap();

    for _ in 0..kani::any::<u8>().min(4) {
        let next = hlc.tick().unwrap();
        assert!(next > prev, "INV-FERR-015: HLC did not advance");
        prev = next;
    }
}

/// INV-FERR-016: receiving a causal predecessor advances the receiver past it.
#[cfg_attr(kani, kani::proof)]
#[cfg_attr(kani, kani::unwind(6))]
#[cfg_attr(not(kani), test)]
#[cfg_attr(not(kani), ignore = "requires Kani verifier")]
fn hlc_causality() {
    let mut sender =
        HybridClock::with_clock(NodeId::from_bytes([1u8; 16]), KaniClock::new([kani::any()]));
    let mut receiver = HybridClock::with_clock(
        NodeId::from_bytes([2u8; 16]),
        KaniClock::new([kani::any(), kani::any()]),
    );

    let send_hlc = sender.tick().unwrap();

    receiver.receive(&send_hlc);
    let recv_hlc = receiver.tick().unwrap();

    assert!(recv_hlc > send_hlc, "INV-FERR-016: recv <= send");
}
