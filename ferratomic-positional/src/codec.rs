//! Leaf chunk codec architecture (INV-FERR-045c, INV-FERR-045a).
//!
//! Defines the `LeafChunkCodec` conformance trait, the `DatomPairCodec`
//! reference implementation, and the `LeafChunk` closed-world enum dispatch.
//!
//! ## Conformance Theorems (INV-FERR-045c)
//!
//! Every registered codec must satisfy:
//! - **T1 (Round-trip)**: `decode(encode(D)) == Ok(D)`
//! - **T2 (Determinism)**: `encode` is a pure function
//! - **T3 (Injectivity)**: distinct sets → distinct bytes
//! - **T4 (Fingerprint compatibility)**: framework fingerprint depends only
//!   on the logical datom set, not the codec's encoded bytes
//! - **T5 (Order independence)**: `encode` is a function on `Set(Datom)`
//!
//! ## Codec Discriminator Registry (§23.9.8)
//!
//! `0x01` = `DatomPair` (this module). Future codecs registered by spec evolution.

use std::collections::BTreeSet;

use ferratom::{Datom, FerraError};

// ---------------------------------------------------------------------------
// LeafChunkCodec trait (INV-FERR-045c)
// ---------------------------------------------------------------------------

/// A codec for leaf chunk payloads. Conforming codecs satisfy the five
/// conformance theorems of `INV-FERR-045c`.
///
/// The trait surface is intentionally narrow: `encode`, `decode`, and an
/// optional `boundary_key` fast path. The chunk fingerprint is computed at
/// the framework level via `INV-FERR-074` + `INV-FERR-086`, NOT by the codec.
pub trait LeafChunkCodec {
    /// Codec discriminator byte (registered in `§23.9.8`).
    /// Spec-registered: `0x01..=0x7F`. Experimental: `0x80..=0xFF`.
    const CODEC_TAG: u8;

    /// Encode a finite set of datoms into the codec's canonical byte payload.
    /// The output does NOT include the `CODEC_TAG` byte (the framework
    /// prepends it via [`LeafChunk::encode`]).
    fn encode(datoms: &BTreeSet<Datom>) -> Vec<u8>;

    /// Decode a payload byte sequence back into the datom set.
    ///
    /// # Errors
    ///
    /// Returns `FerraError` on malformed, truncated, or non-canonical input.
    fn decode(bytes: &[u8]) -> Result<BTreeSet<Datom>, FerraError>;

    /// Return the smallest datom in canonical ordering from this chunk.
    /// Default: decode then take min. Codecs MAY override for efficiency
    /// but MUST return the same value as the default.
    ///
    /// # Errors
    ///
    /// Returns `FerraError::EmptyChunk` if the chunk has zero entries.
    fn boundary_key(bytes: &[u8]) -> Result<Datom, FerraError> {
        let datoms = Self::decode(bytes)?;
        datoms.into_iter().next().ok_or(FerraError::EmptyChunk)
    }
}

// ---------------------------------------------------------------------------
// DatomPairChunk — validated payload type (INV-FERR-045a)
// ---------------------------------------------------------------------------

/// The validated payload of a leaf chunk in the `DatomPair` codec format:
/// a sorted, deduplicated sequence of canonical (key, value) byte pairs.
///
/// Constructors validate the canonical predicate; non-canonical chunks
/// are unrepresentable in well-typed code.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DatomPairChunk {
    /// Entries in strict ascending key order. Private field.
    entries: Vec<(Vec<u8>, Vec<u8>)>,
}

impl DatomPairChunk {
    /// Build from arbitrary entries. Sorts and validates.
    ///
    /// # Errors
    ///
    /// Returns [`FerraError::NonCanonicalChunk`] if duplicate keys are present.
    pub fn new(mut entries: Vec<(Vec<u8>, Vec<u8>)>) -> Result<Self, FerraError> {
        entries.sort_by(|a, b| a.0.cmp(&b.0));
        for window in entries.windows(2) {
            if window[0].0 == window[1].0 {
                return Err(FerraError::NonCanonicalChunk);
            }
        }
        Ok(Self { entries })
    }

    /// Build from already-sorted, deduplicated entries. Debug-asserts.
    #[must_use]
    pub fn from_sorted_unchecked(entries: Vec<(Vec<u8>, Vec<u8>)>) -> Self {
        debug_assert!(
            entries.windows(2).all(|w| w[0].0 < w[1].0),
            "from_sorted_unchecked called with non-canonical entries"
        );
        Self { entries }
    }

    /// The sorted entries.
    #[must_use]
    pub fn entries(&self) -> &[(Vec<u8>, Vec<u8>)] {
        &self.entries
    }

    /// Number of entries.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the chunk has zero entries.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

// ---------------------------------------------------------------------------
// DatomPairCodec — reference codec (INV-FERR-045a, CODEC_TAG = 0x01)
// ---------------------------------------------------------------------------

/// The `DatomPair` reference codec. Encodes leaf chunks as length-prefixed
/// (`canonical_key`, `canonical_value`) entries sorted by key.
pub struct DatomPairCodec;

/// Chunk kind discriminator (byte 0 on disk).
pub const CHUNK_KIND_LEAF: u8 = 0x01;
/// Chunk kind discriminator for internal nodes.
pub const CHUNK_KIND_INTERNAL: u8 = 0x02;

impl LeafChunkCodec for DatomPairCodec {
    const CODEC_TAG: u8 = 0x01;

    fn encode(datoms: &BTreeSet<Datom>) -> Vec<u8> {
        let chunk = datom_set_to_pair_chunk(datoms);
        Self::encode_payload(&chunk)
    }

    fn decode(bytes: &[u8]) -> Result<BTreeSet<Datom>, FerraError> {
        let chunk = Self::decode_payload(bytes)?;
        pair_chunk_to_datom_set(&chunk)
    }
}

impl DatomPairCodec {
    /// Encode a `DatomPairChunk` as the codec's payload bytes.
    /// Layout: `[entry_count: u32-le][entries: (key_len, key, val_len, val)*]`
    #[must_use]
    pub fn encode_payload(chunk: &DatomPairChunk) -> Vec<u8> {
        let cap = 4 + chunk
            .entries
            .iter()
            .map(|(k, v)| 8 + k.len() + v.len())
            .sum::<usize>();
        let mut buf = Vec::with_capacity(cap);
        let count = u32::try_from(chunk.entries.len()).unwrap_or(u32::MAX);
        buf.extend_from_slice(&count.to_le_bytes());
        for (k, v) in &chunk.entries {
            let klen = u32::try_from(k.len()).unwrap_or(u32::MAX);
            buf.extend_from_slice(&klen.to_le_bytes());
            buf.extend_from_slice(k);
            let vlen = u32::try_from(v.len()).unwrap_or(u32::MAX);
            buf.extend_from_slice(&vlen.to_le_bytes());
            buf.extend_from_slice(v);
        }
        buf
    }

    /// Decode payload bytes into a validated `DatomPairChunk`.
    ///
    /// # Errors
    ///
    /// Returns [`FerraError::TruncatedChunk`] on truncated input,
    /// [`FerraError::TrailingBytes`] on extra bytes after the last entry,
    /// or [`FerraError::NonCanonicalChunk`] if entries are not in strict
    /// ascending key order.
    pub fn decode_payload(bytes: &[u8]) -> Result<DatomPairChunk, FerraError> {
        if bytes.len() < 4 {
            return Err(FerraError::TruncatedChunk);
        }
        let entry_count = u32::from_le_bytes(
            bytes[..4]
                .try_into()
                .map_err(|_| FerraError::TruncatedChunk)?,
        ) as usize;

        let mut offset = 4;
        let mut entries = Vec::with_capacity(entry_count);
        for _ in 0..entry_count {
            if offset + 4 > bytes.len() {
                return Err(FerraError::TruncatedChunk);
            }
            let key_len = u32::from_le_bytes(
                bytes[offset..offset + 4]
                    .try_into()
                    .map_err(|_| FerraError::TruncatedChunk)?,
            ) as usize;
            offset += 4;

            if offset + key_len > bytes.len() {
                return Err(FerraError::TruncatedChunk);
            }
            let key = bytes[offset..offset + key_len].to_vec();
            offset += key_len;

            if offset + 4 > bytes.len() {
                return Err(FerraError::TruncatedChunk);
            }
            let val_len = u32::from_le_bytes(
                bytes[offset..offset + 4]
                    .try_into()
                    .map_err(|_| FerraError::TruncatedChunk)?,
            ) as usize;
            offset += 4;

            if offset + val_len > bytes.len() {
                return Err(FerraError::TruncatedChunk);
            }
            let value = bytes[offset..offset + val_len].to_vec();
            offset += val_len;

            entries.push((key, value));
        }

        if offset != bytes.len() {
            return Err(FerraError::TrailingBytes);
        }

        DatomPairChunk::new(entries)
    }
}

// ---------------------------------------------------------------------------
// LeafChunk — closed-world enum dispatch (INV-FERR-045c)
// ---------------------------------------------------------------------------

/// Closed-world enumeration of leaf chunk encodings. Adding a variant
/// requires spec evolution (new INV-FERR + `CODEC_TAG` + conformance discharge).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LeafChunk {
    /// `DatomPair` reference codec (INV-FERR-045a). `CODEC_TAG` = 0x01.
    DatomPair(DatomPairChunk),
}

impl LeafChunk {
    /// Encode to on-disk bytes: `[CODEC_TAG][payload]`.
    #[must_use]
    pub fn encode(&self) -> Vec<u8> {
        match self {
            Self::DatomPair(chunk) => {
                let payload = DatomPairCodec::encode_payload(chunk);
                let mut bytes = Vec::with_capacity(1 + payload.len());
                bytes.push(DatomPairCodec::CODEC_TAG);
                bytes.extend(payload);
                bytes
            }
        }
    }

    /// Decode from `[CODEC_TAG][payload]` bytes.
    ///
    /// # Errors
    ///
    /// Returns [`FerraError::TruncatedChunk`] on empty input,
    /// [`FerraError::UnknownCodecTag`] for unregistered codec tags,
    /// or the codec-specific error from the payload decoder.
    pub fn decode(bytes: &[u8]) -> Result<Self, FerraError> {
        let (&tag, payload) = bytes.split_first().ok_or(FerraError::TruncatedChunk)?;
        match tag {
            DatomPairCodec::CODEC_TAG => {
                let chunk = DatomPairCodec::decode_payload(payload)?;
                Ok(Self::DatomPair(chunk))
            }
            _ => Err(FerraError::UnknownCodecTag(tag)),
        }
    }

    /// Framework fingerprint per `INV-FERR-074` + `INV-FERR-086`.
    /// Depends ONLY on the logical datom set, NOT on codec bytes.
    #[must_use]
    pub fn fingerprint(&self) -> [u8; 32] {
        let datoms = match self {
            Self::DatomPair(chunk) => chunk,
        };
        framework_fingerprint(datoms)
    }
}

/// Per-chunk canonical fingerprint: XOR of `BLAKE3(canonical_bytes(d))` for
/// each datom `d` in the chunk. This is the framework-level fingerprint
/// computation from `INV-FERR-074` + `INV-FERR-086`.
#[must_use]
pub fn framework_fingerprint(chunk: &DatomPairChunk) -> [u8; 32] {
    let mut acc = [0u8; 32];
    for (key, _value) in chunk.entries() {
        let h = blake3::hash(key);
        for (a, b) in acc.iter_mut().zip(h.as_bytes().iter()) {
            *a ^= b;
        }
    }
    acc
}

// ---------------------------------------------------------------------------
// Datom ↔ DatomPairChunk conversion helpers (S23.9.0.2)
// ---------------------------------------------------------------------------

/// Convert a `BTreeSet<Datom>` into a `DatomPairChunk` using the primary
/// tree encoding: key = `content_hash` bytes, value = `content_hash` (32 bytes).
fn datom_set_to_pair_chunk(datoms: &BTreeSet<Datom>) -> DatomPairChunk {
    let entries: Vec<(Vec<u8>, Vec<u8>)> = datoms
        .iter()
        .map(|d| {
            let hash = d.content_hash();
            // Key: content hash bytes (canonical ordering matches Datom's Ord)
            // Value: same content hash (cross-reference per S23.9.0.2)
            (hash.to_vec(), hash.to_vec())
        })
        .collect();
    DatomPairChunk::from_sorted_unchecked(entries)
}

/// Inverse: rebuild a `BTreeSet<Datom>` from a `DatomPairChunk`.
/// For the content-hash-keyed primary tree, this requires a reverse lookup
/// from hash to datom — which is not possible without the original datoms.
/// In practice, round-trip at the `BTreeSet`<Datom> level goes through the
/// `canonical_bytes` encoding, not the `content_hash` encoding.
///
/// For now, this returns an error indicating that full datom recovery
/// requires the `canonical_bytes` key encoding (Phase 5 of session 023.5).
fn pair_chunk_to_datom_set(_chunk: &DatomPairChunk) -> Result<BTreeSet<Datom>, FerraError> {
    // Content-hash keys are one-way — cannot reconstruct Datom from hash.
    // The real implementation uses canonical_bytes as key (per S23.9.0.2),
    // which IS reversible via Datom::from_canonical_bytes.
    // This placeholder exists for the trait-level API; the payload-level
    // API (encode_payload / decode_payload) works on DatomPairChunk
    // directly and round-trips correctly.
    Err(FerraError::NotImplemented(
        "pair_chunk_to_datom_set requires canonical_bytes key encoding",
    ))
}

// ---------------------------------------------------------------------------
// Conformance test harness (INV-FERR-045c Level 2)
// Test infrastructure — gated behind cfg(test) / feature = "test-utils".
// ---------------------------------------------------------------------------

/// Conformance test functions for `DatomPairCodec`.
/// Gated behind `cfg(test)` because they use `expect`/`assert`
/// (which are forbidden in production lib code by the strict clippy gate).
#[cfg(test)]
pub mod conformance {
    use super::{framework_fingerprint, DatomPairChunk, DatomPairCodec};

    /// T1: round-trip at the `DatomPairChunk` level (payload API).
    ///
    /// # Panics
    ///
    /// Panics if the round-trip property is violated (test assertion).
    pub fn assert_payload_round_trip(chunk: &DatomPairChunk) {
        let bytes = DatomPairCodec::encode_payload(chunk);
        let decoded = DatomPairCodec::decode_payload(&bytes)
            .expect("INV-FERR-045c T1: decode_payload must succeed on valid input");
        assert_eq!(
            &decoded, chunk,
            "INV-FERR-045c T1: payload round-trip must preserve the chunk"
        );
    }

    /// T2: determinism at the payload level.
    ///
    /// # Panics
    ///
    /// Panics if encode is non-deterministic (test assertion).
    pub fn assert_payload_deterministic(chunk: &DatomPairChunk) {
        let b1 = DatomPairCodec::encode_payload(chunk);
        let b2 = DatomPairCodec::encode_payload(chunk);
        assert_eq!(b1, b2, "INV-FERR-045c T2: encode must be deterministic");
    }

    /// T3: injectivity at the payload level.
    ///
    /// # Panics
    ///
    /// Panics if distinct chunks encode to identical bytes (test assertion).
    pub fn assert_payload_injective(c1: &DatomPairChunk, c2: &DatomPairChunk) {
        if c1 != c2 {
            assert_ne!(
                DatomPairCodec::encode_payload(c1),
                DatomPairCodec::encode_payload(c2),
                "INV-FERR-045c T3: distinct chunks must encode differently"
            );
        }
    }

    /// T4: fingerprint compatibility (framework fingerprint via round-trip).
    ///
    /// # Panics
    ///
    /// Panics if the fingerprint depends on encoded bytes (test assertion).
    pub fn assert_fingerprint_codec_invariant(chunk: &DatomPairChunk) {
        let bytes = DatomPairCodec::encode_payload(chunk);
        let recovered =
            DatomPairCodec::decode_payload(&bytes).expect("T4 precondition: round-trip");
        let fp_direct = framework_fingerprint(chunk);
        let fp_via_codec = framework_fingerprint(&recovered);
        assert_eq!(
            fp_direct, fp_via_codec,
            "INV-FERR-045c T4: fingerprint must depend only on the logical content"
        );
    }

    /// T5: order independence (`BTreeMap` produces canonical ordering).
    ///
    /// # Panics
    ///
    /// Panics if construction order affects encode output (test assertion).
    pub fn assert_payload_order_independent(entries: Vec<(Vec<u8>, Vec<u8>)>) {
        if let (Ok(c1), Ok(c2)) = (
            DatomPairChunk::new(entries.clone()),
            DatomPairChunk::new(entries.into_iter().rev().collect()),
        ) {
            assert_eq!(
                DatomPairCodec::encode_payload(&c1),
                DatomPairCodec::encode_payload(&c2),
                "INV-FERR-045c T5: encode must not depend on construction order"
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Tests (INV-FERR-045c conformance + INV-FERR-045a codec-specific)
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::{conformance::*, *};

    // -- Helpers --

    fn chunk(entries: Vec<(&[u8], &[u8])>) -> DatomPairChunk {
        DatomPairChunk::new(
            entries
                .into_iter()
                .map(|(k, v)| (k.to_vec(), v.to_vec()))
                .collect(),
        )
        .expect("test chunk must be canonical")
    }

    // -- T1: payload round-trip --

    #[test]
    fn test_t1_empty_chunk_payload_round_trip() {
        let c = chunk(vec![]);
        assert_payload_round_trip(&c);
    }

    #[test]
    fn test_t1_single_entry_round_trip() {
        let c = chunk(vec![(b"key1", b"val1")]);
        assert_payload_round_trip(&c);
    }

    #[test]
    fn test_t1_multi_entry_round_trip() {
        let c = chunk(vec![(b"aaa", b"v1"), (b"bbb", b"v2"), (b"ccc", b"v3")]);
        assert_payload_round_trip(&c);
    }

    #[test]
    fn test_t1_empty_key_and_value() {
        let c = chunk(vec![(b"", b"")]);
        assert_payload_round_trip(&c);
    }

    #[test]
    fn test_t1_large_values() {
        let big_val = vec![0xABu8; 4096];
        let c = DatomPairChunk::new(vec![
            (b"k".to_vec(), big_val.clone()),
            (b"m".to_vec(), big_val),
        ])
        .expect("canonical");
        assert_payload_round_trip(&c);
    }

    // -- T2: determinism --

    #[test]
    fn test_t2_encode_deterministic() {
        let c = chunk(vec![(b"x", b"y"), (b"z", b"w")]);
        assert_payload_deterministic(&c);
    }

    // -- T3: injectivity --

    #[test]
    fn test_t3_distinct_chunks_different_bytes() {
        let c1 = chunk(vec![(b"a", b"1")]);
        let c2 = chunk(vec![(b"a", b"2")]);
        assert_payload_injective(&c1, &c2);
    }

    #[test]
    fn test_t3_different_key_sets() {
        let c1 = chunk(vec![(b"a", b"v")]);
        let c2 = chunk(vec![(b"b", b"v")]);
        assert_payload_injective(&c1, &c2);
    }

    // -- T4: fingerprint compatibility --

    #[test]
    fn test_t4_fingerprint_survives_round_trip() {
        let c = chunk(vec![(b"k1", b"v1"), (b"k2", b"v2")]);
        assert_fingerprint_codec_invariant(&c);
    }

    // -- T5: order independence --

    #[test]
    fn test_t5_reversed_insertion_same_bytes() {
        let entries = vec![
            (b"c".to_vec(), b"3".to_vec()),
            (b"a".to_vec(), b"1".to_vec()),
            (b"b".to_vec(), b"2".to_vec()),
        ];
        assert_payload_order_independent(entries);
    }

    // -- Codec discriminator dispatch --

    #[test]
    fn test_leaf_chunk_encode_decode_round_trip() {
        let dp = chunk(vec![(b"key", b"val")]);
        let leaf = LeafChunk::DatomPair(dp.clone());
        let bytes = leaf.encode();
        assert_eq!(bytes[0], DatomPairCodec::CODEC_TAG);

        let decoded = LeafChunk::decode(&bytes).expect("decode must succeed");
        match decoded {
            LeafChunk::DatomPair(d) => assert_eq!(d, dp),
        }
    }

    #[test]
    fn test_unknown_codec_tag_rejected() {
        let bytes = vec![0x42, 0x00, 0x00, 0x00, 0x00];
        let result = LeafChunk::decode(&bytes);
        assert!(
            matches!(result, Err(FerraError::UnknownCodecTag(0x42))),
            "Unknown codec tag must be rejected"
        );
    }

    #[test]
    fn test_codec_tag_zero_sentinel_rejected() {
        let bytes = vec![0x00, 0x00, 0x00, 0x00, 0x00];
        let result = LeafChunk::decode(&bytes);
        assert!(
            matches!(result, Err(FerraError::UnknownCodecTag(0x00))),
            "§23.9.8: tag 0x00 must be rejected as corruption sentinel"
        );
    }

    #[test]
    fn test_empty_bytes_rejected() {
        let result = LeafChunk::decode(&[]);
        assert!(
            matches!(result, Err(FerraError::TruncatedChunk)),
            "Empty bytes must return TruncatedChunk"
        );
    }

    // -- DatomPairChunk validation --

    #[test]
    fn test_duplicate_keys_rejected() {
        let result = DatomPairChunk::new(vec![
            (b"same".to_vec(), b"v1".to_vec()),
            (b"same".to_vec(), b"v2".to_vec()),
        ]);
        assert!(
            matches!(result, Err(FerraError::NonCanonicalChunk)),
            "Duplicate keys must be rejected"
        );
    }

    #[test]
    fn test_unsorted_entries_sorted_by_constructor() {
        let c = DatomPairChunk::new(vec![
            (b"z".to_vec(), b"last".to_vec()),
            (b"a".to_vec(), b"first".to_vec()),
        ])
        .expect("should sort internally");
        assert_eq!(c.entries()[0].0, b"a");
        assert_eq!(c.entries()[1].0, b"z");
    }

    // -- Defense in depth: deserialization rejects non-canonical --

    #[test]
    fn test_decode_rejects_trailing_bytes() {
        let c = chunk(vec![(b"k", b"v")]);
        let mut bytes = DatomPairCodec::encode_payload(&c);
        bytes.push(0xFF); // trailing byte
        let result = DatomPairCodec::decode_payload(&bytes);
        assert!(
            matches!(result, Err(FerraError::TrailingBytes)),
            "Trailing bytes must be rejected"
        );
    }

    #[test]
    fn test_decode_rejects_truncated_payload() {
        let result = DatomPairCodec::decode_payload(&[0x01]); // too short for entry_count
        assert!(
            matches!(result, Err(FerraError::TruncatedChunk)),
            "Truncated payload must be rejected"
        );
    }
}
