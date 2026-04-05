use std::sync::Arc;

use ferratom::{Attribute, EntityId, Value};

use super::*;
use crate::{store::StoreRepr, writer::Transaction};

#[test]
fn test_inv_ferr_013_roundtrip_empty() {
    let store = Store::genesis();
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("checkpoint.chkp");

    write_checkpoint(&store, &path).unwrap();
    let loaded = load_checkpoint(&path).unwrap();

    assert_eq!(loaded.epoch(), store.epoch());
    assert_eq!(loaded.len(), store.len());
    assert_eq!(loaded.datom_set(), store.datom_set());
    assert_eq!(loaded.schema().len(), store.schema().len());
}

#[test]
fn test_inv_ferr_013_roundtrip_with_datoms() {
    let mut store = Store::genesis();

    let tx = Transaction::new(store.genesis_agent())
        .assert_datom(
            EntityId::from_content(b"entity-1"),
            Attribute::from("db/doc"),
            Value::String(Arc::from("hello world")),
        )
        .commit_unchecked();
    store.transact_test(tx).unwrap();

    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("checkpoint.chkp");

    write_checkpoint(&store, &path).unwrap();
    let mut loaded = load_checkpoint(&path).unwrap();

    assert_eq!(
        loaded.datom_set(),
        store.datom_set(),
        "INV-FERR-013: datom set must be identical after roundtrip"
    );
    assert_eq!(
        loaded.epoch(),
        store.epoch(),
        "INV-FERR-013: epoch must be preserved"
    );
    assert_eq!(
        loaded.schema().len(),
        store.schema().len(),
        "INV-FERR-013: schema must be preserved"
    );
    // bd-h2fz: from_checkpoint builds Positional repr. Promote to
    // OrdMap to verify index bijection via the Indexes API.
    loaded.promote();
    assert!(
        loaded.indexes().unwrap().verify_bijection(),
        "INV-FERR-005: all indexes must have same cardinality after load"
    );
    assert_eq!(
        loaded.indexes().unwrap().len(),
        loaded.len(),
        "INV-FERR-005: index len must match primary after load"
    );
}

#[test]
fn test_inv_ferr_013_corrupted_rejected() {
    let store = Store::genesis();
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("checkpoint.chkp");

    write_checkpoint(&store, &path).unwrap();

    let mut data = std::fs::read(&path).unwrap();
    let midpoint = data.len() / 2;
    data[midpoint] ^= 0xFF;
    std::fs::write(&path, &data).unwrap();

    let result = load_checkpoint(&path);
    assert!(
        result.is_err(),
        "INV-FERR-013: corrupted checkpoint must be rejected"
    );
}

#[test]
fn test_inv_ferr_013_truncated_rejected() {
    let store = Store::genesis();
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("checkpoint.chkp");

    write_checkpoint(&store, &path).unwrap();

    let data = std::fs::read(&path).unwrap();
    std::fs::write(&path, &data[..data.len() / 2]).unwrap();

    let result = load_checkpoint(&path);
    assert!(
        result.is_err(),
        "INV-FERR-013: truncated checkpoint must be rejected"
    );
}

#[test]
fn test_inv_ferr_013_wrong_magic_rejected() {
    let store = Store::genesis();
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("checkpoint.chkp");

    write_checkpoint(&store, &path).unwrap();

    let mut data = std::fs::read(&path).unwrap();
    data[0..4].copy_from_slice(b"XXXX");
    let content_len = data.len() - HASH_SIZE;
    let hash = blake3::hash(&data[..content_len]);
    data[content_len..].copy_from_slice(hash.as_bytes());
    std::fs::write(&path, &data).unwrap();

    let result = load_checkpoint(&path);
    assert!(
        result.is_err(),
        "INV-FERR-013: wrong magic must be rejected"
    );
}

#[test]
fn test_inv_ferr_013_deterministic_output() {
    let store = Store::genesis();
    let dir = tempfile::TempDir::new().unwrap();

    let path1 = dir.path().join("a.chkp");
    let path2 = dir.path().join("b.chkp");

    write_checkpoint(&store, &path1).unwrap();
    write_checkpoint(&store, &path2).unwrap();

    let data1 = std::fs::read(&path1).unwrap();
    let data2 = std::fs::read(&path2).unwrap();

    assert_eq!(
        data1, data2,
        "INV-FERR-031: genesis checkpoint must be deterministic"
    );
}

// ---------------------------------------------------------------------------
// V3 checkpoint tests
// ---------------------------------------------------------------------------

#[test]
fn test_inv_ferr_013_v3_genesis_roundtrip() {
    let store = Store::genesis();
    let bytes = v3::serialize_v3_bytes(&store).unwrap();
    let loaded = v3::deserialize_v3_bytes(&bytes).unwrap();

    assert_eq!(
        loaded.epoch(),
        store.epoch(),
        "INV-FERR-013: V3 genesis epoch must survive roundtrip"
    );
    assert_eq!(
        loaded.len(),
        store.len(),
        "INV-FERR-013: V3 genesis datom count must survive roundtrip"
    );
    assert_eq!(
        loaded.datom_set(),
        store.datom_set(),
        "INV-FERR-013: V3 genesis datom set must survive roundtrip"
    );
    assert_eq!(
        loaded.schema().len(),
        store.schema().len(),
        "INV-FERR-013: V3 genesis schema must survive roundtrip"
    );
    // Verify V3 magic at start of bytes.
    assert_eq!(
        &bytes[0..4],
        b"CHK3",
        "V3 checkpoint must start with CHK3 magic"
    );
}

#[test]
fn test_inv_ferr_013_v3_store_roundtrip() {
    let mut store = Store::genesis();

    let tx = Transaction::new(store.genesis_agent())
        .assert_datom(
            EntityId::from_content(b"v3-entity-1"),
            Attribute::from("db/doc"),
            Value::String(Arc::from("V3 checkpoint test")),
        )
        .commit_unchecked();
    store.transact_test(tx).unwrap();

    let bytes = v3::serialize_v3_bytes(&store).unwrap();
    let loaded = v3::deserialize_v3_bytes(&bytes).unwrap();

    assert_eq!(
        loaded.datom_set(),
        store.datom_set(),
        "INV-FERR-013: V3 datom set must survive roundtrip"
    );
    assert_eq!(
        loaded.epoch(),
        store.epoch(),
        "INV-FERR-013: V3 epoch must survive roundtrip"
    );
    assert_eq!(
        loaded.schema().len(),
        store.schema().len(),
        "INV-FERR-013: V3 schema must survive roundtrip"
    );

    // Verify Positional variant (zero-construction cold start).
    assert!(
        matches!(loaded.repr, StoreRepr::Positional(_)),
        "INV-FERR-076: V3 deserialization must produce Positional repr"
    );

    // Promote and verify index bijection.
    let mut promoted = loaded;
    promoted.promote();
    assert!(
        promoted.indexes().unwrap().verify_bijection(),
        "INV-FERR-005: all indexes must have same cardinality after V3 load"
    );
}

#[test]
fn test_inv_ferr_013_v2_v3_equivalence() {
    let mut store = Store::genesis();

    let tx = Transaction::new(store.genesis_agent())
        .assert_datom(
            EntityId::from_content(b"equiv-entity"),
            Attribute::from("db/doc"),
            Value::String(Arc::from("equivalence test")),
        )
        .commit_unchecked();
    store.transact_test(tx).unwrap();

    // Serialize with V2 and V3, load both, compare datom sets.
    let v2_bytes = serialize_v2_bytes(&store).unwrap();
    let v3_bytes = v3::serialize_v3_bytes(&store).unwrap();

    let v2_loaded = deserialize_v2_bytes(&v2_bytes).unwrap();
    let v3_loaded = v3::deserialize_v3_bytes(&v3_bytes).unwrap();

    assert_eq!(
        v2_loaded.datom_set(),
        v3_loaded.datom_set(),
        "INV-FERR-013: V2 and V3 must produce identical datom sets"
    );
    assert_eq!(
        v2_loaded.epoch(),
        v3_loaded.epoch(),
        "INV-FERR-013: V2 and V3 must produce identical epoch"
    );
    assert_eq!(
        v2_loaded.schema().len(),
        v3_loaded.schema().len(),
        "INV-FERR-013: V2 and V3 must produce identical schema"
    );

    // Magic dispatch: V2 bytes dispatched correctly.
    let dispatched_v2 = deserialize_checkpoint_bytes(&v2_bytes).unwrap();
    assert_eq!(
        dispatched_v2.datom_set(),
        store.datom_set(),
        "Magic dispatch must correctly handle V2 bytes"
    );
    // Magic dispatch: V3 bytes dispatched correctly.
    let dispatched_v3 = deserialize_checkpoint_bytes(&v3_bytes).unwrap();
    assert_eq!(
        dispatched_v3.datom_set(),
        store.datom_set(),
        "Magic dispatch must correctly handle V3 bytes"
    );
}

#[test]
fn test_inv_ferr_013_v3_corrupted_hash() {
    let store = Store::genesis();
    let mut bytes = v3::serialize_v3_bytes(&store).unwrap();

    // Flip a bit in the middle of the payload (before the hash).
    let midpoint = bytes.len() / 2;
    bytes[midpoint] ^= 0xFF;

    let result = v3::deserialize_v3_bytes(&bytes);
    assert!(
        result.is_err(),
        "INV-FERR-013: V3 corrupted hash must be rejected"
    );
}

#[test]
fn test_inv_ferr_013_v3_truncated() {
    let store = Store::genesis();
    let bytes = v3::serialize_v3_bytes(&store).unwrap();

    // Truncate to less than minimum size.
    let truncated = &bytes[..10];
    let result = v3::deserialize_v3_bytes(truncated);
    assert!(
        result.is_err(),
        "INV-FERR-013: V3 truncated data must be rejected"
    );
}

#[test]
fn test_inv_ferr_013_v3_live_bits_mismatch() {
    #[derive(serde::Deserialize, serde::Serialize)]
    struct TamperPayload {
        schema_pairs: Vec<(String, ferratom::AttributeDef)>,
        datoms: Vec<ferratom::wire::WireDatom>,
        live_bits: bitvec::prelude::BitVec<u64, bitvec::prelude::Lsb0>,
    }

    let mut store = Store::genesis();

    let tx = Transaction::new(store.genesis_agent())
        .assert_datom(
            EntityId::from_content(b"live-bits-test"),
            Attribute::from("db/doc"),
            Value::String(Arc::from("mismatch test")),
        )
        .commit_unchecked();
    store.transact_test(tx).unwrap();

    let bytes = v3::serialize_v3_bytes(&store).unwrap();

    // Deserialize the raw payload to tamper with live_bits length.
    // Strategy: take a valid V3, deserialize its payload, alter live_bits,
    // re-serialize, and recompute the BLAKE3 hash.
    let header_size: usize = 4 + 2 + 8 + 16; // V3_HEADER_SIZE
    let hash_size: usize = 32;

    // Extract header.
    let header = &bytes[..header_size];
    // Extract payload (between header and hash).
    let payload_bytes = &bytes[header_size..bytes.len() - hash_size];

    let mut payload: TamperPayload = bincode::deserialize(payload_bytes).unwrap();

    // Add extra bits to make length mismatch.
    payload.live_bits.push(true);
    payload.live_bits.push(false);

    let tampered_payload = bincode::serialize(&payload).unwrap();

    // Rebuild: header + tampered payload + fresh BLAKE3.
    let mut tampered = Vec::with_capacity(header.len() + tampered_payload.len() + hash_size);
    tampered.extend_from_slice(header);
    tampered.extend_from_slice(&tampered_payload);
    let hash = blake3::hash(&tampered);
    tampered.extend_from_slice(hash.as_bytes());

    let result = v3::deserialize_v3_bytes(&tampered);
    assert!(
        result.is_err(),
        "INV-FERR-013: V3 live_bits length mismatch must be rejected"
    );
}

// ---------------------------------------------------------------------------
// LIVE-first V3 tests (INV-FERR-075)
// ---------------------------------------------------------------------------

#[test]
fn test_inv_ferr_075_live_first_roundtrip() {
    // Full round-trip: serialize LIVE-first -> deserialize -> same datom set.
    let mut store = Store::genesis();

    let tx = Transaction::new(store.genesis_agent())
        .assert_datom(
            EntityId::from_content(b"entity-1"),
            Attribute::from("db/doc"),
            Value::String(Arc::from("hello")),
        )
        .commit_unchecked();
    store.transact_test(tx).unwrap();

    let bytes = serialize_live_first_bytes(&store).expect("serialize LIVE-first");
    let loaded = deserialize_checkpoint_bytes(&bytes).expect("deserialize via version dispatch");

    assert_eq!(
        loaded.datom_set(),
        store.datom_set(),
        "INV-FERR-075: full round-trip must preserve datom set"
    );
    assert_eq!(loaded.epoch(), store.epoch());
    assert_eq!(
        loaded.schema(),
        store.schema(),
        "INV-FERR-013: schema must be preserved in LIVE-first round-trip"
    );
}

#[test]
fn test_inv_ferr_075_partial_store_live_only() {
    // PartialStore.store() has only LIVE datoms.
    let mut store = Store::genesis();

    // Assert then retract to create historical datoms.
    let tx1 = Transaction::new(store.genesis_agent())
        .assert_datom(
            EntityId::from_content(b"entity-1"),
            Attribute::from("db/doc"),
            Value::String(Arc::from("v1")),
        )
        .commit_unchecked();
    store.transact_test(tx1).unwrap();

    let tx2 = Transaction::new(store.genesis_agent())
        .retract_datom(
            EntityId::from_content(b"entity-1"),
            Attribute::from("db/doc"),
            Value::String(Arc::from("v1")),
        )
        .commit_unchecked();
    store.transact_test(tx2).unwrap();

    let bytes = serialize_live_first_bytes(&store).expect("serialize");

    // Verify via full dispatch: LIVE-first round-trip preserves all datoms.
    let full_via_dispatch =
        deserialize_checkpoint_bytes(&bytes).expect("dispatch handles LIVE-first");
    assert_eq!(
        full_via_dispatch.datom_set(),
        store.datom_set(),
        "INV-FERR-075: LIVE-first full round-trip must preserve datom set"
    );

    // Verify via partial path: load LIVE-only, then merge historical.
    let partial = v3::deserialize_v3_live_first_partial(&bytes).expect("partial load");

    // Compute expected LIVE count independently from the full store.
    let expected_live_count = {
        let ps = crate::positional::PositionalStore::from_datoms(store.datoms().cloned());
        ps.live_count()
    };

    // The partial store's LIVE-only store must have exactly the LIVE datom count.
    assert_eq!(
        partial.live_store().len(),
        expected_live_count,
        "INV-FERR-075: partial LIVE-only store must have exactly {expected_live_count} datoms"
    );

    // The partial store should have fewer datoms than the full store
    // because the retracted entity-1 datoms are historical.
    let full = partial.load_historical();
    assert_eq!(
        full.datom_set(),
        store.datom_set(),
        "INV-FERR-075: load_historical must recover full datom set"
    );
    // The full store has more datoms than LIVE (assert+retract = 2 non-LIVE datoms).
    assert!(
        expected_live_count < store.len(),
        "INV-FERR-075: LIVE count ({expected_live_count}) must be < full count ({})",
        store.len(),
    );
}

#[test]
fn test_inv_ferr_075_version_dispatch() {
    // Version 0x0003 dispatches to standard V3, 0x0103 to LIVE-first.
    let store = Store::genesis();

    let v3_bytes = serialize_checkpoint_bytes(&store).expect("standard V3");
    let lf_bytes = serialize_live_first_bytes(&store).expect("LIVE-first");

    // Both deserialize through the main dispatch.
    let v3_loaded = deserialize_checkpoint_bytes(&v3_bytes).expect("V3 dispatch");
    let lf_loaded = deserialize_checkpoint_bytes(&lf_bytes).expect("LIVE-first dispatch");

    assert_eq!(v3_loaded.datom_set(), store.datom_set());
    assert_eq!(lf_loaded.datom_set(), store.datom_set());

    // Check version bytes differ.
    assert_eq!(v3_bytes[4..6], [3, 0], "standard V3 version");
    assert_eq!(lf_bytes[4..6], [3, 1], "LIVE-first version 0x0103");
}

#[test]
fn test_inv_ferr_075_100_percent_live() {
    // All datoms LIVE (no retractions) -> hist_datoms empty.
    let mut store = Store::genesis();

    let tx = Transaction::new(store.genesis_agent())
        .assert_datom(
            EntityId::from_content(b"all-live"),
            Attribute::from("db/doc"),
            Value::String(Arc::from("alive")),
        )
        .commit_unchecked();
    store.transact_test(tx).unwrap();

    let bytes = serialize_live_first_bytes(&store).expect("serialize");
    // Full round-trip via dispatch.
    let loaded = deserialize_checkpoint_bytes(&bytes).expect("dispatch");
    assert_eq!(
        loaded.datom_set(),
        store.datom_set(),
        "INV-FERR-075: 100%% LIVE round-trip must preserve datom set"
    );
    // Partial then historical must also recover everything.
    let partial = v3::deserialize_v3_live_first_partial(&bytes).expect("partial");
    let full = partial.load_historical();
    assert_eq!(full.datom_set(), store.datom_set());
}

#[test]
fn test_inv_ferr_075_genesis_only() {
    // Genesis store (0 user datoms, schema only).
    let store = Store::genesis();

    let bytes = serialize_live_first_bytes(&store).expect("serialize");
    let loaded = deserialize_checkpoint_bytes(&bytes).expect("dispatch");
    assert_eq!(loaded.datom_set(), store.datom_set());
    assert_eq!(loaded.epoch(), store.epoch());
}

#[test]
fn test_inv_ferr_075_live_only_query() {
    // Verify the LIVE-only store answers current-state queries correctly
    // WITHOUT calling load_historical() (INV-FERR-075 core purpose).
    let mut store = Store::genesis();

    // Assert entity "alive" with a known value.
    let tx = Transaction::new(store.genesis_agent())
        .assert_datom(
            EntityId::from_content(b"alive"),
            Attribute::from("db/doc"),
            Value::String(Arc::from("present")),
        )
        .commit_unchecked();
    store.transact_test(tx).unwrap();

    let bytes = serialize_live_first_bytes(&store).expect("serialize");
    let partial = v3::deserialize_v3_live_first_partial(&bytes).expect("partial");

    // Query the LIVE-only store via the accessor.
    let live = partial.live_store();

    // The LIVE store should contain the "alive" entity's datom.
    let live_datoms: Vec<_> = live.datoms().collect();
    let has_alive = live_datoms
        .iter()
        .any(|d| d.entity() == EntityId::from_content(b"alive"));
    assert!(
        has_alive,
        "INV-FERR-075: LIVE-only store must contain the asserted entity"
    );

    // The LIVE store should have the same datom count as the full store's LIVE set.
    let full_live_count = {
        let ps = crate::positional::PositionalStore::from_datoms(store.datoms().cloned());
        ps.live_count()
    };
    assert_eq!(
        live.len(),
        full_live_count,
        "INV-FERR-075: LIVE-only store datom count must match full store's LIVE count"
    );

    // Verify load_historical still works after reading live_store.
    let full = partial.load_historical();
    assert_eq!(full.datom_set(), store.datom_set());
}

#[test]
fn test_inv_ferr_075_mixed_live_groups() {
    // Multiple entities: some live, some retracted.
    // Tests that LIVE partitioning is per-(e,a,v) group, not global.
    let mut store = Store::genesis();

    // Entity "alive" — Assert only → LIVE.
    let tx1 = Transaction::new(store.genesis_agent())
        .assert_datom(
            EntityId::from_content(b"alive"),
            Attribute::from("db/doc"),
            Value::String(Arc::from("present")),
        )
        .commit_unchecked();
    store.transact_test(tx1).unwrap();

    // Entity "dead" — Assert then Retract → NOT live.
    let tx2 = Transaction::new(store.genesis_agent())
        .assert_datom(
            EntityId::from_content(b"dead"),
            Attribute::from("db/doc"),
            Value::String(Arc::from("gone")),
        )
        .commit_unchecked();
    store.transact_test(tx2).unwrap();

    let tx3 = Transaction::new(store.genesis_agent())
        .retract_datom(
            EntityId::from_content(b"dead"),
            Attribute::from("db/doc"),
            Value::String(Arc::from("gone")),
        )
        .commit_unchecked();
    store.transact_test(tx3).unwrap();

    // Entity "also-alive" — Assert only → LIVE.
    let tx4 = Transaction::new(store.genesis_agent())
        .assert_datom(
            EntityId::from_content(b"also-alive"),
            Attribute::from("db/doc"),
            Value::String(Arc::from("here")),
        )
        .commit_unchecked();
    store.transact_test(tx4).unwrap();

    let bytes = serialize_live_first_bytes(&store).expect("serialize");

    // Full round-trip preserves all datoms.
    let loaded = deserialize_checkpoint_bytes(&bytes).expect("dispatch");
    assert_eq!(
        loaded.datom_set(),
        store.datom_set(),
        "INV-FERR-075: mixed-group round-trip must preserve datom set"
    );

    // Partial → full also preserves all datoms.
    let partial = v3::deserialize_v3_live_first_partial(&bytes).expect("partial");
    let full = partial.load_historical();
    assert_eq!(
        full.datom_set(),
        store.datom_set(),
        "INV-FERR-075: mixed-group load_historical must recover full datom set"
    );
}

// ---------------------------------------------------------------------------
// LIVE-first V3 error-path tests (INV-FERR-013 / INV-FERR-075)
// ---------------------------------------------------------------------------

#[test]
fn test_inv_ferr_075_corrupted_live_first_rejected() {
    // Bit-flip in LIVE-first payload must be detected by BLAKE3.
    let store = Store::genesis();
    let mut bytes = serialize_live_first_bytes(&store).expect("serialize");

    // Flip a bit in the payload (after the 30-byte header).
    if bytes.len() > 35 {
        bytes[35] ^= 0x01;
    }

    let result = deserialize_checkpoint_bytes(&bytes);
    assert!(
        result.is_err(),
        "INV-FERR-013: corrupted LIVE-first data must be rejected"
    );
}

#[test]
fn test_inv_ferr_075_truncated_live_first_rejected() {
    // Truncated LIVE-first data must be rejected.
    let store = Store::genesis();
    let bytes = serialize_live_first_bytes(&store).expect("serialize");

    // Truncate to just the header.
    let truncated = &bytes[..30];

    let result = deserialize_checkpoint_bytes(truncated);
    assert!(
        result.is_err(),
        "INV-FERR-013: truncated LIVE-first data must be rejected"
    );
}

#[test]
fn test_inv_ferr_075_version_cross_rejection() {
    // LIVE-first bytes (version 0x0103) fed to standard V3 deserializer
    // must be rejected (version mismatch).
    let store = Store::genesis();
    let bytes = serialize_live_first_bytes(&store).expect("serialize");

    // Direct call to standard V3 deserializer (expects version 3).
    let result = v3::deserialize_v3_bytes(&bytes);
    assert!(
        result.is_err(),
        "INV-FERR-075: LIVE-first bytes (0x0103) must be rejected by standard V3 deserializer"
    );
}
