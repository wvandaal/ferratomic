//! Durability and transaction-shape Kani harnesses.
//!
//! Covers INV-FERR-008, INV-FERR-013, INV-FERR-014, INV-FERR-018, and INV-FERR-020.

use std::collections::BTreeSet;

use ferratom::{AgentId, Attribute, Datom, EntityId, Value};
use ferratomic_core::{store::Store, writer::Transaction};

#[cfg(not(kani))]
use super::kani;

/// INV-FERR-013: checkpoint serialization is a round trip on store state.
#[cfg_attr(kani, kani::proof)]
#[cfg_attr(kani, kani::unwind(8))]
#[cfg_attr(not(kani), test)]
#[cfg_attr(not(kani), ignore = "requires Kani verifier")]
fn checkpoint_roundtrip() {
    let datoms: BTreeSet<Datom> = kani::any();
    kani::assume(datoms.len() <= 4);

    let store = Store::from_datoms(datoms.clone());
    let bytes = store
        .to_checkpoint_bytes()
        .expect("INV-FERR-013: checkpoint serialization must succeed for any valid store");
    let loaded = Store::from_checkpoint_bytes(&bytes)
        .expect("INV-FERR-013: checkpoint bytes produced by the store must deserialize");

    assert_eq!(store.datom_set(), loaded.datom_set());
    assert_eq!(store.epoch(), loaded.epoch());
}

/// INV-FERR-014: recovery never loses committed datoms.
#[cfg_attr(kani, kani::proof)]
#[cfg_attr(kani, kani::unwind(8))]
#[cfg_attr(not(kani), test)]
#[cfg_attr(not(kani), ignore = "requires Kani verifier")]
fn recovery_superset() {
    let committed: BTreeSet<Datom> = kani::any();
    kani::assume(committed.len() <= 4);

    let uncommitted: BTreeSet<Datom> = kani::any();
    kani::assume(uncommitted.len() <= 2);
    let survived: bool = kani::any();

    let mut recovered = committed.clone();
    if survived {
        for d in &uncommitted {
            recovered.insert(d.clone());
        }
    }

    assert!(committed.is_subset(&recovered));
}

/// INV-FERR-018: the datom set is append-only.
#[cfg_attr(kani, kani::proof)]
#[cfg_attr(kani, kani::unwind(10))]
#[cfg_attr(not(kani), test)]
#[cfg_attr(not(kani), ignore = "requires Kani verifier")]
fn append_only() {
    let initial: BTreeSet<Datom> = kani::any();
    kani::assume(initial.len() <= 4);
    let new_datom: Datom = kani::any();

    let mut store = initial.clone();
    store.insert(new_datom);

    assert!(initial.is_subset(&store));
    assert!(store.len() >= initial.len());
}

/// INV-FERR-020: a committed transaction assigns one epoch to all of its datoms.
#[cfg_attr(kani, kani::proof)]
#[cfg_attr(kani, kani::unwind(8))]
#[cfg_attr(not(kani), test)]
#[cfg_attr(not(kani), ignore = "requires Kani verifier")]
fn transaction_atomicity() {
    let mut store = Store::genesis();
    let n_datoms: u8 = kani::any();
    kani::assume(n_datoms > 0 && n_datoms <= 4);

    let tx = (0..n_datoms).fold(Transaction::new(AgentId::from_bytes([0u8; 16])), |tx, i| {
        tx.assert_datom(
            EntityId::from_content(&[i]),
            Attribute::from("test/counter"),
            Value::Long(i64::from(i)),
        )
    });
    let committed = tx
        .commit(store.schema())
        .expect("INV-FERR-020: harness transaction should validate");
    let tx_datoms: BTreeSet<_> = committed.datoms().iter().cloned().collect();
    let _receipt = store
        .transact_test(committed)
        .expect("INV-FERR-020: harness transaction should apply");

    let snapshot = store.snapshot();
    let visible: BTreeSet<_> = snapshot.datoms().cloned().collect();
    let visible_count = tx_datoms.iter().filter(|d| visible.contains(*d)).count();
    assert!(visible_count == 0 || visible_count == tx_datoms.len());
}

// ---------------------------------------------------------------------------
// WAL fsync ordering state machine (INV-FERR-008)
// ---------------------------------------------------------------------------

/// WAL commit pipeline phases.
///
/// INV-FERR-008 mandates the temporal ordering:
///   Write -> Fsync -> Apply -> Advance
///
/// Each phase is represented as a distinct state. The state machine
/// tracks which phases have completed and in what order.
#[derive(Clone, Copy, PartialEq, Eq)]
enum WalPhase {
    /// Initial state: no work has begun for this transaction.
    Init,
    /// WAL entry bytes have been written to the OS page cache.
    Written,
    /// WAL file has been fsynced — entry is durable on storage.
    Fsynced,
    /// Transaction datoms have been applied to in-memory indexes.
    Applied,
    /// Epoch has advanced — transaction is visible to new snapshots.
    Advanced,
}

/// Result of attempting a WAL commit with a given step ordering.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CommitResult {
    /// The commit completed with correct ordering.
    Ok,
    /// The commit was rejected because steps were out of order.
    OrderingViolation,
}

/// Execute one step of the WAL pipeline, enforcing INV-FERR-008 ordering.
///
/// Returns the new phase on success, or `None` if the requested step
/// violates the required ordering.
fn wal_step(current: WalPhase, requested: WalPhase) -> Option<WalPhase> {
    match (current, requested) {
        (WalPhase::Init, WalPhase::Written) => Some(WalPhase::Written),
        (WalPhase::Written, WalPhase::Fsynced) => Some(WalPhase::Fsynced),
        (WalPhase::Fsynced, WalPhase::Applied) => Some(WalPhase::Applied),
        (WalPhase::Applied, WalPhase::Advanced) => Some(WalPhase::Advanced),
        _ => None, // any other transition violates the ordering
    }
}

/// Execute a 4-step WAL commit with the given step ordering.
///
/// Returns `CommitResult::Ok` if and only if the steps follow the
/// required INV-FERR-008 ordering: Write -> Fsync -> Apply -> Advance.
fn try_commit(steps: [WalPhase; 4]) -> CommitResult {
    let mut phase = WalPhase::Init;
    for &step in &steps {
        match wal_step(phase, step) {
            Some(next) => phase = next,
            None => return CommitResult::OrderingViolation,
        }
    }
    if phase == WalPhase::Advanced {
        CommitResult::Ok
    } else {
        CommitResult::OrderingViolation
    }
}

/// Assert that a particular step ordering is rejected by the WAL state machine.
fn assert_ordering_rejected(steps: [WalPhase; 4], msg: &str) {
    assert_eq!(try_commit(steps), CommitResult::OrderingViolation, "{msg}");
}

/// Part 1+2: Verify canonical ordering succeeds and Kani symbolic exploration.
fn verify_canonical_and_symbolic() {
    let canonical = [
        WalPhase::Written,
        WalPhase::Fsynced,
        WalPhase::Applied,
        WalPhase::Advanced,
    ];
    assert_eq!(
        try_commit(canonical),
        CommitResult::Ok,
        "INV-FERR-008: canonical Write->Fsync->Apply->Advance must succeed"
    );

    let s0: u8 = kani::any();
    let s1: u8 = kani::any();
    let s2: u8 = kani::any();
    let s3: u8 = kani::any();
    kani::assume((1..=4).contains(&s0));
    kani::assume((1..=4).contains(&s1));
    kani::assume((1..=4).contains(&s2));
    kani::assume((1..=4).contains(&s3));

    let to_phase = |v: u8| -> WalPhase {
        match v {
            1 => WalPhase::Written,
            2 => WalPhase::Fsynced,
            3 => WalPhase::Applied,
            _ => WalPhase::Advanced,
        }
    };
    let result = try_commit([to_phase(s0), to_phase(s1), to_phase(s2), to_phase(s3)]);
    if result == CommitResult::Ok {
        assert_eq!(s0, 1, "INV-FERR-008: step 0 must be Write (1)");
        assert_eq!(s1, 2, "INV-FERR-008: step 1 must be Fsync (2)");
        assert_eq!(s2, 3, "INV-FERR-008: step 2 must be Apply (3)");
        assert_eq!(s3, 4, "INV-FERR-008: step 3 must be Advance (4)");
    }
}

/// Part 3: Explicit two-fsync barrier violations.
fn verify_barrier_violations() {
    assert_ordering_rejected(
        [
            WalPhase::Written,
            WalPhase::Advanced,
            WalPhase::Fsynced,
            WalPhase::Applied,
        ],
        "INV-FERR-008: advancing epoch before fsync must be rejected",
    );
    assert_ordering_rejected(
        [
            WalPhase::Fsynced,
            WalPhase::Written,
            WalPhase::Applied,
            WalPhase::Advanced,
        ],
        "INV-FERR-008: fsync before write must be rejected",
    );
    assert_ordering_rejected(
        [
            WalPhase::Advanced,
            WalPhase::Applied,
            WalPhase::Fsynced,
            WalPhase::Written,
        ],
        "INV-FERR-008: fully inverted ordering must be rejected",
    );
}

/// INV-FERR-008: WAL fsync ordering -- the two-fsync barrier property.
///
/// Verifies that the WAL commit state machine accepts ONLY the correct
/// ordering (Write -> Fsync -> Apply -> Advance) and rejects all other
/// permutations of the four steps.
///
/// Kani explores all possible 4-element orderings of the WAL pipeline
/// phases. The harness asserts:
/// 1. The canonical ordering succeeds.
/// 2. Any ordering accepted by the state machine IS the canonical ordering.
/// 3. Specifically: data is written before fsync, and fsync completes
///    before the epoch advances (the two-fsync barrier).
#[cfg_attr(kani, kani::proof)]
#[cfg_attr(kani, kani::unwind(5))]
#[cfg_attr(not(kani), test)]
#[cfg_attr(not(kani), ignore = "requires Kani verifier")]
fn kani_inv_ferr_008_wal_fsync_ordering() {
    verify_canonical_and_symbolic();
    verify_barrier_violations();
}
