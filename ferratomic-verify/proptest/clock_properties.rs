//! Property tests for INV-FERR-015 (HLC monotonicity) and
//! INV-FERR-016 (HLC causality).

use ferratom::{HybridClock, NodeId, TxId};
use proptest::prelude::*;

proptest! {
    #![proptest_config(ProptestConfig::with_cases(10_000))]

    /// INV-FERR-015: tick() always produces strictly increasing TxIds.
    #[test]
    fn inv_ferr_015_hlc_monotonicity(
        tick_count in 2usize..200,
    ) {
        let node = NodeId::from_bytes([1u8; 16]);
        let mut clock = HybridClock::new(node);
        let mut prev = clock.tick().unwrap();

        for i in 1..tick_count {
            let next = clock.tick().unwrap();
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
        let node = NodeId::from_bytes([2u8; 16]);
        let mut clock = HybridClock::new(node);

        // Advance local clock.
        let mut last_local = clock.tick().unwrap();
        for _ in 1..local_ticks {
            last_local = clock.tick().unwrap();
        }

        // Receive a remote timestamp that may be in the past.
        let remote_node = NodeId::from_bytes([3u8; 16]);
        let remote_tx = TxId::with_node(remote_physical, 0, remote_node);
        clock.receive(&remote_tx);

        // Next tick must still be strictly greater.
        let after_receive = clock.tick().unwrap();
        prop_assert!(
            after_receive > last_local,
            "INV-FERR-015: tick after receive(past) not monotonic: {:?} <= {:?}",
            after_receive, last_local
        );
    }

    /// INV-FERR-016: receive() ensures next tick is greater than the
    /// remote timestamp (causal ordering across nodes).
    #[test]
    fn inv_ferr_016_hlc_causality(
        remote_physical in 0u64..u64::MAX / 2,
        remote_logical in 0u32..1000,
    ) {
        let local_node = NodeId::from_bytes([10u8; 16]);
        let remote_node = NodeId::from_bytes([20u8; 16]);
        let mut clock = HybridClock::new(local_node);

        let remote_tx = TxId::with_node(remote_physical, remote_logical, remote_node);
        clock.receive(&remote_tx);

        let local_tx = clock.tick().unwrap();
        prop_assert!(
            local_tx > remote_tx,
            "INV-FERR-016: local tick {:?} not causally after remote {:?}",
            local_tx, remote_tx
        );
    }

    /// INV-FERR-016: causality is transitive across a chain of nodes.
    #[test]
    fn inv_ferr_016_hlc_causality_chain(
        chain_length in 2usize..10,
    ) {
        let mut nodes: Vec<HybridClock> = (0..chain_length)
            .map(|i| {
                let mut bytes = [0u8; 16];
                bytes[0] = i as u8;
                HybridClock::new(NodeId::from_bytes(bytes))
            })
            .collect();

        // Node 0 ticks, sends to node 1, who ticks and sends to node 2, etc.
        let mut prev_tx = nodes[0].tick().unwrap();

        for (i, node) in nodes.iter_mut().enumerate().skip(1) {
            node.receive(&prev_tx);
            let current = node.tick().unwrap();
            prop_assert!(
                current > prev_tx,
                "INV-FERR-016: node {} tick {:?} not after node {} tick {:?}",
                i, current, i - 1, prev_tx
            );
            prev_tx = current;
        }
    }
}
