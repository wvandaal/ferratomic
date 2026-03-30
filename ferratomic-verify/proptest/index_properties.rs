//! Index consistency property tests.
//!
//! Tests INV-FERR-005 (index bijection), INV-FERR-006 (snapshot isolation),
//! INV-FERR-007 (write linearizability).
//!
//! ALL TESTS MUST FAIL (red phase). Types are not yet implemented.

use ferratom::Datom;
use ferratomic_core::merge::merge;
use ferratomic_core::store::Store;
use ferratomic_verify::generators::*;
use proptest::prelude::*;
use std::collections::BTreeSet;

/// Verify index bijection: primary set == each secondary index set.
fn verify_index_bijection(store: &Store) -> bool {
    let primary: BTreeSet<&Datom> = store.datoms().collect();
    let eavt: BTreeSet<&Datom> = store.indexes().eavt_datoms().collect();
    let aevt: BTreeSet<&Datom> = store.indexes().aevt_datoms().collect();
    let vaet: BTreeSet<&Datom> = store.indexes().vaet_datoms().collect();
    let avet: BTreeSet<&Datom> = store.indexes().avet_datoms().collect();

    primary == eavt && primary == aevt && primary == vaet && primary == avet
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(10_000))]

    /// INV-FERR-005: After every transaction, all indexes match primary.
    ///
    /// Falsification: datom in primary but missing from an index, or
    /// phantom entry in index not in primary.
    #[test]
    fn inv_ferr_005_index_bijection_after_transactions(
        initial in arb_store(20),
        txns in prop::collection::vec(arb_transaction(), 0..10),
    ) {
        let mut store = initial;
        for tx in txns {
            store.transact(tx)
                .expect("INV-FERR-005: transact must succeed for committed tx");
            prop_assert!(
                verify_index_bijection(&store),
                "INV-FERR-005 violated: index bijection broken after transact. \
                 store size={}",
                store.len()
            );
        }
    }

    /// INV-FERR-005: After merge, all indexes match primary.
    #[test]
    fn inv_ferr_005_index_bijection_after_merge(
        a in arb_store(30),
        b in arb_store(30),
    ) {
        let merged = merge(&a, &b);
        prop_assert!(
            verify_index_bijection(&merged),
            "INV-FERR-005 violated: index bijection broken after merge. \
             |A|={}, |B|={}, |merged|={}",
            a.len(), b.len(), merged.len()
        );
    }

    /// INV-FERR-006: Snapshot sees no future transactions.
    ///
    /// Falsification: snapshot grows after later transactions are committed.
    #[test]
    fn inv_ferr_006_snapshot_sees_no_future_txns(
        initial_txns in prop::collection::vec(arb_transaction(), 1..5),
        later_txns in prop::collection::vec(arb_transaction(), 1..5),
    ) {
        let mut store = Store::genesis();
        for tx in initial_txns {
            store.transact(tx)
                .expect("INV-FERR-006: initial transact must succeed");
        }

        let snapshot = store.snapshot();
        let snap_datoms: BTreeSet<_> = snapshot.datoms().cloned().collect();

        for tx in later_txns {
            store.transact(tx)
                .expect("INV-FERR-006: later transact must succeed");
        }

        let snap_datoms_after: BTreeSet<_> = snapshot.datoms().cloned().collect();
        let before_len = snap_datoms.len();
        let after_len = snap_datoms_after.len();
        prop_assert_eq!(
            snap_datoms,
            snap_datoms_after,
            "INV-FERR-006 violated: snapshot changed after later transactions. \
             before={}, after={}",
            before_len,
            after_len
        );
    }

    /// INV-FERR-006: Transaction atomicity — full or nothing visibility.
    ///
    /// Falsification: reader sees subset of a transaction's datoms.
    #[test]
    fn inv_ferr_006_transaction_atomicity(
        txns in prop::collection::vec(arb_multi_datom_transaction(), 1..10),
    ) {
        let mut store = Store::genesis();
        for tx in txns {
            let tx_datoms: BTreeSet<_> = tx.datoms().iter().cloned().collect();
            store.transact(tx)
                .expect("INV-FERR-006: transact must succeed for committed tx");

            let snapshot = store.snapshot();
            let visible: BTreeSet<_> = snapshot.datoms().cloned().collect();

            let visible_count = tx_datoms.iter().filter(|d| visible.contains(d)).count();
            prop_assert!(
                visible_count == 0 || visible_count == tx_datoms.len(),
                "INV-FERR-006 violated: partial transaction visibility. \
                 {} of {} datoms visible",
                visible_count,
                tx_datoms.len()
            );
        }
    }

    /// INV-FERR-007: Committed epochs are strictly monotonically increasing.
    ///
    /// Falsification: two transactions with same epoch, or epoch decreases.
    #[test]
    fn inv_ferr_007_epochs_strictly_increase(
        txns in prop::collection::vec(arb_transaction(), 1..20),
    ) {
        let mut store = Store::genesis();
        let mut prev_epoch: Option<u64> = None;

        for tx in txns {
            let receipt = store.transact(tx)
                .expect("INV-FERR-007: transact must succeed for committed tx");
            if let Some(prev) = prev_epoch {
                prop_assert!(
                    receipt.epoch() > prev,
                    "INV-FERR-007 violated: epoch did not increase. \
                     prev={}, current={}",
                    prev,
                    receipt.epoch()
                );
            }
            prev_epoch = Some(receipt.epoch());
        }
    }
}
