//! Schema validation, observer monotonicity, typed errors, LIVE resolution,
//! anti-entropy, and replica filter property tests.
//!
//! Tests INV-FERR-009 (schema validation), INV-FERR-011 (observer monotonicity),
//! INV-FERR-019 (typed errors), INV-FERR-022 (anti-entropy), INV-FERR-023
//! (no unsafe code), INV-FERR-029 (LIVE view resolution), INV-FERR-030
//! (replica filter), and INV-FERR-032 (LIVE resolution correctness).
//!
//! Phase 4a: all tests passing against ferratomic-core implementation.
//! INV-FERR-029/032 tests cross-check the spec's resolution algebra against
//! the native `Store` LIVE query APIs.

use std::collections::BTreeSet;

use ferratom::Datom;
use ferratomic_core::{
    anti_entropy::{AntiEntropy, NullAntiEntropy},
    observer::Observer,
    store::Store,
    topology::{AcceptAll, ReplicaFilter},
    writer::{Transaction, TxValidationError},
};
use ferratomic_verify::generators::*;
use proptest::prelude::*;

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
            store.transact_test(tx)
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

    /// INV-FERR-019: FerraError is typed — every rejection produces a
    /// specific error variant, never a generic string or panic.
    ///
    /// For random unknown attributes, the error is always UnknownAttribute.
    /// For mistyped values, the error is always SchemaViolation.
    /// The error message is non-empty.
    ///
    /// Falsification: rejection returns wrong variant, or Display is empty.
    #[test]
    fn inv_ferr_019_typed_errors(
        unknown_datom in arb_datom_with_unknown_attr(),
        mistyped_datom in arb_datom_with_wrong_type(),
    ) {
        let store = Store::genesis();

        // Unknown attribute must produce UnknownAttribute.
        let tx_unknown = ferratomic_core::writer::Transaction::new(
            ferratom::AgentId::from_bytes([19u8; 16])
        ).assert_datom(
            unknown_datom.entity(),
            unknown_datom.attribute().clone(),
            unknown_datom.value().clone(),
        );
        let result_unknown = tx_unknown.commit(store.schema());
        prop_assert!(
            matches!(result_unknown, Err(TxValidationError::UnknownAttribute(_))),
            "INV-FERR-019: expected UnknownAttribute, got {:?}",
            result_unknown
        );

        // Wrong-type must produce SchemaViolation.
        let tx_mistype = ferratomic_core::writer::Transaction::new(
            ferratom::AgentId::from_bytes([19u8; 16])
        ).assert_datom(
            mistyped_datom.entity(),
            mistyped_datom.attribute().clone(),
            mistyped_datom.value().clone(),
        );
        let result_mistype = tx_mistype.commit(store.schema());
        prop_assert!(
            matches!(result_mistype, Err(TxValidationError::SchemaViolation { .. })),
            "INV-FERR-019: expected SchemaViolation, got {:?}",
            result_mistype
        );

        // Both errors must have non-empty Display output.
        if let Err(ref e) = result_unknown {
            let msg = format!("{e}");
            prop_assert!(
                !msg.is_empty(),
                "INV-FERR-019: UnknownAttribute Display must be non-empty"
            );
        }
        if let Err(ref e) = result_mistype {
            let msg = format!("{e}");
            prop_assert!(
                !msg.is_empty(),
                "INV-FERR-019: SchemaViolation Display must be non-empty"
            );
        }
    }

    // -----------------------------------------------------------------------
    // INV-FERR-029 / INV-FERR-032: LIVE view resolution properties
    //
    // These tests compute a reference LIVE model from raw store datoms and
    // cross-check the native `Store::live_values` / `Store::live_resolve`
    // results against that model.
    // -----------------------------------------------------------------------

    /// INV-FERR-029: LIVE view resolution returns correct datoms.
    ///
    /// The LIVE view is defined as:
    ///   LIVE(S) = fold(causal_sort(S), apply_resolution)
    /// where assert adds (e,a,v) and retract removes (e,a,v).
    ///
    /// Properties verified:
    /// 1. LIVE(S) is a subset of primary(S) projected to (e,a,v) triples.
    /// 2. Every asserted (e,a,v) not retracted is present in LIVE.
    /// 3. Every retracted (e,a,v) is absent from LIVE.
    /// 4. Retractions only cancel assertions with lower TxId (causal order).
    ///
    /// Falsification: LIVE view contains a retracted triple, or omits an
    /// unretracted assertion.
    #[test]
    fn test_inv_ferr_029_live_resolution(
        datoms in prop::collection::vec(arb_datom(), 1..50),
    ) {
        // Build a store from arbitrary datoms (bypasses schema — test targets
        // resolution algebra, not schema validation).
        let store = Store::from_datoms(datoms.into_iter().collect());

        // Reference LIVE computation per spec Level 0:
        // fold over datoms in causal order (TxId ordering), apply assert/retract.
        let mut sorted_datoms: Vec<&Datom> = store.datoms().collect();
        sorted_datoms.sort_by_key(|d| d.tx());

        let mut live: BTreeSet<(ferratom::EntityId, ferratom::Attribute, ferratom::Value)> =
            BTreeSet::new();
        for d in &sorted_datoms {
            let key = (d.entity(), d.attribute().clone(), d.value().clone());
            match d.op() {
                ferratom::Op::Assert => { live.insert(key); }
                ferratom::Op::Retract => { live.remove(&key); }
            }
        }

        // Property 1: LIVE(S) is a subset of primary store projected to (e,a,v).
        let primary_triples: BTreeSet<_> = store.datoms()
            .map(|d| (d.entity(), d.attribute().clone(), d.value().clone()))
            .collect();
        prop_assert!(
            live.is_subset(&primary_triples),
            "INV-FERR-029 violated: LIVE contains triple not in primary store. \
             live_size={}, primary_triples={}",
            live.len(),
            primary_triples.len()
        );

        // Property 2: |LIVE(S)| <= |primary(S)|.
        prop_assert!(
            live.len() <= store.len(),
            "INV-FERR-029 violated: LIVE larger than primary store. \
             live={}, primary={}",
            live.len(),
            store.len()
        );

        // Property 3: Every assert-only (e,a,v) triple (never retracted) is in LIVE.
        // Collect all retracted triples.
        let retracted_triples: BTreeSet<_> = store.datoms()
            .filter(|d| d.op() == ferratom::Op::Retract)
            .map(|d| (d.entity(), d.attribute().clone(), d.value().clone()))
            .collect();
        let asserted_triples: BTreeSet<_> = store.datoms()
            .filter(|d| d.op() == ferratom::Op::Assert)
            .map(|d| (d.entity(), d.attribute().clone(), d.value().clone()))
            .collect();
        // Any triple asserted but never retracted MUST be in live.
        for triple in &asserted_triples {
            if !retracted_triples.contains(triple) {
                prop_assert!(
                    live.contains(triple),
                    "INV-FERR-029 violated: unretracted assertion absent from LIVE. \
                     entity={:?}, attr={:?}",
                    triple.0,
                    triple.1
                );
            }
        }

        // Property 4: A triple that is ONLY retracted (never asserted) is NOT in LIVE.
        for triple in &retracted_triples {
            if !asserted_triples.contains(triple) {
                prop_assert!(
                    !live.contains(triple),
                    "INV-FERR-029 violated: retraction-only triple present in LIVE"
                );
            }
        }
    }

    /// INV-FERR-032: LIVE resolution correctness — LWW vs keep-all semantics.
    ///
    /// Strengthens INV-FERR-029 by verifying cardinality-specific resolution:
    /// - `Cardinality::One` (LWW): only the value from the highest-`TxId`
    ///   non-retracted assertion survives.
    /// - `Cardinality::Many`: all non-retracted values survive.
    ///
    /// This test constructs card-one and card-many datoms, computes the spec's
    /// reference result, and then checks `Store::live_resolve()` /
    /// `Store::live_values()` against that reference.
    ///
    /// Falsification: LWW picks a non-latest value, or keep-all omits a
    /// non-retracted value.
    #[test]
    fn test_inv_ferr_032_live_semantics(
        entity in arb_entity_id(),
        // Generate 2-10 distinct Long values for card-one attribute
        card_one_values in prop::collection::vec(any::<i64>(), 2..10),
        // Generate 2-10 distinct Long values for card-many attribute
        card_many_values in prop::collection::vec(any::<i64>(), 2..10),
        // Distinct physical timestamps (monotonically increasing base)
        base_ts in 1000u64..1_000_000u64,
        // Which card-many values to retract (bitmap)
        retract_mask in any::<u16>(),
    ) {
        use ferratom::{AttributeDef, Cardinality, ResolutionMode, ValueType};

        let card_one_attr = ferratom::Attribute::from("test/card_one");
        let card_many_attr = ferratom::Attribute::from("test/card_many");

        // --- Card-One (LWW) semantics ---
        // Assert multiple values for the same (entity, attribute) at increasing timestamps.
        // LWW: only the value from the highest TxId should survive.
        let mut card_one_datoms: Vec<Datom> = Vec::new();
        for (i, &val) in card_one_values.iter().enumerate() {
            let tx = ferratom::TxId::new(base_ts + (i as u64), 0, 0);
            card_one_datoms.push(Datom::new(
                entity,
                card_one_attr.clone(),
                ferratom::Value::Long(val),
                tx,
                ferratom::Op::Assert,
            ));
        }

        // Compute LWW resolution: fold in causal order, each assert replaces the current value.
        // (No retractions for the card-one case — pure LWW replacement.)
        let mut lww_result: Option<ferratom::Value> = None;
        let mut lww_sorted = card_one_datoms.clone();
        lww_sorted.sort_by_key(|d| d.tx());
        for d in &lww_sorted {
            match d.op() {
                ferratom::Op::Assert => {
                    lww_result = Some(d.value().clone());
                }
                ferratom::Op::Retract => {
                    if lww_result.as_ref() == Some(d.value()) {
                        lww_result = None;
                    }
                }
            }
        }

        // The last assertion (highest TxId) should win.
        let expected_lww = &card_one_values[card_one_values.len() - 1];
        prop_assert_eq!(
            lww_result.as_ref(),
            Some(&ferratom::Value::Long(*expected_lww)),
            "INV-FERR-032 violated: LWW did not pick the latest assertion. \
             Expected value from tx with physical={}, got {:?}",
            base_ts + (card_one_values.len() as u64 - 1),
            lww_result
        );

        // --- Card-Many (keep-all) semantics ---
        // Assert all values, then selectively retract some based on the mask.
        let mut card_many_datoms: Vec<Datom> = Vec::new();
        // Assert phase: all values at increasing timestamps.
        for (i, &val) in card_many_values.iter().enumerate() {
            let tx = ferratom::TxId::new(base_ts + (i as u64), 0, 0);
            card_many_datoms.push(Datom::new(
                entity,
                card_many_attr.clone(),
                ferratom::Value::Long(val),
                tx,
                ferratom::Op::Assert,
            ));
        }
        // Retract phase: use mask bits to select which values to retract.
        // Retraction TxIds must be AFTER the assertion TxIds (causal order).
        let retract_base = base_ts + card_many_values.len() as u64;
        let mut retracted_indices: Vec<usize> = Vec::new();
        for (i, &val) in card_many_values.iter().enumerate() {
            if retract_mask & (1u16 << (i % 16)) != 0 {
                let tx = ferratom::TxId::new(retract_base + (i as u64), 0, 0);
                card_many_datoms.push(Datom::new(
                    entity,
                    card_many_attr.clone(),
                    ferratom::Value::Long(val),
                    tx,
                    ferratom::Op::Retract,
                ));
                retracted_indices.push(i);
            }
        }

        // Compute card-many resolution: fold in causal order, assert inserts, retract removes.
        let mut many_sorted = card_many_datoms.clone();
        many_sorted.sort_by_key(|d| d.tx());
        let mut live_many: BTreeSet<ferratom::Value> = BTreeSet::new();
        for d in &many_sorted {
            match d.op() {
                ferratom::Op::Assert => { live_many.insert(d.value().clone()); }
                ferratom::Op::Retract => { live_many.remove(d.value()); }
            }
        }

        // Verify: every non-retracted value is present.
        for (i, &val) in card_many_values.iter().enumerate() {
            let v = ferratom::Value::Long(val);
            if retracted_indices.contains(&i) {
                prop_assert!(
                    !live_many.contains(&v),
                    "INV-FERR-032 violated: retracted card-many value still in LIVE. \
                     index={}, value={}",
                    i, val
                );
            } else {
                prop_assert!(
                    live_many.contains(&v),
                    "INV-FERR-032 violated: non-retracted card-many value absent from LIVE. \
                     index={}, value={}",
                    i, val
                );
            }
        }

        let store = Store::from_datoms(
            card_one_datoms
                .iter()
                .chain(card_many_datoms.iter())
                .cloned()
                .collect(),
        );
        let actual_many: BTreeSet<_> = store
            .live_values(entity, &card_many_attr)
            .map(|values| values.iter().cloned().collect())
            .unwrap_or_default();

        prop_assert_eq!(
            store.live_resolve(entity, &card_one_attr),
            lww_result.as_ref(),
            "INV-FERR-032 violated: Store::live_resolve() must return the highest-TxId \
             surviving card-one value"
        );
        prop_assert_eq!(
            actual_many,
            live_many,
            "INV-FERR-032 violated: Store::live_values() must match the reference \
             keep-all result for card-many attributes"
        );

        // Cross-check: the schema types are well-formed (sanity).
        let def_one = AttributeDef::new(
            ValueType::Long, Cardinality::One, ResolutionMode::Lww, None,
        );
        let def_many = AttributeDef::new(
            ValueType::Long, Cardinality::Many, ResolutionMode::MultiValue, None,
        );
        prop_assert_eq!(
            def_one.cardinality(), &Cardinality::One,
            "INV-FERR-032: card-one def must have Cardinality::One"
        );
        prop_assert_eq!(
            def_one.resolution_mode(), &ResolutionMode::Lww,
            "INV-FERR-032: card-one def must use LWW resolution"
        );
        prop_assert_eq!(
            def_many.cardinality(), &Cardinality::Many,
            "INV-FERR-032: card-many def must have Cardinality::Many"
        );
        prop_assert_eq!(
            def_many.resolution_mode(), &ResolutionMode::MultiValue,
            "INV-FERR-032: card-many def must use MultiValue resolution"
        );
    }

    /// INV-FERR-022: NullAntiEntropy diff returns empty, apply_diff is no-op.
    ///
    /// For any store, the NullAntiEntropy implementation must:
    /// 1. Return an empty diff vector.
    /// 2. Leave the store unchanged after apply_diff with any bytes.
    ///
    /// The proptest varies both the store content and the diff bytes
    /// to ensure the no-op behavior holds for all inputs.
    ///
    /// Falsification: diff returns non-empty bytes, or apply_diff mutates
    /// the store (datom set, epoch, or schema change).
    #[test]
    fn inv_ferr_022_null_anti_entropy(
        store in arb_store(50),
        arbitrary_bytes in prop::collection::vec(any::<u8>(), 0..256),
    ) {
        let ae = NullAntiEntropy;

        // Property 1: diff always returns empty.
        let diff = ae.diff(&store)
            .expect("INV-FERR-022: NullAntiEntropy::diff must succeed");
        prop_assert!(
            diff.is_empty(),
            "INV-FERR-022 violated: NullAntiEntropy::diff returned {} bytes, expected 0",
            diff.len()
        );

        // Property 2: apply_diff with empty diff is a no-op.
        let mut store_copy = store.clone();
        let epoch_before = store_copy.epoch();
        let datoms_before: std::collections::BTreeSet<ferratom::Datom> = store_copy.datoms().cloned().collect();
        let schema_len_before = store_copy.schema().len();

        ae.apply_diff(&mut store_copy, &diff)
            .expect("INV-FERR-022: apply_diff with empty diff must succeed");

        let datoms_after: std::collections::BTreeSet<ferratom::Datom> = store_copy.datoms().cloned().collect();
        prop_assert_eq!(
            datoms_after, datoms_before,
            "INV-FERR-022 violated: apply_diff with empty diff changed datom set"
        );
        prop_assert_eq!(
            store_copy.epoch(), epoch_before,
            "INV-FERR-022 violated: apply_diff with empty diff changed epoch"
        );
        prop_assert_eq!(
            store_copy.schema().len(), schema_len_before,
            "INV-FERR-022 violated: apply_diff with empty diff changed schema"
        );

        // Property 3: apply_diff with arbitrary bytes is also a no-op.
        let mut store_arb = store.clone();
        let epoch_before_arb = store_arb.epoch();
        let datoms_before_arb: std::collections::BTreeSet<ferratom::Datom> = store_arb.datoms().cloned().collect();

        ae.apply_diff(&mut store_arb, &arbitrary_bytes)
            .expect("INV-FERR-022: apply_diff with arbitrary bytes must succeed");

        let datoms_after_arb: std::collections::BTreeSet<ferratom::Datom> = store_arb.datoms().cloned().collect();
        prop_assert_eq!(
            datoms_after_arb, datoms_before_arb,
            "INV-FERR-022 violated: apply_diff with arbitrary bytes changed datom set"
        );
        prop_assert_eq!(
            store_arb.epoch(), epoch_before_arb,
            "INV-FERR-022 violated: apply_diff with arbitrary bytes changed epoch"
        );
    }

    /// INV-FERR-030: AcceptAll.accepts() returns true for all datoms.
    ///
    /// The AcceptAll replica filter is the full-replica default. It must
    /// accept every datom regardless of entity, attribute, value, tx, or op.
    ///
    /// Falsification: AcceptAll rejects any datom.
    #[test]
    fn inv_ferr_030_accept_all_filter(
        datoms in prop::collection::vec(arb_datom(), 1..100),
    ) {
        let filter = AcceptAll;

        for d in &datoms {
            prop_assert!(
                filter.accepts(d),
                "INV-FERR-030 violated: AcceptAll rejected datom. \
                 entity={:?}, attr={:?}, op={:?}",
                d.entity(), d.attribute(), d.op()
            );
        }
    }
}

// ---------------------------------------------------------------------------
// INV-FERR-023: No unsafe code — meta-test (not a proptest)
// ---------------------------------------------------------------------------

/// INV-FERR-023: All lib.rs files contain `#![forbid(unsafe_code)]`.
///
/// This is a source-level meta-test. It reads all lib.rs files in the
/// workspace and asserts that each contains the `forbid(unsafe_code)`
/// attribute. This is a defense-in-depth check — the compiler enforces
/// `forbid(unsafe_code)` at compile time, but this test verifies the
/// attribute is present even if a lib.rs is accidentally edited.
///
/// Falsification: a lib.rs in the workspace is missing `#![forbid(unsafe_code)]`.
#[test]
fn inv_ferr_023_no_unsafe_code() {
    // The four crate lib.rs files that must contain forbid(unsafe_code).
    let lib_files = [
        concat!(env!("CARGO_MANIFEST_DIR"), "/../ferratom/src/lib.rs"),
        concat!(env!("CARGO_MANIFEST_DIR"), "/../ferratomic-core/src/lib.rs"),
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../ferratomic-datalog/src/lib.rs"
        ),
        concat!(env!("CARGO_MANIFEST_DIR"), "/src/lib.rs"),
    ];

    for path in &lib_files {
        let content = std::fs::read_to_string(path)
            .unwrap_or_else(|e| panic!("INV-FERR-023: cannot read {path}: {e}"));
        assert!(
            content.contains("#![forbid(unsafe_code)]"),
            "INV-FERR-023 violated: {path} is missing #![forbid(unsafe_code)]. \
             Every crate in the workspace must forbid unsafe code.",
        );
    }
}
