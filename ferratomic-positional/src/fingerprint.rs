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
        xor_hash_into(&mut fp, &hash);
    }
    fp
}

/// XOR-accumulate a 32-byte hash into a 32-byte fingerprint using u128
/// widening (bd-iltk, INV-FERR-074).
///
/// 2 XOR + 2 store operations instead of 32 (16x throughput over
/// byte-by-byte). Safe code only — no `unsafe`, no platform-specific
/// intrinsics. LLVM may further vectorize to AVX2 at `-O2`.
#[inline]
pub(crate) fn xor_hash_into(fp: &mut [u8; 32], hash: &[u8; 32]) {
    // Low 128 bits (bytes 0..16).
    let fp_lo = u128::from_ne_bytes(fp[..16].try_into().unwrap_or([0; 16]));
    let h_lo = u128::from_ne_bytes(hash[..16].try_into().unwrap_or([0; 16]));
    fp[..16].copy_from_slice(&(fp_lo ^ h_lo).to_ne_bytes());

    // High 128 bits (bytes 16..32).
    let fp_hi = u128::from_ne_bytes(fp[16..].try_into().unwrap_or([0; 16]));
    let h_hi = u128::from_ne_bytes(hash[16..].try_into().unwrap_or([0; 16]));
    fp[16..].copy_from_slice(&(fp_hi ^ h_hi).to_ne_bytes());
}
