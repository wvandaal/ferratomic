use std::sync::Arc;

use ferratom::{Attribute, EntityId, Value};

use super::*;
use crate::writer::Transaction;

#[test]
fn test_inv_ferr_013_roundtrip_empty() {
    let store = Store::genesis();
    let dir = tempfile::TempDir::new().unwrap();
    let path = dir.path().join("checkpoint.chkp");

    write_checkpoint(&store, &path).unwrap();
    let loaded = load_checkpoint(&path).unwrap();

    assert_eq!(loaded.epoch(), store.epoch());
    assert_eq!(loaded.len(), store.len());
    assert_eq!(*loaded.datom_set(), *store.datom_set());
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
    let loaded = load_checkpoint(&path).unwrap();

    assert_eq!(
        *loaded.datom_set(),
        *store.datom_set(),
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
    assert!(
        loaded.indexes().verify_bijection(),
        "INV-FERR-005: all indexes must have same cardinality after load"
    );
    assert_eq!(
        loaded.indexes().len(),
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
