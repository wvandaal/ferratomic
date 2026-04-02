//! Anti-entropy Kani harnesses.
//!
//! Covers INV-FERR-022: NullAntiEntropy diff returns empty,
//! apply_diff is a no-op.

use ferratomic_core::{
    anti_entropy::{AntiEntropy, NullAntiEntropy},
    store::Store,
};

/// INV-FERR-022: NullAntiEntropy::diff always returns an empty vec.
///
/// For any store (genesis or with datoms), the null implementation
/// must produce an empty diff. This is the identity element for
/// the anti-entropy protocol.
#[cfg_attr(kani, kani::proof)]
#[cfg_attr(kani, kani::unwind(4))]
#[cfg_attr(not(kani), test)]
#[cfg_attr(not(kani), ignore = "requires Kani verifier")]
fn null_anti_entropy_diff_empty() {
    let store = Store::genesis();
    let ae = NullAntiEntropy;

    let diff = ae
        .diff(&store)
        .expect("INV-FERR-022: NullAntiEntropy::diff must succeed");

    assert!(
        diff.is_empty(),
        "INV-FERR-022: NullAntiEntropy::diff must return empty vec, got {} bytes",
        diff.len()
    );
}

/// INV-FERR-022: NullAntiEntropy::apply_diff is a no-op.
///
/// Applying any diff (empty or non-empty) via NullAntiEntropy must
/// not mutate the store. The epoch, datom set, and schema remain
/// identical before and after apply_diff.
#[cfg_attr(kani, kani::proof)]
#[cfg_attr(kani, kani::unwind(4))]
#[cfg_attr(not(kani), test)]
#[cfg_attr(not(kani), ignore = "requires Kani verifier")]
fn null_anti_entropy_identity() {
    let mut store = Store::genesis();
    let epoch_before = store.epoch();
    let datoms_before = store.datom_set().clone();
    let ae = NullAntiEntropy;

    // Apply an empty diff.
    ae.apply_diff(&mut store, &[])
        .expect("INV-FERR-022: apply_diff on empty diff must succeed");

    assert_eq!(
        store.epoch(),
        epoch_before,
        "INV-FERR-022: NullAntiEntropy must not change epoch"
    );
    assert_eq!(
        *store.datom_set(),
        datoms_before,
        "INV-FERR-022: NullAntiEntropy must not change datom set"
    );

    // Apply a non-empty diff (arbitrary bytes).
    ae.apply_diff(&mut store, &[0xDE, 0xAD, 0xBE, 0xEF])
        .expect("INV-FERR-022: apply_diff on arbitrary bytes must succeed");

    assert_eq!(
        store.epoch(),
        epoch_before,
        "INV-FERR-022: NullAntiEntropy must not change epoch (non-empty diff)"
    );
    assert_eq!(
        *store.datom_set(),
        datoms_before,
        "INV-FERR-022: NullAntiEntropy must not change datom set (non-empty diff)"
    );
}

/// INV-FERR-022: NullAntiEntropy round-trip is identity.
///
/// diff followed by apply_diff leaves the store unchanged.
#[cfg_attr(kani, kani::proof)]
#[cfg_attr(kani, kani::unwind(4))]
#[cfg_attr(not(kani), test)]
#[cfg_attr(not(kani), ignore = "requires Kani verifier")]
fn null_anti_entropy_roundtrip() {
    let mut store = Store::genesis();
    let ae = NullAntiEntropy;

    let diff = ae.diff(&store).expect("INV-FERR-022: diff must succeed");
    ae.apply_diff(&mut store, &diff)
        .expect("INV-FERR-022: apply_diff must succeed");

    // Store must be identical to genesis after round-trip.
    let fresh = Store::genesis();
    assert_eq!(
        store.datom_set(),
        fresh.datom_set(),
        "INV-FERR-022: round-trip must not mutate store"
    );
    assert_eq!(
        store.epoch(),
        fresh.epoch(),
        "INV-FERR-022: round-trip must preserve epoch"
    );
}
