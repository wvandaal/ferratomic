//! Ed25519 transaction signing and verification.
//!
//! INV-FERR-051: Every signed transaction binds user datoms, `tx_id`,
//! predecessors, store fingerprint, and signer public key into a single
//! BLAKE3 + Ed25519 cryptographic attestation.
//!
//! ADR-FERR-021: Metadata datoms (attribute prefix `tx/`) are excluded
//! from the signing message — they are derived, not user-authored.
//!
//! ADR-FERR-031: Signing happens at the Database layer (after HLC tick
//! assigns `tx_id`), not at the Transaction layer.

use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use ferratom::{tx_id_canonical_bytes, Datom, EntityId, FerraError, TxId, TxSignature, TxSigner};

/// Components shared between signing and verification (INV-FERR-051).
///
/// Groups the datoms, transaction ID, predecessor entity IDs, and store
/// fingerprint that together form the signing message input.
pub struct SigningComponents<'a> {
    /// User + metadata datoms (tx/* will be filtered automatically).
    pub datoms: &'a [Datom],
    /// HLC-assigned transaction ID.
    pub tx_id: TxId,
    /// Predecessor transaction entity IDs (D19: `EntityId`, not `TxId`).
    pub predecessor_entity_ids: &'a [EntityId],
    /// Pre-transaction store fingerprint (D17, INV-FERR-074).
    pub store_fingerprint: &'a [u8; 32],
}

/// Compute the INV-FERR-051 signing message.
///
/// BLAKE3 hash of: sorted user datoms (canonical bytes, INV-FERR-086)
/// ++ `tx_id` (canonical bytes) ++ sorted predecessor entity IDs (D19)
/// ++ store fingerprint (D17) ++ signer public key.
///
/// ADR-FERR-021: `tx/*` metadata datoms are excluded.
/// Datoms are sorted via `Datom::Ord` (EAVT order) for determinism.
/// Predecessor entity IDs are sorted for determinism.
#[must_use]
pub fn signing_message(components: &SigningComponents<'_>, signer_pk: &[u8; 32]) -> Vec<u8> {
    let mut user: Vec<&Datom> = components
        .datoms
        .iter()
        .filter(|d| !d.attribute().as_str().starts_with("tx/"))
        .collect();
    user.sort();

    let mut hasher = blake3::Hasher::new();

    // User datoms: sorted, canonical bytes (INV-FERR-086)
    for d in &user {
        hasher.update(&d.canonical_bytes());
    }

    // TxId: canonical 28-byte format (INV-FERR-086)
    hasher.update(&tx_id_canonical_bytes(components.tx_id));

    // Predecessor EntityIds: sorted, 32 bytes each (D19)
    let mut sorted_preds: Vec<EntityId> = components.predecessor_entity_ids.to_vec();
    sorted_preds.sort();
    for eid in &sorted_preds {
        hasher.update(eid.as_bytes());
    }

    // Store fingerprint: 32 bytes (D17, INV-FERR-074)
    hasher.update(components.store_fingerprint);

    // Signer public key: 32 bytes
    hasher.update(signer_pk);

    hasher.finalize().as_bytes().to_vec()
}

/// Sign a transaction (INV-FERR-051).
///
/// Computes the signing message and produces an Ed25519 signature.
/// Called by `Database::transact_signed` after HLC tick assigns `tx_id`.
///
/// Returns `(TxSignature, TxSigner)` for storage as datoms (ADR-FERR-021).
#[must_use]
pub fn sign_transaction(
    components: &SigningComponents<'_>,
    signing_key: &SigningKey,
) -> (TxSignature, TxSigner) {
    let pk = signing_key.verifying_key();
    let msg = signing_message(components, pk.as_bytes());

    let signature = signing_key.sign(&msg);
    (
        TxSignature::from_bytes(signature.to_bytes()),
        TxSigner::from_bytes(pk.to_bytes()),
    )
}

/// Verify a transaction signature (INV-FERR-051).
///
/// Reconstructs the signing message from the provided components and
/// verifies the Ed25519 signature against the signer's public key.
///
/// # Errors
///
/// Returns [`FerraError::SignatureInvalid`] if the signature does not
/// match the reconstructed message or the signer bytes are not a valid
/// Ed25519 public key.
pub fn verify_signature(
    components: &SigningComponents<'_>,
    signature: &TxSignature,
    signer: &TxSigner,
) -> Result<(), FerraError> {
    let vk =
        VerifyingKey::from_bytes(signer.as_bytes()).map_err(|e| FerraError::SignatureInvalid {
            tx_description: format!("invalid signer public key: {e}"),
        })?;

    let msg = signing_message(components, signer.as_bytes());

    let sig = Signature::from_bytes(signature.as_bytes());
    vk.verify(&msg, &sig)
        .map_err(|e| FerraError::SignatureInvalid {
            tx_description: format!("Ed25519 verification failed: {e}"),
        })
}

#[cfg(test)]
mod tests {
    use ferratom::{Attribute, NodeId, Op, Value};

    use super::*;

    fn test_node() -> NodeId {
        NodeId::from_bytes([1u8; 16])
    }

    fn test_tx_id() -> TxId {
        TxId::with_node(1000, 0, test_node())
    }

    fn test_datoms() -> Vec<Datom> {
        let entity = EntityId::from_content(b"test-entity");
        let tx = test_tx_id();
        vec![
            Datom::new(
                entity,
                Attribute::from("db/doc"),
                Value::String("hello".into()),
                tx,
                Op::Assert,
            ),
            // tx/* metadata — should be excluded from signing
            Datom::new(
                entity,
                Attribute::from("tx/time"),
                Value::Instant(1000),
                tx,
                Op::Assert,
            ),
        ]
    }

    fn test_signing_key() -> SigningKey {
        SigningKey::from_bytes(&[42u8; 32])
    }

    fn test_fingerprint() -> [u8; 32] {
        [0xABu8; 32]
    }

    fn test_predecessors() -> Vec<EntityId> {
        vec![EntityId::from_content(b"pred-tx-1")]
    }

    fn test_components<'a>(
        datoms: &'a [Datom],
        preds: &'a [EntityId],
        fp: &'a [u8; 32],
    ) -> SigningComponents<'a> {
        SigningComponents {
            datoms,
            tx_id: test_tx_id(),
            predecessor_entity_ids: preds,
            store_fingerprint: fp,
        }
    }

    // -- Postcondition 1: signing_message deterministic -----------------

    #[test]
    fn test_signing_message_deterministic() {
        let datoms = test_datoms();
        let preds = test_predecessors();
        let fp = test_fingerprint();
        let pk = test_signing_key().verifying_key();
        let c = test_components(&datoms, &preds, &fp);

        let msg1 = signing_message(&c, pk.as_bytes());
        let msg2 = signing_message(&c, pk.as_bytes());
        assert_eq!(
            msg1, msg2,
            "INV-FERR-051: signing_message must be deterministic"
        );
    }

    // -- Postcondition 2: sign + verify round-trip ---------------------

    #[test]
    fn test_sign_and_verify_round_trip() {
        let datoms = test_datoms();
        let preds = test_predecessors();
        let fp = test_fingerprint();
        let sk = test_signing_key();
        let c = test_components(&datoms, &preds, &fp);

        let (sig, signer) = sign_transaction(&c, &sk);
        let result = verify_signature(&c, &sig, &signer);
        assert!(
            result.is_ok(),
            "INV-FERR-051: sign-then-verify must succeed: {result:?}"
        );
    }

    // -- Postcondition 3: tampered datoms fail -------------------------

    #[test]
    fn test_verify_rejects_tampered_datoms() {
        let datoms = test_datoms();
        let preds = test_predecessors();
        let fp = test_fingerprint();
        let sk = test_signing_key();
        let c = test_components(&datoms, &preds, &fp);

        let (sig, signer) = sign_transaction(&c, &sk);

        // Tamper: replace "hello" with "tampered"
        let mut tampered = datoms.clone();
        tampered[0] = Datom::new(
            tampered[0].entity(),
            tampered[0].attribute().clone(),
            Value::String("tampered".into()),
            tampered[0].tx(),
            tampered[0].op(),
        );
        let tc = test_components(&tampered, &preds, &fp);

        let result = verify_signature(&tc, &sig, &signer);
        assert!(
            result.is_err(),
            "INV-FERR-051: tampered datoms must fail verification"
        );
    }

    // -- Postcondition 3: wrong key fails ------------------------------

    #[test]
    fn test_verify_rejects_wrong_key() {
        let datoms = test_datoms();
        let preds = test_predecessors();
        let fp = test_fingerprint();
        let sk = test_signing_key();
        let c = test_components(&datoms, &preds, &fp);

        let (sig, _signer) = sign_transaction(&c, &sk);

        let wrong_key = SigningKey::from_bytes(&[99u8; 32]);
        let wrong_signer = TxSigner::from_bytes(wrong_key.verifying_key().to_bytes());

        let result = verify_signature(&c, &sig, &wrong_signer);
        assert!(
            result.is_err(),
            "INV-FERR-051: wrong signer key must fail verification"
        );
    }

    // -- Postcondition 3: modified TxId fails --------------------------

    #[test]
    fn test_verify_rejects_modified_txid() {
        let datoms = test_datoms();
        let preds = test_predecessors();
        let fp = test_fingerprint();
        let sk = test_signing_key();
        let c = test_components(&datoms, &preds, &fp);

        let (sig, signer) = sign_transaction(&c, &sk);

        let wrong_c = SigningComponents {
            datoms: &datoms,
            tx_id: TxId::with_node(9999, 0, test_node()),
            predecessor_entity_ids: &preds,
            store_fingerprint: &fp,
        };
        let result = verify_signature(&wrong_c, &sig, &signer);
        assert!(
            result.is_err(),
            "INV-FERR-051: modified TxId must fail verification"
        );
    }

    // -- Postcondition 3: modified fingerprint fails -------------------

    #[test]
    fn test_verify_rejects_modified_fingerprint() {
        let datoms = test_datoms();
        let preds = test_predecessors();
        let fp = test_fingerprint();
        let sk = test_signing_key();
        let c = test_components(&datoms, &preds, &fp);

        let (sig, signer) = sign_transaction(&c, &sk);

        let wrong_fp = [0xFFu8; 32];
        let wrong_c = test_components(&datoms, &preds, &wrong_fp);
        let result = verify_signature(&wrong_c, &sig, &signer);
        assert!(
            result.is_err(),
            "INV-FERR-051: modified fingerprint must fail verification"
        );
    }

    // -- Postcondition 4: tx/* metadata excluded -----------------------

    #[test]
    fn test_signing_excludes_tx_metadata() {
        let preds = test_predecessors();
        let fp = test_fingerprint();
        let pk = test_signing_key().verifying_key();
        let tx_id = test_tx_id();
        let entity = EntityId::from_content(b"test-entity");

        let user_only = vec![Datom::new(
            entity,
            Attribute::from("db/doc"),
            Value::String("hello".into()),
            tx_id,
            Op::Assert,
        )];

        let with_metadata = vec![
            Datom::new(
                entity,
                Attribute::from("db/doc"),
                Value::String("hello".into()),
                tx_id,
                Op::Assert,
            ),
            Datom::new(
                entity,
                Attribute::from("tx/time"),
                Value::Instant(1000),
                tx_id,
                Op::Assert,
            ),
            Datom::new(
                entity,
                Attribute::from("tx/provenance"),
                Value::Keyword("provenance/observed".into()),
                tx_id,
                Op::Assert,
            ),
        ];

        let c1 = test_components(&user_only, &preds, &fp);
        let c2 = test_components(&with_metadata, &preds, &fp);
        let msg1 = signing_message(&c1, pk.as_bytes());
        let msg2 = signing_message(&c2, pk.as_bytes());
        assert_eq!(
            msg1, msg2,
            "ADR-FERR-021: tx/* metadata must not affect signing message"
        );
    }

    // -- Postcondition 5: uses EntityIds, not TxIds --------------------

    #[test]
    fn test_signing_message_uses_entity_ids() {
        let fp = test_fingerprint();
        let pk = test_signing_key().verifying_key();
        let datoms = test_datoms();

        let preds_a = vec![EntityId::from_content(b"pred-1")];
        let preds_b = vec![EntityId::from_content(b"pred-2")];

        let ca = test_components(&datoms, &preds_a, &fp);
        let cb = test_components(&datoms, &preds_b, &fp);
        let msg_a = signing_message(&ca, pk.as_bytes());
        let msg_b = signing_message(&cb, pk.as_bytes());
        assert_ne!(
            msg_a, msg_b,
            "INV-FERR-051/D19: different predecessor EntityIds must produce different messages"
        );
    }

    // -- Postcondition 6: fingerprint included -------------------------

    #[test]
    fn test_signing_message_includes_fingerprint() {
        let preds = test_predecessors();
        let pk = test_signing_key().verifying_key();
        let datoms = test_datoms();

        let fp_a = [0x01u8; 32];
        let fp_b = [0x02u8; 32];

        let ca = test_components(&datoms, &preds, &fp_a);
        let cb = test_components(&datoms, &preds, &fp_b);
        let msg_a = signing_message(&ca, pk.as_bytes());
        let msg_b = signing_message(&cb, pk.as_bytes());
        assert_ne!(
            msg_a, msg_b,
            "INV-FERR-051/D17: different fingerprints must produce different messages"
        );
    }

    // -- Datom order independence --------------------------------------

    #[test]
    fn test_signing_message_order_independent() {
        let preds = test_predecessors();
        let fp = test_fingerprint();
        let pk = test_signing_key().verifying_key();
        let tx_id = test_tx_id();
        let entity = EntityId::from_content(b"test-entity");

        let d1 = Datom::new(
            entity,
            Attribute::from("db/doc"),
            Value::String("aaa".into()),
            tx_id,
            Op::Assert,
        );
        let d2 = Datom::new(
            entity,
            Attribute::from("db/ident"),
            Value::Keyword("test/attr".into()),
            tx_id,
            Op::Assert,
        );

        let forward = vec![d1.clone(), d2.clone()];
        let reverse = vec![d2, d1];

        let cf = test_components(&forward, &preds, &fp);
        let cr = test_components(&reverse, &preds, &fp);
        let msg_f = signing_message(&cf, pk.as_bytes());
        let msg_r = signing_message(&cr, pk.as_bytes());
        assert_eq!(
            msg_f, msg_r,
            "INV-FERR-051: signing message must be independent of datom order"
        );
    }
}
