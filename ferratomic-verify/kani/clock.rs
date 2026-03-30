//! Hybrid clock Kani harnesses.
//!
//! Covers INV-FERR-015 and INV-FERR-016.

use ferratom::{AgentId, HybridClock};

type Hlc = HybridClock;

/// INV-FERR-015: every tick must advance the local HLC.
#[kani::proof]
#[kani::unwind(10)]
fn hlc_monotonicity() {
    let mut hlc = Hlc::new(AgentId::from_bytes([1u8; 16]));
    let mut prev = hlc.clone();

    for _ in 0..kani::any::<u8>().min(5) {
        let next = hlc.tick();
        assert!(next > prev, "INV-FERR-015: HLC did not advance");
        prev = next;
    }
}

/// INV-FERR-016: receiving a causal predecessor advances the receiver past it.
#[kani::proof]
#[kani::unwind(6)]
fn hlc_causality() {
    let mut sender = Hlc::new(AgentId::from_bytes([1u8; 16]));
    let mut receiver = Hlc::new(AgentId::from_bytes([2u8; 16]));

    let send_hlc = sender.tick();

    receiver.receive(&send_hlc);
    let recv_hlc = receiver.tick();

    assert!(recv_hlc > send_hlc, "INV-FERR-016: recv <= send");
}
