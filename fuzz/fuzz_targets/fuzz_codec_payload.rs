//! Fuzz target: `DatomPairCodec::decode_payload` deserialization (INV-FERR-045a).
//!
//! Throws arbitrary bytes at the codec payload parser to find panics,
//! OOM, or non-canonical acceptance. If parsing succeeds, the round-trip
//! `encode_payload(decode_payload(data)) == data` is verified.
//!
//! GOALS.md §6.4: deserialization code paths must have fuzz targets.

#![no_main]
use libfuzzer_sys::fuzz_target;

use ferratomic_positional::codec::DatomPairCodec;

fuzz_target!(|data: &[u8]| {
    // Guard: reject inputs >256KB.
    if data.len() > 256 * 1024 {
        return;
    }

    // Crash oracle: parsing arbitrary bytes must not panic.
    let result = DatomPairCodec::decode_payload(data);

    // Round-trip oracle: if parsing succeeds, re-encoding must produce
    // byte-identical output (the codec format is canonical).
    if let Ok(chunk) = result {
        let reserialized = DatomPairCodec::encode_payload(&chunk);
        assert_eq!(
            data, reserialized.as_slice(),
            "INV-FERR-045a: codec payload round-trip violated — \
             decode_payload accepted non-canonical input"
        );
    }
});
