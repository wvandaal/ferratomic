//! Snapshot isolation integration tests.
//!
//! INV-FERR-006, INV-FERR-007, INV-FERR-011.
//! ALL TESTS MUST FAIL (red phase). Types are not yet implemented.

use ferratom::{AgentId, Attribute, EntityId, Op, TxId, Value};
use ferratomic_core::observer::Observer;
use ferratomic_core::store::Store;
use ferratomic_core::writer::Transaction;

/// INV-FERR-006: Snapshot is stable — does not change after later writes.
#[test]
fn inv_ferr_006_snapshot_stability() {
    let mut store = Store::genesis();
    let agent = AgentId::from_bytes([1u8; 16]);

    // Commit first transaction
    let tx1 = Transaction::new(agent.clone())
        .assert_datom(
            EntityId::from_content(b"e1"),
            Attribute::from("db/doc"),
            Value::String("Alice".into()),
        )
        .commit(store.schema())
        .expect("valid tx");
    store.transact(tx1).expect("transact failed");

    // Take snapshot
    let snap = store.snapshot();
    let snap_count = snap.datoms().count();

    // Commit second transaction AFTER snapshot
    let tx2 = Transaction::new(agent)
        .assert_datom(
            EntityId::from_content(b"e2"),
            Attribute::from("db/doc"),
            Value::String("Bob".into()),
        )
        .commit(store.schema())
        .expect("valid tx");
    store.transact(tx2).expect("transact failed");

    // Snapshot must NOT see the second transaction
    let snap_count_after = snap.datoms().count();
    assert_eq!(
        snap_count, snap_count_after,
        "INV-FERR-006: snapshot changed after later transaction. \
         before={}, after={}",
        snap_count, snap_count_after
    );
}

/// INV-FERR-006: Concurrent reads don't see in-progress writes.
#[test]
fn inv_ferr_006_concurrent_read_write() {
    let mut store = Store::genesis();
    let agent = AgentId::from_bytes([1u8; 16]);

    // Snapshot before any user transactions
    let snap_before = store.snapshot();
    let count_before = snap_before.datoms().count();

    // Commit a transaction
    let tx = Transaction::new(agent)
        .assert_datom(
            EntityId::from_content(b"e1"),
            Attribute::from("db/doc"),
            Value::String("Alice".into()),
        )
        .commit(store.schema())
        .expect("valid tx");
    store.transact(tx).expect("transact failed");

    // Snapshot after transaction
    let snap_after = store.snapshot();
    let count_after = snap_after.datoms().count();

    // snap_before must not have changed
    assert_eq!(
        count_before,
        snap_before.datoms().count(),
        "INV-FERR-006: pre-transaction snapshot was mutated"
    );

    // snap_after must see the new datoms
    assert!(
        count_after > count_before,
        "INV-FERR-006: post-transaction snapshot missing new datoms. \
         before={}, after={}",
        count_before, count_after
    );
}

/// INV-FERR-007: Epochs are strictly monotonically increasing.
#[test]
fn inv_ferr_007_epoch_ordering() {
    let mut store = Store::genesis();
    let agent = AgentId::from_bytes([1u8; 16]);

    let mut epochs = Vec::new();
    for i in 0..5i64 {
        let tx = Transaction::new(agent.clone())
            .assert_datom(
                EntityId::from_content(format!("e{}", i).as_bytes()),
                Attribute::from("tx/provenance"),
                Value::String(format!("test-{i}").into()),
            )
            .commit(store.schema())
            .expect("valid tx");
        let receipt = store.transact(tx).expect("transact failed");
        epochs.push(receipt.epoch());
    }

    for i in 1..epochs.len() {
        assert!(
            epochs[i] > epochs[i - 1],
            "INV-FERR-007: epoch did not strictly increase. \
             epoch[{}]={}, epoch[{}]={}",
            i - 1,
            epochs[i - 1],
            i,
            epochs[i]
        );
    }
}

/// INV-FERR-011: Observer epoch is monotonically non-decreasing.
#[test]
fn inv_ferr_011_observer_epoch_monotonic() {
    let mut store = Store::genesis();
    let agent = AgentId::from_bytes([1u8; 16]);
    let observer = Observer::new(AgentId::from_bytes([2u8; 16]));

    let mut prev_epoch = 0u64;

    for i in 0..10i64 {
        let tx = Transaction::new(agent.clone())
            .assert_datom(
                EntityId::from_content(format!("e{}", i).as_bytes()),
                Attribute::from("tx/provenance"),
                Value::String(format!("test-{i}").into()),
            )
            .commit(store.schema())
            .expect("valid tx");
        store.transact(tx).expect("transact failed");

        let snap = observer.observe(&store);
        let epoch = snap.epoch();

        assert!(
            epoch >= prev_epoch,
            "INV-FERR-011: observer epoch regressed. prev={}, current={}",
            prev_epoch, epoch
        );
        prev_epoch = epoch;
    }
}
