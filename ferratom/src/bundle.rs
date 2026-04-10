//! Signed transaction bundle for federation transport.
//!
//! ADR-FERR-025: The transaction is the natural unit of federation.
//! Signatures cover transactions, not datoms. Causality is per-transaction.
//! [`SignedTransactionBundle`] groups a transaction's user datoms with its
//! signing metadata for transport between federated stores.
//!
//! INV-FERR-051: The bundle preserves the signing boundary — the receiver
//! can verify the signature because the exact datoms that were signed are
//! grouped together, not scattered across the store's datom set.

use crate::{Attribute, Datom, EntityId, ProvenanceType, TxId, TxSignature, TxSigner, Value};

// ---------------------------------------------------------------------------
// SignedTransactionBundle
// ---------------------------------------------------------------------------

/// A transaction's user datoms grouped with signing metadata for federation.
///
/// ADR-FERR-025: Transport exposes `fetch_signed_transactions` returning
/// `Vec<SignedTransactionBundle>`. Each bundle contains exactly the datoms
/// that were in one transaction, plus the metadata needed to verify the
/// signature and reconstruct causal predecessors.
///
/// INV-FERR-051: `signing` pairs signature and signer as a single
/// `Option` to prevent the invalid state (signature without signer).
///
/// INV-FERR-061: `predecessors` records the causal frontier at commit time.
///
/// INV-FERR-063: `provenance` carries the epistemic confidence level.
///
/// The `datoms` field contains ONLY user datoms — metadata datoms
/// (`:tx/signature`, `:tx/signer`, `:tx/predecessor`, `:tx/provenance`,
/// `:tx/time`, `:tx/origin`) are extracted into their respective fields
/// during bundle construction.
#[derive(Debug, Clone)]
pub struct SignedTransactionBundle {
    /// HLC-derived transaction identifier.
    tx_id: TxId,
    /// User datoms (excludes all `tx/*` metadata datoms).
    datoms: Vec<Datom>,
    /// Ed25519 signature + signer, paired to prevent invalid states.
    /// `None` for unsigned transactions.
    signing: Option<(TxSignature, TxSigner)>,
    /// Causal predecessor entity IDs (D19: `EntityId`, not `TxId`).
    predecessors: Vec<EntityId>,
    /// Epistemic confidence level (INV-FERR-063).
    /// `None` defaults to `Observed` (the common case).
    provenance: Option<ProvenanceType>,
}

impl SignedTransactionBundle {
    /// Construct a bundle from pre-extracted components.
    #[must_use]
    pub fn new(
        tx_id: TxId,
        datoms: Vec<Datom>,
        signing: Option<(TxSignature, TxSigner)>,
        predecessors: Vec<EntityId>,
        provenance: Option<ProvenanceType>,
    ) -> Self {
        Self {
            tx_id,
            datoms,
            signing,
            predecessors,
            provenance,
        }
    }

    /// Reconstruct a bundle from a store's datom set grouped by `TxId`.
    ///
    /// ADR-FERR-025: Metadata datoms are extracted into their respective
    /// fields. Remaining datoms are user payload. Length mismatches for
    /// signature/signer silently produce `None` (NEG-FERR-001).
    #[must_use]
    pub fn from_store_datoms(datoms: &[Datom], tx_id: TxId) -> Self {
        let mut user_datoms = Vec::new();
        let mut signature = None;
        let mut signer = None;
        let mut provenance = None;
        let mut predecessors = Vec::new();

        for d in datoms {
            if is_tx_metadata(d.attribute()) {
                extract_metadata(
                    d,
                    &mut signature,
                    &mut signer,
                    &mut provenance,
                    &mut predecessors,
                );
            } else {
                user_datoms.push(d.clone());
            }
        }

        let signing = signature.zip(signer);

        Self {
            tx_id,
            datoms: user_datoms,
            signing,
            predecessors,
            provenance,
        }
    }

    /// Transaction identifier.
    #[must_use]
    pub fn tx_id(&self) -> TxId {
        self.tx_id
    }

    /// User datoms (excludes all `tx/*` metadata).
    #[must_use]
    pub fn datoms(&self) -> &[Datom] {
        &self.datoms
    }

    /// Ed25519 signature, if signed.
    #[must_use]
    pub fn signature(&self) -> Option<&TxSignature> {
        self.signing.as_ref().map(|(sig, _)| sig)
    }

    /// Ed25519 verifying key, if signed.
    #[must_use]
    pub fn signer(&self) -> Option<&TxSigner> {
        self.signing.as_ref().map(|(_, key)| key)
    }

    /// Causal predecessor `TxId`s.
    #[must_use]
    pub fn predecessors(&self) -> &[EntityId] {
        &self.predecessors
    }

    /// Epistemic provenance type.
    #[must_use]
    pub fn provenance(&self) -> Option<ProvenanceType> {
        self.provenance
    }

    /// Whether this bundle represents a signed transaction.
    #[must_use]
    pub fn is_signed(&self) -> bool {
        self.signing.is_some()
    }
}

/// Extract metadata fields from a `tx/*` datom into accumulator slots.
fn extract_metadata(
    d: &Datom,
    signature: &mut Option<TxSignature>,
    signer: &mut Option<TxSigner>,
    provenance: &mut Option<ProvenanceType>,
    predecessors: &mut Vec<EntityId>,
) {
    match d.attribute().as_str() {
        "tx/signature" => {
            if let Value::Bytes(ref b) = d.value() {
                if let Ok(arr) = <[u8; 64]>::try_from(&**b) {
                    *signature = Some(TxSignature::from_bytes(arr));
                }
            }
        }
        "tx/signer" => {
            if let Value::Bytes(ref b) = d.value() {
                if let Ok(arr) = <[u8; 32]>::try_from(&**b) {
                    *signer = Some(TxSigner::from_bytes(arr));
                }
            }
        }
        "tx/provenance" => {
            if let Value::Keyword(ref kw) = d.value() {
                *provenance = ProvenanceType::from_keyword(kw);
            }
        }
        "tx/predecessor" => {
            // D19: predecessor Ref values are EntityIds (BLAKE3 of TxId bytes).
            if let Value::Ref(eid) = d.value() {
                predecessors.push(*eid);
            }
        }
        _ => {} // tx/time, tx/origin, etc.
    }
}

/// Check if an attribute is engine-level transaction metadata (`tx/*`).
fn is_tx_metadata(attr: &Attribute) -> bool {
    attr.as_str().starts_with("tx/")
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::{EntityId, Op, Value};

    fn make_test_datom(attr: &str, value: Value) -> Datom {
        Datom::new(
            EntityId::from_content(b"test-entity"),
            Attribute::from(attr),
            value,
            TxId::new(1, 0, 0),
            Op::Assert,
        )
    }

    #[test]
    fn test_inv_ferr_025_bundle_construction() {
        let bundle = SignedTransactionBundle::new(
            TxId::new(1, 0, 0),
            vec![make_test_datom(
                "user/name",
                Value::String(Arc::from("Alice")),
            )],
            None,
            vec![],
            None,
        );
        assert_eq!(
            bundle.datoms().len(),
            1,
            "ADR-FERR-025: bundle must contain user datoms"
        );
        assert!(!bundle.is_signed(), "ADR-FERR-025: unsigned bundle");
    }

    #[test]
    fn test_inv_ferr_051_bundle_extracts_signature() {
        let sig_bytes = [0xAA; 64];
        let signer_bytes = [0xBB; 32];
        let datoms = vec![
            make_test_datom("user/name", Value::String(Arc::from("Alice"))),
            make_test_datom(
                "tx/signature",
                Value::Bytes(Arc::from(sig_bytes.as_slice())),
            ),
            make_test_datom(
                "tx/signer",
                Value::Bytes(Arc::from(signer_bytes.as_slice())),
            ),
            make_test_datom("tx/time", Value::Instant(1_000_000)),
        ];

        let bundle = SignedTransactionBundle::from_store_datoms(&datoms, TxId::new(1, 0, 0));

        assert_eq!(
            bundle.datoms().len(),
            1,
            "INV-FERR-051: excludes tx/* metadata"
        );
        assert!(bundle.is_signed(), "INV-FERR-051: signed bundle");
        assert_eq!(
            bundle.signature().map(TxSignature::as_bytes),
            Some(&sig_bytes),
            "INV-FERR-051: signature preserved"
        );
        assert_eq!(
            bundle.signer().map(TxSigner::as_bytes),
            Some(&signer_bytes),
            "INV-FERR-051: signer preserved"
        );
    }

    #[test]
    fn test_inv_ferr_025_unsigned_bundle() {
        let datoms = vec![
            make_test_datom("user/name", Value::String(Arc::from("Bob"))),
            make_test_datom("tx/time", Value::Instant(2_000_000)),
        ];
        let bundle = SignedTransactionBundle::from_store_datoms(&datoms, TxId::new(2, 0, 0));
        assert_eq!(bundle.datoms().len(), 1, "ADR-FERR-025: user datoms only");
        assert!(!bundle.is_signed(), "ADR-FERR-025: unsigned");
    }

    #[test]
    fn test_inv_ferr_063_extracts_provenance() {
        let datoms = vec![
            make_test_datom("user/data", Value::Long(42)),
            make_test_datom(
                "tx/provenance",
                Value::Keyword(Arc::from("provenance/inferred")),
            ),
        ];
        let bundle = SignedTransactionBundle::from_store_datoms(&datoms, TxId::new(3, 0, 0));
        assert_eq!(
            bundle.provenance(),
            Some(ProvenanceType::Inferred),
            "INV-FERR-063: provenance extracted"
        );
    }

    #[test]
    fn test_inv_ferr_051_wrong_length_ignored() {
        let datoms = vec![
            make_test_datom("user/data", Value::Long(1)),
            make_test_datom(
                "tx/signature",
                Value::Bytes(Arc::from([0u8; 32].as_slice())),
            ),
        ];
        let bundle = SignedTransactionBundle::from_store_datoms(&datoms, TxId::new(4, 0, 0));
        assert!(
            bundle.signature().is_none(),
            "INV-FERR-051: wrong-length signature ignored (NEG-FERR-001)"
        );
    }

    #[test]
    fn test_inv_ferr_025_no_tx_metadata_in_user_datoms() {
        let datoms = vec![
            make_test_datom("user/name", Value::String(Arc::from("Carol"))),
            make_test_datom("tx/time", Value::Instant(5_000)),
            make_test_datom("tx/rationale", Value::String(Arc::from("test"))),
        ];
        let bundle = SignedTransactionBundle::from_store_datoms(&datoms, TxId::new(5, 0, 0));
        for d in bundle.datoms() {
            assert!(
                !d.attribute().as_str().starts_with("tx/"),
                "ADR-FERR-025: found tx/* metadata in user datoms: '{}'",
                d.attribute().as_str()
            );
        }
    }
}
