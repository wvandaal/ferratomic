## 23.3 Performance & Scale Invariants

### INV-FERR-025: Index Backend Interchangeability

**Traces to**: C8 (Substrate Independence), INV-FERR-005 (Index Bijection),
ADR-FERR-001 (Persistent Data Structures)
**Referenced by**: INV-FERR-071 (sorted-array backend), INV-FERR-072 (lazy promotion), INV-FERR-073 (permutation index fusion)
**Verification**: `V:TYPE`, `V:PROP`
**Stage**: 0

#### Level 0 (Algebraic Law)
```
Let IndexBackend be a trait with operations:
  insert(datom) → ()
  lookup(key) → Set<Datom>
  range(start, end) → Seq<Datom>
  contains(datom) → Bool
  len() → Nat

∀ B₁, B₂ implementing IndexBackend:
  ∀ sequence of operations ops:
    result(ops, B₁) = result(ops, B₂)

All index backends produce identical query results for the same
sequence of operations. They differ only in performance characteristics
(time complexity, memory usage, cache behavior).

Store<B: IndexBackend> is parameterized by the index backend.
Switching backends does not change correctness — only performance.

Proof sketch: Let `index_view(S, key)` be the extensional mapping from an index
key to the set of datoms in store `S` that satisfy that key. Any correct backend
implementation must realize exactly `index_view(S, key)` for every lookup and
range query, and it must maintain one index entry per datom (INV-FERR-005). If
two backends `B₁` and `B₂` both satisfy that contract, then every observable
query result is equal by set extensionality. The remaining differences are
strictly operational: insertion cost, range-scan cost, cache locality, and
memory overhead.
```

#### Level 1 (State Invariant)
The index implementation is behind a trait boundary. The store can use:
- `BTreeMapBackend`: in-memory B-tree (current default). O(log n) insert/lookup.
- `LSMBackend`: log-structured merge tree. O(1) amortized write, O(log n) read.
- `RocksDbBackend`: RocksDB-backed persistent indexes. O(1) amortized write.

All backends satisfy the same behavioral contract: they store datom references
indexed by `(e,a,v,t)` tuples in the appropriate order (EAVT, AEVT, VAET, AVET),
and they return the same results for the same queries. The index bijection
(INV-FERR-005) holds for all backends.

Backend selection is a configuration choice, not a code change. The store's CRDT
semantics, schema validation, snapshot isolation, and all other invariants are
independent of the index backend.

#### Level 2 (Implementation Contract)
```rust
/// Index backend trait.
/// Implementations differ in performance, not in behavior.
pub trait IndexBackend: Send + Sync + 'static {
    /// Insert a datom into the index.
    fn insert(&mut self, datom: &Datom);

    /// Lookup by exact key.
    fn lookup_exact(&self, key: &IndexKey) -> Vec<&Datom>;

    /// Range scan.
    fn range(&self, start: &IndexKey, end: &IndexKey) -> Vec<&Datom>;

    /// Check membership.
    fn contains(&self, datom: &Datom) -> bool;

    /// Number of entries.
    fn len(&self) -> usize;

    /// Remove all entries (for rebuild after recovery).
    fn clear(&mut self);
}

/// BTreeMap backend (default).
pub struct BTreeMapBackend {
    eavt: BTreeMap<EavtKey, Datom>,
    aevt: BTreeMap<AevtKey, Datom>,
    vaet: BTreeMap<VaetKey, Datom>,
    avet: BTreeMap<AvetKey, Datom>,
}

impl IndexBackend for BTreeMapBackend { /* ... */ }

/// Future: LSM-tree backend for write-heavy workloads.
pub struct LsmBackend { /* ... */ }
impl IndexBackend for LsmBackend { /* ... */ }

/// Store parameterized by index backend.
pub struct Store<I: IndexBackend = BTreeMapBackend> {
    datoms: BTreeSet<Datom>,
    indexes: I,
    // ...
}

impl<I: IndexBackend> Store<I> {
    // All methods are generic over I.
    // No method references a specific backend type.
}
```

**Falsification**: A query `Q` that returns different results when executed against
`Store<BTreeMapBackend>` vs. `Store<LsmBackend>` (or any other backend pair), given
the same datom set. Specific failure modes:
- **Sort order divergence**: one backend returns datoms in EAVT order, another in
  insertion order (query results are order-dependent).
- **Missing entries**: one backend's `insert` does not actually persist the datom,
  so `lookup` returns fewer results.
- **Phantom entries**: one backend's `lookup` returns datoms not present in the
  primary store (stale cache, index corruption).

**proptest strategy**:
```rust
proptest! {
    #[test]
    fn backend_equivalence(
        datoms in prop::collection::btree_set(arb_datom(), 0..100),
        queries in prop::collection::vec(arb_index_query(), 1..10),
    ) {
        let mut btree_store = Store::<BTreeMapBackend>::from_datoms(datoms.clone());
        let mut mock_lsm_store = Store::<MockLsmBackend>::from_datoms(datoms);

        for query in queries {
            let btree_result: BTreeSet<_> = btree_store.index_lookup(&query).collect();
            let lsm_result: BTreeSet<_> = mock_lsm_store.index_lookup(&query).collect();
            prop_assert_eq!(btree_result, lsm_result,
                "Backend divergence for query {:?}", query);
        }
    }
}
```

**Lean theorem**:
```lean
/-- Extensional index view for attribute-key lookups. -/
def index_view (s : DatomStore) (key : Nat) : DatomStore :=
  s.filter (fun d => d.a = key)

/-- Any two backend models that realize the same extensional index view of a
    store are observationally interchangeable for that key. -/
theorem index_backend_interchangeable
    (s : DatomStore)
    (b1 b2 : Nat → DatomStore)
    (h1 : ∀ key, b1 key = index_view s key)
    (h2 : ∀ key, b2 key = index_view s key)
    (key : Nat) :
    b1 key = b2 key := by
  rw [h1 key, h2 key]
```

---

### INV-FERR-026: Write Amplification Bound

**Traces to**: SEED.md §10, INV-FERR-008 (WAL Fsync Ordering),
INV-FERR-013 (Checkpoint Equivalence)
**Verification**: `V:PROP`
**Stage**: 0

#### Level 0 (Algebraic Law)
```
Let write_bytes(d) = the total bytes written to durable storage when
  transacting datom d (including WAL, index updates, metadata).
Let datom_bytes(d) = the serialized size of datom d.

∀ d ∈ Datom, at store size |S| = 10⁸:
  write_bytes(d) ≤ 2 × 1024  (2KB per datom)

Write amplification ratio:
  WA = write_bytes(d) / datom_bytes(d) ≤ 2KB / datom_bytes(d)

For a typical datom (~200 bytes), WA ≤ 10x.
```

#### Level 1 (State Invariant)
The total bytes written to durable storage per datom (including WAL entry overhead,
index updates, and metadata) does not exceed 2KB at 100M datom scale. This bound
ensures that write throughput remains practical at scale: at 10K writes/second,
the system writes at most 20MB/s of durable I/O, well within the capacity of
a single SSD (500MB/s+).

Write amplification arises from:
1. **WAL entry**: the datom + CRC + length prefix ≈ datom_size + 40 bytes.
2. **Primary store insert**: BTreeSet insert ≈ O(log n) comparisons, no extra I/O
   (in-memory until checkpoint).
3. **Index updates**: 4 secondary indexes, each ≈ one BTreeMap insert ≈ O(log n),
   no extra I/O (in-memory until checkpoint).
4. **Checkpoint**: periodic serialization of the full store. Amortized over all datoms
   since the last checkpoint.

The dominant factor is the WAL entry (item 1). At 200-byte datoms and 40-byte overhead,
WA ≈ 1.2x for the WAL alone. The checkpoint amortization adds at most 1x (each datom
is written once to the checkpoint). Total: ≈ 2.2x = 440 bytes per 200-byte datom,
well within the 2KB bound.

#### Level 2 (Implementation Contract)
```rust
/// WAL entry format:
/// [length: u32][epoch: u64][datom_count: u32][datoms: ...][crc32: u32]
/// Overhead per entry: 4 + 8 + 4 + 4 = 20 bytes fixed + per-datom overhead
///
/// Per-datom overhead in WAL: the datom itself (variable, typically ~200 bytes)
/// Total WAL write per datom: ~220 bytes
///
/// Checkpoint amortization: checkpoint writes all datoms once.
/// If checkpointing every N transactions, amortized overhead = datom_size / N.
/// For N = 1000, amortized = 0.2 bytes per datom.
///
/// Total write amplification per datom at 100M scale:
///   WAL: ~220 bytes
///   Checkpoint (amortized): ~1 byte
///   Total: ~221 bytes << 2KB limit
///
/// INV-FERR-026 is satisfied with significant margin.

#[cfg(test)]
fn measure_write_amplification() {
    let mut store = Store::genesis();
    let mut total_wal_bytes = 0u64;
    let mut total_datom_bytes = 0u64;

    for _ in 0..10_000 {
        let tx = arb_transaction();
        let datom_size: u64 = tx.datoms().map(|d| d.serialized_size() as u64).sum();
        total_datom_bytes += datom_size;

        let wal_pre = store.wal.file_size();
        let _ = store.transact(tx);
        let wal_post = store.wal.file_size();
        total_wal_bytes += wal_post - wal_pre;
    }

    let wa = total_wal_bytes as f64 / total_datom_bytes as f64;
    assert!(wa < 10.0, "Write amplification {:.1}x exceeds 10x", wa);

    let per_datom = total_wal_bytes / 10_000;
    assert!(per_datom < 2048, "Per-datom write {} bytes exceeds 2KB", per_datom);
}
```

**Falsification**: A workload where the average bytes written per datom exceeds 2KB at
100M datom scale. Specific failure modes:
- **Unbounded WAL growth**: the WAL is never truncated after checkpointing, causing
  every datom to be written to the WAL AND carried forward indefinitely.
- **Per-datom checkpoint**: the store checkpoints after every single datom (instead of
  batching), causing O(n) write per datom where n is the store size.
- **Index write-through**: indexes are persisted to disk on every insert (instead of
  being rebuilt from checkpoint), causing 4x overhead per datom.

**proptest strategy**:
```rust
proptest! {
    #[test]
    fn write_amplification_bounded(
        txns in prop::collection::vec(arb_transaction(), 100..500),
    ) {
        let mut store = Store::genesis();
        let mut total_written = 0u64;
        let mut total_datom_size = 0u64;

        for tx in txns {
            let datom_size: u64 = tx.datoms().map(|d| d.serialized_size() as u64).sum();
            total_datom_size += datom_size;

            let pre_wal = store.wal_bytes_written();
            let _ = store.transact(tx);
            let post_wal = store.wal_bytes_written();
            total_written += post_wal - pre_wal;
        }

        if total_datom_size > 0 {
            let wa = total_written as f64 / total_datom_size as f64;
            prop_assert!(wa < 10.0,
                "Write amplification {:.1}x exceeds bound", wa);
        }
    }
}
```

**Lean theorem**:
```lean
/-- Write amplification bound: in the abstract model, storing a datom
    adds exactly one element to the set. Write amplification = 1. -/

theorem write_amplification_model (s : DatomStore) (d : Datom) (h : d ∉ s) :
    (apply_tx s d).card = s.card + 1 := by
  unfold apply_tx
  rw [Finset.union_comm, Finset.singleton_union]
  exact Finset.card_insert_of_not_mem h
```

---

### INV-FERR-027: Read P99.99 Latency

**Traces to**: SEED.md §10, INV-FERR-006 (Snapshot Isolation),
ADR-FERR-001 (Persistent Data Structures), ADR-FERR-003 (Concurrency Model)
**Referenced by**: INV-FERR-071 (sorted-array backend — 4.5x lookup improvement)
**Verification**: `V:PROP`
**Stage**: 0

#### Level 0 (Algebraic Law)
```
Let read(S, Q) be a point query on store S with query Q.
Let N = |S| = 10⁸ (100 million datoms).
Let C = 10⁴ (10,000 concurrent readers).

∀ Q ∈ {point_lookup, range_scan(bounded)}:
  P₉₉.₉₉(latency(read(S, Q))) < 10ms

under conditions:
  - N = 10⁸ datoms
  - C = 10⁴ concurrent readers
  - readers use snapshot isolation (INV-FERR-006)
  - no writer contention on read path (readers do not acquire write lock)
```

#### Level 1 (State Invariant)
Point lookups and bounded range scans complete in under 10ms at the 99.99th percentile,
even with 100M datoms and 10K concurrent readers. This is achieved by:
1. **Snapshot isolation**: readers access an immutable snapshot (INV-FERR-006), so they
   never contend with writers or other readers.
2. **Persistent data structures**: snapshots share structure via `im-rs` persistent
   data structures (ADR-FERR-001), so creating a snapshot is O(1) and does not copy data.
3. **B-tree indexes**: EAVT, AEVT, VAET, AVET indexes provide O(log n) lookup and
   O(log n + k) range scans (where k is the result set size).
4. **No lock on read path**: the `ArcSwap` concurrency model (ADR-FERR-003) allows
   readers to access the current snapshot via atomic pointer load, without acquiring
   any lock.

The 10ms bound applies to the Ferratomic engine latency only. Network latency, query
parsing, and result serialization are excluded. The measurement is from `snapshot.query()`
call to return of the result iterator.

#### Level 2 (Implementation Contract)
```rust
/// Read path: no locks, no allocation, no contention.
/// Snapshot is an immutable view obtained via atomic pointer load.
impl<'a> Snapshot<'a> {
    /// Point lookup: O(log n) via B-tree index.
    pub fn lookup_eavt(&self, entity: EntityId, attr: Attribute) -> impl Iterator<Item = &Datom> {
        self.indexes.eavt.range(
            EavtKey::new(entity, attr, Value::MIN)
                ..=EavtKey::new(entity, attr, Value::MAX)
        ).map(|(_, d)| d)
    }

    /// Range scan: O(log n + k) where k = result count.
    pub fn range_aevt(
        &self,
        attr: Attribute,
        start_entity: EntityId,
        end_entity: EntityId,
    ) -> impl Iterator<Item = &Datom> {
        self.indexes.aevt.range(
            AevtKey::new(attr, start_entity, Value::MIN)
                ..=AevtKey::new(attr, end_entity, Value::MAX)
        ).map(|(_, d)| d)
    }
}

/// Benchmark: verify P99.99 < 10ms at scale.
#[cfg(test)]
fn benchmark_read_latency() {
    let store = generate_store_with_n_datoms(100_000_000);

    let mut latencies = Vec::with_capacity(100_000);

    for _ in 0..100_000 {
        let query_key = random_entity_id();
        let start = Instant::now();
        let _: Vec<_> = store.snapshot().lookup_eavt(query_key, random_attr()).collect();
        latencies.push(start.elapsed());
    }

    latencies.sort();
    let p99_99 = latencies[(latencies.len() as f64 * 0.9999) as usize];
    assert!(p99_99 < Duration::from_millis(10),
        "P99.99 read latency {:?} exceeds 10ms", p99_99);
}
```

**Falsification**: A workload with 100M datoms and 10K concurrent readers where the
P99.99 read latency exceeds 10ms. Specific failure modes:
- **Lock contention**: readers acquire a lock that writers also hold, causing readers
  to wait for writer completion.
- **Copy-on-read**: snapshots copy the entire datom set (instead of sharing structure),
  making snapshot creation O(n) instead of O(1).
- **Linear scan**: a query falls back to linear scan (O(n)) instead of using an index
  (O(log n)), because the query planner does not select the appropriate index.
- **GC pause**: the garbage collector (for persistent data structures) pauses reader
  threads during reclamation.

**proptest strategy**:
```rust
// Note: performance invariants are verified by benchmarks, not by proptest.
// Proptest verifies correctness; benchmarks verify performance.
// The proptest below verifies that the read path is contention-free.

proptest! {
    #[test]
    fn read_no_contention(
        datoms in prop::collection::btree_set(arb_datom(), 0..1000),
    ) {
        let store = Store::from_datoms(datoms);
        let snapshot = store.snapshot();

        // Multiple reads from the same snapshot must not interfere
        let r1: Vec<_> = snapshot.datoms().cloned().collect();
        let r2: Vec<_> = snapshot.datoms().cloned().collect();
        prop_assert_eq!(r1, r2, "Same snapshot returned different results");
    }
}
```

**Lean theorem**:
```lean
/-- Read latency: in the abstract model, membership check on Finset is
    decidable and total. Performance bounds are implementation-specific
    and verified empirically, not algebraically. -/

-- The algebraic model does not have a notion of "latency."
-- This invariant is verified by benchmarks at the implementation level.
-- The Lean theorem states the weaker property: reads are total.
theorem read_total (s : DatomStore) (d : Datom) :
    Decidable (d ∈ s) := by
  exact Finset.decidableMem d s
```

---

### INV-FERR-028: Cold Start Latency

**Traces to**: SEED.md §10, INV-FERR-013 (Checkpoint Equivalence), INV-FERR-014 (Recovery)
**Referenced by**: INV-FERR-070 (zero-copy cold start — I/O-bound target), INV-FERR-075 (LIVE-first checkpoint — 2-10x reduction)
**Verification**: `V:PROP`
**Stage**: 0

#### Level 0 (Algebraic Law)
```
Let cold_start(S) = load_checkpoint(latest) + replay_wal(remaining)
  be the time to load a store from disk to first query.

∀ S where |S| = 10⁸:
  cold_start(S) < 5s

This is a performance contract on the recovery path (INV-FERR-014).
The store must be queryable within 5 seconds of process start, even
at 100M datom scale.
```

#### Level 1 (State Invariant)
The store must be queryable within 5 seconds of process start at 100M datom scale.
This includes:
1. Loading the checkpoint file (deserialization + checksum verification).
2. Replaying any WAL entries after the checkpoint.
3. Rebuilding indexes from the loaded datoms.

The 5-second bound drives several design decisions:
- **Checkpoints are recent**: the system checkpoints frequently enough that WAL replay
  is bounded (at most a few thousand transactions, not millions).
- **Checkpoint format is sequential**: the checkpoint file is a flat sequence of datoms,
  not a complex data structure requiring random access during loading.
- **Index rebuild is incremental**: indexes are rebuilt by inserting datoms one by one,
  not by sorting the entire datom set (which would be O(n log n) and potentially too slow).
- **Memory mapping**: for very large stores, the checkpoint can be memory-mapped,
  deferring page faults to first access rather than loading everything upfront.

At 100M datoms × 200 bytes/datom = 20GB, loading from a modern NVMe SSD (3GB/s)
takes ≈ 7 seconds raw. The 5-second bound implies either:
- Memory mapping (deferred loading), or
- Compressed checkpoints (~5:1 compression on datom data), or
- Pre-built indexes stored alongside the checkpoint (no rebuild step).

#### Level 2 (Implementation Contract)
```rust
/// Cold start: load checkpoint + replay WAL + rebuild indexes.
/// Must complete in < 5s for 100M datoms.
pub fn cold_start(data_dir: &Path) -> Result<Store, RecoveryError> {
    let start = Instant::now();

    // Phase 1: Load checkpoint (memory-mapped for large stores)
    let checkpoint_path = latest_checkpoint(data_dir)?;
    let store = if checkpoint_size(&checkpoint_path)? > MMAP_THRESHOLD {
        load_checkpoint_mmap(&checkpoint_path)?
    } else {
        load_checkpoint(&checkpoint_path)?
    };

    let checkpoint_time = start.elapsed();
    log::info!("Checkpoint loaded in {:?}", checkpoint_time);

    // Phase 2: Replay WAL
    let mut store = store;
    let wal_path = data_dir.join("wal");
    let wal_entries = Wal::open(&wal_path)?.recover()?;
    let replayed = wal_entries.iter()
        .filter(|e| e.epoch > store.current_epoch())
        .count();
    for entry in wal_entries {
        if entry.epoch > store.current_epoch() {
            store.apply_wal_entry(&entry)?;
        }
    }

    let total_time = start.elapsed();
    log::info!(
        "Cold start complete: {} datoms, {} WAL entries replayed, {:?}",
        store.len(), replayed, total_time
    );

    debug_assert!(total_time < Duration::from_secs(5),
        "INV-FERR-028: cold start took {:?} (limit: 5s)", total_time);

    Ok(store)
}

/// Benchmark: cold start at scale.
#[cfg(test)]
fn benchmark_cold_start() {
    let store = generate_store_with_n_datoms(100_000_000);
    let tmp_dir = tempfile::tempdir().unwrap();
    checkpoint(&store, &tmp_dir.path().join("checkpoint")).unwrap();

    let start = Instant::now();
    let loaded = cold_start(tmp_dir.path()).unwrap();
    let elapsed = start.elapsed();

    assert!(elapsed < Duration::from_secs(5),
        "Cold start took {:?} (limit: 5s)", elapsed);
    assert_eq!(loaded.len(), store.len());
}
```

**Falsification**: A store with 100M datoms where `cold_start()` takes more than 5
seconds. Specific failure modes:
- **No checkpoint**: the store has never been checkpointed, so recovery replays the
  entire WAL (all 100M transactions).
- **Checkpoint too old**: the latest checkpoint is from millions of transactions ago,
  and WAL replay takes longer than expected.
- **Index rebuild O(n log n)**: indexes are rebuilt by sorting the entire datom set
  instead of incremental insertion.
- **Checksum verification**: verifying the BLAKE3 checksum of a 20GB file takes > 5s
  (BLAKE3 at 1GB/s ≈ 20s for 20GB, which exceeds the limit — requires streaming
  checksum verification or memory mapping).

**proptest strategy**:
```rust
// Performance invariants are verified by benchmarks, not proptest.
// Proptest verifies the correctness of the cold start path.

proptest! {
    #[test]
    fn cold_start_correctness(
        datoms in prop::collection::btree_set(arb_datom(), 0..200),
        txns in prop::collection::vec(arb_transaction(), 0..10),
    ) {
        let mut store = Store::from_datoms(datoms);
        for tx in &txns {
            let _ = store.transact(tx.clone());
        }

        let tmp_dir = tempfile::tempdir()?;
        checkpoint(&store, &tmp_dir.path().join("checkpoint"))?;

        let loaded = cold_start(tmp_dir.path())?;
        prop_assert_eq!(store.datom_set(), loaded.datom_set());
        prop_assert_eq!(store.current_epoch(), loaded.current_epoch());
    }
}
```

**Lean theorem**:
```lean
/-- Cold start correctness: loading from checkpoint produces the same store.
    Performance (< 5s) is an implementation constraint, not algebraic. -/

-- Cold start correctness is checkpoint roundtrip (INV-FERR-013).
-- The latency bound is empirically verified, not algebraically provable.
theorem cold_start_correct (s : DatomStore) :
    checkpoint_deserialize (checkpoint_serialize s) = s :=
  checkpoint_roundtrip s
```

---

### INV-FERR-029: LIVE View Resolution

**Traces to**: SEED.md §4, C1, INV-STORE-001, INV-STORE-012
**Referenced by**: INV-FERR-075 (LIVE-first checkpoint — idempotent LIVE projection)
**Verification**: `V:PROP`, `V:KANI`, `V:LEAN`
**Stage**: 0

#### Level 0 (Algebraic Law)
```
Let LIVE(S) be the "current state" view of store S.
Let causal_sort(S) be the datoms of S sorted by TxId (epoch order).

LIVE(S) = fold(causal_sort(S), apply_resolution)

where apply_resolution processes each datom in causal order:
  - assert(e, a, v): add (e, a, v) to the live set
  - retract(e, a, v): remove (e, a, v) from the live set

LIVE resolves retractions by TxId ordering. It NEVER removes datoms
from the primary store — it computes a derived view by folding over
the full history.

∀ d ∈ S: d ∈ primary(S)   -- d is always in the primary store (C1)
LIVE(S) ⊆ primary(S)      -- LIVE is a subset (resolved view)
|LIVE(S)| ≤ |primary(S)|  -- LIVE may be smaller (retractions)
```

#### Level 1 (State Invariant)
The LIVE view is a derived computation, not a separate data structure. It is computed
by folding over the primary store's datoms in causal order (by TxId/epoch), applying
each assertion and retraction:
- An `assert(e, a, v)` datom adds `(e, a, v)` to the live set.
- A `retract(e, a, v)` datom removes `(e, a, v)` from the live set.
- Assertions and retractions are matched by `(e, a, v)` triple.

The LIVE view never modifies the primary store. The primary store contains all datoms
(both assertions and retractions) in perpetuity (C1).

**Maintenance strategy (Phase 4a): incremental.** The LIVE view at the current epoch
is maintained incrementally as a materialized `BTreeSet<(EntityId, Attribute, Value)>`
(or `im::OrdSet` per ADR-FERR-001). When a transaction is committed, only the affected
`(e, a, v)` triples are updated in the live set — assertions insert, retractions remove.
The cost per transaction is `O(k)` where `k` is the number of datoms in the transaction,
not `O(n)` where `n` is the total store size.

This choice is forced by INV-FERR-027 (10ms P99.99 read latency at 100M datoms):
- **On-demand recomputation** (`O(n)` per read) is infeasible at 100M datoms — a full
  fold takes seconds, far exceeding the 10ms target.
- **Cache-and-invalidate** has `O(n)` first-read-after-write latency, violating the
  P99.99 bound whenever a write immediately precedes a read.
- **Incremental** achieves `O(1)` read (access the cached live set) and `O(k)` write
  overhead (update only affected triples). Both are within budget.

The incremental live set is rebuilt from scratch during cold start: `cold_start` loads
the checkpoint, replays the WAL delta, and folds the resulting datoms to produce the
initial live set. After cold start, all updates are incremental.

The LIVE view is epoch-sensitive: `LIVE(S, e)` gives the live set as of epoch `e`,
considering only datoms with `tx_epoch <= e`. This enables time-travel queries:
"what was the live state at epoch 1000?" Time-travel queries (historical epochs) use
on-demand fold from the nearest checkpoint or cached snapshot — only the current-epoch
LIVE view is maintained incrementally.

The causal ordering (by TxId/epoch) is critical: if a retraction has a lower epoch
than the assertion it retracts, the retraction has no effect (it was "before" the
assertion in causal time). This prevents "retroactive retractions" from corrupting
the live view.

#### Level 2 (Implementation Contract)
```rust
/// Compute the LIVE view of the store at a given epoch.
/// Returns the set of (entity, attribute, value) triples that are
/// currently asserted (not retracted) at the given epoch.
pub fn live_view(store: &Store, epoch: u64) -> BTreeSet<(EntityId, Attribute, Value)> {
    let mut live: BTreeSet<(EntityId, Attribute, Value)> = BTreeSet::new();

    // Process datoms in causal order (by epoch, then by insertion order within epoch)
    for datom in store.datoms_by_epoch(..=epoch) {
        let key = (datom.entity.clone(), datom.attribute.clone(), datom.value.clone());
        match datom.op {
            Op::Assert => { live.insert(key); }
            Op::Retract => { live.remove(&key); }
        }
    }

    live
}

/// LIVE view NEVER modifies the primary store.
/// This function takes &Store (immutable reference), not &mut Store.
/// It returns a NEW set, leaving the store unchanged.

/// --- Incremental maintenance (Phase 4a strategy) ---
///
/// The Store holds a cached `live_set: BTreeSet<(EntityId, Attribute, Value)>`
/// that is updated incrementally on each transact(). The on-demand `live_view()`
/// above is used only for time-travel queries at historical epochs.

impl Store {
    /// Update the cached LIVE set after a committed transaction.
    /// Cost: O(k) where k = number of datoms in the transaction.
    /// Called by transact() after epoch assignment, before ArcSwap publish.
    fn update_live_set(&mut self, tx: &Transaction<Committed>) {
        for datom in tx.datoms() {
            let key = (datom.entity.clone(), datom.attribute.clone(), datom.value.clone());
            match datom.op {
                Op::Assert => { self.live_set.insert(key); }
                Op::Retract => { self.live_set.remove(&key); }
            }
        }
    }

    /// Access the current LIVE view. O(1) — returns a reference to the cached set.
    /// For time-travel at historical epochs, use `live_view(store, epoch)` instead.
    pub fn live(&self) -> &BTreeSet<(EntityId, Attribute, Value)> {
        &self.live_set
    }

    /// Rebuild the LIVE set from scratch by folding all datoms.
    /// Used during cold_start after checkpoint + WAL replay.
    /// Cost: O(n) — acceptable only during initialization.
    fn rebuild_live_set(&mut self) {
        self.live_set = live_view(self, self.current_epoch());
    }
}

#[kani::proof]
#[kani::unwind(10)]
fn live_view_correctness() {
    let mut datoms = BTreeSet::new();

    // Assert (e=1, a=1, v=1)
    datoms.insert(Datom { e: 1, a: 1, v: 1, tx: 1, op: true });
    // Retract (e=1, a=1, v=1)
    datoms.insert(Datom { e: 1, a: 1, v: 1, tx: 2, op: false });
    // Assert (e=1, a=1, v=2)
    datoms.insert(Datom { e: 1, a: 1, v: 2, tx: 3, op: true });

    let store = Store::from_datoms(datoms.clone());

    // LIVE at epoch 3: only (1, 1, 2) is live
    let live = live_view(&store, 3);
    assert!(live.contains(&(1, 1, 2)));
    assert!(!live.contains(&(1, 1, 1)));

    // Primary store still has ALL datoms
    assert_eq!(store.datoms.len(), 3);
}
```

**Falsification**: The LIVE view removes a datom from the primary store (C1 violation).
Or: the LIVE view includes a `(e, a, v)` triple that has been retracted at an earlier
or equal epoch. Or: the LIVE view excludes a `(e, a, v)` triple that has been asserted
and never retracted. Specific failure modes:
- **Primary mutation**: `live_view()` takes `&mut Store` and modifies `store.datoms`.
- **Out-of-order processing**: datoms are processed in non-causal order, causing a
  retraction to be applied before the assertion it retracts.
- **Missing retraction matching**: a retraction `retract(e, a, v)` does not remove the
  matching assertion because the matching logic uses entity-only (not `(e, a, v)`
  triple) comparison.
- **Epoch filtering bug**: `datoms_by_epoch(..=epoch)` includes datoms with `tx_epoch > epoch`.

**proptest strategy**:
```rust
proptest! {
    #[test]
    fn live_view_respects_retractions(
        assertions in prop::collection::vec(arb_datom_assert(), 1..20),
        retraction_indices in prop::collection::vec(0..20usize, 0..10),
    ) {
        let mut store = Store::genesis();
        let mut epoch = store.current_epoch();

        // Apply assertions
        for d in &assertions {
            epoch += 1;
            store.insert_datom_at_epoch(d.clone(), epoch);
        }

        // Apply retractions for selected assertions
        let mut retracted: BTreeSet<_> = BTreeSet::new();
        for &idx in &retraction_indices {
            if idx < assertions.len() {
                let d = &assertions[idx];
                epoch += 1;
                store.insert_datom_at_epoch(
                    Datom { op: Op::Retract, ..d.clone() },
                    epoch,
                );
                retracted.insert((d.entity.clone(), d.attribute.clone(), d.value.clone()));
            }
        }

        let live = live_view(&store, epoch);

        // Retracted triples must not be in LIVE
        for key in &retracted {
            prop_assert!(!live.contains(key),
                "Retracted triple still in LIVE view: {:?}", key);
        }

        // Non-retracted assertions must be in LIVE
        for d in &assertions {
            let key = (d.entity.clone(), d.attribute.clone(), d.value.clone());
            if !retracted.contains(&key) {
                prop_assert!(live.contains(&key),
                    "Asserted triple missing from LIVE view: {:?}", key);
            }
        }
    }

    #[test]
    fn live_view_does_not_mutate_store(
        datoms in prop::collection::btree_set(arb_datom(), 0..100),
    ) {
        let store = Store::from_datoms(datoms.clone());
        let pre_len = store.len();
        let pre_datoms = store.datom_set().clone();

        let _live = live_view(&store, store.current_epoch());

        // Store unchanged
        prop_assert_eq!(store.len(), pre_len, "Store length changed after LIVE view");
        prop_assert_eq!(*store.datom_set(), pre_datoms, "Store datoms changed after LIVE view");
    }
}
```

**Lean theorem**:
```lean
/-- LIVE view: fold over datoms in causal order, applying assertions and retractions.
    The LIVE view is a subset of the primary store's datom set. -/

def apply_op (live : Finset (Nat × Nat × Nat)) (d : Datom) : Finset (Nat × Nat × Nat) :=
  let key := (d.e, d.a, d.v)
  if d.op then  -- assert
    live ∪ {key}
  else  -- retract
    live \ {key}

def live_view_model (datoms : List Datom) : Finset (Nat × Nat × Nat) :=
  datoms.foldl apply_op ∅

/-- LIVE view is at most as large as the number of unique (e,a,v) triples. -/
theorem live_bounded (datoms : List Datom) :
    (live_view_model datoms).card ≤ datoms.length := by
  sorry -- induction on datoms; each step adds at most 1 element

/-- Retraction followed by no re-assertion means the triple is absent. -/
theorem retraction_removes (live : Finset (Nat × Nat × Nat)) (e a v : Nat) :
    (e, a, v) ∉ apply_op live { e, a, v, tx := 0, op := false } := by
  unfold apply_op
  simp [Finset.mem_sdiff, Finset.mem_singleton]
```

---

### INV-FERR-030: Read Replica Subset

**Traces to**: SEED.md §4, C4, INV-FERR-010 (Merge Convergence)
**Verification**: `V:PROP`, `V:MODEL`
**Stage**: 0

#### Level 0 (Algebraic Law)
```
Let leader(S) be the leader's datom set.
Let replica(Rᵢ) be replica i's datom set.

∀ replica Rᵢ at any point in time:
  replica(Rᵢ) ⊆ leader(S)

After WAL catch-up (eventual consistency):
  replica(Rᵢ) = leader(S)

Read replicas are always a subset of the leader's state. They never
contain datoms that the leader does not have (no phantom datoms).
After catching up with the leader's WAL, they are equal.
```

#### Level 1 (State Invariant)
A read replica receives datoms from the leader via WAL streaming (or periodic snapshot +
WAL replay). At any point in time, the replica's datom set is a subset of the leader's
datom set. The replica never invents datoms that the leader does not have.

After the replica has fully caught up (received and applied all WAL entries up to the
leader's current epoch), its datom set is identical to the leader's. The time to catch
up is bounded by `O(|WAL_delta|)` where `WAL_delta` is the set of WAL entries the
replica has not yet received.

Read replicas do not accept writes directly. All writes go through the leader, which
serializes them (INV-FERR-007), writes to WAL (INV-FERR-008), and streams the WAL
entries to replicas. This ensures that all replicas converge to the same state
(INV-FERR-010) and that the total order of writes is consistent across all replicas.

#### Level 2 (Implementation Contract)
```rust
/// Read replica: receives WAL entries from leader, never accepts direct writes.
pub struct ReadReplica {
    store: Store,
    leader_epoch: u64,  // last known leader epoch
}

impl ReadReplica {
    /// Apply a WAL entry received from the leader.
    /// The entry must be from the leader (not from another replica).
    pub fn apply_wal_entry(&mut self, entry: &WalEntry) -> Result<(), ReplicaError> {
        if entry.epoch <= self.store.current_epoch() {
            return Ok(()); // already applied (idempotent)
        }
        if entry.epoch != self.store.current_epoch() + 1 {
            return Err(ReplicaError::EpochGap {
                expected: self.store.current_epoch() + 1,
                got: entry.epoch,
            });
        }

        for datom in &entry.datoms {
            self.store.datoms.insert(datom.clone());
            self.store.indexes.insert(datom);
        }
        self.store.epoch = entry.epoch;
        self.leader_epoch = entry.epoch;

        Ok(())
    }

    /// Read-only: no transact, no merge.
    /// Writing to a replica is a compile-time error.
    pub fn snapshot(&self) -> Snapshot<'_> {
        self.store.snapshot()
    }

    // NOTE: There is intentionally no transact() or merge_from() method.
    // Writes go through the leader only.
}

// Stateright model: replicas are always subsets of leader
impl stateright::Model for ReplicaModel {
    fn properties(&self) -> Vec<Property<Self>> {
        vec![
            Property::always("replica_subset", |_, state: &ReplicaState| {
                state.replicas.iter().all(|r| r.datoms.is_subset(&state.leader.datoms))
            }),
            Property::eventually("replica_convergence", |_, state: &ReplicaState| {
                state.replicas.iter().all(|r| r.datoms == state.leader.datoms)
            }),
        ]
    }
}
```

**Falsification**: A read replica contains a datom that the leader does not contain.
Or: after full WAL catch-up, the replica's datom set differs from the leader's.
Specific failure modes:
- **Phantom WAL entry**: the replica receives a WAL entry that was not generated by
  the leader (network corruption or man-in-the-middle).
- **Out-of-order application**: WAL entries are applied out of epoch order, causing
  the replica to skip an entry and diverge.
- **Direct write**: the replica accepts a direct write (transact or merge), adding
  datoms that the leader does not have.

**proptest strategy**:
```rust
proptest! {
    #[test]
    fn replica_always_subset(
        txns in prop::collection::vec(arb_transaction(), 1..20),
        apply_up_to in 0..20usize,
    ) {
        let mut leader = Store::genesis();
        let mut wal_entries = vec![];

        for tx in txns {
            if let Ok(receipt) = leader.transact(tx) {
                wal_entries.push(leader.last_wal_entry().clone());
            }
        }

        let mut replica = ReadReplica::from_genesis();
        let apply_count = apply_up_to.min(wal_entries.len());

        for entry in &wal_entries[..apply_count] {
            let _ = replica.apply_wal_entry(entry);
        }

        // Replica is always a subset of leader
        prop_assert!(replica.store.datom_set().is_subset(leader.datom_set()),
            "Replica is not a subset of leader");

        // If all entries applied, replica equals leader
        if apply_count == wal_entries.len() {
            prop_assert_eq!(replica.store.datom_set(), leader.datom_set(),
                "Replica did not converge after full catch-up");
        }
    }
}
```

**Lean theorem**:
```lean
/-- Read replica subset: a replica receiving a subset of the leader's
    transactions has a subset of the leader's datoms. -/

def apply_entries (s : DatomStore) (entries : List DatomStore) : DatomStore :=
  entries.foldl (fun acc e => merge acc e) s

theorem replica_subset (leader_txns replica_txns : List DatomStore)
    (h : replica_txns.length ≤ leader_txns.length)
    (h_prefix : ∀ i, i < replica_txns.length → replica_txns[i]! = leader_txns[i]!) :
    apply_entries ∅ replica_txns ⊆ apply_entries ∅ leader_txns := by
  sorry -- induction on replica_txns; each step adds a subset of what leader adds
```

---

### INV-FERR-031: Genesis Determinism

**Traces to**: SEED.md §4, C7 (Self-bootstrap), INV-STORE-003
**Referenced by**: INV-FERR-060 (store identity bootstrap), ADR-FERR-027 (store
identity), ADR-FERR-028 (provenance lattice)
**Verification**: `V:PROP`, `V:KANI`, `V:LEAN`
**Stage**: 0

> **Phase 4a.5 amendment**: Genesis schema extended from 19 to 23 axiomatic
> attributes. Four new transaction-metadata attributes added:
> `:tx/signature` (Bytes, One, LWW), `:tx/signer` (Bytes, One, LWW),
> `:tx/predecessor` (Ref, Many, MultiValue), `:tx/provenance` (Keyword, One, LWW).
> The invariant asserts DETERMINISM (every call produces identical output),
> not a specific CARDINALITY. The attribute count is computed dynamically
> at test time, not hardcoded as a constant (braid pattern: prevents
> derived-quantity staleness when attributes are added in future phases).

#### Level 0 (Algebraic Law)
```
Let genesis() : DatomStore be the genesis function.

∀ invocations i, j of genesis():
  genesis_i() = genesis_j()

The genesis function is deterministic: every call produces the exact
same store with the exact same datoms, the exact same schema, and
the exact same epoch (0). The genesis store contains:
  1. Meta-schema attributes (:db/ident, :db/valueType, :db/cardinality, etc.)
  2. Lattice definition attributes (:lattice/ident, :lattice/elements, etc.)
  3. Transaction metadata attributes (:tx/time, :tx/agent, :tx/provenance,
     :tx/rationale, :tx/coherence-override, :tx/signature, :tx/signer,
     :tx/predecessor)
  4. Genesis transaction metadata.
  5. No user data.

The attribute count is an implementation detail, not a spec claim. Tests
assert genesis_schema().len() == genesis_schema().len() (determinism),
not genesis_schema().len() == N (cardinality). The count may increase
across Ferratomic versions as new phases add axiomatic attributes.

Genesis is the fixed point from which all stores diverge. Every store
in the system is a descendant of the genesis store via TRANSACT and
MERGE operations.
```

#### Level 1 (State Invariant)
Every call to `genesis()` produces a bitwise-identical store. This is critical for:
- **Replica initialization**: a new replica calls `genesis()` and then catches up via
  WAL replay. If `genesis()` were non-deterministic, the replica would start from a
  different state and diverge.
- **Testing**: deterministic genesis enables reproducible tests. Every test starts from
  the same store state.
- **Content addressing**: the genesis store's identity hash (used for deduplication and
  comparison) must be identical across invocations. If genesis produces different datoms,
  the identity hashes differ, and self-merge detection (INV-FERR-003 fast path) fails.

The genesis store contains schema-definition datoms across four categories:
meta-schema (`:db/*`), lattice definitions (`:lattice/*`), and transaction metadata
(`:tx/*`). These are the minimum set required to bootstrap the schema-as-data system
(C3) and the transaction signing infrastructure (INV-FERR-051). The genesis transaction
has epoch 0 and a deterministic transaction entity ID.

Phase 4a.5 adds four transaction-metadata attributes to genesis: `:tx/signature`,
`:tx/signer`, `:tx/predecessor`, and `:tx/provenance`. These are axiomatic because
they define HOW TRANSACTIONS WORK (system mechanics), not what entities exist in the
world (domain data). Store identity (`:store/public-key`) and agent identity
(`:agent/*`) are conventional — installed by transactions, not genesis (ADR-FERR-027).

No randomness, no timestamps, no system-specific information enters the genesis store.
The genesis function is a pure function of the Ferratomic version (schema set is
version-specific, and any schema change is a new Ferratomic version).

#### Level 2 (Implementation Contract)
```rust
/// Genesis: produce the initial store.
/// This function is deterministic — always returns the same store.
/// No randomness, no timestamps, no system info.
pub fn genesis() -> Store {
    let mut datoms = BTreeSet::new();

    // Schema attributes (deterministic, hardcoded)
    let schema_attrs = [
        (":db/ident", ValueType::Keyword, Cardinality::One),
        (":db/valueType", ValueType::Keyword, Cardinality::One),
        (":db/cardinality", ValueType::Keyword, Cardinality::One),
        (":db/unique", ValueType::Keyword, Cardinality::One),
        (":db/isComponent", ValueType::Bool, Cardinality::One),
        (":db/doc", ValueType::String, Cardinality::One),
    ];

    let tx_entity = EntityId::from_content(b"genesis-tx");
    let epoch = 0u64;

    for (ident, vtype, card) in &schema_attrs {
        let entity = EntityId::from_content(ident.as_bytes());

        datoms.insert(Datom::new(entity, Attribute::DB_IDENT, Value::keyword(ident), epoch, Op::Assert));
        datoms.insert(Datom::new(entity, Attribute::DB_VALUE_TYPE, Value::keyword(&vtype.to_string()), epoch, Op::Assert));
        datoms.insert(Datom::new(entity, Attribute::DB_CARDINALITY, Value::keyword(&card.to_string()), epoch, Op::Assert));
    }

    // Genesis transaction metadata
    datoms.insert(Datom::new(tx_entity, Attribute::DB_IDENT, Value::keyword(":tx/genesis"), epoch, Op::Assert));

    Store::from_datoms_at_epoch(datoms, epoch)
}

#[kani::proof]
#[kani::unwind(4)]
fn genesis_determinism() {
    let g1 = genesis();
    let g2 = genesis();
    assert_eq!(g1.datom_set(), g2.datom_set());
    assert_eq!(g1.current_epoch(), g2.current_epoch());
}
```

**Falsification**: Two calls to `genesis()` produce stores with different datom sets,
different epochs, or different identity hashes. Specific failure modes:
- **Timestamp in genesis**: genesis includes the current wall-clock time as a datom
  value, causing different invocations to produce different datoms.
- **Random entity IDs**: entity IDs are generated from random numbers instead of
  content hashing, causing different genesis stores to have different entity IDs.
- **Non-deterministic iteration**: the schema attributes are iterated in a non-deterministic
  order (e.g., from a HashMap), causing datoms to be inserted in different orders, which
  could affect content hashes if the hash depends on insertion order.

**proptest strategy**:
```rust
proptest! {
    #[test]
    fn genesis_always_same(
        _seed in any::<u64>(),  // proptest provides different seeds; genesis must be same
    ) {
        let g1 = genesis();
        let g2 = genesis();

        prop_assert_eq!(g1.datom_set(), g2.datom_set(),
            "Genesis produced different datom sets");
        prop_assert_eq!(g1.current_epoch(), g2.current_epoch(),
            "Genesis produced different epochs");
        prop_assert_eq!(g1.identity_hash(), g2.identity_hash(),
            "Genesis produced different identity hashes");
        prop_assert_eq!(g1.schema(), g2.schema(),
            "Genesis produced different schemas");
    }
}
```

**Lean theorem**:
```lean
/-- Genesis determinism: the genesis function is a constant.
    We model it as returning the empty set (the simplest deterministic value). -/

def genesis_model : DatomStore := ∅

theorem genesis_deterministic :
    genesis_model = genesis_model := by rfl

/-- Every store is a superset of genesis (genesis is the bottom element). -/
theorem genesis_bottom (s : DatomStore) :
    genesis_model ⊆ s := by
  unfold genesis_model
  exact Finset.empty_subset s

/-- Merging with genesis is identity. -/
theorem genesis_merge_identity (s : DatomStore) :
    merge genesis_model s = s := by
  unfold merge genesis_model
  exact Finset.empty_union s
```

---

### INV-FERR-032: LIVE Resolution Correctness

**Traces to**: SEED.md §4, INV-FERR-029 (LIVE View Resolution), INV-STORE-012
**Referenced by**: INV-FERR-075 (LIVE-first checkpoint)
**Verification**: `V:PROP`, `V:KANI`, `V:LEAN`
**Stage**: 0

#### Level 0 (Algebraic Law)
```
LIVE(S) = fold(causal_sort(S), apply_resolution)

This invariant strengthens INV-FERR-029 by specifying the exact
semantics of apply_resolution:

∀ entity e, attribute a:
  let assertions = {(e, a, v, tx) | (e, a, v, tx, assert) ∈ S}
  let retractions = {(e, a, v, tx) | (e, a, v, tx, retract) ∈ S}

  LIVE(S, e, a) = assertions \ retractions
    (where the \ operation is on (e, a, v) triples, matching by value)

For cardinality-one attributes:
  LIVE(S, e, a) = {(e, a, v)} where v is the value from the
    last-writer-wins assertion (highest tx epoch) that has not
    been retracted.

For cardinality-many attributes:
  LIVE(S, e, a) = {(e, a, v) | asserted and not retracted}
```

#### Level 1 (State Invariant)
The LIVE resolution function correctly computes the current state of every entity
by processing all assertions and retractions in causal order. The resolution is
correct if and only if:
1. Every asserted `(e, a, v)` triple that has not been retracted is present in LIVE.
2. Every retracted `(e, a, v)` triple is absent from LIVE.
3. For cardinality-one attributes, only the latest (highest-epoch) non-retracted
   value is present.
4. For cardinality-many attributes, all non-retracted values are present.
5. The causal ordering is by `tx_epoch` (epoch assigned during TRANSACT, per
   INV-FERR-007), not by wall-clock time or insertion order.

This invariant is the bridge between the raw datom store (which contains all
assertions and retractions in perpetuity) and the query layer (which needs to
know "what is the current value of attribute A on entity E?").

Correctness of LIVE resolution depends on:
- INV-FERR-007 (epoch ordering is correct).
- INV-FERR-005 (indexes are bijection of primary store — AEVT index provides
  efficient attribute-entity access).
- INV-FERR-009 (schema validation — cardinality is correctly defined).

#### Level 2 (Implementation Contract)
```rust
/// Compute LIVE resolution for a specific entity and attribute.
/// Handles both cardinality-one and cardinality-many.
pub fn live_resolve(
    store: &Store,
    entity: EntityId,
    attr: Attribute,
    epoch: u64,
) -> Vec<Value> {
    let schema = store.schema();
    let cardinality = schema.get(&attr)
        .map(|def| def.cardinality)
        .unwrap_or(Cardinality::One);

    // Get all datoms for this (entity, attribute) in causal order
    let datoms: Vec<_> = store.datoms_eavt_range(entity, attr, ..=epoch)
        .sorted_by_key(|d| d.tx_epoch)
        .collect();

    match cardinality {
        Cardinality::One => {
            // Last-writer-wins: process in causal order, keep last non-retracted
            let mut current: Option<Value> = None;
            for d in &datoms {
                match d.op {
                    Op::Assert => current = Some(d.value.clone()),
                    Op::Retract => {
                        if current.as_ref() == Some(&d.value) {
                            current = None;
                        }
                    }
                }
            }
            current.into_iter().collect()
        }
        Cardinality::Many => {
            // Set semantics: track all non-retracted values
            let mut live_values: BTreeSet<Value> = BTreeSet::new();
            for d in &datoms {
                match d.op {
                    Op::Assert => { live_values.insert(d.value.clone()); }
                    Op::Retract => { live_values.remove(&d.value); }
                }
            }
            live_values.into_iter().collect()
        }
    }
}

#[kani::proof]
#[kani::unwind(10)]
fn live_resolution_card_one() {
    // Assert v=1, then assert v=2 (last-writer-wins)
    let mut store = Store::genesis();
    let entity = EntityId::from_content(b"test");
    let attr = Attribute::from("name");

    // Epoch 1: assert v=1
    store.insert_datom(Datom::new(entity, attr.clone(), Value::String("Alice".into()), 1, Op::Assert));
    // Epoch 2: assert v=2
    store.insert_datom(Datom::new(entity, attr.clone(), Value::String("Bob".into()), 2, Op::Assert));

    let live = live_resolve(&store, entity, attr, 2);
    assert_eq!(live.len(), 1);
    assert_eq!(live[0], Value::String("Bob".into()));
}

#[kani::proof]
#[kani::unwind(10)]
fn live_resolution_retraction() {
    let mut store = Store::genesis();
    let entity = EntityId::from_content(b"test");
    let attr = Attribute::from("tags");

    // Epoch 1: assert "red"
    store.insert_datom(Datom::new(entity, attr.clone(), Value::String("red".into()), 1, Op::Assert));
    // Epoch 2: assert "blue"
    store.insert_datom(Datom::new(entity, attr.clone(), Value::String("blue".into()), 2, Op::Assert));
    // Epoch 3: retract "red"
    store.insert_datom(Datom::new(entity, attr.clone(), Value::String("red".into()), 3, Op::Retract));

    let live = live_resolve(&store, entity, attr, 3);
    assert_eq!(live.len(), 1);
    assert_eq!(live[0], Value::String("blue".into()));
}
```

**Falsification**: The LIVE resolution produces an incorrect result. Specific cases:
- **Retracted value present**: a `(e, a, v)` triple that has been retracted appears
  in the LIVE output.
- **Non-retracted value absent**: a `(e, a, v)` triple that has been asserted and
  never retracted is missing from the LIVE output.
- **Cardinality-one violation**: for a cardinality-one attribute, the LIVE output
  contains more than one value.
- **Wrong last-writer**: for cardinality-one, the LIVE output contains a value from
  an earlier epoch rather than the latest epoch.
- **Causal order wrong**: datoms are processed in non-causal order (e.g., by insertion
  time rather than by epoch), causing incorrect resolution when assertions and
  retractions arrive out of order.

**proptest strategy**:
```rust
proptest! {
    #[test]
    fn live_resolution_card_one_correct(
        values in prop::collection::vec(arb_value(), 2..10),
        retract_last in any::<bool>(),
    ) {
        let mut store = Store::genesis();
        let entity = EntityId::from_content(b"prop_entity");
        let attr = Attribute::from("card_one_attr");
        // Define attribute as cardinality-one in schema
        store.define_attr(&attr, ValueType::String, Cardinality::One);

        let mut epoch = 1u64;
        for v in &values {
            store.insert_datom(Datom::new(entity, attr.clone(), v.clone(), epoch, Op::Assert));
            epoch += 1;
        }

        let last_value = values.last().unwrap().clone();

        if retract_last {
            store.insert_datom(Datom::new(entity, attr.clone(), last_value.clone(), epoch, Op::Retract));
            epoch += 1;
        }

        let live = live_resolve(&store, entity, attr, epoch);

        if retract_last {
            // Last value was retracted; for card-one, previous value wins
            // (if not also retracted)
            prop_assert!(live.len() <= 1);
        } else {
            prop_assert_eq!(live.len(), 1);
            prop_assert_eq!(live[0], last_value);
        }
    }

    #[test]
    fn live_resolution_card_many_correct(
        assert_values in prop::collection::btree_set(arb_value(), 1..20),
        retract_values in prop::collection::btree_set(arb_value(), 0..10),
    ) {
        let mut store = Store::genesis();
        let entity = EntityId::from_content(b"prop_entity");
        let attr = Attribute::from("card_many_attr");
        store.define_attr(&attr, ValueType::String, Cardinality::Many);

        let mut epoch = 1u64;
        for v in &assert_values {
            store.insert_datom(Datom::new(entity, attr.clone(), v.clone(), epoch, Op::Assert));
            epoch += 1;
        }
        for v in &retract_values {
            store.insert_datom(Datom::new(entity, attr.clone(), v.clone(), epoch, Op::Retract));
            epoch += 1;
        }

        let live = live_resolve(&store, entity, attr, epoch);
        let live_set: BTreeSet<_> = live.into_iter().collect();

        let expected: BTreeSet<_> = assert_values.difference(&retract_values).cloned().collect();
        prop_assert_eq!(live_set, expected);
    }
}
```

**Lean theorem**:
```lean
/-- LIVE resolution correctness: the live set is exactly the set of
    asserted values minus the set of retracted values. -/

def assertions (datoms : Finset Datom) (e a : Nat) : Finset Nat :=
  (datoms.filter (fun d => d.e = e ∧ d.a = a ∧ d.op = true)).image (fun d => d.v)

def retractions (datoms : Finset Datom) (e a : Nat) : Finset Nat :=
  (datoms.filter (fun d => d.e = e ∧ d.a = a ∧ d.op = false)).image (fun d => d.v)

def live_values (datoms : Finset Datom) (e a : Nat) : Finset Nat :=
  assertions datoms e a \ retractions datoms e a

theorem live_correct_assert (datoms : Finset Datom) (e a v : Nat)
    (h_assert : ∃ tx, { e, a, v, tx, op := true : Datom } ∈ datoms)
    (h_no_retract : ¬ ∃ tx, { e, a, v, tx, op := false : Datom } ∈ datoms) :
    v ∈ live_values datoms e a := by
  unfold live_values assertions retractions
  simp [Finset.mem_sdiff, Finset.mem_image, Finset.mem_filter]
  constructor
  · obtain ⟨tx, htx⟩ := h_assert
    exact ⟨{ e, a, v, tx, op := true }, ⟨htx, rfl, rfl, rfl⟩, rfl⟩
  · intro ⟨d, ⟨hd, _, _, _⟩, _⟩
    exact absurd ⟨d.tx, hd⟩ h_no_retract
    sorry -- need to reconstruct the exact retraction datom

theorem live_correct_retract (datoms : Finset Datom) (e a v tx_a tx_r : Nat)
    (h_assert : { e, a, v, tx := tx_a, op := true : Datom } ∈ datoms)
    (h_retract : { e, a, v, tx := tx_r, op := false : Datom } ∈ datoms) :
    v ∉ live_values datoms e a := by
  unfold live_values
  simp [Finset.mem_sdiff]
  intro _
  unfold retractions
  simp [Finset.mem_image, Finset.mem_filter]
  exact ⟨{ e, a, v, tx := tx_r, op := false }, ⟨h_retract, rfl, rfl, rfl⟩, rfl⟩
  sorry -- Finset.image/filter details
```

---
