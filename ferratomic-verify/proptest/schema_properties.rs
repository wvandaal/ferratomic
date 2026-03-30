//! Schema validation and observer monotonicity property tests.
//!
//! Tests INV-FERR-009 (schema validation) and INV-FERR-011 (observer monotonicity).
//!
//! ALL TESTS MUST FAIL (red phase). Types are not yet implemented.

use ferratom::Datom;
use ferratomic_core::observer::Observer;
use ferratomic_core::store::Store;
use ferratomic_core::writer::{Transaction, TxValidationError};
use ferratomic_verify::generators::*;
use proptest::prelude::*;
use std::collections::BTreeSet;

proptest! {
    #![proptest_config(ProptestConfig::with_cases(10_000))]

    /// INV-FERR-009: Valid datoms (matching schema) are accepted.
    ///
    /// Falsification: datom with known attribute and correct type is rejected.
    #[test]
    fn inv_ferr_009_valid_datoms_accepted(
        datoms in prop::collection::vec(arb_schema_valid_datom(), 1..10),
    ) {
        let store = Store::genesis();
        let tx = datoms.into_iter().fold(
            Transaction::new(store.genesis_agent()),
            |tx, d| tx.assert_datom(d.entity(), d.attribute().clone(), d.value().clone()),
        );
        let result = tx.commit(store.schema());
        prop_assert!(
            result.is_ok(),
            "INV-FERR-009 violated: valid datoms were rejected: {:?}",
            result.err()
        );
    }

    /// INV-FERR-009: Datoms with unknown attributes are rejected.
    ///
    /// Falsification: datom with nonexistent attribute passes validation.
    #[test]
    fn inv_ferr_009_invalid_attr_rejected(
        datom in arb_datom_with_unknown_attr(),
    ) {
        let store = Store::genesis();
        let tx = Transaction::new(store.genesis_agent())
            .assert_datom(datom.entity(), datom.attribute().clone(), datom.value().clone());
        let result = tx.commit(store.schema());
        prop_assert!(
            matches!(result, Err(TxValidationError::UnknownAttribute(_))),
            "INV-FERR-009 violated: datom with unknown attribute was accepted"
        );
    }

    /// INV-FERR-009: Datoms with mistyped values are rejected.
    ///
    /// Falsification: datom with wrong value type passes validation.
    #[test]
    fn inv_ferr_009_mistyped_value_rejected(
        datom in arb_datom_with_wrong_type(),
    ) {
        let store = Store::genesis();
        let tx = Transaction::new(store.genesis_agent())
            .assert_datom(datom.entity(), datom.attribute().clone(), datom.value().clone());
        let result = tx.commit(store.schema());
        prop_assert!(
            matches!(result, Err(TxValidationError::SchemaViolation { .. })),
            "INV-FERR-009 violated: datom with wrong value type was accepted"
        );
    }

    /// INV-FERR-011: Observer epoch never regresses.
    ///
    /// Falsification: observer sees epoch e₂ < e₁ after seeing e₁.
    #[test]
    fn inv_ferr_011_observer_never_regresses(
        txns in prop::collection::vec(arb_transaction(), 1..20),
        observe_points in prop::collection::vec(0..20usize, 1..10),
        observer_agent in arb_agent_id(),
    ) {
        let mut store = Store::genesis();
        let observer = Observer::new(observer_agent);
        let mut prev_epoch: Option<u64> = None;
        let mut prev_datoms: Option<BTreeSet<Datom>> = None;

        for (i, tx) in txns.into_iter().enumerate() {
            store.transact(tx)
                .expect("INV-FERR-011: transact must succeed for committed tx");

            if observe_points.contains(&i) {
                let snap = observer.observe(&store);
                let epoch = snap.epoch();
                let datoms: BTreeSet<_> = snap.datoms().cloned().collect();

                if let Some(prev_e) = prev_epoch {
                    prop_assert!(
                        epoch >= prev_e,
                        "INV-FERR-011 violated: observer epoch regressed. \
                         prev={}, current={}",
                        prev_e, epoch
                    );
                }
                if let Some(ref prev_d) = prev_datoms {
                    prop_assert!(
                        prev_d.is_subset(&datoms),
                        "INV-FERR-011 violated: observer lost datoms. \
                         prev_size={}, current_size={}",
                        prev_d.len(),
                        datoms.len()
                    );
                }

                prev_epoch = Some(epoch);
                prev_datoms = Some(datoms);
            }
        }
    }
}
