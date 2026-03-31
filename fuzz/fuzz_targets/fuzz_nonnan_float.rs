//! Fuzz target: NonNanFloat deserialization (invariant oracle).
//!
//! CR-003 / INV-FERR-012: NonNanFloat must NEVER contain NaN after
//! deserialization. The custom Deserialize impl rejects NaN, but a
//! fuzzer can explore all possible f64 bit patterns to verify this.

#![no_main]
use libfuzzer_sys::fuzz_target;

use ferratom::NonNanFloat;

fuzz_target!(|data: &[u8]| {
    // Guard: f64 is 8 bytes; bincode overhead is small.
    if data.len() > 64 {
        return;
    }

    // Attempt to deserialize arbitrary bytes as NonNanFloat.
    let result: Result<NonNanFloat, _> = bincode::deserialize(data);

    if let Ok(f) = result {
        // INVARIANT: if deserialization succeeds, the value must NOT be NaN.
        // INV-FERR-012: NaN breaks deterministic hashing.
        assert!(
            !f.into_inner().is_nan(),
            "CR-003 VIOLATION: NonNanFloat deserialized to NaN! \
             Input bytes: {data:?}"
        );
    }
    // Err is expected for most random bytes — not a bug.
});
