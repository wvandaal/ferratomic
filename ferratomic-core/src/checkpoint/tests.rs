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
