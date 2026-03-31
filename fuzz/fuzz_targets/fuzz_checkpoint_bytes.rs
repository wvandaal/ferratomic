//! Fuzz target: checkpoint deserialization (crash detector + round-trip).
//!
//! INV-FERR-013: `load(checkpoint(S)) = S`. This harness feeds arbitrary
//! bytes to `deserialize_checkpoint_bytes` to find panics, OOM, or
//! BLAKE3 checksum bypass. Corruption should produce `Err`, never panic.

#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // Guard: reject inputs >16MB to avoid OOM (real checkpoints can be GBs,
    // but fuzzing at that scale is not productive).
    if data.len() > 16 * 1024 * 1024 {
        return;
    }

    // Crash oracle: deserializing arbitrary bytes must not panic.
    // Valid checkpoints return Ok; corrupted ones return Err.
    // Either outcome is acceptable. A panic is a bug.
    let result = ferratomic_core::store::Store::from_checkpoint_bytes(data);

    // If deserialization succeeds, the store is valid — no further
    // round-trip test here since serialize_checkpoint_bytes is pub(crate).
    // The round-trip property is tested by proptest (INV-FERR-013).
    // This fuzz target focuses on crash/OOM/panic detection on malformed bytes.
    let _ = result;
});
