//! Kani bounded model checking for INV-FERR-045c (Leaf Chunk Codec Conformance)
//! and INV-FERR-045a (DatomPair Reference Codec).
//!
//! VERIFY-DRIFT-013: This harness was missing — filed during the
//! lifecycle/18 verification audit.

use ferratomic_positional::codec::{DatomPairChunk, DatomPairCodec, LeafChunk, LeafChunkCodec};

/// INV-FERR-045a T1: DatomPair payload round-trip under bounded model checking.
///
/// Symbolically explores 2-entry chunks with 2-byte keys and 2-byte values.
/// Kani verifies that for ALL inputs within the bound, decode_payload is the
/// exact inverse of encode_payload.
#[cfg_attr(kani, kani::proof)]
#[cfg_attr(kani, kani::unwind(4))]
#[cfg_attr(not(kani), test)]
#[cfg_attr(not(kani), ignore = "requires Kani verifier")]
fn datom_pair_payload_roundtrip_bounded() {
    let k1: [u8; 2] = kani::any();
    let v1: [u8; 2] = kani::any();
    let k2: [u8; 2] = kani::any();
    let v2: [u8; 2] = kani::any();
    kani::assume(k1 != k2);

    let entries = vec![(k1.to_vec(), v1.to_vec()), (k2.to_vec(), v2.to_vec())];
    let chunk = DatomPairChunk::new(entries).expect("distinct keys are canonical");

    let bytes = DatomPairCodec::encode_payload(&chunk);
    let decoded =
        DatomPairCodec::decode_payload(&bytes).expect("INV-FERR-045a T1: payload must round-trip");
    assert_eq!(decoded, chunk);
}

/// INV-FERR-045a T2: DatomPair encode determinism under bounded model checking.
#[cfg_attr(kani, kani::proof)]
#[cfg_attr(kani, kani::unwind(4))]
#[cfg_attr(not(kani), test)]
#[cfg_attr(not(kani), ignore = "requires Kani verifier")]
fn datom_pair_encode_deterministic_bounded() {
    let k: [u8; 2] = kani::any();
    let v: [u8; 2] = kani::any();

    let entries = vec![(k.to_vec(), v.to_vec())];
    let chunk = DatomPairChunk::new(entries).expect("single entry is canonical");

    let b1 = DatomPairCodec::encode_payload(&chunk);
    let b2 = DatomPairCodec::encode_payload(&chunk);
    assert_eq!(b1, b2, "INV-FERR-045c T2: encode must be deterministic");
}

/// INV-FERR-045c: LeafChunk enum dispatch round-trip.
///
/// Verifies the full layered path: LeafChunk::encode (prepends CODEC_TAG)
/// then LeafChunk::decode (dispatches on CODEC_TAG) recovers the original.
#[cfg_attr(kani, kani::proof)]
#[cfg_attr(kani, kani::unwind(4))]
#[cfg_attr(not(kani), test)]
#[cfg_attr(not(kani), ignore = "requires Kani verifier")]
fn leaf_chunk_dispatch_roundtrip_bounded() {
    let k: [u8; 2] = kani::any();
    let v: [u8; 2] = kani::any();

    let entries = vec![(k.to_vec(), v.to_vec())];
    let dp = DatomPairChunk::new(entries).expect("single entry is canonical");
    let leaf = LeafChunk::DatomPair(dp.clone());

    let bytes = leaf.encode();
    assert_eq!(bytes[0], DatomPairCodec::CODEC_TAG);

    let decoded = LeafChunk::decode(&bytes).expect("LeafChunk dispatch must round-trip");
    match decoded {
        LeafChunk::DatomPair(d) => assert_eq!(d, dp),
    }
}

/// §23.9.8: Unknown codec tags must be rejected.
#[cfg_attr(kani, kani::proof)]
#[cfg_attr(kani, kani::unwind(4))]
#[cfg_attr(not(kani), test)]
#[cfg_attr(not(kani), ignore = "requires Kani verifier")]
fn unknown_codec_tag_rejected_bounded() {
    let tag: u8 = kani::any();
    kani::assume(tag != DatomPairCodec::CODEC_TAG);

    // Construct minimal bytes with the unknown tag + a valid payload
    let mut bytes = vec![tag];
    bytes.extend_from_slice(&0u32.to_le_bytes()); // zero entries

    let result = LeafChunk::decode(&bytes);
    assert!(
        result.is_err(),
        "Unknown codec tag {tag:#04x} must be rejected"
    );
}

// Stub module for non-Kani compilation
#[cfg(not(kani))]
use super::kani;
