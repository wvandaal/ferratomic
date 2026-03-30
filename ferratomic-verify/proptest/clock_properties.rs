//! Property tests for INV-FERR-015 (HLC monotonicity) and
//! INV-FERR-016 (HLC causality).

use ferratom::{AgentId, HybridClock, TxId};
use proptest::prelude::*;

proptest! {
    #![proptest_config(ProptestConfig::with_cases(10_000))]

    /// INV-FERR-015: tick() always produces strictly increasing TxIds.
    #[test]
    fn inv_ferr_015_hlc_monotonicity(
        tick_count in 2usize..200,
    ) {
        let agent = AgentId::from_bytes([1u8; 16]);
        let mut clock = HybridClock::new(agent);
        let mut prev = clock.tick();

        for i in 1..tick_count {
            let next = clock.tick();
            prop_assert!(
                next > prev,
                "INV-FERR-015: tick {} ({:?}) not greater than tick {} ({:?})",
                i, next, i - 1, prev
            );
            prev = next;
        }
    }

    /// INV-FERR-015: tick() is monotonic even after receiving a remote
    /// timestamp in the past.
    #[test]
    fn inv_ferr_015_hlc_monotonic_after_past_receive(
        local_ticks in 1usize..50,
        remote_physical in 0u64..1000,
    ) {
        let agent = AgentId::from_bytes([2u8; 16]);
        let mut clock = HybridClock::new(agent);

        // Advance local clock.
        let mut last_local = clock.tick();
        for _ in 1..local_ticks {
            last_local = clock.tick();
        }

        // Receive a remote timestamp that may be in the past.
        let remote_agent = AgentId::from_bytes([3u8; 16]);
        let remote_tx = TxId::with_agent(remote_physical, 0, remote_agent);
        clock.receive(&remote_tx);

        // Next tick must still be strictly greater.
        let after_receive = clock.tick();
        prop_assert!(
            after_receive > last_local,
            "INV-FERR-015: tick after receive(past) not monotonic: {:?} <= {:?}",
            after_receive, last_local
        );
    }

    /// INV-FERR-016: receive() ensures next tick is greater than the
    /// remote timestamp (causal ordering across agents).
    #[test]
    fn inv_ferr_016_hlc_causality(
        remote_physical in 0u64..u64::MAX / 2,
        remote_logical in 0u32..1000,
    ) {
        let local_agent = AgentId::from_bytes([10u8; 16]);
        let remote_agent = AgentId::from_bytes([20u8; 16]);
        let mut clock = HybridClock::new(local_agent);

        let remote_tx = TxId::with_agent(remote_physical, remote_logical, remote_agent);
        clock.receive(&remote_tx);

        let local_tx = clock.tick();
        prop_assert!(
            local_tx > remote_tx,
            "INV-FERR-016: local tick {:?} not causally after remote {:?}",
            local_tx, remote_tx
        );
    }

    /// INV-FERR-016: causality is transitive across a chain of agents.
    #[test]
    fn inv_ferr_016_hlc_causality_chain(
        chain_length in 2usize..10,
    ) {
        let mut agents: Vec<HybridClock> = (0..chain_length)
            .map(|i| {
                let mut bytes = [0u8; 16];
                bytes[0] = i as u8;
                HybridClock::new(AgentId::from_bytes(bytes))
            })
            .collect();

        // Agent 0 ticks, sends to agent 1, who ticks and sends to agent 2, etc.
        let mut prev_tx = agents[0].tick();

        for i in 1..chain_length {
            agents[i].receive(&prev_tx);
            let current = agents[i].tick();
            prop_assert!(
                current > prev_tx,
                "INV-FERR-016: agent {} tick {:?} not after agent {} tick {:?}",
                i, current, i - 1, prev_tx
            );
            prev_tx = current;
        }
    }
}
