//! Fuzz target: `Datom::from_canonical_bytes` deserialization (INV-FERR-086).
//!
//! Throws arbitrary bytes at the canonical datom parser to find panics,
//! OOM, or non-canonical acceptance. If parsing succeeds, the round-trip
//! `canonical_bytes(from_canonical_bytes(data)) == data` is verified —
//! any deviation means the parser accepted non-canonical input.
//!
//! GOALS.md §6.4: deserialization code paths must have fuzz targets.

#![no_main]
use libfuzzer_sys::fuzz_target;

use ferratom::Datom;

fuzz_target!(|data: &[u8]| {
    // Guard: reject inputs >64KB (canonical datom bytes are typically
    // <1KB; fuzzing huge inputs is not productive).
    if data.len() > 64 * 1024 {
        return;
    }

    // Crash oracle: parsing arbitrary bytes must not panic.
    let result = Datom::from_canonical_bytes(data);

    // Round-trip oracle: if parsing succeeds, re-serializing must
    // produce byte-identical output (canonical form is unique).
    if let Ok(datom) = result {
        let reserialized = datom.canonical_bytes();
        assert_eq!(
            data, reserialized.as_slice(),
            "INV-FERR-086: canonical_bytes round-trip violated — \
             from_canonical_bytes accepted non-canonical input"
        );

        // Content hash consistency: content_hash must equal BLAKE3 of
        // the canonical bytes (INV-FERR-086 + INV-FERR-012 unification).
        let hash_from_method = datom.content_hash();
        let hash_from_bytes = *blake3::hash(&reserialized).as_bytes();
        assert_eq!(
            hash_from_method, hash_from_bytes,
            "INV-FERR-086: content_hash != BLAKE3(canonical_bytes)"
        );
    }
});
