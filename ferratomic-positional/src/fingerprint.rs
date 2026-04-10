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
/// `H(S) = XOR_{d in S} content_hash(d)` where `content_hash` is
/// `BLAKE3(canonical_bytes(d))` per the INV-FERR-086 + INV-FERR-012
/// unification. The streaming `content_hash()` method on `Datom`
/// produces the same hash as `blake3::hash(&d.canonical_bytes())`
/// without intermediate allocation.
///
/// Properties:
/// - Commutative: XOR is commutative -> H is independent of iteration order.
/// - Homomorphic over disjoint union: `H(A | B) = H(A) ^ H(B)` when `A & B = {}`.
/// - Identity: `H({}) = [0; 32]`.
/// - Codec-invariant: uses `canonical_bytes`, not codec-specific encoding
///   (INV-FERR-045c T4 fingerprint homomorphism compatibility).
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
    let (fp_lo, fp_hi) = split_u128(fp);
    let (h_lo, h_hi) = split_u128_ref(hash);
    (fp_lo ^ h_lo)
        .to_ne_bytes()
        .iter()
        .enumerate()
        .for_each(|(i, &b)| fp[i] = b);
    (fp_hi ^ h_hi)
        .to_ne_bytes()
        .iter()
        .enumerate()
        .for_each(|(i, &b)| fp[16 + i] = b);
}

/// Split `[u8; 32]` into two `u128` values. Infallible by construction.
#[inline]
fn split_u128_ref(bytes: &[u8; 32]) -> (u128, u128) {
    let lo = u128::from_ne_bytes([
        bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7], bytes[8],
        bytes[9], bytes[10], bytes[11], bytes[12], bytes[13], bytes[14], bytes[15],
    ]);
    let hi = u128::from_ne_bytes([
        bytes[16], bytes[17], bytes[18], bytes[19], bytes[20], bytes[21], bytes[22], bytes[23],
        bytes[24], bytes[25], bytes[26], bytes[27], bytes[28], bytes[29], bytes[30], bytes[31],
    ]);
    (lo, hi)
}

/// Split mutable `[u8; 32]` into two `u128` values. Infallible by construction.
#[inline]
fn split_u128(bytes: &[u8; 32]) -> (u128, u128) {
    split_u128_ref(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_xor_hash_into_identity() {
        let mut fp = [0u8; 32];
        let hash = [0xAB; 32];
        xor_hash_into(&mut fp, &hash);
        assert_eq!(fp, hash, "INV-FERR-074: XOR with zero is identity");
    }

    #[test]
    fn test_xor_hash_into_self_inverse() {
        let mut fp = [0x42; 32];
        let original = fp;
        let hash = [0xFF; 32];
        xor_hash_into(&mut fp, &hash);
        assert_ne!(fp, original);
        xor_hash_into(&mut fp, &hash);
        assert_eq!(fp, original, "INV-FERR-074: double XOR is identity");
    }

    #[test]
    fn test_xor_hash_into_commutativity() {
        let mut fp1 = [0u8; 32];
        let mut fp2 = [0u8; 32];
        let a = [0x11; 32];
        let b = [0x22; 32];
        xor_hash_into(&mut fp1, &a);
        xor_hash_into(&mut fp1, &b);
        xor_hash_into(&mut fp2, &b);
        xor_hash_into(&mut fp2, &a);
        assert_eq!(fp1, fp2, "INV-FERR-074: XOR is commutative");
    }
}
