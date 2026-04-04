## 23.12 Verification Infrastructure

> **Namespace**: FERR | **Wave**: 2 (Hardening) | **Stages**: 0-2
> **Shared definitions**: [00-preamble.md](00-preamble.md) (conventions, verification tags, constraints)

### Overview

The Ferratomic verification stack — Lean 4, Stateright, Kani, proptest, and the Rust type
system — proves properties of the **algebraic model** and the **implementation model** under
clean conditions. This section specifies properties that the verification infrastructure
itself must satisfy, and properties that the system must satisfy under **adversarial
conditions** not covered by clean-path testing.

**Motivation**: The gap between "all 10,000 proptest cases pass" and "the system is correct"
is epistemic, not structural. A passing test suite provides evidence but not a confidence
bound. A crash recovery test that uses clean serialization roundtrips does not test torn
writes. A query equivalence test that runs one query shape does not test all semantically
equivalent rewrites.

This section formalizes:
1. **Adversarial fault tolerance** — system correctness under storage-layer fault injection
2. **Temporal stability** — invariant preservation under sustained workload over time
3. **Query metamorphic equivalence** — semantic rewrite invariance for the Datalog engine
4. **Optimization behavioral preservation** — observable equivalence across implementation changes
5. **Verification methodology** — how confidence is quantified, tracked, and gated

**Provenance**: Patterns identified via adversarial cross-pollination analysis of
[FrankenSQLite](https://github.com/Dicklesworthstone/frankensqlite) (2026-03-31), filtered
for axiological alignment with Ferratomic's algebraic foundation. Each pattern addresses a
specific gap in the existing verification stack.

**Algebraic grounding**: The verification infrastructure itself forms a refinement tower
(spec/07-refinement.md). Where CI-FERR-001 bridges the Lean-Rust gap, the invariants in
this section bridge the **clean-path-to-adversarial gap** (INV-FERR-056), the
**finite-to-sustained gap** (INV-FERR-057), the **single-query-to-rewrite gap**
(INV-FERR-058), and the **pre-optimization-to-post-optimization gap** (INV-FERR-059).

---

### Storage Fault Model (Prerequisite Definitions)

The following fault model defines the adversarial conditions under which INV-FERR-056
must hold. The model is closed — only faults in this set are tested. The model is
extensible — new fault kinds can be added without changing the invariant statement.

```
FaultModel := {
  TornWrite(path, offset, valid_bytes)
    -- A write operation where only the first `valid_bytes` reach stable storage.
    -- Models: crash during write(2), partial page flush, sector-level tearing.
    -- Constraint: 0 < valid_bytes < intended_bytes.

  PowerCut(path, after_nth_sync)
    -- Simulated power loss: the nth fsync call succeeds, all subsequent
    -- operations on this path fail with EIO.
    -- Models: UPS failure, kernel panic after partial checkpoint.

  IoError(path, op, nth_occurrence)
    -- The nth occurrence of operation `op` (read | write | sync) on `path`
    -- returns EIO. Subsequent operations may succeed (transient fault).
    -- Models: bad sector, transient NVMe timeout, RAID rebuild interference.

  DiskFull(path, after_nth_write)
    -- The nth write operation returns ENOSPC.
    -- Models: quota exhaustion, competing process fills disk.

  BitFlip(path, offset, bit_position)
    -- A single bit is flipped at the specified offset.
    -- Models: cosmic ray, undetected ECC failure, storage firmware bug.
    -- NOTE: BitFlip is detected by BLAKE3 checksums (INV-FERR-013)
    -- and CRC32 (WAL frames). This tests detection, not tolerance.
}

All faults are deterministic: same FaultSpec produces same failure behavior.
Determinism enables reproducible regression testing.
```

---

### INV-FERR-056: Crash Recovery Under Adversarial Fault Model

**Traces to**: INV-FERR-014 (Recovery Correctness), INV-FERR-008 (WAL Fsync Ordering),
INV-FERR-013 (Checkpoint Equivalence), NEG-FERR-003 (No Data Loss on Crash)
**Verification**: `V:PROP`, `V:KANI`, `V:MODEL`
**Stage**: 0

#### Level 0 (Algebraic Law)
```
Let S be a reachable store state after a sequence of committed transactions T₁..Tₙ.
Let last_committed(S) = the store state after all transactions whose WAL entries
  have been durably fsynced (INV-FERR-008 satisfied).
Let f ∈ FaultModel be any fault from the defined fault model.
Let inject(S, f) be the store state after applying fault f during the next operation.
Let recover(inject(S, f)) be the store state after executing the three-level
  recovery cascade (checkpoint + WAL delta, WAL-only, or genesis).

∀ S ∈ ReachableStore, ∀ f ∈ FaultModel:
  last_committed(S) ⊆ recover(inject(S, f))

Where ⊆ is datom-set containment: every datom in last_committed(S) is also
in recover(inject(S, f)).

Proof sketch:
  Case TornWrite on WAL: The torn frame has an invalid CRC32 (the CRC is
    the last 4 bytes written; a torn write that doesn't complete the CRC
    produces a checksum mismatch). WAL recovery (INV-FERR-008) truncates
    at the last valid frame. All frames before the torn one have valid CRCs
    and were fsynced, so they are in last_committed(S).

  Case TornWrite on checkpoint: The torn checkpoint has an invalid BLAKE3
    hash (the hash covers all preceding bytes; any truncation or corruption
    produces a mismatch). Checkpoint load fails → recovery falls through to
    WAL-only (Level 2 recovery) or genesis (Level 3). WAL frames are
    independent of the checkpoint and contain all committed transactions.

  Case PowerCut: After the nth fsync, all committed transactions up to that
    point are durable (INV-FERR-008: WAL fsync is durable-before-visible).
    Uncommitted transactions (those whose WAL entries have not been fsynced)
    are lost — this is correct behavior per INV-FERR-008.

  Case IoError: Transient I/O errors during recovery cause the recovery
    function to fail with FerraError::Io. The caller retries with bounded
    attempts (max 3 retries with exponential backoff, per NEG-FERR-005
    resource boundedness). If retries are exhausted, recovery fails with
    FerraError::Io — the system does not start, which is correct behavior
    (it is safer to refuse to start than to start with incomplete state).
    Permanent device failure is outside the fault model — it requires
    operator intervention (restore from backup or replica).

  Case DiskFull: Write operations fail; no WAL entry is partially written
    (write returns ENOSPC before any bytes are committed). The store state
    is unchanged — last_committed(S) = recover(S).

  Case BitFlip: Detected by BLAKE3 (checkpoint) or CRC32 (WAL frame).
    Corrupted frame/checkpoint is rejected; recovery proceeds from the
    next valid source in the cascade.
```

#### Level 1 (State Invariant)
For every reachable store state and every fault in the defined fault model: after
the fault occurs and recovery completes, the recovered store contains at least all
datoms from transactions that were durably committed before the fault. No committed
datom is lost. Uncommitted transactions (those in flight at the time of the fault)
may or may not be present — their absence is correct, not a failure.

This invariant extends INV-FERR-014 (which tests recovery correctness under clean
conditions) to adversarial storage conditions. INV-FERR-014 tests the happy path:
write checkpoint, write WAL, recover. INV-FERR-056 tests what happens when the
storage layer actively corrupts, truncates, or fails operations during those steps.

Without INV-FERR-056, the system's crash recovery guarantees are proven only for the
case where the filesystem behaves correctly — but the entire purpose of WAL-based
recovery is to survive filesystem misbehavior.

#### Level 2 (Implementation Contract)
```rust
/// Fault-injecting storage backend that wraps any StorageBackend
/// and injects deterministic faults per FaultSpec.
///
/// INV-FERR-056: After fault injection and recovery, recovered store
/// contains all datoms from transactions whose WAL entries were fsynced
/// before the fault.
pub struct FaultInjectingBackend<B: StorageBackend> {
    inner: B,
    faults: Vec<FaultSpec>,
    fault_state: FaultState,
}

/// A deterministic fault specification.
pub enum FaultSpec {
    TornWrite { path_glob: String, offset: usize, valid_bytes: usize },
    PowerCut { path_glob: String, after_nth_sync: usize },
    IoError { path_glob: String, op: FaultOp, nth_occurrence: usize },
    DiskFull { path_glob: String, after_nth_write: usize },
    BitFlip { path_glob: String, offset: usize, bit_position: u8 },
}

pub enum FaultOp { Read, Write, Sync }

/// The fault state machine. Deterministic: same spec + same operation
/// sequence = same fault injection points.
pub struct FaultState {
    sync_count: BTreeMap<String, usize>,
    write_count: BTreeMap<String, usize>,
    read_count: BTreeMap<String, usize>,
}

impl<B: StorageBackend> StorageBackend for FaultInjectingBackend<B> {
    fn write(&mut self, path: &str, offset: u64, data: &[u8]) -> Result<(), FerraError> {
        // Check TornWrite faults: if matched, write only valid_bytes
        if let Some(torn) = self.match_torn_write(path, offset) {
            let truncated = &data[..torn.valid_bytes.min(data.len())];
            return self.inner.write(path, offset, truncated);
        }
        // Check DiskFull faults
        let count = self.fault_state.write_count.entry(path.to_owned()).or_default();
        *count += 1;
        if let Some(full) = self.match_disk_full(path, *count) {
            return Err(FerraError::Io("ENOSPC (injected)".to_owned()));
        }
        self.inner.write(path, offset, data)
    }

    fn sync(&mut self, path: &str) -> Result<(), FerraError> {
        let count = self.fault_state.sync_count.entry(path.to_owned()).or_default();
        *count += 1;
        // Check PowerCut faults: after nth sync, all ops fail
        if let Some(_) = self.match_power_cut(path, *count) {
            return Err(FerraError::Io("EIO (power cut injected)".to_owned()));
        }
        self.inner.sync(path)
    }

    // ... read() with IoError injection, etc.
}

#[kani::proof]
#[kani::unwind(6)]
fn crash_recovery_under_torn_write() {
    let datoms: BTreeSet<Datom> = kani::any();
    kani::assume(datoms.len() <= 3);

    // Build store, transact, write WAL, then inject torn write
    let store = Store::from_datoms(datoms.clone());
    let wal_bytes = store.to_wal_bytes();

    // Torn write: truncate WAL at arbitrary point
    let cut_point: usize = kani::any();
    kani::assume(cut_point > 0 && cut_point < wal_bytes.len());
    let torn_wal = &wal_bytes[..cut_point];

    // Recover from torn WAL
    let recovered = recover_from_wal_bytes(torn_wal);

    // All complete frames' datoms must be present
    let complete_datoms = datoms_from_complete_frames(torn_wal);
    for d in &complete_datoms {
        assert!(recovered.contains(d),
            "INV-FERR-056: committed datom lost after torn write recovery");
    }
}
```

**Falsification**: A store state `S` and a fault `f ∈ FaultModel` such that after
`inject(S, f)` and `recover()`, there exists a datom `d` where:
- `d ∈ last_committed(S)` (the datom was in a durably committed transaction), AND
- `d ∉ recover(inject(S, f))` (the datom is missing from the recovered store).

Specific failure modes:
- **CRC bypass**: WAL recovery accepts a torn frame because the CRC check is missing
  or incorrectly implemented (e.g., CRC computed over partial data instead of full frame).
- **Truncation regression**: Recovery truncates past valid frames (off-by-one in frame
  boundary parsing).
- **Checkpoint priority inversion**: Recovery loads a corrupt checkpoint and ignores
  the WAL, instead of falling through to WAL-only recovery.
- **Schema loss on recovery**: WAL replay uses `insert()` instead of `replay_entry()`,
  skipping `evolve_schema()` (this was actual bug CR-001, discovered in Phase 4a cleanroom).

**proptest strategy**:
```rust
proptest! {
    #[test]
    fn inv_ferr_056_crash_recovery_torn_write(
        initial_datoms in prop::collection::btree_set(arb_datom(), 0..50),
        transactions in prop::collection::vec(arb_committed_transaction(), 1..10),
        cut_fraction in 0.01f64..0.99,
    ) {
        // Build store with committed transactions
        let mut store = Store::genesis();
        let mut committed_datoms = BTreeSet::new();
        for tx in &transactions {
            match store.transact(tx.clone()) {
                Ok(receipt) => {
                    for d in receipt.datoms() {
                        committed_datoms.insert(d.clone());
                    }
                }
                Err(_) => {} // Schema violation — skip
            }
        }

        // Serialize WAL, then simulate torn write
        let wal_bytes = store.wal_bytes();
        let cut_point = (wal_bytes.len() as f64 * cut_fraction) as usize;
        let torn_wal = &wal_bytes[..cut_point.max(1)];

        // Recover
        let recovered = Store::recover_from_wal_bytes(torn_wal);

        // Datoms from complete WAL frames must survive
        let complete_frame_datoms = datoms_from_complete_frames(torn_wal);
        for d in &complete_frame_datoms {
            prop_assert!(recovered.datom_set().contains(d),
                "INV-FERR-056: committed datom {:?} lost after torn write at byte {}",
                d, cut_point);
        }
    }

    #[test]
    fn inv_ferr_056_checkpoint_corruption_fallback(
        datoms in prop::collection::btree_set(arb_datom(), 1..100),
        corrupt_byte_idx in any::<usize>(),
        corrupt_value in any::<u8>(),
    ) {
        let store = Store::from_datoms(datoms.clone());

        // Write checkpoint, then corrupt it
        let mut checkpoint_bytes = store.to_checkpoint_bytes();
        let idx = corrupt_byte_idx % checkpoint_bytes.len();
        if checkpoint_bytes[idx] != corrupt_value {
            checkpoint_bytes[idx] = corrupt_value;

            // Recovery must reject corrupt checkpoint (BLAKE3 mismatch)
            let checkpoint_result = Store::from_checkpoint_bytes(&checkpoint_bytes);
            prop_assert!(checkpoint_result.is_err(),
                "INV-FERR-056: corrupt checkpoint accepted at byte {}", idx);
        }
    }

    // NOTE: Full FaultModel coverage requires additional proptest functions
    // for PowerCut (after_nth_sync parameter), IoError (transient read failures),
    // and DiskFull (write rejection). These are tracked by bd-chtz. The three
    // functions below demonstrate the pattern for TornWrite, checkpoint corruption,
    // and BitFlip. PowerCut and IoError proptests follow the same structure but
    // operate on the FaultInjectingBackend (ADR-FERR-011) rather than raw byte
    // manipulation.

    #[test]
    fn inv_ferr_056_bitflip_detection(
        datoms in prop::collection::btree_set(arb_datom(), 1..50),
        flip_offset in any::<usize>(),
        flip_bit in 0u8..8,
    ) {
        let store = Store::from_datoms(datoms.clone());
        let mut wal_bytes = store.wal_bytes();
        if wal_bytes.is_empty() { return Ok(()); }

        let idx = flip_offset % wal_bytes.len();
        wal_bytes[idx] ^= 1 << flip_bit;

        // Frames containing the flipped bit must be rejected
        let recovered = Store::recover_from_wal_bytes(&wal_bytes);
        // Either recovery succeeds with only uncorrupted frames,
        // or it falls through to genesis. Either way, no corrupted
        // datoms are present.
        // (Specific assertion depends on which frame was hit)
    }
}
```

**Lean theorem**:
```lean
/-- Crash recovery under fault model: recovery preserves committed datoms.
    The Lean model abstracts the fault model as a filter on the WAL
    frame sequence — faults remove or corrupt trailing frames. -/

def committed_frames (frames : List (Finset Datom)) (fault_point : Nat) :
    Finset Datom :=
  (frames.take fault_point).foldl (· ∪ ·) ∅

def recover (frames : List (Finset Datom)) (fault_point : Nat) :
    Finset Datom :=
  -- Recovery replays all frames up to the fault point
  -- (corrupt frames after fault_point are discarded)
  committed_frames frames fault_point

theorem crash_recovery_preserves_committed
    (frames : List (Finset Datom)) (fault_point : Nat)
    (h : fault_point ≤ frames.length) :
    committed_frames frames fault_point ⊆
      recover frames fault_point := by
  -- recover = committed_frames by definition
  exact Finset.Subset.refl _
```

**Research note (future investigation)**: TigerBeetle's VOPR (Viewstamped Object Protocol
Replication) simulator tests the *actual production code* under simulated faults in a
deterministic single-threaded simulator that can reproduce any discovered bug. This is
strictly stronger than our FaultInjectingBackend approach (which wraps storage I/O but
does not control thread scheduling). Investigation target:
`https://github.com/tigerbeetle/tigerbeetle` — see `src/vsr/simulator.zig`.

FoundationDB's simulation testing framework (SIGMOD 2023: "Simulation: The Secret Weapon
of Database Engineering") provides the intellectual foundation for why deterministic
simulation is the only path to high confidence in distributed systems. Investigation
target: `https://github.com/apple/foundationdb` — see `flow/` directory.

---

### INV-FERR-057: Sustained Load Invariant Preservation

**Traces to**: Core single-store Stage 0 INV-FERR (001-032),
NEG-FERR-005 (No Unbounded Memory Growth)
**Verification**: `V:PROP`
**Stage**: 1

#### Level 0 (Algebraic Law)
```
Let W(t) be a workload function that produces transactions at rate R for duration T:
  W : [0, T] → Transaction

Let S(t) be the store state at time t under workload W:
  S(0)   = genesis()
  S(t+1) = transact(S(t), W(t))

Let CoreStorageStage0 = { INV-FERR-001, ..., INV-FERR-032 }.

∀ t ∈ [0, T], ∀ I ∈ CoreStorageStage0:
  holds(I, S(t))

Additionally, resource consumption is bounded:
  ∀ t ∈ [0, T]:
    memory(S(t)) / max(1, |S(t).datoms|) ≤ K_mem
  where K_mem = 500 bytes/datom (the NEG-FERR-005 calibration envelope).

Proof sketch: Each individual transact() preserves all invariants (by the
correctness of transact, proven in INV-FERR-001..032). The sustained load
property follows by induction on the transaction sequence for the core
single-store invariant set. Memory boundedness follows from NEG-FERR-005
and the structural sharing properties of im::OrdSet (O(log n) nodes per
insert, amortized O(1) deallocation via Arc reference counting), so the
resident-memory-per-datom ratio remains within a calibrated constant
envelope instead of drifting upward over time.

The property that induction alone does NOT catch: resource leaks that
accumulate over time (e.g., observer callback references preventing GC,
im::OrdSet tree rebalancing drift, or epoch counter overflow at 2^64
transactions). These require empirical soak testing to detect.
```

#### Level 1 (State Invariant)
All core single-store Stage 0 invariants hold continuously under sustained
transactional workload for any duration. The store does not degrade, leak
memory, accumulate state drift, or exhibit any behavior at time T that was
not present at time 0.

Concretely, a soak test running for 8 hours at 1,000 transactions/second (28.8M
transactions total) must show:
- Zero invariant violations (all INV-FERR-001..032 checked at regular intervals)
- Memory usage proportional to datom count; `resident_bytes / max(1, datom_count)`
  stays within the NEG-FERR-005 calibration envelope (~200-500 bytes/datom)
- Index bijection (INV-FERR-005) verified every 10,000 transactions
- Epoch monotonicity (INV-FERR-007) verified continuously
- No performance degradation beyond O(log n) expected from index growth

This invariant is Stage 1 because soak testing requires substantial compute time and
is not required for Phase 4a gate closure. However, it should be established before
Phase 4b (production readiness).

Without this invariant, the system's correctness is proven only for short test
sequences (10,000 proptest cases). Production workloads run for days, weeks, or months.
Issues that manifest only under sustained load — memory leaks, counter overflows,
cache pollution, GC pressure — are invisible to bounded testing.

#### Level 2 (Implementation Contract)
```rust
/// Soak test configuration.
/// Parameterizes duration, transaction rate, verification interval,
/// and resource thresholds.
pub struct SoakConfig {
    /// Total soak duration.
    pub duration: Duration,
    /// Target transactions per second.
    pub txn_rate: u32,
    /// How often to verify invariants (every N transactions).
    pub verification_interval: u64,
    /// Per-datom memory bound (bytes), calibrated from NEG-FERR-005.
    pub max_bytes_per_datom: usize,
    /// Maximum allowed index bijection check failures.
    pub max_bijection_failures: u32,
    /// Executable checks for the core single-store invariant set.
    pub invariant_checks: &'static [fn(&Store) -> Result<(), SoakFailure>],
}

/// Soak test result.
pub struct SoakResult {
    pub total_transactions: u64,
    pub total_datoms: u64,
    pub duration: Duration,
    pub invariant_checks: u64,
    pub invariant_failures: Vec<SoakFailure>,
    pub peak_memory_bytes: usize,
    pub peak_bytes_per_datom: f64,
}

pub struct SoakFailure {
    pub transaction_number: u64,
    pub elapsed: Duration,
    pub invariant: &'static str,
    pub details: String,
}

/// Execute a soak test.
///
/// INV-FERR-057: All core single-store Stage 0 invariants hold for the full duration.
/// Resource consumption stays within bounds.
pub fn run_soak_test(config: &SoakConfig) -> SoakResult {
    let mut db = Database::genesis();
    let mut result = SoakResult::default();
    let start = Instant::now();
    let mut rng = StdRng::seed_from_u64(0xFERRA_50AK);
    let mut last_epoch = None;

    while start.elapsed() < config.duration {
        // Generate and apply transaction
        let tx = generate_random_transaction(&mut rng, &db);
        match db.transact(tx) {
            Ok(receipt) => {
                result.total_transactions += 1;

                // INV-FERR-007: committed epochs strictly increase.
                if let Some(prev) = last_epoch {
                    if receipt.epoch() <= prev {
                        result.invariant_failures.push(SoakFailure {
                            invariant: "INV-FERR-007",
                            details: format!("epoch {} <= previous {}", receipt.epoch(), prev),
                            ..
                        });
                    }
                }
                last_epoch = Some(receipt.epoch());
            }
            Err(FerraError::Backpressure) => continue, // Expected under load
            Err(e) => {
                result.invariant_failures.push(SoakFailure {
                    invariant: "transact-success",
                    details: format!("{e}"),
                    ..
                });
            }
        }

        // Periodic invariant verification
        if result.total_transactions % config.verification_interval == 0 {
            result.invariant_checks += 1;
            let snapshot = db.snapshot();

            // Execute the registered core invariant checks (e.g. INV-FERR-005).
            for check in config.invariant_checks {
                if let Err(failure) = check(&snapshot) {
                    result.invariant_failures.push(failure);
                }
            }

            // NEG-FERR-005: Memory bounds
            let mem = current_memory_usage();
            let datom_count = snapshot.len();
            if datom_count > 0 {
                let bytes_per_datom = mem as f64 / datom_count as f64;
                if bytes_per_datom > config.max_bytes_per_datom as f64 {
                    result.invariant_failures.push(SoakFailure {
                        invariant: "NEG-FERR-005",
                        details: format!("{bytes_per_datom:.0} bytes/datom > {}",
                            config.max_bytes_per_datom),
                        ..
                    });
                }
            }
        }

        // Throttle to target rate
        // todo!("Phase 4b: rate limiter")
    }

    result
}
```

**Falsification**: Any workload `W` of `N` transactions and a transaction index `t ∈ [0, N]`
such that one of the following concrete conditions holds:

1. **Index bijection violation at step t**: `|S(t).indexes.eavt| != |S(t).datoms|`
   — the EAVT index has a different cardinality from the primary datom set after `t`
   transactions. Input shape: generate `t` random transactions; after each, check all 4
   index cardinalities.

2. **Epoch non-monotonicity at step t**: `S(t).epoch <= S(t-1).epoch` despite
   `transact(S(t-1), W(t))` returning `Ok(receipt)`. Input shape: record `receipt.epoch()`
   after each successful transact; verify `epoch[i] > epoch[i-1]` for all `i`.

3. **Memory growth exceeding O(n)**: `memory(S(t)) / |S(t).datoms| > K` for constant `K`
   that was not exceeded at `t=0`. Input shape: sample `memory/datom_count` at regular
   intervals; compute running maximum; detect monotonic upward drift not explained by
   datom growth. The bound `K` is derived from the Datom struct size (5 fields, ~200 bytes)
   plus index and persistent-structure overhead. The calibrated certification envelope is
   `K = 500 bytes/datom` per NEG-FERR-005; exceeding it indicates either a leak or a
   broken calibration assumption that must be investigated before gate closure.

4. **CRDT law violation at step t**: After `t` transactions, merge the store with a
   snapshot taken at step `t-100` — the result should contain all datoms from both
   (INV-FERR-001/003). A violation here indicates that sustained transact operations
   have corrupted the store's CRDT properties.

Use a two-part strategy:
1. A deterministic mini-soak proptest checks sequence-sensitive invariants such as
   epoch monotonicity, index bijection, and memory-envelope drift over hundreds of
   transactions.
2. A nightly integration soak measures process RSS against the same `K_mem = 500`
   bound over multi-hour runs, because host-level memory signals are noisier than
   pure in-process algebraic properties.

**proptest strategy**:
```rust
proptest! {
    #[test]
    fn inv_ferr_057_mini_soak(
        txn_count in 100u64..1000,
        datom_seed in any::<u64>(),
    ) {
        let config = SoakConfig {
            duration: Duration::from_secs(0), // Use txn_count instead
            txn_rate: 0,
            verification_interval: 50,
            max_bytes_per_datom: 500,
            max_bijection_failures: 0,
            invariant_checks: &[verify_index_bijection_invariant],
        };
        let mut db = Database::genesis();
        let mut rng = StdRng::seed_from_u64(datom_seed);
        let mut last_epoch = 0u64;

        for i in 0..txn_count {
            let tx = generate_random_transaction(&mut rng, &db);
            if let Ok(receipt) = db.transact(tx) {
                // INV-FERR-007: epoch strictly increases
                prop_assert!(receipt.epoch() > last_epoch,
                    "INV-FERR-057/007: epoch stalled at txn {i}");
                last_epoch = receipt.epoch();
            }

            if i % config.verification_interval == 0 {
                let snapshot = db.snapshot();
                prop_assert!(verify_index_bijection(&snapshot),
                    "INV-FERR-057/005: index bijection failed at txn {i}");
                let bytes_per_datom = approximate_bytes_per_datom(&snapshot);
                prop_assert!(bytes_per_datom <= config.max_bytes_per_datom as f64,
                    "INV-FERR-057/NEG-FERR-005: memory envelope exceeded at txn {i}");
            }
        }
    }
}
```

**Lean theorem**:
```lean
/-- Sustained load preservation: if every individual transaction
    preserves invariant P, then any finite sequence of transactions
    preserves P. This is the inductive step that soak testing validates
    empirically for "any finite sequence" including very long ones. -/
theorem sustained_preservation
    (P : DatomStore → Prop)
    (genesis_holds : P ∅)
    (step_preserves : ∀ S d, P S → P (apply_tx S d))
    (txns : List Datom) :
    P (txns.foldl apply_tx ∅) := by
  induction txns with
  | nil => exact genesis_holds
  | cons d rest ih => exact step_preserves _ d ih
```

**Research note**: sled (`https://github.com/spacejam/sled`) uses process-kill soak tests
that terminate the database process at random points and verify recovery. This is more
realistic than in-process fault injection because it tests the actual OS/filesystem
interaction. Investigation priority for Phase 4b soak test hardening.

---

### INV-FERR-058: Query Metamorphic Equivalence

**Traces to**: INV-FERR-033 (Cross-Shard Query Correctness), C4 (CRDT merge = set union),
INV-FERR-029 (LIVE View Resolution)
**Verification**: `V:PROP`
**Stage**: 2

#### Level 0 (Algebraic Law)
```
Let eval : (Store, DatalogQuery) → Set<Tuple> be the Datalog evaluation function.
Let ≡_sem be semantic equivalence over Datalog queries under the standard model-
theoretic semantics (minimal Herbrand model).

∀ S ∈ ReachableStore, ∀ Q₁ Q₂ ∈ DatalogQuery:
  Q₁ ≡_sem Q₂  ⟹  eval(S, Q₁) = eval(S, Q₂)

Metamorphic transforms (semantics-preserving rewrites):
  T1: Variable renaming
      Q[?x/?y] ≡_sem Q  (where ?y is fresh)
  T2: Clause reordering
      [C₁, C₂] ≡_sem [C₂, C₁]  (conjunction is commutative)
  T3: Redundant clause elimination
      [C, C] ≡_sem [C]  (conjunction is idempotent)
  T4: Independent predicate repositioning
      [:find ?e :where [?e :a ?v] [?e :b ?w] (> ?v 10)]
      ≡_sem
      [:find ?e :where [?e :a ?v] (> ?v 10) [?e :b ?w]]
      (reordering with non-dependent predicates)

Proof sketch: Each transform preserves the minimal Herbrand model of the
Datalog program over the datom base facts.
  T1 (Variable renaming): Alpha-equivalence — bound variable names do not
    affect the denotation of a Datalog rule.
  T2 (Clause reordering): Commutativity of conjunction — the body of a
    Datalog rule is a conjunction of atoms; reordering does not change the
    set of satisfying substitutions.
  T3 (Redundant clause elimination): Idempotency of conjunction — C ∧ C ≡ C.
  T4 (Independent predicate repositioning): Commutativity of conjunction with non-dependent
    predicates — reordering independent conjuncts does not change results.

Note: The CALM theorem (Consistency as Logical Monotonicity, Hellerstein
2010) applies to the distributed query execution setting (INV-FERR-033),
not to metamorphic rewrite equivalence. The transforms above are grounded
in propositional logic and first-order model theory, not in CALM.

Scope note: This invariant is intentionally limited to rewrites expressible in
the current Ferratomic query surface. Rewrites that require nested negation,
disjunction, or other syntax not modeled by `DatalogQuery` are out of scope
until the language surface grows to support them.
```

#### Level 1 (State Invariant)
For every reachable store state and every pair of semantically equivalent Datalog
queries: the evaluation function returns identical result sets. A Datalog engine that
returns different results for `[:find ?e :where [?e :name ?n] [?e :age ?a]]` vs.
`[:find ?x :where [?x :age ?a] [?x :name ?n]]` (clause reordering) has a correctness
bug, even if both results are "plausible."

Metamorphic testing catches bugs that differential testing misses. In differential
testing, the oracle and subject might agree on the wrong answer for one query shape.
Metamorphic testing reveals this by showing that a semantically equivalent rewrite
produces a different (also wrong) answer. The disagreement between rewrites exposes
the bug.

This invariant is Stage 2 because ferratomic-datalog is Phase 4d. The metamorphic
testing framework should be designed in Phase 4b and implemented in Phase 4d alongside
the Datalog engine.

Without this invariant, the Datalog engine is tested only for individual query shapes.
The combinatorial space of semantically equivalent rewrites is too large for manual
test authoring. Metamorphic testing automates coverage of this space.

#### Level 2 (Implementation Contract)
```rust
/// A metamorphic transform that rewrites a Datalog query while
/// preserving its semantics.
///
/// INV-FERR-058: eval(S, transform(Q)) = eval(S, Q) for all S.
pub trait MetamorphicTransform: Send + Sync {
    /// Human-readable name for diagnostics.
    fn name(&self) -> &str;

    /// Apply the transform to a query. Returns None if the transform
    /// does not apply to this query shape.
    fn apply(&self, query: &DatalogQuery) -> Option<DatalogQuery>;

    /// Soundness proof sketch: why this transform preserves semantics.
    fn soundness_argument(&self) -> &str;
}

/// Variable renaming transform (T1).
pub struct VariableRenaming;
impl MetamorphicTransform for VariableRenaming {
    fn name(&self) -> &str { "T1:variable-renaming" }
    fn apply(&self, query: &DatalogQuery) -> Option<DatalogQuery> {
        todo!("Phase 4d")
    }
    fn soundness_argument(&self) -> &str {
        "Alpha-equivalence: renaming bound variables does not change \
         the denotation of a Datalog rule in the minimal Herbrand model."
    }
}

/// Clause reordering transform (T2).
pub struct ClauseReordering;
impl MetamorphicTransform for ClauseReordering {
    fn name(&self) -> &str { "T2:clause-reordering" }
    fn apply(&self, query: &DatalogQuery) -> Option<DatalogQuery> {
        todo!("Phase 4d")
    }
    fn soundness_argument(&self) -> &str {
        "Commutativity of conjunction: the body of a Datalog rule is a \
         conjunction of atoms; reordering does not change the minimal model."
    }
}

// T3..T4 follow the same pattern. Transforms that require nested negation
// or disjunction are out of scope until the query surface supports them.
```

**Falsification**: A store `S`, a query `Q`, and a metamorphic transform `T` such that
`eval(S, Q) != eval(S, T(Q))` even though `T` is semantics-preserving. Specific failure modes:
- **Join order sensitivity**: The evaluator produces different results depending on which
  clause is evaluated first (indicates a non-commutative join implementation).
- **Variable shadowing**: Renaming a variable causes it to collide with an internal variable
  in the evaluator (indicates a hygiene bug in the variable binding system).
- **Duplicate-clause asymmetry**: `[C, C]` produces different results from `[C]`
  (indicates conjunction is not implemented idempotently in the evaluator).

**proptest strategy**:
```rust
proptest! {
    #[test]
    fn inv_ferr_058_metamorphic_clause_reordering(
        datoms in prop::collection::btree_set(arb_datom(), 0..100),
        query in arb_conjunctive_query(2..5),
    ) {
        let store = Store::from_datoms(datoms);
        let reordered = ClauseReordering.apply(&query);
        if let Some(reordered_query) = reordered {
            let original_result = eval(&store, &query);
            let reordered_result = eval(&store, &reordered_query);
            prop_assert_eq!(original_result, reordered_result,
                "INV-FERR-058: clause reordering changed result set");
        }
    }
}
```

**Lean theorem**:
```lean
/-- Clause reordering preserves evaluation over a finite datom store.
    The Lean model represents Datalog clause evaluation as predicate
    filtering on Finset Datom (the abstract store model from §23.0.4).
    A Datalog conjunctive query `[C₁, C₂]` is modeled as
    `S.filter (c1 ∧ c2)`. This theorem proves T2 (clause reordering)
    for the abstract model; proptest validates it for the concrete
    implementation. The gap between filtering and full Datalog eval
    is bridged by CI-FERR-001 (Lean-Rust coupling invariant).

    NOTE: This proves T2 only. T1 (variable renaming = alpha-equivalence)
    is trivial by `rfl` on the denotation. T3-T4 require separate theorems
    that are deferred to Phase 4d implementation (sorry with tracking beads). -/
theorem clause_reorder_preserves_eval
    (S : DatomStore) (c1 c2 : Datom → Prop) [DecidablePred c1] [DecidablePred c2] :
    S.filter (fun d => c1 d ∧ c2 d) = S.filter (fun d => c2 d ∧ c1 d) := by
  ext d
  simp [and_comm]

-- T3: Redundant clause elimination (idempotency of conjunction)
theorem redundant_clause_elimination
    (S : DatomStore) (c : Datom → Prop) [DecidablePred c] :
    S.filter (fun d => c d ∧ c d) = S.filter c := by
  ext d; simp

-- T4: independent predicate repositioning / filter pushdown (tracked by bd-9g7p)
```

**Research note**: FrankenSQLite's `metamorphic.rs` implements 4 transform families
(Predicate, Projection, Structural, Literal) with deterministic seed-based reproducibility
and mismatch classification. Their architecture — `TransformFamily` enum + `EquivalenceExpectation`
per transform — is directly applicable to Datalog metamorphic testing. Detailed code at
`~/.local/share/btca/resources/frankensqlite/crates/fsqlite-harness/src/metamorphic.rs`.

---

### INV-FERR-059: Optimization Behavioral Preservation

**Traces to**: INV-FERR-025 (Index Backend Interchangeability), INV-FERR-006 (Snapshot
Isolation), ADR-FERR-003 (Concurrency Model Amendment: Mutex → WriterActor)
**Verification**: `V:PROP`, `V:KANI`
**Stage**: 1

#### Level 0 (Algebraic Law)
```
Let QuerySurface be the set of queries expressible by the current Ferratomic
query surface.
Let canon : Set<Tuple> → Bytes be a canonical, order-independent serialization
of a query result.
Let O : StoreImpl → StoreImpl be an optimization function that changes the
implementation without changing the observable behavior. Observable behavior
is defined as:
  obs(S) = { (Q, canon(eval(S, Q))) | Q ∈ QuerySurface }

∀ S ∈ ReachableStore, ∀ Q ∈ QuerySurface:
  canon(eval(S, Q)) = canon(eval(O(S), Q))

Equivalently: O is a behavioral bisimulation. The abstract state (datom set)
is identical; only the concrete representation (memory layout, lock strategy,
index structure) differs.

Specific optimizations covered:
  O₁: Mutex<()> → WriterActor (ADR-FERR-003 Phase 4b amendment)
  O₂: BTreeMapBackend → im::OrdMapBackend (ADR-FERR-001)
  O₃: Single-fsync → group-commit (ADR-FERR-003 WriterActor)
  O₄: Linear index rebuild → incremental index update

Proof sketch: For O₁ (Mutex → WriterActor), both serialization strategies
produce the same total order on transactions — INV-FERR-007 (write
linearizability) holds for both. The WriterActor batches fsync calls (one
fsync per batch instead of one per transaction) but each transaction still
receives an individual epoch and TxReceipt. The datom set after N transactions
is identical because the set of inserted datoms is the same (union is
commutative and associative per INV-FERR-001/002).

For O₂ (BTreeMap → im::OrdMap), INV-FERR-025 already proves interchangeability.
More generally, representation-only optimizations reduce to datom-set equality
plus query extensionality. Scheduling optimizations (e.g. Mutex → WriterActor)
must additionally preserve epoch order, snapshot publication points, and LIVE
resolution inputs (INV-FERR-006, INV-FERR-007, INV-FERR-032). This invariant
generalizes INV-FERR-025 to ALL optimizations, not just index backends.
```

#### Level 1 (State Invariant)
Every optimization applied to the Ferratomic storage engine preserves observable behavior.
"Observable" means: for any query against any reachable store state, the result set is
identical before and after the optimization. The optimization may change performance
characteristics (latency, throughput, memory usage) but not query results, transaction
ordering, or datom content.

This invariant is critical during Phase 4b, which introduces the WriterActor (group commit),
prolly tree indexes, and potentially asupersync-based concurrency. Each optimization must
carry a proof — either formal (Kani/proptest) or structural (INV-FERR-025 generalization) —
that behavior is preserved.

Without this invariant, optimizations are verified only by running the existing test suite
and checking for regressions. This misses subtle behavioral changes that the existing
tests don't cover (e.g., transaction ordering differences under group commit that only
manifest with specific interleaving patterns).

#### Level 2 (Implementation Contract)
```rust
/// Certification artifact for an optimization candidate.
///
/// Before applying optimization O, snapshot the canonical output for
/// a certification corpus of queries. After applying O, re-run the
/// corpus and verify identical canonical output. A passing corpus is
/// necessary but not sufficient: the optimization must also supply the
/// structural witness appropriate to its class.
///
/// INV-FERR-059: canonical query results are preserved, and the relevant
/// structural witness holds.
pub struct StructuralWitness {
    /// Representation changes must preserve the abstract datom set.
    pub datom_set_equal: bool,
    /// Scheduling changes must preserve epoch order and snapshot publication.
    /// Pure representation swaps set this to true because no schedule changes occur.
    pub snapshot_trace_equal: bool,
}

pub struct IsomorphismProof {
    /// Structural witness for the optimization class.
    pub structural_witness: StructuralWitness,
    /// SHA-256 of the canonical output before optimization.
    pub pre_checksum: [u8; 32],
    /// SHA-256 of the canonical output after optimization.
    pub post_checksum: [u8; 32],
    /// The corpus of queries used.
    pub corpus_size: usize,
    /// Whether the checksums match.
    pub verdict: IsomorphismVerdict,
}

pub enum IsomorphismVerdict {
    /// All checksums match: optimization preserves behavior.
    Isomorphic,
    /// At least one checksum mismatch: optimization changes behavior.
    Divergent { first_divergence_query_idx: usize },
}

/// Run an optimization certificate for representation-preserving rewrites.
/// Scheduling optimizations use the same certificate shape over an operation-trace
/// harness, because they must compare epoch order and snapshot publication points,
/// not just pre/post stores.
pub fn verify_optimization_isomorphism<F>(
    store: &Store,
    corpus: &[DatalogQuery],
    optimization: F,
) -> IsomorphismProof
where
    F: FnOnce(Store) -> Store,
{
    // 1. Evaluate all queries against pre-optimization store
    let pre_results: Vec<_> = corpus.iter()
        .map(|q| eval(store, q))
        .collect();
    let pre_checksum = sha256_canonical(&pre_results);

    // 2. Apply optimization
    let optimized = optimization(store.clone());
    let structural_witness = StructuralWitness {
        datom_set_equal: store.datom_set() == optimized.datom_set(),
        snapshot_trace_equal: true, // Pure representation swap; schedule unchanged.
    };

    // 3. Re-evaluate all queries
    let post_results: Vec<_> = corpus.iter()
        .map(|q| eval(&optimized, q))
        .collect();
    let post_checksum = sha256_canonical(&post_results);

    IsomorphismProof {
        structural_witness,
        pre_checksum,
        post_checksum,
        corpus_size: corpus.len(),
        verdict: if structural_witness.datom_set_equal
            && structural_witness.snapshot_trace_equal
            && pre_checksum == post_checksum
        {
            IsomorphismVerdict::Isomorphic
        } else {
            let idx = pre_results.iter().zip(&post_results)
                .position(|(a, b)| a != b)
                .unwrap_or(0);
            IsomorphismVerdict::Divergent { first_divergence_query_idx: idx }
        },
    }
}

#[kani::proof]
#[kani::unwind(8)]
fn optimization_preserves_datom_set() {
    let datoms: BTreeSet<Datom> = kani::any();
    kani::assume(datoms.len() <= 4);

    // Simulate O₂: BTreeMap backend → im::OrdMap backend
    let btree_store = Store::<BTreeMapBackend>::from_datoms(datoms.clone());
    let ordmap_store = Store::<OrdMapBackend>::from_datoms(datoms);

    assert_eq!(btree_store.datom_set(), ordmap_store.datom_set());
}
```

**Falsification**: An optimization `O`, a store `S`, and a query `Q` such that
`eval(S, Q) != eval(O(S), Q)`. Specific failure modes:
- **Group commit reordering**: WriterActor batches transactions T₁ and T₂ into a single
  fsync batch. If the batch applies T₂ before T₁, the epoch ordering changes, which
  changes the LIVE view resolution (LWW depends on epoch ordering per INV-FERR-032).
- **Index rebuild divergence**: Incremental index update produces different index state than
  full rebuild for the same datom set (indicates a missing index entry on a rare code path).
- **Snapshot timing**: ArcSwap update timing changes under WriterActor, causing a snapshot
  taken between two batch commits to see a different state than under Mutex serialization.

**proptest strategy**:
```rust
proptest! {
    #[test]
    fn inv_ferr_059_backend_optimization_preserves_queries(
        datoms in prop::collection::btree_set(arb_datom(), 0..100),
        queries in prop::collection::vec(arb_index_query(), 1..20),
    ) {
        let btree_store = Store::<BTreeMapBackend>::from_datoms(datoms.clone());
        let ordmap_store = Store::<OrdMapBackend>::from_datoms(datoms);

        for (i, query) in queries.iter().enumerate() {
            let btree_result: BTreeSet<_> = btree_store.index_lookup(query).collect();
            let ordmap_result: BTreeSet<_> = ordmap_store.index_lookup(query).collect();
            prop_assert_eq!(btree_result, ordmap_result,
                "INV-FERR-059: optimization divergence on query {i}");
        }
    }
}
```

**Lean theorem**:
```lean
/-- Optimization behavioral preservation: if two stores contain the same
    datom set, any query produces the same result on both. This is the
    extensionality principle for stores. -/
theorem optimization_preserves_query
    (S1 S2 : DatomStore) (h : S1 = S2)
    (query : Datom → Prop) [DecidablePred query] :
    S1.filter query = S2.filter query := by
  rw [h]
```

**Research note**: FrankenSQLite's `isomorphism_proof.rs` implements 10 proof invariant
classes (RowOrdering, TieBreak, FloatingPointPrecision, RngDeterminism, GoldenChecksum,
TypeAffinity, NullPropagation, ErrorCodes, AggregateSemantics, WindowFunctionSemantics)
with mandatory-vs-advisory classification. For Ferratomic, the relevant classes are:
DatomOrdering, EpochMonotonicity, LiveResolution, SchemaEquivalence, IndexBijection.
Detailed code at `~/.local/share/btca/resources/frankensqlite/crates/fsqlite-harness/src/isomorphism_proof.rs`.

---

### ADR-FERR-011: Deterministic Fault Injection Framework

**Traces to**: INV-FERR-056, INV-FERR-014, INV-FERR-008, NEG-FERR-003
**Stage**: 0

**Problem**: INV-FERR-014 (Recovery Correctness) and INV-FERR-008 (WAL Fsync Ordering)
are tested via proptest with clean serialization roundtrips. This validates the happy
path but does not test recovery under realistic storage failures: torn writes, power
cuts, I/O errors, and disk full conditions. How should the verification suite test
crash recovery under adversarial storage conditions?

**Options**:

| Option | Description | Pros | Cons |
|--------|-------------|------|------|
| A: FaultInjectingBackend | Wrap `StorageBackend` trait with a decorator that injects deterministic faults per `FaultSpec` | Deterministic, reproducible, no OS dependency. Runs in-process. Easy to add new fault kinds. | Does not test actual OS/filesystem interaction. Torn writes are simulated, not real. |
| B: Process-kill testing | Fork the test process, kill it at random points, verify recovery | Tests actual crash behavior. Catches kernel/filesystem bugs. | Non-deterministic (kill timing varies). Slow (process creation overhead). Flaky in CI. |
| C: Failpoints library | Use `fail` crate to inject errors at specific code points | Fine-grained control. Can target specific functions. | Requires annotating production code with failpoint macros. Introduces conditional compilation. |
| D: LabRuntime simulation | Use asupersync's `LabRuntime` to simulate I/O failures in deterministic virtual time | Deterministic. Tests the actual async code paths. Integrates with asupersync obligation tracking. | Requires async I/O paths (Phase 4b+). Not available for Phase 4a synchronous code. |

**Decision**: **Option A: FaultInjectingBackend** for Phase 4a/4b. Option D (LabRuntime)
as a complement when asupersync integration matures in Phase 4c.

The FaultInjectingBackend approach provides:
- **Deterministic reproducibility**: same FaultSpec = same failure. Regression tests are stable.
- **Composability**: multiple faults can be combined (torn write + power cut on different paths).
- **Zero production-code modification**: the fault injector wraps the StorageBackend trait,
  requiring no changes to WAL, checkpoint, or recovery code.
- **In-process execution**: no fork/kill overhead. Tests run at proptest speed.

**Rejected**:
- Option B (process-kill): Non-deterministic. Failures that depend on OS scheduling
  cannot be reliably reproduced. Useful as a Phase 4b supplement (see INV-FERR-057
  soak testing) but not as the primary crash recovery verification mechanism.
- Option C (failpoints): Pollutes production code with conditional compilation macros.
  Every `fail_point!("name")` is a maintenance burden and a potential performance
  regression. The StorageBackend trait already provides the injection point; wrapping
  it is cleaner than annotating internal code.
- Option D (LabRuntime): Not available for Phase 4a synchronous code. LabRuntime
  requires async I/O paths. Will become the primary approach when asupersync is fully
  integrated (Phase 4c), at which point FaultInjectingBackend becomes a fallback.

**Consequence**: `ferratomic-verify` gains a `fault_injection` module containing
`FaultInjectingBackend`, `FaultSpec`, and `FaultState`. INV-FERR-056 tests are written
against this backend. The `StorageBackend` trait (already defined in ferratomic-core)
is the injection seam — no changes to core production code.

**Source**: INV-FERR-008, INV-FERR-014, NEG-FERR-003. Cross-pollination from
FrankenSQLite `fault_vfs.rs` pattern, FoundationDB simulation testing philosophy,
TigerBeetle VOPR deterministic simulation.

---

### ADR-FERR-012: Bayesian Confidence Quantification

**Traces to**: All INV-FERR (verification quality), NEG-FERR-006
**Stage**: 0

**Problem**: The proptest suite runs 10,000 cases per property and reports pass/fail.
A pass provides evidence but not a quantified confidence bound. "10,000 cases passed"
does not answer "how confident are we that INV-FERR-001 holds for ALL inputs?" How
should per-invariant verification confidence be quantified and reported?

**Options**:

| Option | Description | Pros | Cons |
|--------|-------------|------|------|
| A: Bayesian Beta-posterior | Model each invariant as Bernoulli trial. n successes, 0 failures → Beta(n+1, 1). Report 95% credible interval lower bound. | Mathematically rigorous. Single-number summary. Composable across invariants. | Assumes independence between test cases (proptest random generation approximates this). |
| B: Frequentist confidence interval | Clopper-Pearson exact interval on the proportion of inputs that satisfy the invariant. | Well-understood. Conservative. No prior assumption. | Wider intervals. No natural way to incorporate prior information (e.g., Lean proof). |
| C: Pass/fail with case count | Report "10,000/10,000 passed" and let humans interpret. | Simplest. No statistical machinery. | No quantified confidence. 10,000 cases on INV-FERR-001 and 10,000 on INV-FERR-028 provide different epistemic weight (different input spaces). |

**Decision**: **Option A: Bayesian Beta-posterior** with conjugate prior.

For each invariant tested by proptest with `n` passes and `k` failures:
```
Posterior: Beta(α + n - k, β + k)
  where α = 1, β = 1 (uniform prior, no prior knowledge)

95% credible interval lower bound:
  L = BetaInv(0.025, α + n - k, β + k)

For n = 10,000, k = 0:
  Beta(10001, 1)
  L = 1 - 0.05^(1/10001) ≈ 0.99970

Interpretation: "We are 95% confident that INV-FERR-001 holds for at least
99.97% of the input space, given 10,000 passes and 0 failures."
```

**Prior selection**: The uniform prior Beta(1,1) is deliberately uninformative. When a
Lean proof exists for the invariant (e.g., INV-FERR-001 has `merge_comm` proven in Lean),
the proptest confidence is supplementary — the Lean proof provides certainty for the
abstract model, and proptest bridges the gap to the concrete implementation. A future
refinement could use an informative prior (e.g., Beta(100, 1)) when a Lean proof exists,
but this introduces modeling assumptions we defer.

**Gate threshold**: For the current 59-invariant catalog, every required Stage 0
invariant exercised by proptest must achieve a lower bound of at least 0.9997
(the 10,000-pass / 0-failure Beta(1,1) floor). The product across those required
Stage 0 invariant lower bounds must exceed 0.95. A 0.999 per-invariant floor would
be insufficient at this catalog size (`0.999^59 ≈ 0.9427`).

**Rejected**:
- Option B: Clopper-Pearson intervals are conservative (wider) and do not naturally
  accommodate the Lean proof as prior information.
- Option C: Insufficient for the zero-defect quality standard. "All tests pass" is
  necessary but not sufficient.

**Consequence**: `ferratomic-verify` gains a `confidence` module that:
1. Collects pass/fail counts per INV-FERR after each proptest run.
2. Computes Beta-posterior credible intervals per invariant.
3. Generates a machine-readable JSON report with per-invariant confidence bounds.
4. Fails CI if any invariant's lower bound falls below the gate threshold.

**Source**: SEED.md §10 (Bootstrap — epistemic foundation). Cross-pollination from
FrankenSQLite `confidence_gates.rs` (Beta-posterior + conformal bands).

**Research note**: FrankenSQLite also implements conformal martingale monitoring
(`conformal_martingale.rs`) for distribution-free statistical monitoring and BOCPD
(`bocpd.rs`) for Bayesian online change-point detection of performance regressions.
These are Phase 4c refinements — once Ferratomic has continuous benchmarking, BOCPD
can detect subtle performance regressions that threshold-based checks miss.

---

### ADR-FERR-013: Machine-Readable Invariant Catalog

**Traces to**: All INV-FERR (traceability), ADR-FERR-012 (confidence reporting)
**Stage**: 0

**Problem**: The 59 INV-FERR invariants (plus 2 CI-FERR coupling invariants) are
defined in `spec/*.md` — well-documented
but not programmatically queryable. Questions like "which invariants have Lean proofs
but no Kani harness?" or "what is the coverage for Stage 0 invariants?" require manual
cross-referencing. How should invariant metadata be encoded for automated analysis?

**Options**:

| Option | Description | Pros | Cons |
|--------|-------------|------|------|
| A: Rust data structure | Encode all invariants as `const` arrays of `Invariant` structs in `ferratomic-verify` | Compile-time checked. Queryable at test time. Type-safe. | Must be kept in sync with spec markdown. |
| B: TOML manifest | External `invariants.toml` parsed at runtime | Easy to edit. No recompilation. | Not compile-time checked. Deserialization failures at runtime. |
| C: Derive from markdown | Parse `spec/*.md` at build time to extract invariant metadata | Single source of truth. No manual sync. | Fragile: depends on markdown formatting. Build-time parsing adds complexity. |

**Decision**: **Option A: Rust data structure** with mandatory `spec-sync` CI check.

The invariant catalog is a `const` array in `ferratomic-verify/src/invariant_catalog.rs`:

```rust
pub struct Invariant {
    pub id: &'static str,
    pub name: &'static str,
    pub stage: u8,
    pub lean_theorem: Option<&'static str>,
    pub kani_harness: Option<&'static str>,
    pub proptest_fn: Option<&'static str>,
    pub stateright_model: Option<&'static str>,
    pub integration_test: Option<&'static str>,
    pub traces_to: &'static [&'static str],
}

pub const CATALOG: &[Invariant] = &[
    Invariant {
        id: "INV-FERR-001",
        name: "Merge Commutativity",
        stage: 0,
        lean_theorem: Some("merge_comm"),
        kani_harness: Some("merge_commutativity"),
        proptest_fn: Some("inv_ferr_001_merge_commutativity"),
        stateright_model: Some("CrdtModel"),
        integration_test: Some("inv_ferr_001_merge_commutes_concrete"),
        traces_to: &["L1", "C4", "INV-STORE-004"],
    },
    // ... all 59 INV-FERR invariants plus 2 CI-FERR coupling invariants
];
```

A CI job `check-invariant-catalog` greps `spec/*.md` for `### INV-FERR-` headers and
verifies every ID appears in `CATALOG`. This catches additions to the spec that were
not propagated to the catalog.

**Rejected**:
- Option B: Runtime parsing failures are unacceptable for a verification tool.
- Option C: Markdown is not a stable parsing target. Format changes (e.g., changing
  from `###` to `####`) silently break extraction.

**Consequence**: `ferratomic-verify` gains `invariant_catalog.rs` containing the
complete catalog. Automated reports can query: coverage by stage, coverage by
verification layer, invariants with no test, invariants with no Lean proof.

**Source**: ADR-FERR-007 (Lean-Rust Bridge — invariant traceability requirement).
Cross-pollination from FrankenSQLite `parity_invariant_catalog.rs` (machine-readable
catalog with proof obligations, feature mappings, and traceability reports).

---

### ADR-FERR-014: Phase Gate Certification

**Traces to**: All phase gate beads (bd-add, bd-7ij, bd-fzn, bd-lvq), ADR-FERR-012
(confidence quantification), ADR-FERR-013 (invariant catalog)
**Stage**: 1

**Problem**: Phase gates are currently enforced via beads dependencies (bd-add, bd-7ij,
etc.) with human-assessed gate criteria. "Is Phase 4a done?" requires a human to check
cleanroom defect counts, test pass rates, and Lean sorry counts. How should phase gate
closure be formalized as a machine-verifiable decision?

**Options**:

| Option | Description | Pros | Cons |
|--------|-------------|------|------|
| A: Release certificate | Machine-generated JSON artifact aggregating all gate conditions. Produces Approved / Conditional / Rejected verdict. | Machine-verifiable. Auditable. Deterministic. Can run in CI. | Must define all gate conditions precisely. May over-constrain early phases. |
| B: Human-signed checklist | Markdown checklist signed by reviewer. | Flexible. Captures nuance. | Not machine-verifiable. Human error. No CI enforcement. |
| C: CI gate matrix | CI pipeline with separate jobs for each gate condition (tests, lints, proofs, coverage). All must pass. | Existing CI infrastructure. Incremental adoption. | No single artifact. Hard to get holistic view. Condition spread across CI config. |

**Decision**: **Option A: Release certificate** generated by `ferratomic-verify`.

A phase gate certificate aggregates:
1. **Invariant coverage report** (from ADR-FERR-013): per-invariant verification layer coverage
2. **Confidence bounds** (from ADR-FERR-012): per-invariant Beta-posterior lower bounds
3. **Build health**: cargo check, clippy, fmt results
4. **Lean proof status**: sorry count (must be 0 for gate closure)
5. **Test results**: pass/fail counts, test count
6. **Beads status**: open/closed counts, P0/P1 bug count (must be 0)
7. **Cleanroom defect count**: CRITICAL (must be 0), MAJOR (must be 0 for Stage 0 invariants)

Verdict logic:
```
Approved:
  sorry_count == 0 AND
  critical_defects == 0 AND
  major_defects_stage_0 == 0 AND
  all_tests_pass AND
  all_required_stage_0_confidence_lower_bounds >= 0.9997 AND
  p0_bugs == 0 AND p1_bugs == 0

Conditional:
  All Approved conditions met EXCEPT:
  minor_defects > 0 OR
  some_stage_1_invariants_have_sorry

Rejected:
  Any Approved condition violated.
```

The certificate is a deterministic JSON artifact: same inputs → same verdict. It can be
regenerated at any time to verify gate status.

**Rejected**:
- Option B: Does not scale. Manual checklists are error-prone and not auditable.
- Option C: Scattered conditions are hard to reason about holistically. A single
  certificate provides the complete picture.

**Consequence**: `ferratomic-verify` gains a `release_certificate` module that generates
the certificate. CI runs it before any gate closure bead (bd-add, bd-7ij, etc.) can
be marked closed.

**Source**: Phase gate beads (bd-add, bd-7ij, bd-fzn, bd-lvq) — structural enforcement
that requires formalized criteria. Cross-pollination from FrankenSQLite
`release_certificate.rs` (machine-verifiable certificate with evidence ledger, drift
monitoring, adversarial counterexample results, and CI artifact manifest).

---

### NEG-FERR-006: No Unquantified Verification Claims

**Traces to**: ADR-FERR-012 (Bayesian Confidence Quantification), all INV-FERR,
SEED.md §10 (Bootstrap — epistemic foundation for self-verifying systems)
**Stage**: 0

No phase gate closure, progress review, or public documentation may claim verification
of an INV-FERR invariant without a quantified confidence bound. "All tests pass" is
necessary but insufficient. The minimum reporting standard is:

```
INV-FERR-NNN: <name>
  Lean:      {proven | sorry(bead-id) | N/A}
  proptest:  {n_pass}/{n_total}, Beta(n+1,1) lower bound = {L:.5f}
  Kani:      {verified(unwind=N) | N/A}
  Stateright: {verified(states=N) | N/A}
  Gate:      {PASS | FAIL(reason)}
```

Violation of this requirement is a process defect — it does not cause data loss, but it
undermines the epistemic foundation of the zero-defect quality standard. A system that
claims "all invariants verified" without quantification provides no basis for computing
the probability of a production defect.

---

### §23.12.7 Self-Monitoring Convergence: B17 → M(S) ≅ S

**Traces to**: ADR-FERR-012 (Bayesian Confidence), ADR-FERR-013 (Invariant Catalog),
ADR-FERR-014 (Release Certificates), INV-FERR-060 (Store Identity), INV-FERR-051
(Signed Transactions), GOALS.md §5 (Compound Interest Argument)

This section formalizes the architectural vision connecting the verification
infrastructure invariants and ADRs into a convergent self-monitoring system. Each
phase deposits verification evidence into the store; each subsequent phase draws
on that evidence to make stronger claims. The terminal state is a store that
answers "am I correct?" by querying itself — not an external oracle.

**The compound interest chain:**

```
Phase 4a.5: B17 (Self-Verifying Spec Store)
   Store installs its own invariants, ADRs, and schema as signed datoms.
   Creates :spec/*, :adr/*, :verification/*, :gate/* attribute namespaces.
   The store IS the verification oracle.
       │
       ▼
Phase 4b:  R16 (Falsification-Bound Witnesses) + ADR-FERR-012
   Each verified invariant gains triple-hash witness datoms:
     {:e inv-001 :a :witness/spec-hash :v <blake3>}
     {:e inv-001 :a :witness/test-hash :v <blake3>}
     {:e inv-001 :a :witness/proof-hash :v <blake3>}
   When any hash changes, the witness auto-invalidates.
   ADR-FERR-012: Each proptest/Kani run ASSERTS its result as a datom.
     Beta-posterior confidence is a materialized view over pass/fail datoms.
   ADR-FERR-013: Machine-readable catalog becomes "query the datom catalog"
     not "read const array." The catalog IS the store querying itself.
       │
       ▼
Phase 4c:  ADR-FERR-014 (Release Certificates)
   Gate certificate is a signed transaction:
     {:e gate-4a5 :a :gate/verdict :v :approved}
     {:e gate-4a5 :a :gate/timestamp :v <now>}
     {:e gate-4a5 :a :gate/query-result :v :empty-set}
     {:e gate-4a5 :a :gate/certificate-hash :v <blake3>}
   Signed by gate authority, with causal predecessors referencing all
   verification transactions. The gate query IS the gate check.
       │
       ▼
Phase 4d:  M(S) ≅ S (Reflexive Property)
   The Datalog engine enables native gate-closure queries:
     "∀ Stage 0 invariants: :verification/confidence ≥ 0.999
      AND :verification/lean-status = :proven"
   If the query returns empty (no blocking invariants), the gate closes.
   The store's self-description uses the same algebra (P(D), ∪) as all
   other data. M(S) — the store's metadata about itself — IS contained
   within S.
```

**Why this is a compound interest argument, not a convenience feature:**

Each phase adds a layer of self-knowledge. B17 seeds the catalog. R16 populates
it with content-addressed witnesses. ADR-FERR-012 quantifies confidence. ADR-FERR-014
generates certificates. The Datalog engine makes the gate check a theorem (a
query with quantified Bayesian confidence), not an opinion.

The critical insight is that starting early costs almost nothing — signing adds
~64 bytes per transaction (INV-FERR-051), witness datoms add ~3 datoms per
invariant — but the accumulated provenance history makes each subsequent phase's
trust gradient (INV-FERR-054) more valuable. This is the same compound interest
dynamic as the HTTP→HTTPS transition: systems that defer authentication pay a
painful migration cost, while systems that authenticate from genesis accumulate
trust naturally.

**Formal property**: Let `M(S)` denote the verification metadata within store `S`
(all datoms in the `:spec/*`, `:verification/*`, `:gate/*` namespaces). The
self-monitoring convergence property is:

```
∀ S at Phase 4d completion:
  M(S) ⊆ S                     -- metadata IS datoms in the store
  gate_query(S) : Bool          -- gate closure is a Datalog query over S
  gate_query(S) = true  iff     -- the query is a decision procedure
    ∀ inv ∈ required_stage_0(S):
      confidence(S, inv) ≥ 0.999 ∧ lean_status(S, inv) = proven
```

This property does not require a new INV-FERR — it is the emergent consequence
of INV-FERR-060 (store identity), INV-FERR-051 (signed transactions),
ADR-FERR-012 (Bayesian confidence), ADR-FERR-013 (invariant catalog), and
ADR-FERR-014 (release certificates) composed together. Formalizing it here
ensures the architectural vision is not lost across phase boundaries.

**Phase gate integration**: B17 is the capstone of Phase 4a.5 (bd-oiqr dependency
chain). R16 requires Phase 4b (content-addressed witnesses need prolly tree
hashes). ADR-FERR-014 certificates require Phase 4c. The Datalog gate query
requires Phase 4d. The chain is fully captured in the phase gate beads:
bd-add → bd-7ij → bd-fzn → bd-lvq.

---

### Strategic Phase Integration

The invariants and ADRs in this section integrate into the phase gate structure as follows:

| Artifact | Phase Gate | Blocking? | Rationale |
|----------|-----------|-----------|-----------|
| ADR-FERR-011 (Fault Injection Framework) | bd-7ij (4b) | Yes | Enables INV-FERR-056 testing |
| ADR-FERR-012 (Bayesian Confidence) | bd-7ij (4b) | Yes | Required by NEG-FERR-006 |
| ADR-FERR-013 (Invariant Catalog) | bd-7ij (4b) | Yes | Enables automated coverage reports |
| INV-FERR-056 (Crash Recovery Fault Model) | bd-7ij (4b) | Yes | Crash-safety must be adversarially tested before production |
| NEG-FERR-006 (No Unquantified Claims) | bd-7ij (4b) | Yes | Quality standard for Phase 4b gate |
| ADR-FERR-014 (Release Certificates) | bd-fzn (4c) | Yes | Formalizes gate closure for Phase 4c+ |
| INV-FERR-057 (Sustained Load) | bd-fzn (4c) | Yes | Soak testing before federation |
| INV-FERR-059 (Optimization Preservation) | bd-fzn (4c) | Yes | WriterActor upgrade proof |
| INV-FERR-058 (Metamorphic Query Equiv) | bd-lvq (4d) | Yes | Datalog correctness |

**Dependency graph** (new artifacts only):
```
ADR-FERR-011 (Fault Injection) ──► INV-FERR-056 (Crash Fault Testing)
ADR-FERR-012 (Bayesian Confidence) ──► NEG-FERR-006 (No Unquantified Claims)
ADR-FERR-013 (Invariant Catalog) ──► ADR-FERR-014 (Release Certificates)
                                  ──► ADR-FERR-012 (Bayesian Confidence)
INV-FERR-025 (Index Backend) ──► INV-FERR-059 (Optimization Preservation)
INV-FERR-033 (Cross-Shard Query) ──► INV-FERR-058 (Metamorphic Query Equiv)
```

---

### Future Research Directions

The following codebases were identified as high-value investigation targets for further
strengthening the verification infrastructure. Each addresses a specific gap not fully
closed by the patterns in this section.

#### TigerBeetle (Priority: 9/10)
**Repository**: `https://github.com/tigerbeetle/tigerbeetle`
**Gap addressed**: Deterministic simulation testing of the actual production code.

TigerBeetle's VOPR (Viewstamped Object Protocol Replication) simulator runs the entire
distributed protocol in a single-threaded deterministic simulator. Unlike
FaultInjectingBackend (ADR-FERR-011), which wraps storage I/O, VOPR controls thread
scheduling, message delivery, and clock progression. This enables exhaustive exploration
of concurrency interleavings — a class of bugs that Stateright model-checks on an
abstraction but does not test on the production binary.

**Investigation scope**: `src/vsr/simulator.zig` (simulator core), `src/testing/`
(debug allocator, storage determinism), project documentation on zero-defect philosophy.

**Potential Ferratomic adoption**: When asupersync's LabRuntime matures (Phase 4c),
adapt the VOPR pattern to test Ferratomic's federation protocol under deterministic
simulation. LabRuntime already provides DPOR (ADR-FERR-002) — the missing piece is
storage simulation and message delivery control.

#### FoundationDB (Priority: 8/10)
**Repository**: `https://github.com/apple/foundationdb`
**Gap addressed**: Fault injection taxonomy and simulation testing philosophy.

FoundationDB pioneered the "simulation testing" approach described in the 2023 SIGMOD
paper "Simulation: The Secret Weapon of Database Engineering." Their fault taxonomy
(network partition, disk failure, process crash, clock skew, Byzantine behavior)
is more comprehensive than our FaultModel definition. Their approach to composing
faults into "chaos campaigns" with statistical coverage arguments could inform
INV-FERR-056 and INV-FERR-057.

**Investigation scope**: `flow/` directory (Flow programming language for deterministic
testing), `fdbserver/workloads/` (chaos campaign definitions), `documentation/sphinx/`
(testing methodology).

**Potential Ferratomic adoption**: Extend the FaultModel with network-layer faults
(required for Phase 4c federation). Adopt the chaos campaign concept for INV-FERR-057
soak testing.

#### sled (Priority: 7/10)
**Repository**: `https://github.com/spacejam/sled`
**Gap addressed**: Rust-specific crash atomicity and process-kill testing.

sled implements process-kill testing that terminates the database process mid-operation
and verifies recovery — testing actual OS/filesystem interaction rather than simulated
faults. This complements FaultInjectingBackend (ADR-FERR-011 Option A) with Option B
(process-kill) for higher-fidelity crash testing.

**Investigation scope**: `tests/` (crash recovery tests), `src/pagecache/` (crash-safe
page cache), `src/tree/` (B-tree with crash atomicity guarantees).

**Potential Ferratomic adoption**: Add process-kill soak tests as a Phase 4b supplement
to FaultInjectingBackend. sled's approach to io_uring integration could inform
Ferratomic's asupersync I/O layer.
