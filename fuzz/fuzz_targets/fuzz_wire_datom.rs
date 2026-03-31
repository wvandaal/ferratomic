//! Fuzz target: WireDatom bincode deserialization (round-trip oracle).
//!
//! ADR-FERR-010: Wire types are the ONLY types that touch untrusted bytes.
//! This harness throws arbitrary bytes at `bincode::deserialize::<Vec<WireDatom>>`
//! to find panics, OOM, or deserialization bypass. If deserialization succeeds,
//! the round-trip `serialize(into_trusted(wire)) == serialize(original)` is checked.

#![no_main]
use libfuzzer_sys::fuzz_target;

use ferratom::wire::WireDatom;

fuzz_target!(|data: &[u8]| {
    // Guard: reject inputs >4MB (WAL payloads have a 256MB limit,
    // but fuzzing large payloads is not productive).
    if data.len() > 4 * 1024 * 1024 {
        return;
    }

    // Crash oracle: deserializing arbitrary bytes as WireDatom must not panic.
    let result: Result<Vec<WireDatom>, _> = bincode::deserialize(data);

    // Round-trip oracle: if deserialization succeeds, converting through
    // the trust boundary and re-serializing must produce identical bytes.
    if let Ok(wire_datoms) = result {
        // Convert through trust boundary
        let core_datoms: Vec<ferratom::Datom> = wire_datoms
            .into_iter()
            .map(WireDatom::into_trusted)
            .collect();

        // Re-serialize core datoms (they keep Serialize)
        if let Ok(reserialized) = bincode::serialize(&core_datoms) {
            // Deserialize again as wire types
            let wire2: Vec<WireDatom> = bincode::deserialize(&reserialized)
                .expect("re-serialized datoms must deserialize as WireDatom");
            let core2: Vec<ferratom::Datom> = wire2
                .into_iter()
                .map(WireDatom::into_trusted)
                .collect();

            assert_eq!(
                core_datoms, core2,
                "ADR-FERR-010: wire round-trip violated: \
                 serialize(into_trusted(deser(bytes))) != into_trusted(deser(serialize(...)))"
            );
        }
    }
});
