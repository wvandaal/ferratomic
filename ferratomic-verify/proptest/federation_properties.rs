//! Federation foundation property tests (Phase 4a.5).
//!
//! Tests INV-FERR-063 (provenance lattice total order),
//! INV-FERR-039 (DatomFilter monotonicity and composition),
//! INV-FERR-051 (signing type round-trips),
//! ADR-FERR-025 (SignedTransactionBundle extraction).
//!
//! Implementation-level invariants (INV-FERR-060, 061, 062, 086) require
//! functions that do not yet exist (genesis_with_identity, emit_predecessors,
//! selective_merge, canonical_bytes). Those tests are stubbed with markers
//! indicating which bead must ship before they can be uncommented.

use std::sync::Arc;

use ferratom::{
    Attribute, Datom, DatomFilter, EntityId, Op, ProvenanceType, SignedTransactionBundle,
    TxSignature, TxSigner, Value,
};
use ferratomic_verify::generators::*;
use proptest::prelude::*;

proptest! {
    #![proptest_config(ProptestConfig::with_cases(10_000))]

    // ═══════════════════════════════════════════════════════════════════
    // INV-FERR-063: Provenance Lattice Total Order
    // ═══════════════════════════════════════════════════════════════════

    /// INV-FERR-063: Total order — every pair is comparable.
    #[test]
    fn inv_ferr_063_provenance_total_order(
        a in arb_provenance_type(),
        b in arb_provenance_type(),
    ) {
        prop_assert!(
            a <= b || b <= a,
            "INV-FERR-063: ProvenanceType must be totally ordered, \
             but {a:?} and {b:?} are incomparable"
        );
    }

    /// INV-FERR-063: Transitivity — if a <= b and b <= c, then a <= c.
    #[test]
    fn inv_ferr_063_provenance_transitivity(
        a in arb_provenance_type(),
        b in arb_provenance_type(),
        c in arb_provenance_type(),
    ) {
        if a <= b && b <= c {
            prop_assert!(
                a <= c,
                "INV-FERR-063: transitivity violated: {a:?} <= {b:?} <= {c:?} but {a:?} > {c:?}"
            );
        }
    }

    /// INV-FERR-063: Antisymmetry — if a <= b and b <= a, then a == b.
    #[test]
    fn inv_ferr_063_provenance_antisymmetry(
        a in arb_provenance_type(),
        b in arb_provenance_type(),
    ) {
        if a <= b && b <= a {
            prop_assert_eq!(a, b, "INV-FERR-063: antisymmetry violated");
        }
    }

    /// INV-FERR-063: Confidence is monotone with ordering.
    #[test]
    fn inv_ferr_063_confidence_monotone(
        a in arb_provenance_type(),
        b in arb_provenance_type(),
    ) {
        if a <= b {
            prop_assert!(
                a.confidence() <= b.confidence(),
                "INV-FERR-063: confidence must be monotone with ordering. \
                 {a:?}({}) <= {b:?}({}) in order but not in confidence",
                a.confidence(), b.confidence()
            );
        }
    }

    /// INV-FERR-063: Keyword round-trip for all variants.
    #[test]
    fn inv_ferr_063_keyword_round_trip(prov in arb_provenance_type()) {
        let kw = prov.as_keyword();
        let recovered = ProvenanceType::from_keyword(kw);
        prop_assert_eq!(recovered, Some(prov),
            "INV-FERR-063: keyword round-trip failed");
    }

    // ═══════════════════════════════════════════════════════════════════
    // INV-FERR-039 / ADR-FERR-022: DatomFilter Correctness
    // ═══════════════════════════════════════════════════════════════════

    /// INV-FERR-039: DatomFilter::All matches every datom.
    #[test]
    fn inv_ferr_039_filter_all_matches_everything(d in arb_datom()) {
        prop_assert!(
            DatomFilter::All.matches(&d),
            "INV-FERR-039: DatomFilter::All must match every datom"
        );
    }

    /// INV-FERR-039: And(empty) is vacuously true.
    #[test]
    fn inv_ferr_039_empty_and_is_true(d in arb_datom()) {
        let filter = DatomFilter::And(vec![]);
        prop_assert!(
            filter.matches(&d),
            "INV-FERR-039: And([]) must be vacuously true"
        );
    }

    /// INV-FERR-039: Or(empty) is vacuously false.
    #[test]
    fn inv_ferr_039_empty_or_is_false(d in arb_datom()) {
        let filter = DatomFilter::Or(vec![]);
        prop_assert!(
            !filter.matches(&d),
            "INV-FERR-039: Or([]) must be vacuously false"
        );
    }

    /// INV-FERR-039: And(All, f) == f for any filter f.
    #[test]
    fn inv_ferr_039_and_identity(
        d in arb_datom(),
        f in arb_datom_filter(),
    ) {
        let combined = DatomFilter::And(vec![DatomFilter::All, f.clone()]);
        prop_assert_eq!(
            combined.matches(&d),
            f.matches(&d),
            "INV-FERR-039: And(All, f) must equal f"
        );
    }

    /// INV-FERR-039: Or(All, f) == All for any filter f.
    #[test]
    fn inv_ferr_039_or_absorbs_all(
        d in arb_datom(),
        f in arb_datom_filter(),
    ) {
        let combined = DatomFilter::Or(vec![DatomFilter::All, f]);
        prop_assert!(
            combined.matches(&d),
            "INV-FERR-039: Or(All, f) must match everything"
        );
    }

    /// INV-FERR-044: AttributeNamespace matches only datoms with
    /// matching prefixes.
    #[test]
    fn inv_ferr_044_namespace_soundness(
        d in arb_datom(),
        prefix in "[a-z]{1,5}/",
    ) {
        let filter = DatomFilter::AttributeNamespace(vec![prefix.clone()]);
        if filter.matches(&d) {
            prop_assert!(
                d.attribute().as_str().starts_with(&prefix),
                "INV-FERR-044: namespace filter matched but attribute '{}' \
                 doesn't start with prefix '{prefix}'",
                d.attribute().as_str()
            );
        }
    }

    // ═══════════════════════════════════════════════════════════════════
    // INV-FERR-051: Signing Type Properties
    // ═══════════════════════════════════════════════════════════════════

    /// INV-FERR-051: TxSignature round-trip through Value::Bytes.
    #[test]
    fn inv_ferr_051_signature_value_round_trip(sig in arb_tx_signature()) {
        let val: Value = sig.into();
        let recovered = TxSignature::try_from(&val);
        prop_assert!(
            recovered.is_ok(),
            "INV-FERR-051: TxSignature must round-trip through Value"
        );
        prop_assert_eq!(
            recovered.as_ref().map(TxSignature::as_bytes),
            Ok(sig.as_bytes()),
            "INV-FERR-051: TxSignature bytes must be preserved"
        );
    }

    /// INV-FERR-051: TxSigner round-trip through Value::Bytes.
    #[test]
    fn inv_ferr_051_signer_value_round_trip(signer in arb_tx_signer()) {
        let val: Value = signer.into();
        let recovered = TxSigner::try_from(&val);
        prop_assert!(
            recovered.is_ok(),
            "INV-FERR-051: TxSigner must round-trip through Value"
        );
        prop_assert_eq!(
            recovered.as_ref().map(TxSigner::as_bytes),
            Ok(signer.as_bytes()),
            "INV-FERR-051: TxSigner bytes must be preserved"
        );
    }

    // ═══════════════════════════════════════════════════════════════════
    // ADR-FERR-025: SignedTransactionBundle Extraction
    // ═══════════════════════════════════════════════════════════════════

    /// ADR-FERR-025: Bundle extraction excludes all tx/* metadata datoms.
    #[test]
    fn inv_ferr_025_bundle_excludes_tx_metadata(
        user_datoms in prop::collection::vec(arb_datom(), 1..10),
        tx_id in arb_tx_id(),
    ) {
        // Build a mixed set: user datoms + some tx/* metadata
        let entity = EntityId::from_content(b"tx-entity");
        let mut all_datoms = user_datoms;
        all_datoms.push(Datom::new(
            entity,
            Attribute::from("tx/time"),
            Value::Instant(12345),
            tx_id,
            Op::Assert,
        ));
        all_datoms.push(Datom::new(
            entity,
            Attribute::from("tx/origin"),
            Value::Bytes(Arc::from([0u8; 16].as_slice())),
            tx_id,
            Op::Assert,
        ));

        let bundle = SignedTransactionBundle::from_store_datoms(&all_datoms, tx_id);

        for d in bundle.datoms() {
            prop_assert!(
                !d.attribute().as_str().starts_with("tx/"),
                "ADR-FERR-025: bundle user datoms must not contain tx/* metadata, \
                 found '{}'", d.attribute().as_str()
            );
        }
    }

    /// ADR-FERR-025: Bundle extraction preserves signature bytes exactly.
    #[test]
    fn inv_ferr_051_bundle_preserves_signature(
        sig_bytes in any::<[u8; 64]>(),
        signer_bytes in any::<[u8; 32]>(),
        tx_id in arb_tx_id(),
    ) {
        let entity = EntityId::from_content(b"tx-entity");
        let datoms = vec![
            Datom::new(entity, Attribute::from("user/data"), Value::Long(1), tx_id, Op::Assert),
            Datom::new(entity, Attribute::from("tx/signature"),
                Value::Bytes(Arc::from(sig_bytes.as_slice())), tx_id, Op::Assert),
            Datom::new(entity, Attribute::from("tx/signer"),
                Value::Bytes(Arc::from(signer_bytes.as_slice())), tx_id, Op::Assert),
        ];

        let bundle = SignedTransactionBundle::from_store_datoms(&datoms, tx_id);

        prop_assert!(bundle.is_signed(), "INV-FERR-051: bundle must be signed");
        prop_assert_eq!(
            bundle.signature().map(TxSignature::as_bytes), Some(&sig_bytes),
            "INV-FERR-051: signature bytes must be preserved exactly"
        );
        prop_assert_eq!(
            bundle.signer().map(TxSigner::as_bytes), Some(&signer_bytes),
            "INV-FERR-051: signer bytes must be preserved exactly"
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════
// Stubs for implementation-level invariants (uncomment when impl ships)
// ═══════════════════════════════════════════════════════════════════════

// INV-FERR-060: Store Identity Persistence
// Requires: Database::genesis_with_identity (bd-mklv)
// Tests: identity tx survives merge, identity tx is self-signed

// INV-FERR-061: Causal Predecessor Completeness
// Requires: emit_predecessors (bd-3t63), Database::transact_signed (bd-6j0r)
// Tests: predecessor count == frontier size, predecessor DAG acyclic

// INV-FERR-062: Merge Receipt Completeness
// Requires: selective_merge (bd-sup6)
// Tests: 4 receipt datoms present, transferred count accurate

// INV-FERR-086: Canonical Datom Format Determinism
// Requires: Datom::canonical_bytes (not yet implemented)
// Tests: determinism (same datom → same bytes), injectivity (d1 != d2 → bytes differ)
