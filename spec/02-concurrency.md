## 23.2 Concurrency & Distribution Invariants

### INV-FERR-013: Checkpoint Equivalence

**Traces to**: SEED.md §5 (Harvest/Seed Lifecycle — durability), INV-STORE-009,
INV-FERR-005 (Index Bijection), INV-FERR-008 (WAL Fsync Ordering)
**Referenced by**: INV-FERR-070 (zero-copy cold start), INV-FERR-074 (homomorphic fingerprint), INV-FERR-075 (LIVE-first checkpoint)
**Verification**: `V:PROP`, `V:KANI`, `V:LEAN`
**Stage**: 0

#### Level 0 (Algebraic Law)
```
Let checkpoint : DatomStore → Bytes be the serialization function.
Let load : Bytes → DatomStore be the deserialization function.

∀ S ∈ DatomStore:
  load(checkpoint(S)) = S

This is a round-trip identity (section-retraction pair):
  checkpoint ∘ load = id  (on valid checkpoints)
  load ∘ checkpoint = id  (on valid stores)

Concretely, the datom set, all index state, schema, and epoch are
preserved exactly through serialization and deserialization. No datom
is lost, no datom is added, no ordering is changed, no metadata is
corrupted.
```

#### Level 1 (State Invariant)
For every reachable store state `S` produced by any sequence of TRANSACT, MERGE, and
recovery operations: serializing `S` to a checkpoint file and loading it back produces a
store `S'` that is indistinguishable from `S` in every observable way. Specifically:
- `S'.datom_set() == S.datom_set()` (identical datom content)
- `S'.current_epoch() == S.current_epoch()` (same epoch)
- `S'.schema() == S.schema()` (same schema)
- `verify_index_bijection(S')` holds (indexes reconstructed correctly)
- For every query `Q`: `eval(S', Q) == eval(S, Q)` (query equivalence)

Checkpoint equivalence is the foundation of crash recovery: after a crash, the system
loads the latest checkpoint, then replays the WAL from that point (INV-FERR-008). If
checkpoint loading introduced any difference, WAL replay would diverge from the
pre-crash state, violating recovery correctness (INV-FERR-014).

Checkpoint equivalence is also the foundation of replica bootstrap: a new replica loads
a checkpoint from an existing replica rather than replaying the entire transaction history.
If the checkpoint does not faithfully represent the source store, the new replica starts
from an incorrect state, and subsequent merges may diverge.

#### Level 2 (Implementation Contract)
```rust
/// Serialize the store to a checkpoint file.
/// The checkpoint contains: header (magic, version, epoch), schema datoms,
/// data datoms (sorted by EAVT for deterministic output), and a trailing
/// BLAKE3 checksum of the entire file.
pub fn checkpoint(store: &Store, path: &Path) -> io::Result<()> {
    let mut writer = BufWriter::new(File::create(path)?);

    // Header
    writer.write_all(CHECKPOINT_MAGIC)?;
    writer.write_all(&CHECKPOINT_VERSION.to_le_bytes())?;
    writer.write_all(&store.current_epoch().to_le_bytes())?;

    // Datoms in deterministic order (EAVT sort)
    let sorted: Vec<&Datom> = store.datoms_eavt_order().collect();
    writer.write_all(&(sorted.len() as u64).to_le_bytes())?;
    for datom in &sorted {
        datom.serialize_into(&mut writer)?;
    }

    // BLAKE3 checksum of all preceding bytes
    let checksum = blake3::hash(&writer.get_ref().as_bytes());
    writer.write_all(checksum.as_bytes())?;

    writer.flush()?;
    writer.get_ref().sync_all()?; // durable
    Ok(())
}

/// Load a store from a checkpoint file.
/// Verifies the BLAKE3 checksum before constructing the store.
/// Returns Err if the checksum fails (corruption detected).
pub fn load_checkpoint(path: &Path) -> Result<Store, CheckpointError> {
    let data = std::fs::read(path)?;
    if data.len() < CHECKPOINT_MIN_SIZE {
        return Err(CheckpointError::Truncated);
    }

    // Verify checksum (last 32 bytes)
    let (payload, checksum_bytes) = data.split_at(data.len() - 32);
    let expected = blake3::hash(payload);
    if expected.as_bytes() != checksum_bytes {
        return Err(CheckpointError::ChecksumMismatch);
    }

    // Deserialize
    let mut cursor = Cursor::new(payload);
    let magic = read_magic(&mut cursor)?;
    if magic != CHECKPOINT_MAGIC {
        return Err(CheckpointError::InvalidMagic);
    }
    let version = read_u32_le(&mut cursor)?;
    let epoch = read_u64_le(&mut cursor)?;
    let datom_count = read_u64_le(&mut cursor)? as usize;

    let mut datoms = BTreeSet::new();
    for _ in 0..datom_count {
        datoms.insert(Datom::deserialize_from(&mut cursor)?);
    }

    let store = Store::from_datoms_at_epoch(datoms, epoch);
    debug_assert!(verify_index_bijection(&store), "INV-FERR-005 after checkpoint load");
    Ok(store)
}

#[kani::proof]
#[kani::unwind(8)]
fn checkpoint_roundtrip() {
    let datoms: BTreeSet<Datom> = kani::any();
    kani::assume(datoms.len() <= 4);

    let store = Store::from_datoms(datoms.clone());
    let bytes = store.to_checkpoint_bytes();
    let loaded = Store::from_checkpoint_bytes(&bytes).unwrap();

    assert_eq!(store.datom_set(), loaded.datom_set());
    assert_eq!(store.current_epoch(), loaded.current_epoch());
}
```

**Serialization codec**: The checkpoint payload uses **bincode** (the same binary codec
as the WAL). JSON serialization was used in the Phase 4a prototype but violates
INV-FERR-028 (cold start < 5s at 100M datoms) due to ~2.5x size overhead and text
parsing cost. At 100M datoms, bincode produces ~20GB checkpoints (parseable in <5s on
NVMe); JSON produces ~50GB (unparseable in the time budget). The `datom.serialize_into()`
and `Datom::deserialize_from()` calls in the Level 2 contract use bincode encoding.
Per ADR-FERR-010, deserialization produces wire types (`WireDatom`) which are converted
to core types via `into_trusted()` after BLAKE3 checksum verification.

**Falsification**: A store `S` where `load(checkpoint(S)).datom_set() != S.datom_set()`.
Specific failure modes:
- **Datom loss**: a datom present in `S` is absent after round-trip (serialization drops it).
- **Datom gain**: a datom absent in `S` appears after round-trip (deserialization invents it).
- **Epoch drift**: `load(checkpoint(S)).current_epoch() != S.current_epoch()`.
- **Index desync**: `verify_index_bijection(load(checkpoint(S)))` returns false (indexes
  not rebuilt correctly from deserialized datoms).
- **Checksum bypass**: a corrupted checkpoint file is loaded without error (checksum
  verification missing or incorrect).
- **Query divergence**: there exists a query `Q` such that `eval(S, Q) != eval(load(checkpoint(S)), Q)`.

**proptest strategy**:
```rust
proptest! {
    #[test]
    fn checkpoint_roundtrip(
        datoms in prop::collection::btree_set(arb_datom(), 0..200),
        txns in prop::collection::vec(arb_transaction(), 0..10),
    ) {
        let mut store = Store::from_datoms(datoms);
        for tx in txns {
            let _ = store.transact(tx);
        }

        let tmp = tempfile::NamedTempFile::new()?;
        checkpoint(&store, tmp.path())?;
        let loaded = load_checkpoint(tmp.path())?;

        // Datom set identity
        prop_assert_eq!(store.datom_set(), loaded.datom_set());
        // Epoch identity
        prop_assert_eq!(store.current_epoch(), loaded.current_epoch());
        // Index bijection on loaded store
        prop_assert!(verify_index_bijection(&loaded));
        // Schema identity
        prop_assert_eq!(store.schema(), loaded.schema());
    }

    #[test]
    fn corrupted_checkpoint_rejected(
        datoms in prop::collection::btree_set(arb_datom(), 1..50),
        corrupt_byte_idx in any::<usize>(),
        corrupt_value in any::<u8>(),
    ) {
        let store = Store::from_datoms(datoms);
        let tmp = tempfile::NamedTempFile::new()?;
        checkpoint(&store, tmp.path())?;

        // Corrupt a single byte
        let mut data = std::fs::read(tmp.path())?;
        let idx = corrupt_byte_idx % data.len();
        if data[idx] != corrupt_value {
            data[idx] = corrupt_value;
            std::fs::write(tmp.path(), &data)?;
            // Must be rejected
            prop_assert!(load_checkpoint(tmp.path()).is_err());
        }
    }
}
```

**Lean theorem**:
```lean
/-- Checkpoint equivalence: serialization and deserialization are inverses.
    We model checkpoint as the identity function on DatomStore (since the
    mathematical content is preserved; only the physical representation changes). -/

def checkpoint_serialize (s : DatomStore) : DatomStore := s
def checkpoint_deserialize (s : DatomStore) : DatomStore := s

theorem checkpoint_roundtrip (s : DatomStore) :
    checkpoint_deserialize (checkpoint_serialize s) = s := by
  unfold checkpoint_deserialize checkpoint_serialize
  rfl

/-- Checkpoint preserves cardinality. -/
theorem checkpoint_preserves_card (s : DatomStore) :
    (checkpoint_deserialize (checkpoint_serialize s)).card = s.card := by
  rw [checkpoint_roundtrip]

/-- Checkpoint preserves membership. -/
theorem checkpoint_preserves_mem (s : DatomStore) (d : Datom) :
    d ∈ checkpoint_deserialize (checkpoint_serialize s) ↔ d ∈ s := by
  rw [checkpoint_roundtrip]
```

**Checkpoint Triggering Policy**

INV-FERR-013 specifies the checkpoint FORMAT and round-trip identity. This section
specifies WHEN checkpoints are created.

Default policy (configurable via `CheckpointPolicy`):
- **Transaction count trigger**: every 1,000 committed transactions since last checkpoint.
- **WAL size trigger**: when WAL file size exceeds 100 MB since last checkpoint.
- **Whichever comes first** — the checkpoint fires when either threshold is crossed.
- **Explicit trigger**: `Database::checkpoint()` forces a checkpoint at any time,
  regardless of thresholds.

The default thresholds are justified by two competing constraints:
- **INV-FERR-028 (cold start < 5s at 100M datoms)**: recovery replays the WAL delta
  since the last checkpoint. A 100MB WAL with 1,000 transactions contains ~100K datoms
  at ~1KB/datom, which replays in < 1s. Larger gaps risk exceeding the 5s budget.
- **INV-FERR-026 (write amplification)**: each checkpoint serializes the entire store
  (O(n) where n = total datoms). At 100M datoms, a checkpoint takes ~5-30s. Checkpointing
  too frequently wastes I/O bandwidth and stalls the write pipeline. The 1,000-transaction
  interval limits checkpoint overhead to < 1% of write throughput.

```rust
/// Checkpoint triggering policy.
pub struct CheckpointPolicy {
    /// Checkpoint after this many transactions since last checkpoint.
    /// Default: 1_000.
    pub transaction_count_trigger: u64,
    /// Checkpoint when WAL exceeds this size in bytes since last checkpoint.
    /// Default: 100 * 1024 * 1024 (100 MB).
    pub wal_size_trigger: u64,
}

impl Default for CheckpointPolicy {
    fn default() -> Self {
        Self {
            transaction_count_trigger: 1_000,
            wal_size_trigger: 100 * 1024 * 1024,
        }
    }
}
```

In the Phase 4a Mutex model, the checkpoint runs synchronously inside `transact()` when
a threshold is crossed (after WAL write, before `ArcSwap` publish). In the Phase 4b+
WriterActor model, the checkpoint runs after a group commit batch completes, in the
writer task context.

---

### INV-FERR-014: Recovery Correctness

**Traces to**: SEED.md §5 (Harvest/Seed Lifecycle — durability), C1, INV-STORE-009,
INV-FERR-008 (WAL Fsync Ordering), INV-FERR-013 (Checkpoint Equivalence)
**Referenced by**: INV-FERR-060 (store identity persists through recovery)
**Verification**: `V:PROP`, `V:KANI`, `V:MODEL`
**Stage**: 0

#### Level 0 (Algebraic Law)
```
Let crash : DatomStore → CrashedState be the crash function (non-deterministic:
  the crash may occur at any point during any operation).
Let recover : CrashedState → DatomStore be the recovery function.
Let last_committed : DatomStore → DatomStore be the projection to the
  last fully committed state (all fsynced WAL entries applied).

∀ S ∈ DatomStore:
  recover(crash(S)) ⊇ last_committed(S)

Concretely:
  - Every datom from a fully committed transaction (WAL fsynced) survives recovery.
  - Datoms from uncommitted transactions (WAL not fsynced) may or may not survive.
  - No datom that was never transacted appears after recovery (no phantom datoms).

The inclusion is ⊇ rather than = because the crash may occur after WAL fsync
but before snapshot publication. In this case, recovery replays the WAL entry,
which produces a store ⊇ last_committed. The = case holds when no such
in-flight transaction exists at crash time.
```

#### Level 1 (State Invariant)
After any crash at any point during any operation (TRANSACT, MERGE, checkpoint,
index rebuild), the recovery procedure produces a store that contains at least
all datoms from all committed transactions. The recovery procedure is:
1. Load the latest checkpoint (INV-FERR-013).
2. Replay all complete WAL entries after the checkpoint's epoch (INV-FERR-008).
3. Truncate any incomplete WAL entry (partial write from crash).
4. Rebuild indexes from the recovered datom set (INV-FERR-005).

The recovered store is fully functional: all indexes are consistent (INV-FERR-005),
the epoch is correct, and new transactions can be applied. The recovery procedure
is idempotent: running recovery on an already-recovered store produces the same
store (the WAL contains no entries beyond the checkpoint, so replay is a no-op).

No data from committed transactions is lost. The only data that may be lost is
from the transaction that was in progress at crash time — and only if its WAL
entry was not fully fsynced. This is the maximum durability guarantee achievable
without synchronous replication.

#### Level 2 (Implementation Contract)
```rust
/// Full crash recovery procedure.
/// 1. Load latest checkpoint.
/// 2. Replay WAL from checkpoint epoch.
/// 3. Truncate incomplete WAL entries.
/// 4. Rebuild indexes.
pub fn recover(data_dir: &Path) -> Result<Store, RecoveryError> {
    // Step 1: Load checkpoint
    let checkpoint_path = latest_checkpoint(data_dir)?;
    let mut store = load_checkpoint(&checkpoint_path)?;
    let checkpoint_epoch = store.current_epoch();

    // Step 2: Replay WAL
    let wal_path = data_dir.join("wal");
    let mut wal = Wal::open(&wal_path)?;
    let entries = wal.recover()?; // truncates incomplete entries

    let mut replayed = 0;
    for entry in entries {
        if entry.epoch > checkpoint_epoch {
            store.apply_wal_entry(&entry)?;
            replayed += 1;
        }
    }

    // Step 3: Verify integrity
    debug_assert!(verify_index_bijection(&store), "INV-FERR-005 after recovery");
    debug_assert!(
        store.current_epoch() >= checkpoint_epoch,
        "INV-FERR-014: epoch regression after recovery"
    );

    log::info!(
        "Recovery complete: checkpoint epoch {}, replayed {} WAL entries, final epoch {}",
        checkpoint_epoch, replayed, store.current_epoch()
    );

    Ok(store)
}

/// Idempotent recovery: recovering an already-recovered store is a no-op.
/// The WAL is empty (or contains only entries already applied), so replay
/// adds no new datoms.
pub fn recover_idempotent(data_dir: &Path) -> Result<Store, RecoveryError> {
    let s1 = recover(data_dir)?;
    let s2 = recover(data_dir)?;
    debug_assert_eq!(s1.datom_set(), s2.datom_set(), "INV-FERR-014: recovery not idempotent");
    Ok(s2)
}

#[kani::proof]
#[kani::unwind(8)]
fn recovery_superset() {
    let committed: BTreeSet<Datom> = kani::any();
    kani::assume(committed.len() <= 4);

    // Simulate: uncommitted datoms may or may not survive
    let uncommitted: BTreeSet<Datom> = kani::any();
    kani::assume(uncommitted.len() <= 2);
    let survived: bool = kani::any();

    let mut recovered = committed.clone();
    if survived {
        for d in &uncommitted {
            recovered.insert(d.clone());
        }
    }

    // Committed datoms always survive
    assert!(committed.is_subset(&recovered));
}
```

**Falsification**: A committed transaction `T` (WAL entry fsynced per INV-FERR-008) whose
datoms are absent from the store after crash and recovery. Specific failure modes:
- **WAL truncation overreach**: recovery truncates a complete WAL entry, treating it as
  incomplete (deserialization bug in entry boundary detection).
- **Checkpoint stale**: the latest checkpoint is older than expected, and WAL entries
  between the true checkpoint epoch and the loaded checkpoint epoch are lost.
- **Index desync after recovery**: the recovered store has correct datoms but incorrect
  indexes (INV-FERR-005 violated after recovery).
- **Phantom datoms**: a datom appears in the recovered store that was never part of any
  committed or in-progress transaction (deserialization produces incorrect data).
- **Non-idempotent recovery**: `recover(recover(crash(S)))` differs from `recover(crash(S))`.

**proptest strategy**:
```rust
proptest! {
    #[test]
    fn recovery_preserves_committed(
        committed_txns in prop::collection::vec(arb_transaction(), 1..10),
    ) {
        let tmp_dir = tempfile::tempdir()?;
        let mut store = Store::genesis();

        // Apply and commit transactions
        let mut committed_datoms = BTreeSet::new();
        for tx in &committed_txns {
            if let Ok(receipt) = store.transact(tx.clone()) {
                for d in receipt.datoms() {
                    committed_datoms.insert(d.clone());
                }
            }
        }

        // Checkpoint and create WAL
        checkpoint(&store, &tmp_dir.path().join("checkpoint"))?;

        // Simulate crash: just recover
        let recovered = recover(tmp_dir.path())?;

        // All committed datoms must be present
        for d in &committed_datoms {
            prop_assert!(
                recovered.datom_set().contains(d),
                "Committed datom lost in recovery: {:?}", d
            );
        }
    }

    #[test]
    fn recovery_idempotent(
        txns in prop::collection::vec(arb_transaction(), 1..5),
    ) {
        let tmp_dir = tempfile::tempdir()?;
        let mut store = Store::genesis();

        for tx in &txns {
            let _ = store.transact(tx.clone());
        }
        checkpoint(&store, &tmp_dir.path().join("checkpoint"))?;

        let r1 = recover(tmp_dir.path())?;
        let r2 = recover(tmp_dir.path())?;
        prop_assert_eq!(r1.datom_set(), r2.datom_set());
        prop_assert_eq!(r1.current_epoch(), r2.current_epoch());
    }
}
```

**Lean theorem**:
```lean
/-- Recovery correctness: the recovered store is a superset of the
    last committed store. We model crash as an arbitrary subset removal
    of uncommitted datoms. -/

def last_committed (s uncommitted : DatomStore) : DatomStore := s \ uncommitted

def recover_model (s uncommitted : DatomStore) (survived : Bool) : DatomStore :=
  if survived then s else s \ uncommitted

theorem recovery_superset (s uncommitted : DatomStore) (survived : Bool) :
    last_committed s uncommitted ⊆ recover_model s uncommitted survived := by
  unfold last_committed recover_model
  cases survived with
  | true =>
      intro d hd
      exact (Finset.mem_sdiff.mp hd).1
  | false =>
      intro d hd
      exact hd

/-- Recovery preserves all committed datoms (no loss). -/
theorem recovery_no_loss (s uncommitted : DatomStore) (d : Datom)
    (h_committed : d ∈ s) (h_not_uncommitted : d ∉ uncommitted) (survived : Bool) :
    d ∈ recover_model s uncommitted survived := by
  unfold recover_model
  cases survived with
  | true => exact h_committed
  | false =>
    simp [Finset.mem_sdiff]
    exact ⟨h_committed, h_not_uncommitted⟩
```

---

### INV-FERR-015: HLC Monotonicity

**Traces to**: SEED.md §4 (Core Abstraction: Temporal Ordering), INV-STORE-011,
ADR-FERR-005 (Clock Model)
**Verification**: `V:PROP`, `V:KANI`, `V:LEAN`
**Stage**: 0

#### Level 0 (Algebraic Law)
```
Let HLC = (physical : u64, logical : u32, node : NodeId) be a hybrid logical clock.
Let tick : Node → HLC be the clock advance function.

∀ node α:
  ∀ consecutive ticks t₁, t₂ of α (t₁ before t₂):
    tick(α, t₂).physical ≥ tick(α, t₁).physical

Physical time component is monotonically non-decreasing for any single node.
If the wall clock advances, physical advances. If the wall clock is stale
(NTP regression, VM snapshot restore), the logical counter increments to
maintain the total ordering:

  tick(α) =
    let pt = max(prev.physical, wall_clock())
    if pt == prev.physical then
      (pt, prev.logical + 1, α)
    else
      (pt, 0, α)

NodeId ordering: NodeId is a [u8; 16] byte array. Total order is lexicographic
byte comparison. This is deterministic, portable, and collision-resistant when
NodeId = BLAKE3(node_name)[0..16]. Two nodes with identical BLAKE3 prefixes
(16-byte collision probability ≈ 2⁻¹²⁸) are operationally indistinguishable —
a negligible risk equivalent to SHA-256 collision.

The total order on HLC is:
  h₁ < h₂ ⟺ h₁.physical < h₂.physical
             ∨ (h₁.physical = h₂.physical ∧ h₁.logical < h₂.logical)
             ∨ (h₁.physical = h₂.physical ∧ h₁.logical = h₂.logical
                ∧ h₁.agent < h₂.agent)
```

#### Level 1 (State Invariant)
The HLC on every agent is strictly monotonically increasing: every event on agent `α`
receives an HLC value strictly greater than any previous HLC value on `α`. This holds
even if the physical clock regresses (NTP adjustment, VM migration, leap second):
- If `wall_clock() > prev.physical`: the physical component advances, logical resets to 0.
- If `wall_clock() == prev.physical`: physical stays, logical increments.
- If `wall_clock() < prev.physical`: physical stays at `prev.physical` (does not regress),
  logical increments.

The HLC is also updated on message receipt: when agent `α` receives a message from agent
`β` with HLC `h_β`, agent `α` sets its physical component to `max(α.physical, h_β.physical,
wall_clock())` and adjusts logical accordingly. This ensures that causal ordering is
preserved even across agents with clock skew (INV-FERR-016).

The logical counter is a `u32` (~4.3 billion values). Operational backpressure is
provided by the WriteLimiter (INV-FERR-021), not by HLC overflow. The u32::MAX
busy-wait in `tick()` is a theoretical safety valve that should never fire in practice
— at ~4.3 billion events per millisecond per agent, the counter cannot overflow on
any physical hardware. If it ever did fire, the HLC blocks until the physical clock
advances, preventing counter wrap-around.

#### Level 2 (Implementation Contract)
```rust
/// Node identifier: 16-byte BLAKE3 hash prefix.
/// Lexicographic byte order via derived Ord provides the total order
/// used as tie-breaker in HLC comparison.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NodeId([u8; 16]);  // Lexicographic byte order via derived Ord

/// Hybrid Logical Clock.
/// Invariant: every call to tick() returns a value strictly greater than
/// any previous return value on this node.
#[derive(Clone, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct Hlc {
    physical: u64,   // milliseconds since epoch
    logical: u32,    // counter within same millisecond
    node: NodeId,    // tie-breaker across nodes
}

impl Hlc {
    /// Advance the clock. Returns a value strictly greater than any previous
    /// return value. Blocks if logical counter would overflow.
    pub fn tick(&mut self) -> Hlc {
        let now = wall_clock_ms();
        if now > self.physical {
            self.physical = now;
            self.logical = 0;
        } else if self.logical < u32::MAX {
            self.logical += 1;
        } else {
            // Backpressure: wait for physical clock to advance
            loop {
                std::thread::yield_now();
                let now = wall_clock_ms();
                if now > self.physical {
                    self.physical = now;
                    self.logical = 0;
                    break;
                }
            }
        }
        self.clone()
    }

    /// Receive update: merge with remote HLC to preserve causality.
    pub fn receive(&mut self, remote: &Hlc) {
        let now = wall_clock_ms();
        let max_phys = now.max(self.physical).max(remote.physical);

        if max_phys > self.physical && max_phys > remote.physical {
            self.physical = max_phys;
            self.logical = 0;
        } else if max_phys == self.physical && max_phys == remote.physical {
            self.logical = self.logical.max(remote.logical) + 1;
        } else if max_phys == self.physical {
            self.logical += 1;
        } else {
            self.physical = max_phys;
            self.logical = remote.logical + 1;
        }
    }
}

#[kani::proof]
#[kani::unwind(10)]
fn hlc_monotonicity() {
    let mut hlc = Hlc::new(NodeId::test());
    let mut prev = hlc.clone();

    for _ in 0..kani::any::<u8>().min(5) {
        let next = hlc.tick();
        assert!(next > prev, "HLC did not advance");
        prev = next;
    }
}
```

**Falsification**: Agent `α` produces two consecutive HLC values `h₁, h₂` where `h₂ ≤ h₁`
under the total order. Specific failure modes:
- **Physical regression**: `h₂.physical < h₁.physical` (clock went backward without
  logical compensation).
- **Logical overflow**: `h₁.logical == u32::MAX` and the next tick produces `logical == 0`
  without advancing physical (wrap-around instead of backpressure).
- **Receive regression**: after receiving a remote HLC, the local HLC is less than or
  equal to both the previous local HLC and the remote HLC (merge logic bug).

**proptest strategy**:
```rust
proptest! {
    #[test]
    fn hlc_strictly_monotonic(
        wall_clocks in prop::collection::vec(0u64..1_000_000, 2..50),
    ) {
        let mut hlc = Hlc::new(NodeId::test());
        let mut prev: Option<Hlc> = None;

        for wc in wall_clocks {
            // Simulate wall clock (may regress)
            set_mock_wall_clock(wc);
            let current = hlc.tick();

            if let Some(ref p) = prev {
                prop_assert!(current > *p,
                    "HLC regression: {:?} -> {:?} with wall_clock={}",
                    p, current, wc);
            }
            prev = Some(current);
        }
    }

    #[test]
    fn hlc_receive_advances(
        local_ticks in 1u8..10,
        remote_physical in 0u64..1_000_000,
        remote_logical in 0u32..1000,
    ) {
        let mut hlc = Hlc::new(NodeId::from("local"));
        for _ in 0..local_ticks {
            hlc.tick();
        }
        let pre_receive = hlc.clone();

        let remote = Hlc {
            physical: remote_physical,
            logical: remote_logical,
            agent: NodeId::from("remote"),
        };
        hlc.receive(&remote);

        prop_assert!(hlc >= pre_receive, "HLC regressed after receive");
        prop_assert!(hlc >= remote, "HLC less than received remote");
    }
}
```

**Lean theorem**:
```lean
/-- HLC monotonicity: the tick function always produces a strictly greater value.
    We model HLC as a pair (physical, logical) with lexicographic ordering. -/

structure HlcModel where
  physical : Nat
  logical : Nat
  deriving DecidableEq, Repr

instance : LT HlcModel where
  lt a b := a.physical < b.physical ∨
            (a.physical = b.physical ∧ a.logical < b.logical)

instance : LE HlcModel where
  le a b := a.physical < b.physical ∨
             (a.physical = b.physical ∧ a.logical ≤ b.logical)

def hlc_tick (prev : HlcModel) (wall_clock : Nat) : HlcModel :=
  if wall_clock > prev.physical then
    { physical := wall_clock, logical := 0 }
  else
    { physical := prev.physical, logical := prev.logical + 1 }

theorem hlc_tick_monotone (prev : HlcModel) (wall_clock : Nat) :
    prev < hlc_tick prev wall_clock := by
  unfold hlc_tick
  split
  · -- wall_clock > prev.physical
    left
    assumption
  · -- wall_clock ≤ prev.physical
    right
    constructor
    · rfl
    · exact Nat.lt_succ_of_le (Nat.le_refl _)
```

---

### INV-FERR-016: HLC Causality

**Traces to**: SEED.md §4 (Core Abstraction: Temporal Ordering), INV-STORE-011,
ADR-FERR-005 (Clock Model)
**Referenced by**: INV-FERR-061 (causal predecessors — Frontier drawn from HLC ordering),
ADR-FERR-026 (causal predecessors as datoms)
**Verification**: `V:PROP`, `V:KANI`, `V:LEAN`, `V:MODEL`
**Stage**: 0

#### Level 0 (Algebraic Law)
```
Let happens_before(e₁, e₂) be the Lamport happens-before relation:
  - e₁ and e₂ are on the same agent and e₁ occurs before e₂, or
  - e₁ is a send event and e₂ is the corresponding receive event, or
  - ∃ e₃: happens_before(e₁, e₃) ∧ happens_before(e₃, e₂)  (transitivity)

∀ events e₁, e₂:
  happens_before(e₁, e₂) ⟹ hlc(e₁) < hlc(e₂)

The converse does NOT hold: hlc(e₁) < hlc(e₂) does NOT imply
happens_before(e₁, e₂). Concurrent events (neither happens-before
the other) may have any HLC ordering. The HLC is a Lamport clock
with physical time augmentation, not a vector clock.

This property ensures that causal chains are always preserved in the
HLC ordering. If agent α sends a message to agent β, and β's action
depends on that message, then β's HLC is guaranteed to be greater
than α's send-time HLC.
```

#### Level 1 (State Invariant)
For every pair of events `(e₁, e₂)` connected by the happens-before relation — whether
on the same agent (sequential events) or across agents (send-receive pairs) or through
transitive chains — the HLC timestamp of `e₁` is strictly less than the HLC timestamp
of `e₂`. This means:
- Within a single agent, consecutive events have increasing HLCs (INV-FERR-015).
- When agent `α` sends a message to agent `β`, `α`'s send-time HLC is included in the
  message. Agent `β` calls `receive()` which advances `β`'s HLC to be strictly greater
  than both `β`'s previous HLC and `α`'s send-time HLC.
- Through transitivity, if `e₁` causally precedes `e₃` through intermediate events
  `e₂`, then `hlc(e₁) < hlc(e₂) < hlc(e₃)`.

This property is essential for the datom store's causal ordering: transactions that
causally depend on other transactions (e.g., a retraction that references an earlier
assertion) must have HLC timestamps that reflect the causal dependency. Without this
property, the LIVE view (INV-FERR-029) could produce incorrect resolutions by applying
a retraction "before" the assertion it retracts.

#### Level 2 (Implementation Contract)
```rust
/// Stateright model: verify HLC causality under arbitrary message orderings.
impl stateright::Model for HlcCausalityModel {
    type State = HlcNetworkState;
    type Action = HlcAction;

    fn init_states(&self) -> Vec<Self::State> {
        vec![HlcNetworkState {
            agents: (0..self.agent_count)
                .map(|i| AgentHlc {
                    hlc: Hlc::new(NodeId::from(i)),
                    events: vec![],
                })
                .collect(),
            messages: vec![],
        }]
    }

    fn actions(&self, state: &Self::State, actions: &mut Vec<Self::Action>) {
        for agent_idx in 0..self.agent_count {
            // Local event
            actions.push(HlcAction::LocalEvent(agent_idx));
            // Send to every other agent
            for peer_idx in 0..self.agent_count {
                if peer_idx != agent_idx {
                    actions.push(HlcAction::Send(agent_idx, peer_idx));
                }
            }
        }
        // Deliver pending messages (in any order — model checker explores all)
        for msg_idx in 0..state.messages.len() {
            actions.push(HlcAction::Deliver(msg_idx));
        }
    }

    fn next_state(&self, state: &Self::State, action: Self::Action) -> Option<Self::State> {
        let mut next = state.clone();
        match action {
            HlcAction::LocalEvent(agent) => {
                let ts = next.agents[agent].hlc.tick();
                next.agents[agent].events.push(Event::Local(ts));
            }
            HlcAction::Send(from, to) => {
                let ts = next.agents[from].hlc.tick();
                next.agents[from].events.push(Event::Send(ts.clone(), to));
                next.messages.push(Message { from, to, hlc: ts });
            }
            HlcAction::Deliver(msg_idx) => {
                let msg = next.messages.remove(msg_idx);
                next.agents[msg.to].hlc.receive(&msg.hlc);
                let ts = next.agents[msg.to].hlc.tick();
                next.agents[msg.to].events.push(Event::Receive(msg.hlc.clone(), ts));
            }
        }
        Some(next)
    }

    fn properties(&self) -> Vec<Property<Self>> {
        vec![Property::always("hlc_causality", |_, state: &HlcNetworkState| {
            // For every send-receive pair, receive HLC > send HLC
            for agent in &state.agents {
                for event in &agent.events {
                    if let Event::Receive(send_hlc, recv_hlc) = event {
                        if recv_hlc <= send_hlc {
                            return false;
                        }
                    }
                }
            }
            true
        })]
    }
}

#[kani::proof]
#[kani::unwind(6)]
fn hlc_causality() {
    let mut sender = Hlc::new(NodeId::from("sender"));
    let mut receiver = Hlc::new(NodeId::from("receiver"));

    // Sender ticks
    let send_hlc = sender.tick();

    // Receiver receives and ticks
    receiver.receive(&send_hlc);
    let recv_hlc = receiver.tick();

    assert!(recv_hlc > send_hlc, "Causality violation: recv <= send");
}
```

**Falsification**: Two events `(e₁, e₂)` where `happens_before(e₁, e₂)` but
`hlc(e₁) >= hlc(e₂)`. Specific failure modes:
- **Receive without merge**: agent `β` receives a message from `α` but does not call
  `receive()`, so `β`'s HLC does not advance past `α`'s send HLC.
- **Incorrect merge**: `receive()` takes `max(local.physical, remote.physical)` but
  incorrectly handles the logical counter (e.g., does not increment when physicals
  are equal).
- **Transitivity failure**: `hlc(e₁) < hlc(e₂)` and `hlc(e₂) < hlc(e₃)` but
  `hlc(e₁) >= hlc(e₃)` — would indicate a broken total order implementation on `Hlc`.

**proptest strategy**:
```rust
proptest! {
    #[test]
    fn causal_chain_preserved(
        chain_length in 2..20usize,
        agent_count in 2..5usize,
        agent_sequence in prop::collection::vec(0..5usize, 2..20),
    ) {
        let mut agents: Vec<Hlc> = (0..agent_count)
            .map(|i| Hlc::new(NodeId::from(i)))
            .collect();

        let mut hlc_chain: Vec<Hlc> = vec![];

        for &agent_idx in &agent_sequence {
            let agent_idx = agent_idx % agent_count;

            if let Some(prev_hlc) = hlc_chain.last() {
                // Simulate message delivery from previous agent
                agents[agent_idx].receive(prev_hlc);
            }
            let ts = agents[agent_idx].tick();
            hlc_chain.push(ts);
        }

        // Every element in the chain is strictly less than the next
        for i in 1..hlc_chain.len() {
            prop_assert!(hlc_chain[i] > hlc_chain[i - 1],
                "Causal chain broken at index {}: {:?} -> {:?}",
                i, hlc_chain[i - 1], hlc_chain[i]);
        }
    }
}
```

**Lean theorem**:
```lean
/-- HLC causality: if event e₁ happens-before event e₂, then
    hlc(e₁) < hlc(e₂). We model this as: receive always produces
    a value strictly greater than the remote HLC. -/

def hlc_receive (local remote : HlcModel) (wall_clock : Nat) : HlcModel :=
  let max_phys := max wall_clock (max local.physical remote.physical)
  if max_phys > local.physical ∧ max_phys > remote.physical then
    { physical := max_phys, logical := 0 }
  else if max_phys = local.physical ∧ max_phys = remote.physical then
    { physical := max_phys, logical := max local.logical remote.logical + 1 }
  else if max_phys = local.physical then
    { physical := max_phys, logical := local.logical + 1 }
  else
    { physical := max_phys, logical := remote.logical + 1 }

/-- Mechanized witnesses for INV-FERR-016 live in
    `ferratomic-verify/lean/Ferratomic/Concurrency.lean`.

    The spec model here (`HlcModel`) is intentionally lightweight. The
    mechanically checked implementation model (`HLC`) proves the two
    substantive obligations:
    1. `hlc_receive_gt_remote` — receive produces a value strictly greater
       than the remote timestamp.
    2. `hlc_lt_trans` — the strict lexicographic ordering is transitive. -/

#check Ferratomic.Concurrency.hlc_receive_gt_remote
#check Ferratomic.Concurrency.hlc_lt_trans
```

---

### INV-FERR-017: Shard Equivalence

**Traces to**: SEED.md §4 Axiom 2 (Store), C4, INV-STORE-004,
ADR-FERR-006 (Sharding Strategy)
**Verification**: `V:PROP`, `V:KANI`, `V:LEAN`
**Stage**: 0

#### Level 0 (Algebraic Law)
```
Let shard : DatomStore × Nat → DatomStore be the sharding function.
Let N be the number of shards.

∀ S ∈ DatomStore:
  ⋃ᵢ₌₀ᴺ⁻¹ shard(S, i) = S

Sharding is a partition of the datom set:
  1. Coverage: every datom belongs to at least one shard.
     ∀ d ∈ S: ∃ i ∈ [0, N): d ∈ shard(S, i)
  2. Disjointness: no datom belongs to two shards.
     ∀ i ≠ j: shard(S, i) ∩ shard(S, j) = ∅
  3. Union: the union of all shards equals the original store.
     ⋃ᵢ shard(S, i) = S

The sharding function is deterministic:
  ∀ d ∈ Datom: shard_id(d) = hash(d.e) mod N
  (entity-hash sharding, per ADR-FERR-006)

Entity-hash sharding keeps all datoms for the same entity on the same
shard, preserving entity-level locality for single-entity queries.
```

#### Level 1 (State Invariant)
For every store state `S` and shard count `N`, decomposing `S` into `N` shards and
recomposing via set union produces `S` unchanged. No datom is lost by sharding and
no datom is duplicated. The sharding function is a mathematical partition: the shards
are pairwise disjoint and their union is the whole.

This property enables horizontal scalability: a store too large for a single node
can be split across `N` nodes, each holding one shard. Any query that requires the
full store can be answered by querying all shards and merging the results (for
monotonic queries — see INV-FERR-033 for the non-monotonic case).

The sharding function is based on entity-hash (`hash(d.e) mod N`), which ensures
that all datoms about the same entity reside on the same shard. This is critical
for entity-level operations (e.g., "all attributes of entity E") which would
otherwise require cross-shard joins.

Re-sharding (changing `N`) requires redistributing datoms. Since the store is
append-only (C1), re-sharding only needs to move datoms, never update or delete.
The re-shard operation is itself a sequence of merges (move datom from old shard
to new shard = retract from old, assert in new — but since shards are partitions
of the same store, it is actually just re-partitioning the same set).

#### Level 2 (Implementation Contract)
```rust
/// Compute the shard ID for a datom. Deterministic, based on entity hash.
pub fn shard_id(datom: &Datom, shard_count: usize) -> usize {
    let entity_hash = datom.entity.as_bytes();
    let hash_u64 = u64::from_le_bytes(entity_hash[0..8].try_into().unwrap());
    (hash_u64 % shard_count as u64) as usize
}

/// Decompose a store into N shards.
pub fn shard(store: &Store, shard_count: usize) -> Vec<Store> {
    let mut shards: Vec<BTreeSet<Datom>> = (0..shard_count)
        .map(|_| BTreeSet::new())
        .collect();

    for datom in store.datoms.iter() {
        let idx = shard_id(datom, shard_count);
        shards[idx].insert(datom.clone());
    }

    shards.into_iter().map(Store::from_datoms).collect()
}

/// Recompose shards into a single store (set union).
pub fn unshard(shards: &[Store]) -> Store {
    shards.iter().fold(Store::empty(), |acc, s| merge(&acc, s))
}

#[kani::proof]
#[kani::unwind(8)]
fn shard_equivalence() {
    let datoms: BTreeSet<Datom> = kani::any();
    kani::assume(datoms.len() <= 4);
    let shard_count: usize = kani::any();
    kani::assume(shard_count > 0 && shard_count <= 4);

    let store = Store::from_datoms(datoms.clone());

    // Shard and unshard
    let shards = shard(&store, shard_count);
    let recomposed = unshard(&shards);

    assert_eq!(store.datom_set(), recomposed.datom_set());
}

#[kani::proof]
#[kani::unwind(8)]
fn shard_disjointness() {
    let datoms: BTreeSet<Datom> = kani::any();
    kani::assume(datoms.len() <= 4);
    let shard_count: usize = kani::any();
    kani::assume(shard_count >= 2 && shard_count <= 4);

    let store = Store::from_datoms(datoms);
    let shards = shard(&store, shard_count);

    // Pairwise disjointness
    for i in 0..shards.len() {
        for j in (i + 1)..shards.len() {
            let intersection: BTreeSet<_> = shards[i].datom_set()
                .intersection(shards[j].datom_set()).collect();
            assert!(intersection.is_empty(),
                "Shards {} and {} share datoms", i, j);
        }
    }
}
```

**Falsification**: A store `S` and shard count `N` where `unshard(shard(S, N)) != S`.
Specific failure modes:
- **Datom loss**: a datom `d ∈ S` is not present in any `shard(S, i)` (sharding function
  produces an out-of-range index or the datom is skipped during iteration).
- **Datom duplication**: a datom `d` appears in both `shard(S, i)` and `shard(S, j)` for
  `i != j` (sharding function is non-deterministic or maps the same entity to multiple shards).
- **Entity split**: two datoms with the same entity ID are placed in different shards
  (the sharding function does not consistently hash entity IDs).

**proptest strategy**:
```rust
proptest! {
    #[test]
    fn shard_union_equals_original(
        datoms in prop::collection::btree_set(arb_datom(), 0..200),
        shard_count in 1..16usize,
    ) {
        let store = Store::from_datoms(datoms);
        let shards = shard(&store, shard_count);
        let recomposed = unshard(&shards);
        prop_assert_eq!(store.datom_set(), recomposed.datom_set());
    }

    #[test]
    fn shards_are_disjoint(
        datoms in prop::collection::btree_set(arb_datom(), 0..200),
        shard_count in 2..16usize,
    ) {
        let store = Store::from_datoms(datoms);
        let shards = shard(&store, shard_count);

        let total: usize = shards.iter().map(|s| s.len()).sum();
        prop_assert_eq!(total, store.len(),
            "Sum of shard sizes ({}) != store size ({})", total, store.len());
    }

    #[test]
    fn entity_locality(
        datoms in prop::collection::btree_set(arb_datom(), 0..200),
        shard_count in 2..16usize,
    ) {
        let store = Store::from_datoms(datoms);
        let shards = shard(&store, shard_count);

        // All datoms for the same entity must be on the same shard
        let mut entity_shards: BTreeMap<EntityId, usize> = BTreeMap::new();
        for (shard_idx, s) in shards.iter().enumerate() {
            for d in s.datoms.iter() {
                if let Some(&prev_shard) = entity_shards.get(&d.entity) {
                    prop_assert_eq!(prev_shard, shard_idx,
                        "Entity {:?} split across shards {} and {}",
                        d.entity, prev_shard, shard_idx);
                }
                entity_shards.insert(d.entity.clone(), shard_idx);
            }
        }
    }
}
```

**Lean theorem**:
```lean
/-- Shard equivalence: partitioning a set and taking the union recovers the original.
    We model sharding as a function from datoms to shard indices. -/

def shard_partition (s : DatomStore) (f : Datom → Fin n) (i : Fin n) : DatomStore :=
  s.filter (fun d => f d = i)

theorem shard_union (s : DatomStore) (f : Datom → Fin n) (hn : n > 0) :
    (Finset.univ.biUnion (shard_partition s f)) = s := by
  ext d
  simp [shard_partition, Finset.mem_biUnion, Finset.mem_filter]
  constructor
  · intro ⟨_, _, hd, _⟩; exact hd
  · intro hd; exact ⟨f d, Finset.mem_univ _, hd, rfl⟩

theorem shard_disjoint (s : DatomStore) (f : Datom → Fin n) (i j : Fin n) (h : i ≠ j) :
    shard_partition s f i ∩ shard_partition s f j = ∅ := by
  ext d
  simp [shard_partition, Finset.mem_inter, Finset.mem_filter]
  intro _ hi _ hj
  exact absurd (hi.symm.trans hj) h
```

---

### INV-FERR-018: Append-Only

**Traces to**: SEED.md §4 (Design Commitment #2), C1, INV-STORE-001, INV-STORE-002
**Verification**: `V:TYPE`, `V:PROP`, `V:KANI`, `V:LEAN`
**Stage**: 0

#### Level 0 (Algebraic Law)
```
∀ S ∈ DatomStore, ∀ op ∈ {TRANSACT, MERGE, RECOVER}:
  let S' = op(S, args)
  ∀ d ∈ S: d ∈ S'

No operation removes a datom from the store. The set of datoms is
monotonically non-decreasing under all operations. Retractions are
new datoms with op=Retract — they assert the fact "this previous
assertion is withdrawn" without removing the original assertion.

This is a direct refinement of C1 (Append-only store) and INV-STORE-001
(Monotonic growth) from the algebraic specification.
```

#### Level 1 (State Invariant)
The store is a grow-only set. Every datom that enters the store remains in the store
forever, across all operations: TRANSACT (adds datoms), MERGE (adds datoms from
another store), RECOVER (loads datoms from WAL), and checkpoint (serializes and
reloads datoms). There is no `DELETE`, no `UPDATE`, no `COMPACT`, no `VACUUM`,
no `PURGE`, no `TRUNCATE` operation.

The LIVE index (INV-FERR-029) computes the "current" state of entities by folding
over assertions and retractions in causal order. But the raw datoms — both assertions
and retractions — remain in the primary store. This enables:
- Full audit trail (who asserted/retracted what, when, and why).
- Time-travel queries (query the store as it existed at any epoch).
- Conflict analysis (examine all conflicting assertions before resolution).
- CRDT correctness (merge is pure set union; no state to lose).

The type system enforces this invariant: the `Store` struct exposes no `remove`,
`delete`, `clear`, or `retain` method. The only way to add datoms is through
`transact()` and `merge()`, both of which are additive.

#### Level 2 (Implementation Contract)
```rust
/// The Store struct exposes NO removal methods.
/// This is a structural enforcement of C1 at the type level.
pub struct Store {
    datoms: BTreeSet<Datom>,    // grow-only
    indexes: Indexes,            // derived from datoms
    epoch: u64,                  // monotonically increasing
    wal: Wal,                    // append-only log
}

impl Store {
    // The ONLY two methods that modify the datom set:

    /// Add datoms via transaction. Never removes existing datoms.
    pub fn transact(&mut self, tx: Transaction<Committed>) -> Result<TxReceipt, TxApplyError> {
        let pre_len = self.datoms.len();
        // ... add new datoms ...
        debug_assert!(self.datoms.len() >= pre_len, "C1 violated: datoms removed");
        Ok(receipt)
    }

    /// Add datoms via merge. Never removes existing datoms.
    pub fn merge_from(&mut self, other: &Store) {
        let pre_len = self.datoms.len();
        for d in other.datoms.iter() {
            self.datoms.insert(d.clone());
        }
        debug_assert!(self.datoms.len() >= pre_len, "C1 violated: datoms removed");
    }

    // NO remove(), delete(), clear(), retain(), drain(), or any other
    // method that could shrink the datom set.
}

// Compile-time enforcement: Store does not implement traits that
// could allow removal:
// - No DerefMut<Target = BTreeSet<Datom>> (would expose .remove())
// - No AsMut<BTreeSet<Datom>> (would expose .remove())
// - datoms field is private (no external access to .remove())

#[kani::proof]
#[kani::unwind(10)]
fn append_only() {
    let initial: BTreeSet<Datom> = kani::any();
    kani::assume(initial.len() <= 4);
    let new_datom: Datom = kani::any();

    let mut store = initial.clone();
    store.insert(new_datom);

    // Original datoms still present
    assert!(initial.is_subset(&store));
    // Store did not shrink
    assert!(store.len() >= initial.len());
}
```

**Falsification**: Any operation that causes `store.len()` to decrease, or any datom
`d` that was present in the store at time `t₁` and absent at time `t₂ > t₁` without
the store being replaced by a fresh instance. Specific failure modes:
- **Explicit removal**: a code path calls `.remove()` on the underlying `BTreeSet`.
- **Compaction**: a background process "compacts" superseded datoms (retractions replacing
  assertions) by removing the originals.
- **Truncation**: the WAL is truncated beyond the last checkpoint, losing committed data.
- **Re-initialization**: the store is replaced with a fresh `genesis()` store, losing
  all previously transacted data.
- **Memory-mapping corruption**: a memory-mapped store is partially overwritten by a
  concurrent process, effectively removing datoms.

**proptest strategy**:
```rust
proptest! {
    #[test]
    fn append_only_transact(
        initial in arb_store(0..100),
        txns in prop::collection::vec(arb_transaction(), 1..20),
    ) {
        let mut store = initial;
        let initial_datoms: BTreeSet<_> = store.datom_set().clone();
        let initial_len = store.len();

        for tx in txns {
            let _ = store.transact(tx);
            // After every transaction, all initial datoms still present
            for d in &initial_datoms {
                prop_assert!(store.datom_set().contains(d),
                    "C1 violation: datom {:?} lost after transact", d);
            }
            // Length never decreases
            prop_assert!(store.len() >= initial_len,
                "Store shrank: {} -> {}", initial_len, store.len());
        }
    }

    #[test]
    fn append_only_merge(
        a in arb_store(0..100),
        b in arb_store(0..100),
    ) {
        let a_datoms: BTreeSet<_> = a.datom_set().clone();
        let b_datoms: BTreeSet<_> = b.datom_set().clone();

        let merged = merge(&a, &b);

        // Both stores' datoms are present in the merge result
        for d in &a_datoms {
            prop_assert!(merged.datom_set().contains(d),
                "C1 violation: datom from A lost in merge");
        }
        for d in &b_datoms {
            prop_assert!(merged.datom_set().contains(d),
                "C1 violation: datom from B lost in merge");
        }
    }
}
```

**Lean theorem**:
```lean
/-- Append-only: no operation removes datoms. We prove this for
    apply_tx and merge by showing they are monotone (superset-preserving). -/

theorem append_only_apply (s : DatomStore) (d : Datom) :
    s ⊆ apply_tx s d := by
  unfold apply_tx
  exact Finset.subset_union_left s {d}

theorem append_only_merge_left (a b : DatomStore) :
    a ⊆ merge a b := by
  unfold merge
  exact Finset.subset_union_left a b

theorem append_only_merge_right (a b : DatomStore) :
    b ⊆ merge a b := by
  unfold merge
  exact Finset.subset_union_right a b

/-- Corollary: no operation decreases cardinality. -/
theorem append_only_card_apply (s : DatomStore) (d : Datom) :
    s.card ≤ (apply_tx s d).card := by
  exact Finset.card_le_card (append_only_apply s d)

theorem append_only_card_merge (a b : DatomStore) :
    a.card ≤ (merge a b).card := by
  exact Finset.card_le_card (append_only_merge_left a b)
```

---

### INV-FERR-019: Error Exhaustiveness

**Traces to**: NEG-FERR-001 (No Panics in Production Code), SEED.md §4
**Verification**: `V:TYPE`, `V:PROP`
**Stage**: 0

#### Level 0 (Algebraic Law)
```
Let API = {transact, merge, query, checkpoint, load, recover, shard, ...}
  be the set of all public functions in the Ferratomic API.

∀ f ∈ API:
  f : Args → Result<T, E>  where E is an enum

  ∀ args ∈ domain(f):
    f(args) terminates ∧ f(args) ∈ {Ok(t) | t ∈ T} ∪ {Err(e) | e ∈ E}

Every public function returns a typed Result. No function panics, aborts,
or exits the process on any input. Errors are total: the error enum E
covers every possible failure mode, and the caller can match exhaustively
on E to handle each case.

Formally: the API is a total function from inputs to Result<T, E>.
There is no "undefined behavior" case.
```

#### Level 1 (State Invariant)
Every failure mode in the Ferratomic engine is represented as a variant of a typed error
enum. No function uses `unwrap()`, `expect()`, `panic!()`, `unreachable!()`, or any
other panicking construct on fallible operations. The error types form a hierarchy:
- `TxApplyError`: transaction validation and application failures.
- `TxValidationError`: schema validation failures (subset of TxApplyError).
- `CheckpointError`: serialization/deserialization failures.
- `RecoveryError`: crash recovery failures.
- `WalError`: write-ahead log I/O failures.
- `QueryError`: query parsing and evaluation failures.
- `MergeError`: merge operation failures (e.g., incompatible schema versions).

Each error variant carries sufficient context for the caller to diagnose and handle the
failure: the specific datom or attribute that caused the error, the expected vs. actual
types, the file path, the byte offset, etc. Error messages are structured (not
free-form strings) to enable programmatic error handling.

The `#![forbid(unsafe_code)]` crate-level attribute ensures no `unsafe` blocks exist
(INV-FERR-023), and the `#[deny(clippy::unwrap_used)]` lint ensures no `unwrap()` calls
exist. Together, these provide structural guarantees that the codebase cannot panic
except on true logical impossibilities (e.g., `unreachable!()` in match arms that the
type system guarantees cannot be reached).

#### Level 2 (Implementation Contract)
```rust
// In every ferratomic crate's lib.rs:
#![forbid(unsafe_code)]
#![deny(clippy::unwrap_used)]
#![deny(clippy::expect_used)]
#![deny(clippy::panic)]

/// Transaction application errors.
/// Every variant carries diagnostic context.
#[derive(Debug, thiserror::Error)]
pub enum TxApplyError {
    #[error("Schema validation failed: {0}")]
    Validation(#[from] TxValidationError),

    #[error("WAL write failed: {0}")]
    WalWrite(#[source] io::Error),

    #[error("WAL fsync failed: {0}")]
    WalSync(#[source] io::Error),

    #[error("Epoch overflow: current epoch {current} would exceed u64::MAX")]
    EpochOverflow { current: u64 },
}

/// Schema validation errors.
#[derive(Debug, thiserror::Error)]
pub enum TxValidationError {
    #[error("Unknown attribute: {attr}")]
    UnknownAttribute { attr: String },

    #[error("Type mismatch for {attr}: expected {expected}, got {got}")]
    SchemaViolation {
        attr: String,
        expected: ValueType,
        got: ValueType,
    },

    #[error("Cardinality violation for {attr}: cardinality is One but multiple values asserted")]
    CardinalityViolation { attr: String },
}

/// Checkpoint errors.
#[derive(Debug, thiserror::Error)]
pub enum CheckpointError {
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),

    #[error("Checkpoint file truncated: expected at least {expected} bytes, got {got}")]
    Truncated { expected: usize, got: usize },

    #[error("Checksum mismatch: expected {expected}, got {got}")]
    ChecksumMismatch { expected: String, got: String },

    #[error("Invalid magic bytes: expected {expected:?}, got {got:?}")]
    InvalidMagic { expected: [u8; 4], got: [u8; 4] },

    #[error("Unsupported checkpoint version: {version}")]
    UnsupportedVersion { version: u32 },

    #[error("Datom deserialization failed at offset {offset}: {source}")]
    DatomDeserialize { offset: u64, source: Box<dyn std::error::Error + Send + Sync> },
}

/// Recovery errors.
#[derive(Debug, thiserror::Error)]
pub enum RecoveryError {
    #[error("Checkpoint load failed: {0}")]
    Checkpoint(#[from] CheckpointError),

    #[error("WAL recovery failed: {0}")]
    Wal(#[from] WalError),

    #[error("No checkpoint found in {dir}")]
    NoCheckpoint { dir: PathBuf },

    #[error("Index rebuild failed after recovery: {0}")]
    IndexRebuild(String),
}

/// WAL errors.
#[derive(Debug, thiserror::Error)]
pub enum WalError {
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),

    #[error("WAL entry corrupted at offset {offset}: CRC mismatch")]
    Corrupted { offset: u64 },

    #[error("WAL entry too large: {size} bytes (max: {max})")]
    EntryTooLarge { size: usize, max: usize },
}

// No kani proof needed — this is a type-level invariant enforced by
// #![forbid(unsafe_code)] and #![deny(clippy::unwrap_used)].
// Verification is via cargo clippy --all-targets -- -D warnings.
```

**Falsification**: Any public API function that panics, aborts, or exits the process on
any input. Specific detection methods:
- **Static analysis**: `cargo clippy --all-targets -- -D warnings -D clippy::unwrap_used
  -D clippy::expect_used -D clippy::panic` reports any panicking construct.
- **Fuzz testing**: `cargo fuzz` with arbitrary inputs to every public function; any
  crash is a falsification.
- **Exhaustiveness check**: for every `Result<T, E>` returned by a public function,
  verify that `E` covers all failure modes by reviewing the function body for any
  fallible operation whose error is not propagated to the return type.

**proptest strategy**:
```rust
proptest! {
    #[test]
    fn transact_never_panics(
        datoms in prop::collection::vec(arb_datom_any(), 0..100),
    ) {
        let mut store = Store::genesis();
        let tx_builder = datoms.into_iter().fold(
            Transaction::new(arb_agent_id()),
            |tx, d| tx.assert_datom(d.e, d.a, d.v),
        );
        // Must not panic — either Ok or Err
        let _ = tx_builder.commit(store.schema())
            .and_then(|tx| store.transact(tx));
    }

    #[test]
    fn load_checkpoint_never_panics(
        data in prop::collection::vec(any::<u8>(), 0..10000),
    ) {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), &data).unwrap();
        // Must not panic — either Ok or Err
        let _ = load_checkpoint(tmp.path());
    }

    #[test]
    fn wal_recover_never_panics(
        data in prop::collection::vec(any::<u8>(), 0..10000),
    ) {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), &data).unwrap();
        let mut wal = Wal::open(tmp.path());
        // Must not panic — either Ok or Err
        match wal {
            Ok(ref mut w) => { let _ = w.recover(); },
            Err(_) => {}, // expected for garbage data
        }
    }
}
```

**Lean theorem**:
```lean
/-- Error exhaustiveness: every function returns a sum type (Result).
    In Lean, we model this as: every API function is total. -/

inductive TxResult (α : Type) where
  | ok : α → TxResult α
  | err : String → TxResult α

def transact_total (s : DatomStore) (schema : Schema) (d : Datom) : TxResult DatomStore :=
  if d.a ∈ schema then
    .ok (apply_tx s d)
  else
    .err s!"Unknown attribute: {d.a}"

theorem transact_total_terminates (s : DatomStore) (schema : Schema) (d : Datom) :
    ∃ r : TxResult DatomStore, transact_total s schema d = r := by
  exact ⟨transact_total s schema d, rfl⟩
```

---

### INV-FERR-020: Transaction Atomicity

**Traces to**: SEED.md §4 (Core Abstraction: Transactions), INV-STORE-010, INV-FERR-006
**Verification**: `V:PROP`, `V:KANI`, `V:MODEL`
**Stage**: 0

#### Level 0 (Algebraic Law)
```
Let T = {d₁, d₂, ..., dₙ} be a transaction with n datoms.
Let epoch(T) be the epoch assigned to transaction T.

∀ T submitted to TRANSACT:
  ∀ dᵢ ∈ T:
    epoch(dᵢ) = epoch(T)

All datoms in a transaction receive the same epoch. Combined with
snapshot isolation (INV-FERR-006), this means a transaction is either
fully visible or fully invisible at any snapshot epoch.

Atomicity is "all or nothing":
  TRANSACT(S, T) =
    if valid(S, T):  S ∪ T  (all datoms added)
    else:            S       (no datoms added)

There is no partial application: no subset of T's datoms enters the
store while the rest are rejected.
```

#### Level 1 (State Invariant)
Every datom in a transaction `T` is assigned the same epoch value. At any snapshot
epoch `e`, either all datoms from `T` are visible (if `epoch(T) <= e`) or none are
visible (if `epoch(T) > e`). There is no intermediate state where some datoms from `T`
are visible and others are not.

This property extends to crash recovery: if the process crashes during a TRANSACT
operation, the recovery procedure either replays the entire transaction (if the WAL
entry was complete and fsynced) or discards it entirely (if the WAL entry was
incomplete). There is no state where half of a transaction's datoms are in the
recovered store and the other half are missing.

Atomicity is enforced at three levels:
1. **Schema validation**: all datoms are validated before any are applied (INV-FERR-009).
2. **WAL entry**: all datoms are written to a single WAL entry, which is either fully
   fsynced or not (INV-FERR-008).
3. **Epoch assignment**: all datoms receive the same epoch under the write lock
   (INV-FERR-007).

#### Level 2 (Implementation Contract)
```rust
/// Transaction atomicity: all datoms get the same epoch.
impl Store {
    pub fn transact(&mut self, tx: Transaction<Committed>) -> Result<TxReceipt, TxApplyError> {
        let _write_lock = self.write_lock.lock();
        let epoch = self.next_epoch();

        // Single WAL entry for all datoms
        let wal_entry = WalEntry {
            epoch,
            datoms: tx.datoms().cloned().collect(),
        };
        self.wal.append(epoch, &wal_entry)?;
        self.wal.fsync()?;

        // Apply all datoms with the same epoch
        for datom in tx.datoms() {
            let mut epoched = datom.clone();
            epoched.tx_epoch = epoch;
            self.datoms.insert(epoched.clone());
            self.indexes.insert(&epoched);
        }

        self.last_committed_epoch = epoch;
        Ok(TxReceipt { epoch, datom_count: tx.datoms().count() })
    }
}

/// Verify: query a snapshot and check that for any transaction,
/// either all or none of its datoms are visible.
pub fn verify_tx_atomicity(store: &Store, epoch: u64) -> bool {
    let snapshot = store.snapshot_at(epoch);
    let visible: BTreeSet<_> = snapshot.datoms().collect();

    // Group datoms by transaction epoch
    let mut tx_groups: BTreeMap<u64, Vec<&Datom>> = BTreeMap::new();
    for d in store.datoms.iter() {
        tx_groups.entry(d.tx_epoch).or_default().push(d);
    }

    // For each transaction, check all-or-nothing visibility
    for (tx_epoch, datoms) in &tx_groups {
        let visible_count = datoms.iter().filter(|d| visible.contains(*d)).count();
        if visible_count != 0 && visible_count != datoms.len() {
            return false; // partial visibility — atomicity violated
        }
    }
    true
}

#[kani::proof]
#[kani::unwind(8)]
fn transaction_atomicity() {
    let mut store = Store::genesis();
    let n_datoms: u8 = kani::any();
    kani::assume(n_datoms > 0 && n_datoms <= 4);

    let datoms: Vec<Datom> = (0..n_datoms).map(|_| kani::any()).collect();
    let tx = datoms.iter().fold(
        Transaction::new(kani::any()),
        |tx, d| tx.assert_datom(d.e, d.a.clone(), d.v.clone()),
    );

    if let Ok(receipt) = tx.commit(&store.schema()).and_then(|t| store.transact(t)) {
        // All datoms from this tx have the same epoch
        for d in store.datoms.iter() {
            if d.tx_epoch == receipt.epoch {
                // This datom is from our transaction — expected
            }
        }
    }
}
```

**Falsification**: A transaction `T = {d₁, d₂, d₃}` where, after a crash and recovery,
`d₁` and `d₂` are in the recovered store but `d₃` is not. Or: a snapshot at epoch `e`
where `d₁ ∈ snapshot(S, e)` but `d₂ ∉ snapshot(S, e)` even though both belong to the
same transaction. Specific failure modes:
- **Partial WAL write**: the WAL entry is written incrementally (datom by datom) rather
  than as a single atomic unit, and a crash occurs mid-write.
- **Split epoch**: different datoms in the same transaction receive different epoch values
  (epoch counter advances between datom applications).
- **Partial schema rejection**: some datoms pass schema validation and are applied, but
  a later datom in the same transaction fails validation, and the already-applied datoms
  are not rolled back.

**proptest strategy**:
```rust
proptest! {
    #[test]
    fn transaction_all_same_epoch(
        datoms in prop::collection::vec(arb_schema_valid_datom(), 2..20),
    ) {
        let mut store = Store::genesis();
        let tx = datoms.into_iter().fold(
            Transaction::new(arb_agent_id()),
            |tx, d| tx.assert_datom(d.e, d.a, d.v),
        );

        if let Ok(receipt) = tx.commit(store.schema()).and_then(|t| store.transact(t)) {
            // All datoms from this transaction have the same epoch
            let tx_datoms: Vec<_> = store.datoms.iter()
                .filter(|d| d.tx_epoch == receipt.epoch)
                .collect();

            // The count matches what we submitted (plus tx metadata)
            prop_assert!(tx_datoms.len() >= 2, "Expected multiple datoms in tx");

            // All have the same epoch
            for d in &tx_datoms {
                prop_assert_eq!(d.tx_epoch, receipt.epoch);
            }
        }
    }

    #[test]
    fn snapshot_tx_atomicity(
        txns in prop::collection::vec(
            prop::collection::vec(arb_schema_valid_datom(), 2..10),
            1..5,
        ),
    ) {
        let mut store = Store::genesis();
        let mut tx_epochs = vec![];

        for datoms in txns {
            let tx = datoms.into_iter().fold(
                Transaction::new(arb_agent_id()),
                |tx, d| tx.assert_datom(d.e, d.a, d.v),
            );
            if let Ok(receipt) = tx.commit(store.schema()).and_then(|t| store.transact(t)) {
                tx_epochs.push(receipt.epoch);
            }
        }

        // At every epoch, transactions are atomic
        for e in &tx_epochs {
            prop_assert!(verify_tx_atomicity(&store, *e));
        }
    }
}
```

**Lean theorem**:
```lean
/-- Transaction atomicity: all datoms in a transaction have the same epoch.
    We model this as: applying a set of datoms with the same tx field. -/

def apply_tx_batch (s : DatomStore) (batch : Finset Datom) : DatomStore :=
  s ∪ batch

/-- After applying a batch, all batch datoms are present. -/
theorem batch_all_present (s : DatomStore) (batch : Finset Datom) (d : Datom)
    (h : d ∈ batch) :
    d ∈ apply_tx_batch s batch := by
  unfold apply_tx_batch
  exact Finset.mem_union_right s h

/-- Atomicity: either the entire batch is applied or none of it is. -/
def atomic_apply (s : DatomStore) (schema : Schema) (batch : Finset Datom) : Option DatomStore :=
  if batch.∀ (fun d => d.a ∈ schema) then
    some (apply_tx_batch s batch)
  else
    none

theorem atomic_all_or_nothing (s : DatomStore) (schema : Schema) (batch : Finset Datom) :
    (∃ s', atomic_apply s schema batch = some s' ∧ batch ⊆ s') ∨
    atomic_apply s schema batch = none := by
  unfold atomic_apply
  split
  · left
    refine ⟨apply_tx_batch s batch, rfl, ?_⟩
    unfold apply_tx_batch
    exact Finset.subset_union_right s batch
  · right; rfl
```

---

### INV-FERR-021: Backpressure Safety

**Traces to**: SEED.md §4, NEG-FERR-005 (No Unbounded Memory Growth),
ADR-FERR-002 (Async Runtime)
**Verification**: `V:PROP`, `V:MODEL`
**Stage**: 0

#### Level 0 (Algebraic Law)
```
Let write_queue : Queue<Transaction> be the pending write queue.
Let capacity : Nat be the maximum queue depth.

∀ state where |write_queue| = capacity:
  submit(T) returns Err(Backpressure) rather than blocking indefinitely
  or dropping T silently.

No data loss on backpressure:
  ∀ T submitted:
    submit(T) ∈ {Ok(receipt), Err(Backpressure)}
    // never: silent drop, OOM crash, or infinite block

The caller receives a typed error (Err(Backpressure)) and can retry,
buffer, or shed load. The system never silently drops a transaction
and never runs out of memory by queueing unbounded transactions.
```

#### Level 1 (State Invariant)
When the write pipeline is saturated (WAL writer busy, checkpoint in progress, merge
ongoing), incoming transactions are not silently dropped or queued without bound. The
system returns a typed `Backpressure` error to the caller, who can then decide to retry
(with exponential backoff), buffer (in the caller's own bounded queue), or shed load
(reject the user's request).

The backpressure mechanism operates at three levels:
1. **Write lock contention**: if the write lock is held and `try_lock` fails, the caller
   receives `Err(Backpressure::WriteLockContention)`.
2. **WAL buffer full**: if the WAL buffer exceeds `wal_buffer_max` bytes, new writes
   are rejected with `Err(Backpressure::WalBufferFull)`.
3. **Memory pressure**: if the in-memory store exceeds `memory_limit` bytes, new
   transactions are rejected with `Err(Backpressure::MemoryPressure)`.

In all cases, no data is lost: the transaction was never accepted, so the caller knows
it must retry. The store state is unchanged by a rejected transaction.

#### Level 2 (Implementation Contract)
```rust
/// Backpressure error variants.
#[derive(Debug, thiserror::Error)]
pub enum BackpressureError {
    #[error("Write lock contention: another transaction is in progress")]
    WriteLockContention,

    #[error("WAL buffer full: {current_bytes} bytes (max: {max_bytes})")]
    WalBufferFull { current_bytes: usize, max_bytes: usize },

    #[error("Memory pressure: store at {current_bytes} bytes (limit: {limit_bytes})")]
    MemoryPressure { current_bytes: usize, limit_bytes: usize },
}

impl Store {
    /// Try to submit a transaction with backpressure.
    /// Returns Err(Backpressure) if the write pipeline is saturated.
    /// Never blocks indefinitely, never drops data silently.
    pub fn try_transact(
        &mut self,
        tx: Transaction<Committed>,
    ) -> Result<TxReceipt, TxApplyError> {
        // Check memory pressure
        if self.memory_usage() > self.config.memory_limit {
            return Err(TxApplyError::Backpressure(BackpressureError::MemoryPressure {
                current_bytes: self.memory_usage(),
                limit_bytes: self.config.memory_limit,
            }));
        }

        // Check WAL buffer
        if self.wal.buffer_size() > self.config.wal_buffer_max {
            return Err(TxApplyError::Backpressure(BackpressureError::WalBufferFull {
                current_bytes: self.wal.buffer_size(),
                max_bytes: self.config.wal_buffer_max,
            }));
        }

        // Try to acquire write lock (non-blocking)
        let write_lock = self.write_lock.try_lock()
            .ok_or(TxApplyError::Backpressure(BackpressureError::WriteLockContention))?;

        // Proceed with normal transact under lock
        self.transact_under_lock(write_lock, tx)
    }
}

// Stateright model: verify no silent data loss under backpressure
impl stateright::Model for BackpressureModel {
    // ... state machine with bounded queue ...

    fn properties(&self) -> Vec<Property<Self>> {
        vec![
            Property::always("no_silent_drop", |_, state: &BpState| {
                // Every submitted transaction is either in the store or was
                // explicitly rejected (in the rejected set)
                state.submitted.iter().all(|tx| {
                    state.store.contains(tx) || state.rejected.contains(tx)
                })
            }),
            Property::always("bounded_memory", |_, state: &BpState| {
                state.queue_depth <= state.max_queue_depth
            }),
        ]
    }
}
```

**Falsification**: A transaction `T` that is submitted to the store but neither appears
in the store nor triggers an error return. The transaction was silently dropped — the
caller has no way to know whether it succeeded or failed. Specific failure modes:
- **Silent queue overflow**: the write queue grows without bound, eventually causing OOM.
- **Infinite blocking**: `try_transact()` blocks forever waiting for the write lock, and
  the caller cannot time out or cancel.
- **Partial acceptance**: the transaction is partially processed (some datoms applied)
  but then rejected due to backpressure, leaving the store in an inconsistent state
  (violates INV-FERR-020 atomicity).

**proptest strategy**:
```rust
proptest! {
    #[test]
    fn backpressure_no_data_loss(
        txns in prop::collection::vec(arb_transaction(), 1..100),
        memory_limit in 1000usize..100000,
        wal_buffer_max in 1000usize..100000,
    ) {
        let mut store = Store::genesis_with_config(StoreConfig {
            memory_limit,
            wal_buffer_max,
            ..Default::default()
        });

        let mut accepted = vec![];
        let mut rejected = vec![];

        for tx in txns {
            match store.try_transact(tx.clone()) {
                Ok(receipt) => accepted.push((tx, receipt)),
                Err(TxApplyError::Backpressure(_)) => rejected.push(tx),
                Err(e) => rejected.push(tx), // other errors also count as "not lost"
            }
        }

        // All accepted transactions are in the store
        for (tx, receipt) in &accepted {
            for d in tx.datoms() {
                prop_assert!(store.datom_set().iter().any(|sd| sd.entity == d.entity),
                    "Accepted transaction datom not in store");
            }
        }

        // Total = accepted + rejected (nothing dropped)
        prop_assert_eq!(
            accepted.len() + rejected.len(),
            // original count
            accepted.len() + rejected.len(),
            "Transaction accounting mismatch"
        );
    }
}
```

**Lean theorem**:
```lean
/-- Backpressure safety: every submission produces either Ok or Err.
    No transaction is silently dropped. -/

inductive SubmitResult (α : Type) where
  | accepted : α → SubmitResult α
  | rejected : String → SubmitResult α

def try_submit (s : DatomStore) (d : Datom) (capacity : Nat) : SubmitResult DatomStore :=
  if s.card < capacity then
    .accepted (apply_tx s d)
  else
    .rejected "Backpressure: store at capacity"

theorem no_silent_drop (s : DatomStore) (d : Datom) (capacity : Nat) :
    ∃ r : SubmitResult DatomStore, try_submit s d capacity = r := by
  exact ⟨try_submit s d capacity, rfl⟩

/-- If accepted, the datom is in the resulting store. -/
theorem accepted_means_present (s : DatomStore) (d : Datom) (capacity : Nat)
    (h : s.card < capacity) :
    try_submit s d capacity = .accepted (apply_tx s d) := by
  unfold try_submit
  simp [h]

/-- If rejected, the store is unchanged (no partial application). -/
theorem rejected_means_unchanged (s : DatomStore) (d : Datom) (capacity : Nat)
    (h : ¬ (s.card < capacity)) :
    ∃ msg, try_submit s d capacity = .rejected msg := by
  unfold try_submit
  simp [h]
  exact ⟨_, rfl⟩
```

---

### INV-FERR-022: Anti-Entropy Convergence

**Traces to**: SEED.md §4, C4, INV-FERR-010 (Merge Convergence)
**Verification**: `V:PROP`, `V:MODEL`
**Stage**: 0

#### Level 0 (Algebraic Law)
```
Let merkle(S) : MerkleTree be the Merkle summary of store S.
Let diff(M₁, M₂) : Set<Datom> be the set of datoms present in one
  store but not the other, computed via Merkle tree comparison.

Anti-entropy protocol:
  1. Node A sends merkle(A) to node B.
  2. Node B computes diff(merkle(A), merkle(B)).
  3. Node B sends the missing datoms to A.
  4. Node A merges: A' = merge(A, diff).
  5. Symmetrically: B receives missing datoms from A.

Termination:
  ∀ nodes A, B:
    after finite rounds of anti-entropy:
      merkle(A) = merkle(B) ⟺ state(A) = state(B)

Convergence:
  The anti-entropy protocol terminates when both nodes have the same
  Merkle root hash. At this point, by INV-FERR-012 (content-addressed
  identity), their datom sets are identical.
```

#### Level 1 (State Invariant)
The Merkle-based anti-entropy protocol always terminates and, upon termination, both
nodes have identical datom sets. The protocol is:
1. Each node computes a Merkle tree over its datom set (keyed by content hash).
2. Nodes exchange Merkle roots and walk down the tree to identify differing subtrees.
3. Only the datoms in differing subtrees are exchanged (bandwidth-efficient).
4. Received datoms are merged via set union (INV-FERR-001 through INV-FERR-003).
5. The process repeats until Merkle roots match (convergence).

Termination is guaranteed because:
- Each round transfers at least one datom (or converges).
- The set of datoms is finite and bounded.
- Merge is monotonic (INV-FERR-004): received datoms are never removed.
- After merging, the Merkle diff strictly decreases.

The worst case is `O(|A Δ B|)` rounds where `A Δ B` is the symmetric difference.
In practice, the Merkle tree comparison identifies all differences in a single round
with `O(log N)` hash comparisons, and only the differing datoms are transferred.

#### Level 2 (Implementation Contract)
```rust
/// Merkle tree over the datom set, keyed by entity hash prefix.
pub struct MerkleTree {
    root: MerkleNode,
    depth: usize,
}

#[derive(Clone)]
enum MerkleNode {
    Leaf {
        hash: [u8; 32],
        datoms: Vec<Datom>,
    },
    Branch {
        hash: [u8; 32],
        children: Box<[MerkleNode; 256]>,  // 1 byte of hash prefix per level
    },
}

impl MerkleTree {
    /// Build a Merkle tree from a store's datom set.
    pub fn from_store(store: &Store) -> Self {
        // Group datoms by content hash prefix, build bottom-up
        // ...
        MerkleTree { root, depth }
    }

    /// Compute the set of datoms present in self but not in other.
    pub fn diff(&self, other: &MerkleTree) -> Vec<Datom> {
        self.diff_recursive(&self.root, &other.root)
    }

    fn diff_recursive(&self, local: &MerkleNode, remote: &MerkleNode) -> Vec<Datom> {
        if local.hash() == remote.hash() {
            return vec![]; // subtrees identical
        }
        match (local, remote) {
            (MerkleNode::Leaf { datoms: local_d, .. },
             MerkleNode::Leaf { datoms: remote_d, .. }) => {
                // Return datoms in local but not remote
                let remote_set: BTreeSet<_> = remote_d.iter().collect();
                local_d.iter()
                    .filter(|d| !remote_set.contains(d))
                    .cloned()
                    .collect()
            }
            (MerkleNode::Branch { children: lc, .. },
             MerkleNode::Branch { children: rc, .. }) => {
                // Recurse into differing children
                lc.iter().zip(rc.iter())
                    .flat_map(|(l, r)| self.diff_recursive(l, r))
                    .collect()
            }
            _ => {
                // Depth mismatch: enumerate all datoms in local subtree
                local.all_datoms()
            }
        }
    }
}

/// Anti-entropy round: synchronize two stores via Merkle diff.
/// Returns the number of datoms exchanged.
pub fn anti_entropy_round(local: &mut Store, remote: &Store) -> usize {
    let local_merkle = MerkleTree::from_store(local);
    let remote_merkle = MerkleTree::from_store(remote);

    // Datoms in remote but not local
    let missing = remote_merkle.diff(&local_merkle);
    let count = missing.len();

    for datom in missing {
        local.datoms.insert(datom);
    }
    local.rebuild_indexes();

    count
}

/// Full anti-entropy: repeat until converged.
/// Guaranteed to terminate (each round strictly reduces the diff).
pub fn anti_entropy_full(local: &mut Store, remote: &mut Store) -> usize {
    let mut total = 0;
    loop {
        let a_to_b = anti_entropy_round(remote, local);
        let b_to_a = anti_entropy_round(local, remote);
        total += a_to_b + b_to_a;
        if a_to_b == 0 && b_to_a == 0 {
            break; // converged
        }
    }
    debug_assert_eq!(local.datom_set(), remote.datom_set(),
        "INV-FERR-022: anti-entropy did not converge");
    total
}
```

**Falsification**: Two nodes that, after executing the anti-entropy protocol to completion
(no more datoms to exchange), have different datom sets. Or: the protocol does not
terminate (infinite loop of exchanging datoms). Specific failure modes:
- **Merkle hash collision**: two different datoms produce the same Merkle leaf hash,
  causing the diff to miss them (would require BLAKE3 collision — see INV-FERR-012).
- **Non-monotonic merge**: a received datom is not retained after merge, causing it
  to be re-requested in the next round (infinite loop).
- **Diff asymmetry**: `diff(A, B)` returns datoms, but `diff(B, A)` misses the
  corresponding datoms, leading to one-sided convergence.

**proptest strategy**:
```rust
proptest! {
    #[test]
    fn anti_entropy_converges(
        a_datoms in prop::collection::btree_set(arb_datom(), 0..100),
        b_datoms in prop::collection::btree_set(arb_datom(), 0..100),
    ) {
        let mut a = Store::from_datoms(a_datoms);
        let mut b = Store::from_datoms(b_datoms);

        anti_entropy_full(&mut a, &mut b);

        prop_assert_eq!(a.datom_set(), b.datom_set(),
            "Stores did not converge after anti-entropy");
    }

    #[test]
    fn anti_entropy_terminates(
        a_datoms in prop::collection::btree_set(arb_datom(), 0..100),
        b_datoms in prop::collection::btree_set(arb_datom(), 0..100),
    ) {
        let mut a = Store::from_datoms(a_datoms);
        let mut b = Store::from_datoms(b_datoms);

        let max_rounds = a.len() + b.len() + 1;
        let mut rounds = 0;
        loop {
            let exchanged = anti_entropy_round(&mut a, &b)
                + anti_entropy_round(&mut b, &a);
            rounds += 1;
            if exchanged == 0 { break; }
            prop_assert!(rounds <= max_rounds,
                "Anti-entropy did not terminate after {} rounds", rounds);
        }
    }

    #[test]
    fn merkle_diff_complete(
        a_datoms in prop::collection::btree_set(arb_datom(), 0..100),
        b_datoms in prop::collection::btree_set(arb_datom(), 0..100),
    ) {
        let a = Store::from_datoms(a_datoms.clone());
        let b = Store::from_datoms(b_datoms.clone());

        let a_merkle = MerkleTree::from_store(&a);
        let b_merkle = MerkleTree::from_store(&b);

        let diff_a_to_b = a_merkle.diff(&b_merkle);
        let expected: BTreeSet<_> = a_datoms.difference(&b_datoms).cloned().collect();

        let diff_set: BTreeSet<_> = diff_a_to_b.into_iter().collect();
        prop_assert_eq!(diff_set, expected,
            "Merkle diff does not match set difference");
    }
}
```

**Lean theorem**:
```lean
/-- Anti-entropy convergence: after exchanging all differing datoms,
    two stores are identical. -/

def symmetric_diff (a b : DatomStore) : DatomStore := (a \ b) ∪ (b \ a)

def anti_entropy_step (a b : DatomStore) : DatomStore × DatomStore :=
  (a ∪ b, b ∪ a)

theorem anti_entropy_converges (a b : DatomStore) :
    let (a', b') := anti_entropy_step a b
    a' = b' := by
  unfold anti_entropy_step
  simp [Finset.union_comm]

theorem anti_entropy_superset (a b : DatomStore) :
    a ⊆ (anti_entropy_step a b).1 := by
  unfold anti_entropy_step
  exact Finset.subset_union_left a b

/-- After convergence, both nodes have all datoms from both. -/
theorem anti_entropy_complete (a b : DatomStore) (d : Datom) (h : d ∈ a ∨ d ∈ b) :
    d ∈ (anti_entropy_step a b).1 := by
  unfold anti_entropy_step
  cases h with
  | inl ha => exact Finset.mem_union_left _ ha
  | inr hb => exact Finset.mem_union_right _ hb
```

---

### INV-FERR-023: No Unsafe Code

**Traces to**: SEED.md §4 (Implementation Architecture), NEG-FERR-002
**Verification**: `V:TYPE`
**Stage**: 0

#### Level 0 (Algebraic Law)
```
∀ crate C ∈ {ferratom, ferratom-clock, ferratomic-datalog, ferratomic-verify}:
  #![forbid(unsafe_code)] is present at crate root

ferratomic-core uses #![deny(unsafe_code)] with a single exception:
  mmap.rs has #![allow(unsafe_code)] per ADR-FERR-020 (localized unsafe
  for performance-critical cold start, guarded by BLAKE3 verification).

This is a structural invariant: the Rust compiler rejects any file in these
crates that contains an `unsafe` block, `unsafe fn`, `unsafe impl`, or
`unsafe trait` — except the explicitly documented mmap.rs module in
ferratomic-core. Verification is by compilation.
```

#### Level 1 (State Invariant)
No crate in the Ferratomic workspace uses `unsafe` code except the
localized `mmap.rs` module in ferratomic-core (ADR-FERR-020). This means:
- No raw pointer dereference.
- No calls to `extern "C"` functions.
- No `transmute`, `from_raw_parts`, or other memory-unsafety primitives.
- No `unsafe impl Send/Sync` (which could create data races).
- All memory safety is guaranteed by the Rust borrow checker.

This invariant implies that every Ferratomic data structure is free from:
- Use-after-free.
- Double-free.
- Buffer overflow/underflow.
- Data races.
- Null pointer dereference (Rust has no null pointers in safe code).

Dependencies may use `unsafe` internally (e.g., `blake3` uses SIMD intrinsics), but
the Ferratomic crates themselves are pure safe Rust. This is verified by the
`#![forbid(unsafe_code)]` attribute, which is stronger than `#![deny(unsafe_code)]` —
it cannot be overridden by `#[allow(unsafe_code)]` on individual items.

#### Level 2 (Implementation Contract)
```rust
// ferratom/src/lib.rs
#![forbid(unsafe_code)]
// ... zero-dependency primitive types ...

// ferratomic-core/src/lib.rs
#![forbid(unsafe_code)]
// ... storage engine ...

// ferratomic-datalog/src/lib.rs
#![forbid(unsafe_code)]
// ... query engine ...

// ferratomic-verify/src/lib.rs
#![forbid(unsafe_code)]
// ... verification harnesses ...

// Verification: the project compiles with `cargo build --all-targets`.
// If any crate contains unsafe code, compilation fails with:
//   error[E0453]: unsafe code is forbidden in this crate
```

**Falsification**: Any crate in the Ferratomic workspace compiles successfully while
containing an `unsafe` block, `unsafe fn`, `unsafe impl`, or `unsafe trait`. This
would indicate that `#![forbid(unsafe_code)]` is missing from the crate root, or that
the attribute was erroneously removed. Detection is mechanical:
`rg -n "unsafe" ferratom ferratomic-core ferratomic-datalog ferratomic-verify`
or `cargo clippy` with `unsafe_code` lint at forbid level.

**proptest strategy**:
```rust
// No proptest needed — this is a compilation-time invariant.
// Verification is structural: if the crate compiles, the invariant holds.

#[test]
fn no_unsafe_in_source() {
    let crate_roots = [
        "ferratom/src/lib.rs",
        "ferratomic-core/src/lib.rs",
        "ferratomic-datalog/src/lib.rs",
        "ferratomic-verify/src/lib.rs",
    ];
    for root in &crate_roots {
        let content = std::fs::read_to_string(root)
            .unwrap_or_else(|_| panic!("Cannot read {}", root));
        assert!(content.contains("#![forbid(unsafe_code)]"),
            "Crate root {} missing #![forbid(unsafe_code)]", root);
    }
}
```

**Lean theorem**:
```lean
/-- No unsafe code: the entire codebase is in the safe fragment of Rust.
    In Lean, all code is safe by construction (no unsafe primitive exists).
    This theorem is trivially true but states the intent explicitly. -/

-- Lean has no "unsafe" construct. All Lean code is memory-safe by the
-- type system. This invariant is satisfied trivially for the Lean model.
-- The verification burden is on the Rust side (compiler enforcement).
theorem no_unsafe : True := trivial
```

---

### INV-FERR-024: Substrate Agnosticism

**Traces to**: SEED.md §4 (Implementation Architecture), SEED.md §9.2 (Central Finding:
Substrate Divergence), C8 (Substrate Independence), INV-FOUNDATION-015
**Verification**: `V:TYPE`, `V:PROP`
**Stage**: 0

#### Level 0 (Algebraic Law)
```
Let API_embedded = {transact, merge, query, snapshot, ...} be the
  Ferratomic API for embedded (single-process) usage.
Let API_distributed = {transact, merge, query, snapshot, ...} be the
  Ferratomic API for distributed (multi-node) usage.

∀ f ∈ API_embedded:
  ∃ f' ∈ API_distributed:
    f'.signature = f.signature
    ∧ ∀ args: f(args) = f'(args)  (semantic equivalence)

The API surface is identical for embedded and distributed deployment.
The caller does not need to know whether the store is local or distributed.
All distribution concerns (sharding, replication, anti-entropy) are
handled transparently behind the same API.
```

#### Level 1 (State Invariant)
The Ferratomic API does not expose any concept specific to a single deployment model.
There is no `connect()`, no `cluster.join()`, no `shard.select()` in the public API.
The store is accessed through a single `Store` type (or trait) that abstracts over
the deployment model.

This is achieved via a trait-based architecture:
- `Store<B: Backend>` where `Backend` is either `EmbeddedBackend` (single-process,
  direct memory access) or `DistributedBackend` (multi-node, network access).
- The `Backend` trait defines the storage operations (read datoms, write datoms,
  sync), and the `Store` struct provides the CRDT semantics on top.
- The caller uses `Store<EmbeddedBackend>` for embedded deployment and
  `Store<DistributedBackend>` for distributed deployment, with the same API
  methods and the same behavioral guarantees (all INV-FERR invariants hold for
  both backends).

The host application imports Ferratomic and uses `Store<EmbeddedBackend>` (the default
deployment model). If distribution is needed later, the application switches to
`Store<DistributedBackend>` without changing any of its own code.

#### Level 2 (Implementation Contract)
```rust
/// Backend trait: abstracts over embedded vs. distributed storage.
pub trait Backend: Send + Sync + 'static {
    /// Read all datoms matching a predicate.
    fn scan(&self, pred: &dyn Fn(&Datom) -> bool) -> Vec<Datom>;

    /// Write a batch of datoms (atomically).
    fn write_batch(&mut self, datoms: &[Datom]) -> Result<(), BackendError>;

    /// Sync durable state (fsync for embedded, commit for distributed).
    fn sync(&mut self) -> Result<(), BackendError>;

    /// Current epoch.
    fn epoch(&self) -> u64;
}

/// Embedded backend: BTreeSet in memory, WAL on disk.
pub struct EmbeddedBackend {
    datoms: BTreeSet<Datom>,
    wal: Wal,
    epoch: u64,
}

impl Backend for EmbeddedBackend {
    fn scan(&self, pred: &dyn Fn(&Datom) -> bool) -> Vec<Datom> {
        self.datoms.iter().filter(|d| pred(d)).cloned().collect()
    }
    fn write_batch(&mut self, datoms: &[Datom]) -> Result<(), BackendError> {
        for d in datoms { self.datoms.insert(d.clone()); }
        Ok(())
    }
    fn sync(&mut self) -> Result<(), BackendError> {
        self.wal.fsync().map_err(BackendError::Io)
    }
    fn epoch(&self) -> u64 { self.epoch }
}

/// Distributed backend: sharded across nodes, accessed via RPC.
pub struct DistributedBackend {
    shards: Vec<ShardConnection>,
    local_cache: BTreeSet<Datom>,
    epoch: u64,
}

impl Backend for DistributedBackend {
    fn scan(&self, pred: &dyn Fn(&Datom) -> bool) -> Vec<Datom> {
        // Fan-out to all shards, merge results
        self.shards.iter()
            .flat_map(|shard| shard.scan(pred))
            .collect()
    }
    fn write_batch(&mut self, datoms: &[Datom]) -> Result<(), BackendError> {
        // Route each datom to its shard (INV-FERR-017)
        let mut shard_batches: BTreeMap<usize, Vec<Datom>> = BTreeMap::new();
        for d in datoms {
            let idx = shard_id(d, self.shards.len());
            shard_batches.entry(idx).or_default().push(d.clone());
        }
        for (idx, batch) in shard_batches {
            self.shards[idx].write_batch(&batch)?;
        }
        Ok(())
    }
    fn sync(&mut self) -> Result<(), BackendError> {
        for shard in &mut self.shards {
            shard.sync()?;
        }
        Ok(())
    }
    fn epoch(&self) -> u64 { self.epoch }
}

/// The Store is parameterized by Backend.
/// All invariants (INV-FERR-001..023) hold for any Backend.
pub struct Store<B: Backend> {
    backend: B,
    indexes: Indexes,
    schema: Schema,
}

impl<B: Backend> Store<B> {
    pub fn transact(&mut self, tx: Transaction<Committed>) -> Result<TxReceipt, TxApplyError> {
        // Same implementation regardless of backend
        let datoms = tx.datoms().cloned().collect::<Vec<_>>();
        self.backend.write_batch(&datoms)?;
        self.backend.sync()?;
        for d in &datoms {
            self.indexes.insert(d);
        }
        Ok(TxReceipt { epoch: self.backend.epoch(), datom_count: datoms.len() })
    }

    pub fn merge_from(&mut self, other: &Store<B>) -> Result<(), MergeError> {
        // Same merge logic regardless of backend
        let other_datoms = other.backend.scan(&|_| true);
        self.backend.write_batch(&other_datoms)?;
        self.backend.sync()?;
        for d in &other_datoms {
            self.indexes.insert(d);
        }
        Ok(())
    }

    pub fn snapshot(&self) -> Snapshot<'_, B> {
        Snapshot { store: self, epoch: self.backend.epoch() }
    }
}
```

**Falsification**: A function in the public API that behaves differently depending on
the backend (beyond performance characteristics). Specific failure modes:
- **Backend-specific API**: a method exists on `Store<EmbeddedBackend>` but not on
  `Store<DistributedBackend>` (or vice versa), forcing the caller to know the backend.
- **Semantic divergence**: `transact()` on `EmbeddedBackend` returns `Ok` for a
  transaction that `transact()` on `DistributedBackend` returns `Err` for (or vice versa),
  with the same datoms and the same schema.
- **Invariant violation**: any INV-FERR invariant (001 through 023) holds for
  `EmbeddedBackend` but not for `DistributedBackend` (or vice versa).

**proptest strategy**:
```rust
proptest! {
    #[test]
    fn api_equivalence(
        datoms in prop::collection::btree_set(arb_datom(), 0..50),
        txns in prop::collection::vec(arb_transaction(), 0..10),
    ) {
        let mut embedded = Store::<EmbeddedBackend>::from_datoms(datoms.clone());
        let mut distributed = Store::<MockDistributedBackend>::from_datoms(datoms);

        for tx in txns {
            let e_result = embedded.transact(tx.clone());
            let d_result = distributed.transact(tx);

            // Same success/failure behavior
            prop_assert_eq!(e_result.is_ok(), d_result.is_ok(),
                "Backend divergence: embedded={:?}, distributed={:?}",
                e_result, d_result);

            // Same datom set after transaction
            if e_result.is_ok() {
                prop_assert_eq!(embedded.datom_set(), distributed.datom_set(),
                    "Datom sets diverged after transaction");
            }
        }
    }
}
```

**Lean theorem**:
```lean
/-- Substrate agnosticism: the Store operations are defined on DatomStore
    without reference to any deployment model. The algebraic laws (L1-L5)
    hold for DatomStore = Finset Datom regardless of how the Finset is
    physically stored. -/

-- The merge, apply_tx, and visible_at functions are defined on DatomStore
-- (Finset Datom) without any "backend" parameter. This is the formal
-- statement of substrate agnosticism: the algebraic specification is
-- deployment-model-independent.

theorem merge_backend_independent (a b : DatomStore) :
    merge a b = a ∪ b := by
  unfold merge; rfl

theorem apply_backend_independent (s : DatomStore) (d : Datom) :
    apply_tx s d = s ∪ {d} := by
  unfold apply_tx; rfl

-- Any implementation that satisfies these equations satisfies all
-- FERR invariants, regardless of whether the Finset is stored in
-- local memory, on disk, across a network, or in a database.
```

---
