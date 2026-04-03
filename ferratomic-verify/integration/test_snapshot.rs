//! Snapshot isolation, concurrency, and durability integration tests.
//!
//! INV-FERR-005 (bijection), INV-FERR-006 (snapshot isolation),
//! INV-FERR-007 (epoch ordering), INV-FERR-011 (observer monotonicity),
//! INV-FERR-013 (checkpoint corruption), INV-FERR-015 (HLC monotonicity),
//! INV-FERR-016 (HLC causality), INV-FERR-018 (append-only),
//! INV-FERR-020 (transaction atomicity), INV-FERR-021 (backpressure),
//! INV-FERR-025 (index backend trait), INV-FERR-029 (live resolution),
//! INV-FERR-032 (live correctness).
//! Phase 4a: all tests passing against ferratomic-core implementation.

use std::sync::{Arc, Mutex};

use ferratom::{AgentId, Attribute, EntityId, Value};
use ferratomic_core::{
    db::Database,
    observer::{DatomObserver, Observer},
    store::Store,
    writer::Transaction,
};

#[derive(Debug, Clone, PartialEq, Eq)]
enum ObserverEvent {
    Commit { epoch: u64, count: usize },
    Catchup { from_epoch: u64, count: usize },
}

struct RecordingObserver {
    events: Arc<Mutex<Vec<ObserverEvent>>>,
}

impl RecordingObserver {
    fn new(events: Arc<Mutex<Vec<ObserverEvent>>>) -> Self {
        Self { events }
    }
}

impl DatomObserver for RecordingObserver {
    fn on_commit(&self, epoch: u64, datoms: &[ferratom::Datom]) {
        self.events
            .lock()
            .expect("observer commit events lock")
            .push(ObserverEvent::Commit {
                epoch,
                count: datoms.len(),
            });
    }

    fn on_catchup(&self, from_epoch: u64, datoms: &[ferratom::Datom]) {
        self.events
            .lock()
            .expect("observer catchup events lock")
            .push(ObserverEvent::Catchup {
                from_epoch,
                count: datoms.len(),
            });
    }

    fn name(&self) -> &str {
        "integration-recorder"
    }
}

/// INV-FERR-006: Snapshot is stable — does not change after later writes.
#[test]
fn inv_ferr_006_snapshot_stability() {
    let mut store = Store::genesis();
    let agent = AgentId::from_bytes([1u8; 16]);

    // Commit first transaction
    let tx1 = Transaction::new(agent)
        .assert_datom(
            EntityId::from_content(b"e1"),
            Attribute::from("db/doc"),
            Value::String("Alice".into()),
        )
        .commit(store.schema())
        .expect("valid tx");
    store.transact_test(tx1).expect("transact failed");

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
    store.transact_test(tx2).expect("transact failed");

    // Snapshot must NOT see the second transaction
    let snap_count_after = snap.datoms().count();
    assert_eq!(
        snap_count, snap_count_after,
        "INV-FERR-006: snapshot changed after later transaction. \
         before={}, after={}",
        snap_count, snap_count_after
    );
}

/// INV-FERR-005: Index bijection holds after transacting multiple datoms.
///
/// bd-zws: creates a store, transacts several datoms across multiple
/// transactions, then verifies that Store::verify_bijection returns Ok
/// and all 4 secondary index counts match the primary set.
#[test]
fn test_inv_ferr_005_bijection_after_transact() {
    let mut store = Store::genesis();
    let agent = AgentId::from_bytes([5u8; 16]);

    // Transact several batches of datoms.
    for i in 0..5i64 {
        let tx = Transaction::new(agent)
            .assert_datom(
                EntityId::from_content(format!("bijection-e{}", i).as_bytes()),
                Attribute::from("db/doc"),
                Value::String(format!("bijection-test-{i}").into()),
            )
            .commit(store.schema())
            .expect("valid tx");
        store.transact_test(tx).expect("transact failed");

        // Verify bijection after every transaction.
        assert!(
            store.indexes().unwrap().verify_bijection(),
            "INV-FERR-005: index bijection violated after transact"
        );
    }

    // Final explicit 4-index cardinality check.
    let primary_count = store.len();
    let eavt_count = store.indexes().unwrap().eavt().len();
    let aevt_count = store.indexes().unwrap().aevt().len();
    let vaet_count = store.indexes().unwrap().vaet().len();
    let avet_count = store.indexes().unwrap().avet().len();

    assert_eq!(
        eavt_count, primary_count,
        "INV-FERR-005: EAVT count ({}) != primary count ({})",
        eavt_count, primary_count
    );
    assert_eq!(
        aevt_count, primary_count,
        "INV-FERR-005: AEVT count ({}) != primary count ({})",
        aevt_count, primary_count
    );
    assert_eq!(
        vaet_count, primary_count,
        "INV-FERR-005: VAET count ({}) != primary count ({})",
        vaet_count, primary_count
    );
    assert_eq!(
        avet_count, primary_count,
        "INV-FERR-005: AVET count ({}) != primary count ({})",
        avet_count, primary_count
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
    store.transact_test(tx).expect("transact failed");

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
        count_before,
        count_after
    );
}

/// INV-FERR-007: Epochs are strictly monotonically increasing.
#[test]
fn inv_ferr_007_epoch_ordering() {
    let mut store = Store::genesis();
    let agent = AgentId::from_bytes([1u8; 16]);

    let mut epochs = Vec::new();
    for i in 0..5i64 {
        let tx = Transaction::new(agent)
            .assert_datom(
                EntityId::from_content(format!("e{}", i).as_bytes()),
                Attribute::from("tx/provenance"),
                Value::String(format!("test-{i}").into()),
            )
            .commit(store.schema())
            .expect("valid tx");
        let receipt = store.transact_test(tx).expect("transact failed");
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
        let tx = Transaction::new(agent)
            .assert_datom(
                EntityId::from_content(format!("e{}", i).as_bytes()),
                Attribute::from("tx/provenance"),
                Value::String(format!("test-{i}").into()),
            )
            .commit(store.schema())
            .expect("valid tx");
        store.transact_test(tx).expect("transact failed");

        let snap = observer.observe(&store);
        let epoch = snap.epoch();

        assert!(
            epoch >= prev_epoch,
            "INV-FERR-011: observer epoch regressed. prev={}, current={}",
            prev_epoch,
            epoch
        );
        prev_epoch = epoch;
    }
}

/// INV-FERR-011: registering after writes triggers catch-up delivery.
#[test]
fn inv_ferr_011_database_observer_catchup_delivery() {
    let db = Database::genesis();
    let agent = AgentId::from_bytes([3u8; 16]);

    for i in 0..2i64 {
        let tx = Transaction::new(agent)
            .assert_datom(
                EntityId::from_content(format!("catchup-e{}", i).as_bytes()),
                Attribute::from("db/doc"),
                Value::String(format!("catchup-{i}").into()),
            )
            .commit(&db.schema())
            .expect("valid tx");
        db.transact(tx).expect("transact failed");
    }

    let events = Arc::new(Mutex::new(Vec::new()));
    let observer = Box::new(RecordingObserver::new(Arc::clone(&events)));
    db.register_observer(observer)
        .expect("observer registration should succeed");

    let recorded = events.lock().expect("events lock");
    assert!(
        matches!(recorded.as_slice(), [ObserverEvent::Catchup { from_epoch: 0, count }] if *count > 0),
        "INV-FERR-011: observer must receive catchup delivery after late registration, got {:?}",
        *recorded
    );
}

/// INV-FERR-011: registered observers receive post-commit delivery.
#[test]
fn inv_ferr_011_database_observer_commit_delivery() {
    let db = Database::genesis();
    let events = Arc::new(Mutex::new(Vec::new()));
    let observer = Box::new(RecordingObserver::new(Arc::clone(&events)));
    db.register_observer(observer)
        .expect("observer registration should succeed");

    let tx = Transaction::new(AgentId::from_bytes([4u8; 16]))
        .assert_datom(
            EntityId::from_content(b"observer-db"),
            Attribute::from("db/doc"),
            Value::String("observer".into()),
        )
        .commit(&db.schema())
        .expect("valid tx");
    db.transact(tx).expect("transact failed");

    let recorded = events.lock().expect("events lock");
    assert!(
        recorded.iter().any(|event| {
            matches!(event, ObserverEvent::Commit { epoch: 1, count } if *count > 0)
        }),
        "INV-FERR-011: registered observer must receive on_commit, got {:?}",
        *recorded
    );
}

/// Assert that a `FerraError` is an `InvariantViolation` citing the expected
/// invariant and containing the expected keyword in its details.
fn assert_invariant_violation(
    err: &ferratom::FerraError,
    expected_invariant: &str,
    expected_keyword: &str,
) {
    match err {
        ferratom::FerraError::InvariantViolation { invariant, details } => {
            assert_eq!(
                invariant, expected_invariant,
                "error must cite {expected_invariant}, got invariant={invariant}"
            );
            assert!(
                details.contains(expected_keyword),
                "error details must mention '{expected_keyword}', got details={details}"
            );
        }
        other => {
            panic!("expected InvariantViolation, got {other:?}");
        }
    }
}

/// INV-FERR-007: Epoch overflow must return InvariantViolation, not panic.
///
/// When the epoch counter is at u64::MAX, `Store::transact` calls
/// `checked_add(1)` which overflows. The store must return
/// `FerraError::InvariantViolation` citing INV-FERR-007, never wrap
/// around or panic.
///
/// bd-n1i: error-path test for epoch overflow.
#[test]
fn test_inv_ferr_007_epoch_overflow() {
    let agent = AgentId::from_bytes([7u8; 16]);
    let mut store = Store::from_checkpoint(u64::MAX, agent, Vec::new(), Vec::new());

    let tx = Transaction::new(agent)
        .assert_datom(
            EntityId::from_content(b"overflow-entity"),
            Attribute::from("db/doc"),
            Value::String("should fail".into()),
        )
        .commit_unchecked();

    let result = store.transact_test(tx);
    assert!(
        result.is_err(),
        "INV-FERR-007: transact at u64::MAX must fail"
    );

    assert_invariant_violation(&result.unwrap_err(), "INV-FERR-007", "overflow");
    assert_eq!(
        store.epoch(),
        u64::MAX,
        "INV-FERR-007: epoch must not change after failed transact"
    );
}

/// INV-FERR-015: HLC `tick()` produces strictly increasing `TxId` values.
///
/// Concrete test: create an HLC, tick N times, verify each `TxId` is
/// strictly greater than the previous one.
#[test]
fn inv_ferr_015_hlc_tick_monotonic() {
    use ferratom::{AgentId, HybridClock};

    let agent = AgentId::from_bytes([15u8; 16]);
    let mut clock = HybridClock::new(agent);
    let mut prev = clock.tick();

    for i in 1..100 {
        let next = clock.tick();
        assert!(
            next > prev,
            "INV-FERR-015: tick {} ({:?}) not greater than tick {} ({:?})",
            i,
            next,
            i - 1,
            prev
        );
        prev = next;
    }
}

/// INV-FERR-015: HLC tick() is monotonic even when wall clock has not advanced.
///
/// Two ticks taken in rapid succession (same millisecond) must still produce
/// strictly increasing `TxId` values via the logical counter.
#[test]
fn inv_ferr_015_hlc_same_millisecond_monotonic() {
    use ferratom::{AgentId, HybridClock};

    let agent = AgentId::from_bytes([15u8; 16]);
    let mut clock = HybridClock::new(agent);

    // Rapid-fire ticks in the same millisecond window.
    let t1 = clock.tick();
    let t2 = clock.tick();
    let t3 = clock.tick();

    assert!(t2 > t1, "INV-FERR-015: t2 must exceed t1");
    assert!(t3 > t2, "INV-FERR-015: t3 must exceed t2");
}

/// INV-FERR-016: HLC receive() ensures causality across agents.
///
/// Agent A ticks, sends timestamp to Agent B. Agent B receives, then ticks.
/// Agent B's tick must be strictly greater than Agent A's timestamp.
#[test]
fn inv_ferr_016_hlc_causality_two_agents() {
    use ferratom::{AgentId, HybridClock};

    let agent_a = AgentId::from_bytes([16u8; 16]);
    let agent_b = AgentId::from_bytes([17u8; 16]);
    let mut clock_a = HybridClock::new(agent_a);
    let mut clock_b = HybridClock::new(agent_b);

    // Agent A produces a timestamp.
    let a_tx = clock_a.tick();

    // Agent B receives A's timestamp and ticks.
    clock_b.receive(&a_tx);
    let b_tx = clock_b.tick();

    assert!(
        b_tx > a_tx,
        "INV-FERR-016: B's tick ({:?}) must be causally after A's tick ({:?})",
        b_tx,
        a_tx
    );
}

/// INV-FERR-016: HLC causality chain across three agents.
///
/// A -> B -> C: each receive+tick must produce a timestamp strictly greater
/// than the preceding agent's timestamp.
#[test]
fn inv_ferr_016_hlc_causality_chain() {
    use ferratom::{AgentId, HybridClock};

    let mut clocks: Vec<HybridClock> = (0..3)
        .map(|i| {
            let mut bytes = [0u8; 16];
            bytes[0] = 100 + i;
            HybridClock::new(AgentId::from_bytes(bytes))
        })
        .collect();

    let t0 = clocks[0].tick();
    clocks[1].receive(&t0);
    let t1 = clocks[1].tick();
    clocks[2].receive(&t1);
    let t2 = clocks[2].tick();

    assert!(
        t1 > t0,
        "INV-FERR-016: agent 1 tick must exceed agent 0 tick"
    );
    assert!(
        t2 > t1,
        "INV-FERR-016: agent 2 tick must exceed agent 1 tick"
    );
    assert!(
        t2 > t0,
        "INV-FERR-016: agent 2 tick must exceed agent 0 tick (transitivity)"
    );
}

/// INV-FERR-018: Retraction adds a datom, never removes one.
///
/// After asserting and then retracting a fact, the store is strictly
/// larger than before the retraction (retract datom added).
#[test]
fn inv_ferr_018_retract_adds_datom() {
    let mut store = Store::genesis();
    let agent = AgentId::from_bytes([18u8; 16]);

    // Assert a fact.
    let tx1 = Transaction::new(agent)
        .assert_datom(
            EntityId::from_content(b"retract-test-entity"),
            Attribute::from("db/doc"),
            Value::String("to be retracted".into()),
        )
        .commit(store.schema())
        .expect("valid assert tx");
    store.transact_test(tx1).expect("assert transact");
    let after_assert = store.len();

    // Retract the same fact.
    let tx2 = Transaction::new(agent)
        .retract_datom(
            EntityId::from_content(b"retract-test-entity"),
            Attribute::from("db/doc"),
            Value::String("to be retracted".into()),
        )
        .commit_unchecked();
    store.transact_test(tx2).expect("retract transact");
    let after_retract = store.len();

    assert!(
        after_retract > after_assert,
        "INV-FERR-018: retraction must add datoms (append-only). \
         after_assert={}, after_retract={}",
        after_assert,
        after_retract
    );
}

/// INV-FERR-018: Store is append-only across multiple transactions.
///
/// After each transaction, the store size is >= previous size.
/// No datom from a previous snapshot is ever missing.
#[test]
fn inv_ferr_018_append_only_concrete() {
    let mut store = Store::genesis();
    let agent = AgentId::from_bytes([18u8; 16]);
    let mut prev_datoms: std::collections::BTreeSet<ferratom::Datom> =
        store.datoms().cloned().collect();

    for i in 0..5i64 {
        let tx = Transaction::new(agent)
            .assert_datom(
                EntityId::from_content(format!("append-only-{i}").as_bytes()),
                Attribute::from("db/doc"),
                Value::String(format!("val-{i}").into()),
            )
            .commit(store.schema())
            .expect("valid tx");
        store.transact_test(tx).expect("transact failed");

        let current_datoms: std::collections::BTreeSet<ferratom::Datom> =
            store.datoms().cloned().collect();

        // Every datom from the previous state must still exist.
        assert!(
            prev_datoms.is_subset(&current_datoms),
            "INV-FERR-018: datom lost after transaction {i}. \
             prev_size={}, current_size={}",
            prev_datoms.len(),
            current_datoms.len()
        );

        prev_datoms = current_datoms;
    }
}

/// INV-FERR-020: All datoms from a committed transaction share one epoch.
///
/// Transact a multi-datom transaction, verify all datoms in the receipt
/// share the same `TxId` physical component (epoch).
#[test]
fn inv_ferr_020_transaction_epoch_atomicity() {
    let mut store = Store::genesis();
    let agent = AgentId::from_bytes([20u8; 16]);

    let tx = Transaction::new(agent)
        .assert_datom(
            EntityId::from_content(b"atom-e1"),
            Attribute::from("db/doc"),
            Value::String("first".into()),
        )
        .assert_datom(
            EntityId::from_content(b"atom-e2"),
            Attribute::from("db/doc"),
            Value::String("second".into()),
        )
        .assert_datom(
            EntityId::from_content(b"atom-e3"),
            Attribute::from("db/doc"),
            Value::String("third".into()),
        )
        .commit(store.schema())
        .expect("valid multi-datom tx");
    let receipt = store.transact_test(tx).expect("transact failed");
    let epoch = receipt.epoch();

    // All datoms at this epoch must share the same physical timestamp.
    let tx_datoms: Vec<_> = store
        .datoms()
        .filter(|d| d.tx().physical() == epoch)
        .collect();

    assert!(
        tx_datoms.len() >= 3,
        "INV-FERR-020: expected at least 3 datoms at epoch {}, got {}",
        epoch,
        tx_datoms.len()
    );

    for d in &tx_datoms {
        assert_eq!(
            d.tx().physical(),
            epoch,
            "INV-FERR-020: datom epoch {:?} differs from transaction epoch {}",
            d.tx(),
            epoch
        );
    }
}

/// Assert that a `WriteLimiter` has exactly `expected` active guards.
fn assert_limiter_count(
    limiter: &ferratomic_core::backpressure::WriteLimiter,
    expected: usize,
    context: &str,
) {
    assert_eq!(limiter.active_count(), expected, "INV-FERR-021: {context}");
}

/// INV-FERR-021: `WriteLimiter` correctly bounds concurrent writes.
///
/// Integration-level test: acquire to capacity, verify overflow rejected,
/// release and re-acquire.
#[test]
fn inv_ferr_021_backpressure_integration() {
    use ferratomic_core::backpressure::{BackpressurePolicy, WriteLimiter};

    let policy = BackpressurePolicy {
        max_concurrent_writes: 3,
    };
    let limiter = WriteLimiter::new(&policy);

    // Acquire 3 guards.
    let g1 = limiter.try_acquire();
    let g2 = limiter.try_acquire();
    let g3 = limiter.try_acquire();
    assert!(g1.is_some(), "INV-FERR-021: first acquire must succeed");
    assert!(g2.is_some(), "INV-FERR-021: second acquire must succeed");
    assert!(g3.is_some(), "INV-FERR-021: third acquire must succeed");
    assert_limiter_count(&limiter, 3, "active count must be 3 after 3 acquires");

    // 4th acquire must fail.
    assert!(
        limiter.try_acquire().is_none(),
        "INV-FERR-021: overflow must fail"
    );
    assert_limiter_count(&limiter, 3, "failed acquire must not change count");

    // Drop one guard, re-acquire.
    drop(g3);
    assert_limiter_count(&limiter, 2, "active must drop to 2 after release");

    let _g4 = limiter.try_acquire();
    assert!(
        _g4.is_some(),
        "INV-FERR-021: acquire after release must succeed"
    );
    assert_limiter_count(&limiter, 3, "active must return to 3");
}

/// Corrupt a checkpoint file by flipping a byte in its payload region.
///
/// The header is 18 bytes (magic=4 + version=2 + epoch=8 + length=4)
/// and the BLAKE3 hash is the trailing 32 bytes, so the payload sits
/// in [18 .. len-32).
fn corrupt_checkpoint_payload(path: &std::path::Path) {
    let mut data = std::fs::read(path).expect("read checkpoint file");
    let header_size = 18usize;
    let hash_size = 32usize;
    assert!(
        data.len() > header_size + hash_size,
        "INV-FERR-013: checkpoint file must be larger than header + hash"
    );
    let payload_mid = header_size + (data.len() - header_size - hash_size) / 2;
    data[payload_mid] ^= 0xFF;
    std::fs::write(path, &data).expect("write corrupted checkpoint");
}

/// INV-FERR-013: Corrupted checkpoint bytes must be detected and rejected.
///
/// Write a valid checkpoint, flip a byte in the payload region, then verify
/// that `load_checkpoint` returns `FerraError::CheckpointCorrupted`.
///
/// bd-n1i: error-path test for checkpoint corruption.
#[test]
fn test_inv_ferr_013_checkpoint_corruption() {
    use ferratomic_core::checkpoint::{load_checkpoint, write_checkpoint};

    let mut store = Store::genesis();
    let agent = AgentId::from_bytes([13u8; 16]);
    let tx = Transaction::new(agent)
        .assert_datom(
            EntityId::from_content(b"corruption-entity"),
            Attribute::from("db/doc"),
            Value::String("checkpoint corruption test".into()),
        )
        .commit_unchecked();
    store.transact_test(tx).expect("setup transact failed");

    let dir = tempfile::TempDir::new().expect("failed to create temp dir");
    let path = dir.path().join("corrupt.chkp");
    write_checkpoint(&store, &path).expect("write_checkpoint failed");
    corrupt_checkpoint_payload(&path);

    let result = load_checkpoint(&path);
    assert!(
        result.is_err(),
        "INV-FERR-013: corrupted checkpoint must error"
    );

    match &result.unwrap_err() {
        ferratom::FerraError::CheckpointCorrupted { expected, actual } => {
            assert_ne!(expected, actual, "INV-FERR-013: checksums must differ");
        }
        other => panic!("INV-FERR-013: expected CheckpointCorrupted, got {other:?}"),
    }
}

/// Build test datoms for index backend verification.
fn build_index_test_datoms() -> Vec<ferratom::Datom> {
    vec![
        ferratom::Datom::new(
            EntityId::from_content(b"idx-e1"),
            Attribute::from("user/name"),
            Value::String("Alice".into()),
            ferratom::TxId::new(1, 0, 0),
            ferratom::Op::Assert,
        ),
        ferratom::Datom::new(
            EntityId::from_content(b"idx-e2"),
            Attribute::from("user/name"),
            Value::String("Bob".into()),
            ferratom::TxId::new(2, 0, 0),
            ferratom::Op::Assert,
        ),
        ferratom::Datom::new(
            EntityId::from_content(b"idx-e3"),
            Attribute::from("user/age"),
            Value::Long(25),
            ferratom::TxId::new(3, 0, 0),
            ferratom::Op::Assert,
        ),
    ]
}

/// Insert datoms into all four OrdMap index backends and verify cardinality.
fn verify_ordmap_index_cardinality(datoms: &[ferratom::Datom]) {
    use ferratomic_core::indexes::{AevtKey, AvetKey, EavtKey, IndexBackend, VaetKey};
    use im::OrdMap;

    let mut eavt: OrdMap<EavtKey, ferratom::Datom> = OrdMap::new();
    let mut aevt: OrdMap<AevtKey, ferratom::Datom> = OrdMap::new();
    let mut vaet: OrdMap<VaetKey, ferratom::Datom> = OrdMap::new();
    let mut avet: OrdMap<AvetKey, ferratom::Datom> = OrdMap::new();

    for d in datoms {
        eavt.backend_insert(EavtKey::from_datom(d), d.clone());
        aevt.backend_insert(AevtKey::from_datom(d), d.clone());
        vaet.backend_insert(VaetKey::from_datom(d), d.clone());
        avet.backend_insert(AvetKey::from_datom(d), d.clone());
    }

    let expected = datoms.len();
    assert_eq!(
        eavt.backend_len(),
        expected,
        "INV-FERR-025: EAVT len mismatch"
    );
    assert_eq!(
        aevt.backend_len(),
        expected,
        "INV-FERR-025: AEVT len mismatch"
    );
    assert_eq!(
        vaet.backend_len(),
        expected,
        "INV-FERR-025: VAET len mismatch"
    );
    assert_eq!(
        avet.backend_len(),
        expected,
        "INV-FERR-025: AVET len mismatch"
    );

    for d in datoms {
        let key = EavtKey::from_datom(d);
        assert!(
            eavt.backend_get(&key).is_some(),
            "INV-FERR-025: EAVT lookup failed for {:?}",
            d.entity()
        );
    }
}

/// INV-FERR-025: `OrdMapBackend` satisfies `IndexBackend`, bijection holds.
///
/// bd-7tb0: verifies that the default `OrdMap`-based index backend maintains
/// bijection after multiple insertions.
#[test]
fn test_inv_ferr_025_index_backend_trait() {
    let datoms = build_index_test_datoms();
    verify_ordmap_index_cardinality(&datoms);

    // Also verify via the Store-level verify_bijection.
    let mut store = Store::genesis();
    let agent = AgentId::from_bytes([25u8; 16]);
    for i in 0..5i64 {
        let tx = Transaction::new(agent)
            .assert_datom(
                EntityId::from_content(format!("bijection-idx-{i}").as_bytes()),
                Attribute::from("db/doc"),
                Value::String(format!("index-test-{i}").into()),
            )
            .commit(store.schema())
            .expect("INV-FERR-025: valid tx");
        store.transact_test(tx).expect("INV-FERR-025: transact ok");
    }

    assert!(
        store.indexes().unwrap().verify_bijection(),
        "INV-FERR-025: index bijection violated after 5 transactions"
    );
}

/// INV-FERR-029: Live resolution — assert + retract, verify live view.
///
/// bd-7tb0: the LIVE view of a store is the set of (entity, attribute, value)
/// triples that have been asserted and not subsequently retracted. This test
/// asserts a fact, retracts it, and verifies the triple is no longer live.
/// Since live_view is not yet a first-class Store method (Phase 4b), we
/// compute it manually by scanning the datom set.
#[test]
fn test_inv_ferr_029_live_resolution() {
    let mut store = Store::genesis();
    let agent = AgentId::from_bytes([29u8; 16]);
    let entity = EntityId::from_content(b"live-resolution-entity");
    let attr = Attribute::from("db/doc");
    let value = Value::String("to-be-retracted".into());

    // Phase 1: Assert a fact.
    let tx1 = Transaction::new(agent)
        .assert_datom(entity, attr.clone(), value.clone())
        .commit(store.schema())
        .expect("INV-FERR-029: assert tx must commit");
    store
        .transact_test(tx1)
        .expect("INV-FERR-029: assert transact ok");

    // Compute live view: (e,a,v) triples where assert count > retract count.
    let live_before = compute_live_view(&store);
    assert!(
        live_before.contains(&(entity, attr.clone(), value.clone())),
        "INV-FERR-029: asserted triple must be live before retraction"
    );

    // Phase 2: Retract the same fact.
    let tx2 = Transaction::new(agent)
        .retract_datom(entity, attr.clone(), value.clone())
        .commit_unchecked();
    store
        .transact_test(tx2)
        .expect("INV-FERR-029: retract transact ok");

    // Verify the triple is no longer live.
    let live_after = compute_live_view(&store);
    assert!(
        !live_after.contains(&(entity, attr, value)),
        "INV-FERR-029: retracted triple must not be live after retraction"
    );
}

/// Assert a datom on a database and return the committed transaction result.
fn assert_on_db(db: &Database, agent: AgentId, entity: EntityId, attr: &Attribute, val: &Value) {
    let tx = Transaction::new(agent)
        .assert_datom(entity, attr.clone(), val.clone())
        .commit(&db.schema())
        .expect("INV-FERR-032: assert commit");
    db.transact(tx).expect("INV-FERR-032: assert transact");
}

/// Retract a datom on a database (unchecked commit for retract).
fn retract_on_db(db: &Database, agent: AgentId, entity: EntityId, attr: &Attribute, val: &Value) {
    let tx = Transaction::new(agent)
        .retract_datom(entity, attr.clone(), val.clone())
        .commit_unchecked();
    db.transact(tx).expect("INV-FERR-032: retract transact");
}

/// INV-FERR-032: LIVE resolution correctness -- end-to-end assert+retract+query.
///
/// bd-7tb0: strengthens INV-FERR-029 by testing multiple entities, attributes,
/// and interleaved assert/retract sequences via the Database API.
#[test]
fn test_inv_ferr_032_live_correctness() {
    let db = Database::genesis();
    let agent = AgentId::from_bytes([32u8; 16]);
    let entity_a = EntityId::from_content(b"live-correct-a");
    let entity_b = EntityId::from_content(b"live-correct-b");
    let attr = Attribute::from("db/doc");
    let val_old = Value::String("old-value".into());
    let val_new = Value::String("new-value".into());

    assert_on_db(&db, agent, entity_a, &attr, &val_old);
    assert_on_db(&db, agent, entity_b, &attr, &val_new);
    retract_on_db(&db, agent, entity_a, &attr, &val_old);
    assert_on_db(&db, agent, entity_a, &attr, &val_new);

    let snap = db.snapshot();
    let live = compute_live_view_from_snapshot(&snap);

    assert!(
        !live.contains(&(entity_a, attr.clone(), val_old)),
        "INV-FERR-032: retracted (entity_a, old_value) must not be live"
    );
    assert!(
        live.contains(&(entity_a, attr.clone(), val_new.clone())),
        "INV-FERR-032: re-asserted (entity_a, new_value) must be live"
    );
    assert!(
        live.contains(&(entity_b, attr, val_new)),
        "INV-FERR-032: (entity_b, new_value) was never retracted, must be live"
    );
}

// ---------------------------------------------------------------------------
// Helpers for LIVE view computation
// ---------------------------------------------------------------------------

/// Compute the LIVE view from a Store.
///
/// Returns the set of (entity, attribute, value) triples that are currently
/// asserted and not retracted. This mirrors the spec's `live_view` function.
fn compute_live_view(store: &Store) -> std::collections::BTreeSet<(EntityId, Attribute, Value)> {
    let mut live = std::collections::BTreeSet::new();
    for datom in store.datoms() {
        let key = (
            datom.entity(),
            datom.attribute().clone(),
            datom.value().clone(),
        );
        match datom.op() {
            ferratom::Op::Assert => {
                live.insert(key);
            }
            ferratom::Op::Retract => {
                live.remove(&key);
            }
        }
    }
    live
}

/// Compute the LIVE view from a Snapshot.
fn compute_live_view_from_snapshot(
    snap: &ferratomic_core::store::Snapshot,
) -> std::collections::BTreeSet<(EntityId, Attribute, Value)> {
    let mut live = std::collections::BTreeSet::new();
    for datom in snap.datoms() {
        let key = (
            datom.entity(),
            datom.attribute().clone(),
            datom.value().clone(),
        );
        match datom.op() {
            ferratom::Op::Assert => {
                live.insert(key);
            }
            ferratom::Op::Retract => {
                live.remove(&key);
            }
        }
    }
    live
}
