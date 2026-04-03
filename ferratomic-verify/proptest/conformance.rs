//! CI-FERR-001 conformance bridge: abstract Lean laws vs concrete Rust behavior.
//!
//! These properties do not re-prove the algebraic laws. Lean already does that.
//! Instead, they verify that the Rust implementation matches the abstract
//! set-theoretic predictions for the same operations.

use std::collections::BTreeSet;

use ferratom::Datom;
use ferratomic_core::{merge::merge, store::Store};
use ferratomic_verify::generators::*;
use proptest::{prelude::*, test_runner::TestCaseError};

/// A single conformance vector pairing an abstract operation with its Lean theorem.
struct ConformanceCase {
    operation: &'static str,
    lean_theorem: &'static str,
    invariant: &'static str,
}

/// Forget the concrete storage representation and view a `Store` as a mathematical set.
fn abstract_datoms(store: &Store) -> BTreeSet<Datom> {
    store.datoms().cloned().collect()
}

/// Lean model: `merge = union`.
fn abstract_merge(left: &BTreeSet<Datom>, right: &BTreeSet<Datom>) -> BTreeSet<Datom> {
    left.union(right).cloned().collect()
}

/// Lean model: `apply_tx = insert`.
fn abstract_apply(store: &BTreeSet<Datom>, datom: &Datom) -> BTreeSet<Datom> {
    let mut next = store.clone();
    next.insert(datom.clone());
    next
}

/// Assert that a concrete Rust store matches the abstract Lean prediction.
fn assert_rust_matches_prediction(
    case: &ConformanceCase,
    rust_store: &Store,
    expected: &BTreeSet<Datom>,
) -> Result<(), TestCaseError> {
    let actual = abstract_datoms(rust_store);
    prop_assert_eq!(
        &actual,
        expected,
        "{} violated: Rust operation '{}' diverged from Lean theorem '{}'",
        case.invariant,
        case.operation,
        case.lean_theorem
    );
    Ok(())
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(10_000))]

    /// CI-FERR-001 bridge for Lean theorem `merge_comm`.
    #[test]
    fn ci_ferr_001_merge_comm_conformance(
        a in arb_store(12),
        b in arb_store(12),
    ) {
        let case = ConformanceCase {
            operation: "merge(A, B)",
            lean_theorem: "merge_comm",
            invariant: "CI-FERR-001",
        };

        let abstract_a = abstract_datoms(&a);
        let abstract_b = abstract_datoms(&b);
        let expected_ab = abstract_merge(&abstract_a, &abstract_b);
        let expected_ba = abstract_merge(&abstract_b, &abstract_a);

        prop_assert_eq!(
            &expected_ab,
            &expected_ba,
            "{} violated before Rust comparison: Lean theorem '{}' predicts equal unions",
            case.invariant,
            case.lean_theorem
        );

        let rust_ab = merge(&a, &b)
            .expect("CI-FERR-001 / merge_comm: merge(A,B) must succeed");
        let rust_ba = merge(&b, &a)
            .expect("CI-FERR-001 / merge_comm: merge(B,A) must succeed");

        assert_rust_matches_prediction(&case, &rust_ab, &expected_ab)?;
        assert_rust_matches_prediction(&case, &rust_ba, &expected_ba)?;
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(10_000))]

    /// CI-FERR-001 bridge for Lean theorem `merge_assoc`.
    #[test]
    fn ci_ferr_001_merge_assoc_conformance(
        a in arb_store(8),
        b in arb_store(8),
        c in arb_store(8),
    ) {
        let case = ConformanceCase {
            operation: "merge(merge(A, B), C)",
            lean_theorem: "merge_assoc",
            invariant: "CI-FERR-001",
        };

        let abstract_a = abstract_datoms(&a);
        let abstract_b = abstract_datoms(&b);
        let abstract_c = abstract_datoms(&c);
        let expected_left = abstract_merge(&abstract_merge(&abstract_a, &abstract_b), &abstract_c);
        let expected_right = abstract_merge(&abstract_a, &abstract_merge(&abstract_b, &abstract_c));

        prop_assert_eq!(
            &expected_left,
            &expected_right,
            "{} violated before Rust comparison: Lean theorem '{}' predicts regrouping invariance",
            case.invariant,
            case.lean_theorem
        );

        let rust_left = merge(
            &merge(&a, &b).expect("CI-FERR-001 / merge_assoc: merge(A,B) must succeed"),
            &c,
        )
        .expect("CI-FERR-001 / merge_assoc: merge(merge(A,B),C) must succeed");
        let rust_right = merge(
            &a,
            &merge(&b, &c).expect("CI-FERR-001 / merge_assoc: merge(B,C) must succeed"),
        )
        .expect("CI-FERR-001 / merge_assoc: merge(A,merge(B,C)) must succeed");

        assert_rust_matches_prediction(&case, &rust_left, &expected_left)?;
        assert_rust_matches_prediction(&case, &rust_right, &expected_right)?;
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(10_000))]

    /// CI-FERR-001 bridge for Lean theorem `merge_idemp`.
    #[test]
    fn ci_ferr_001_merge_idemp_conformance(
        store in arb_store(12),
    ) {
        let case = ConformanceCase {
            operation: "merge(A, A)",
            lean_theorem: "merge_idemp",
            invariant: "CI-FERR-001",
        };

        let abstract_store = abstract_datoms(&store);
        let expected = abstract_merge(&abstract_store, &abstract_store);

        prop_assert_eq!(
            &expected,
            &abstract_store,
            "{} violated before Rust comparison: Lean theorem '{}' predicts self-merge is identity",
            case.invariant,
            case.lean_theorem
        );

        let rust_merged = merge(&store, &store)
            .expect("CI-FERR-001 / merge_idemp: self-merge must succeed");

        assert_rust_matches_prediction(&case, &rust_merged, &expected)?;
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(10_000))]

    /// CI-FERR-001 bridge for Lean monotonic-growth theorems
    /// `apply_superset` and `apply_monotone` (spec alias: monotonic growth).
    #[test]
    fn ci_ferr_001_apply_superset_conformance(
        store in arb_store(12),
        datom in arb_datom(),
    ) {
        let case = ConformanceCase {
            operation: "apply_tx(S, d)",
            lean_theorem: "apply_superset / apply_monotone",
            invariant: "CI-FERR-001",
        };

        let before = abstract_datoms(&store);
        let expected = abstract_apply(&before, &datom);

        prop_assert!(
            before.is_subset(&expected),
            "{} violated before Rust comparison: Lean theorem '{}' predicts no datom loss",
            case.invariant,
            case.lean_theorem
        );
        prop_assert!(
            expected.len() >= before.len(),
            "{} violated before Rust comparison: Lean theorem '{}' predicts non-decreasing cardinality",
            case.invariant,
            case.lean_theorem
        );

        let mut rust_store = store;
        rust_store.insert(&datom);

        assert_rust_matches_prediction(&case, &rust_store, &expected)?;
    }
}
