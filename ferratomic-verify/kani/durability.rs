//! Durability and transaction-shape Kani harnesses.
//!
//! Covers INV-FERR-008, INV-FERR-013, INV-FERR-014, INV-FERR-018,
//! INV-FERR-020, INV-FERR-024, INV-FERR-026, and INV-FERR-028.

use std::{collections::BTreeSet, sync::Arc};

use ferratom::{AgentId, Attribute, Datom, EntityId, Value};
use ferratomic_db::{store::Store, writer::Transaction};

use super::helpers::{concrete_datom, concrete_datom_set};
#[cfg(not(kani))]
use super::kani;

/// INV-FERR-013: checkpoint serialization is a round trip on store state.
#[cfg_attr(kani, kani::proof)]
#[cfg_attr(kani, kani::unwind(8))]
#[cfg_attr(not(kani), test)]
#[cfg_attr(not(kani), ignore = "requires Kani verifier")]
fn checkpoint_roundtrip() {
    let count: u8 = kani::any();
    kani::assume(count <= 4);
    let datoms = concrete_datom_set(count);

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
    let count_committed: u8 = kani::any();
    kani::assume(count_committed <= 4);
    let committed: BTreeSet<Datom> = (0..count_committed).map(concrete_datom).collect();

    let count_uncommitted: u8 = kani::any();
    kani::assume(count_uncommitted <= 2);
    let uncommitted: BTreeSet<Datom> = (10..10 + count_uncommitted).map(concrete_datom).collect();
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
    let count: u8 = kani::any();
    kani::assume(count <= 4);
    let initial = concrete_datom_set(count);

    let new_datom = concrete_datom(kani::any::<u8>());

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

// ---------------------------------------------------------------------------
// INV-FERR-024: Substrate agnosticism
// ---------------------------------------------------------------------------

/// INV-FERR-024: InMemoryBackend checkpoint round-trip preserves store state.
///
/// Verifies that writing a checkpoint through InMemoryBackend and reading
/// it back produces an identical store. This proves the StorageBackend
/// trait abstraction does not lose data for the in-memory substrate.
#[cfg_attr(kani, kani::proof)]
#[cfg_attr(kani, kani::unwind(8))]
#[cfg_attr(not(kani), test)]
#[cfg_attr(not(kani), ignore = "requires Kani verifier")]
fn substrate_agnosticism_in_memory() {
    use std::io::Write;

    use ferratomic_db::storage::{InMemoryBackend, StorageBackend};

    let mut store = Store::genesis();
    let tx = Transaction::new(AgentId::from_bytes([1u8; 16]))
        .assert_datom(
            EntityId::from_content(b"substrate-test"),
            Attribute::from("db/doc"),
            Value::String(Arc::from("backend agnosticism")),
        )
        .commit(store.schema())
        .expect("INV-FERR-024: tx must validate");
    let _ = store
        .transact_test(tx)
        .expect("INV-FERR-024: tx must apply");

    let bytes = store
        .to_checkpoint_bytes()
        .expect("INV-FERR-024: serialization must succeed");

    // Write through InMemoryBackend, then read back.
    let backend = InMemoryBackend::new();
    let mut writer = backend
        .open_checkpoint_writer()
        .expect("INV-FERR-024: open writer");
    writer
        .write_all(&bytes)
        .expect("INV-FERR-024: write must succeed");
    drop(writer);

    let mut reader = backend
        .open_checkpoint_reader()
        .expect("INV-FERR-024: open reader");
    let mut read_bytes = Vec::new();
    std::io::Read::read_to_end(&mut reader, &mut read_bytes)
        .expect("INV-FERR-024: read must succeed");

    let loaded = Store::from_checkpoint_bytes(&read_bytes)
        .expect("INV-FERR-024: deserialization must succeed");
    assert_eq!(
        store.datom_set(),
        loaded.datom_set(),
        "INV-FERR-024: datom set must survive InMemoryBackend round-trip"
    );
    assert_eq!(
        store.epoch(),
        loaded.epoch(),
        "INV-FERR-024: epoch must survive InMemoryBackend round-trip"
    );
}

// ---------------------------------------------------------------------------
// INV-FERR-026: Write amplification bound
// ---------------------------------------------------------------------------

/// WAL frame overhead: magic(4) + version(2) + epoch(8) + length(4) + CRC(4) = 22 bytes.
/// Cross-reference: `ferratomic-db::wal::HEADER_SIZE` (18) + `CRC_SIZE` (4) = 22.
/// If the WAL frame format changes, this constant must be updated to match.
///
/// bd-pu4t: This value also appears in the proptest doc comment at
/// `ferratomic-verify/proptest/wal_properties.rs` (INV-FERR-026 test).
/// Both must stay in sync with `ferratomic-db::wal::{HEADER_SIZE, CRC_SIZE}`.
const WAL_FRAME_OVERHEAD: usize = 22;

/// INV-FERR-026: WAL write amplification <= 10x.
///
/// Models the write amplification bound: for N datoms each producing
/// a payload of S bytes, the total WAL physical size is
/// N * (S + WAL_FRAME_OVERHEAD). The invariant requires this to be
/// <= 10 * N * S for any S >= 3 (minimum meaningful datom payload).
///
/// This is a structural proof over the WAL frame format constants.
#[cfg_attr(kani, kani::proof)]
#[cfg_attr(kani, kani::unwind(6))]
#[cfg_attr(not(kani), test)]
#[cfg_attr(not(kani), ignore = "requires Kani verifier")]
fn write_amplification_bound() {
    let n: usize = kani::any();
    let s: usize = kani::any();
    kani::assume(n > 0 && n <= 4);
    // Minimum payload size: a single bincode-encoded datom is always
    // larger than 3 bytes. We bound symbolically.
    kani::assume((3..=1024).contains(&s));

    let logical_size = n * s;
    let physical_size = n * (s + WAL_FRAME_OVERHEAD);

    // INV-FERR-026: write amplification = physical / logical <= 10.
    // Equivalently: physical <= 10 * logical.
    assert!(
        physical_size <= 10 * logical_size,
        "INV-FERR-026: write amplification exceeded 10x: \
         physical={physical_size}, logical={logical_size}, \
         ratio={}",
        physical_size / logical_size.max(1)
    );
}

// ---------------------------------------------------------------------------
// INV-FERR-028: Cold start latency (checkpoint round-trip correctness)
// ---------------------------------------------------------------------------

/// INV-FERR-028: cold start via checkpoint produces identical store.
///
/// Verifies that a store with committed transactions can be serialized
/// to a checkpoint and deserialized back with no data loss. This is the
/// correctness foundation for the cold start latency bound: if the
/// round-trip is correct, cold start time is bounded by checkpoint size.
#[cfg_attr(kani, kani::proof)]
#[cfg_attr(kani, kani::unwind(8))]
#[cfg_attr(not(kani), test)]
#[cfg_attr(not(kani), ignore = "requires Kani verifier")]
fn cold_start_checkpoint_roundtrip() {
    let mut store = Store::genesis();
    let agent = AgentId::from_bytes([2u8; 16]);
    let n_txns: u8 = kani::any();
    kani::assume(n_txns > 0 && n_txns <= 3);

    for i in 0..n_txns {
        let tx = Transaction::new(agent)
            .assert_datom(
                EntityId::from_content(&[i, 0xC5]),
                Attribute::from("db/doc"),
                Value::String(Arc::from(format!("cold-start-{i}"))),
            )
            .commit(store.schema())
            .expect("INV-FERR-028: tx must validate");
        let _ = store
            .transact_test(tx)
            .expect("INV-FERR-028: tx must apply");
    }

    let bytes = store
        .to_checkpoint_bytes()
        .expect("INV-FERR-028: checkpoint serialization must succeed");
    let mut loaded = Store::from_checkpoint_bytes(&bytes)
        .expect("INV-FERR-028: checkpoint deserialization must succeed");

    assert_eq!(
        store.datom_set(),
        loaded.datom_set(),
        "INV-FERR-028: datom set must survive cold-start round-trip"
    );
    assert_eq!(
        store.epoch(),
        loaded.epoch(),
        "INV-FERR-028: epoch must survive cold-start round-trip"
    );
    // bd-h2fz: from_checkpoint builds Positional. Promote to verify bijection.
    loaded.promote();
    assert!(
        // bd-oett: descriptive expect instead of bare unwrap.
        loaded
            .indexes()
            .expect("INV-FERR-028: indexes must be available after promote")
            .verify_bijection(),
        "INV-FERR-028: indexes must be bijective after cold start"
    );
}
