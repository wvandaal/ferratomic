//! XOR homomorphic fingerprint over canonical datom arrays (INV-FERR-074).
//!
//! `H(S) = XOR_{d in S} content_hash(d)` where `content_hash` is the
//! BLAKE3 hash of the full 5-tuple (INV-FERR-012).
//!
//! Properties:
//! - Commutative: XOR is commutative -> H is independent of iteration order.
//! - Homomorphic over disjoint union: `H(A | B) = H(A) ^ H(B)` when `A & B = {}`.
//! - Identity: `H({}) = [0; 32]`.

use ferratom::Datom;

/// XOR homomorphic fingerprint over canonical datom array (INV-FERR-074).
///
/// `H(S) = XOR_{d in S} content_hash(d)` where `content_hash` is the
/// BLAKE3 hash of the full 5-tuple (INV-FERR-012).
///
/// Properties:
/// - Commutative: XOR is commutative -> H is independent of iteration order.
/// - Homomorphic over disjoint union: `H(A | B) = H(A) ^ H(B)` when `A & B = {}`.
/// - Identity: `H({}) = [0; 32]`.
///
/// # Deviation from spec Level 2
///
/// Spec L2 describes `blake3::hash(bincode::serialize(datom))`. This
/// implementation uses `Datom::content_hash()` (INV-FERR-012) instead.
/// The deviation is intentional: `content_hash()` reuses the
/// content-addressed identity already computed for each datom, avoiding a
/// `bincode` dependency in the hash path and ensuring the fingerprint is
/// consistent with the canonical datom identity throughout the system.
pub(crate) fn compute_fingerprint(canonical: &[Datom]) -> [u8; 32] {
    let mut fp = [0u8; 32];
    for datom in canonical {
        let hash = datom.content_hash();
        for (acc, byte) in fp.iter_mut().zip(hash.iter()) {
            *acc ^= byte;
        }
    }
    fp
}
