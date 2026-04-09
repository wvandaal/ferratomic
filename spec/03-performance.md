## 23.3 Performance & Scale Invariants

### INV-FERR-025: Index Backend Interchangeability

**Traces to**: C8 (Substrate Independence), INV-FERR-005 (Index Bijection),
ADR-FERR-001 (Persistent Data Structures)
**Referenced by**: INV-FERR-071 (sorted-array backend), INV-FERR-072 (lazy promotion), INV-FERR-073 (permutation index fusion),
NEG-FERR-007 (FM-Index inapplicability), ADR-FERR-030 (wavelet matrix convergence target)
**Verification**: `V:TYPE`, `V:PROP`, `V:KANI`, `V:LEAN`
**Stage**: 0

#### Level 0 (Algebraic Law)
```
Let `ι ∈ {EAVT, AEVT, VAET, AVET}` range over the four secondary-index orders.
Let `key_ι(d)` be the total ordering key induced by order `ι` for datom `d`.
Let `proj_ι(S) = { key_ι(d) ↦ d | d ∈ S }` be the extensional ordered-map view
of store `S` in order `ι`.

For any two backend families `B₁, B₂` implementing `IndexBackend<K, Datom>`:

  realizes(B₁, proj_ι(S)) ∧ realizes(B₂, proj_ι(S))
    ⇒ exact_lookup(B₁, k) = exact_lookup(B₂, k)
     ∧ ordered_values(B₁) = ordered_values(B₂)
     ∧ len(B₁) = len(B₂)

for every store `S`, order `ι`, and key `k`.

Backend interchangeability is therefore an observational equivalence property:
changing the backend may change asymptotic cost, cache behavior, or write path,
but it must not change any exact lookup result, any ordered iteration result, or
any index cardinality derived from the same primary datom set.

Proof sketch: `INV-FERR-005` fixes the extensional content of each index: every
datom in the primary set induces exactly one key/value entry in each order. If
two backend implementations both realize that same extensional projection, then
all observable queries are equal by map extensionality. The only remaining
degrees of freedom are operational ones: representation layout, mutation cost,
and iteration mechanics.
```

#### Level 1 (State Invariant)
Ferratomic’s current backend abstraction lives at the per-index layer, not at the
`Store` type-parameter layer. The canonical trait is `IndexBackend<K, V>`, and the
current codebase ships two concrete backend families:
- `im::OrdMap`, used by the `Indexes` type alias as the persistent ordered-map baseline.
- `SortedVecBackend`, used by `SortedVecIndexes` as the read-optimized sorted-array
  backend.

The `Store` itself is not generic over the backend. Instead it owns a representation
that may be `Positional` or `OrdMap`-backed, and when promoted it exposes
`SortedVecIndexes` through `Store::indexes()`. The architectural claim of this
invariant is therefore narrower and sharper than the older generic-store story:
all backend variation is confined to the secondary-index realization, while the
primary datom set and CRDT semantics remain unchanged.

Future backend families such as LSM or external KV stores are allowed only if they
implement the same exact key semantics and ordered iteration semantics. They are
not part of the currently certified verification surface.

#### Level 2 (Implementation Contract)
```rust
/// Per-index backend abstraction.
pub trait IndexBackend<K: Ord, V>: Clone + Default + Debug {
    fn backend_insert(&mut self, key: K, value: V);
    fn backend_get(&self, key: &K) -> Option<&V>;
    fn backend_len(&self) -> usize;
    fn backend_values(&self) -> Box<dyn Iterator<Item = &V> + '_>;
}

pub type Indexes = GenericIndexes<
    OrdMap<EavtKey, Datom>,
    OrdMap<AevtKey, Datom>,
    OrdMap<VaetKey, Datom>,
    OrdMap<AvetKey, Datom>,
>;

pub type SortedVecIndexes = GenericIndexes<
    SortedVecBackend<EavtKey, Datom>,
    SortedVecBackend<AevtKey, Datom>,
    SortedVecBackend<VaetKey, Datom>,
    SortedVecBackend<AvetKey, Datom>,
>;

pub struct Store {
    repr: StoreRepr, // Positional or OrdMap-backed primary store
    // ...
}

impl Store {
    /// Returns `Some` only for the promoted representation.
    pub fn indexes(&self) -> Option<&SortedVecIndexes> { /* ... */ }

    /// Promotion preserves the primary datom set while rebuilding query-ready
    /// secondary indexes from that same set.
    pub fn promote(&mut self) { /* ... */ }
}
```

**Falsification**: Build `Indexes` and `SortedVecIndexes` from the same primary
datom set and observe any of the following:
- **Exact-lookup divergence**: `backend_get(key)` returns `Some(d)` on one backend and
  `None` or a different datom on the other.
- **Ordered-iteration divergence**: `backend_values()` yields a different key order for
  the same index projection.
- **Cardinality divergence**: `backend_len()` differs for the same primary datom set.
- **Primary/index drift**: a backend returns a datom not present in the store’s primary
  datom set.

**proptest strategy**:
```rust
proptest! {
    #[test]
    fn inv_ferr_025_backend_observational_equivalence(
        datoms in prop::collection::btree_set(arb_datom(), 0..200),
    ) {
        let ord_like: Indexes = Indexes::from_datoms(datoms.iter());
        let mut sorted: SortedVecIndexes = SortedVecIndexes::from_datoms(datoms.iter());
        sorted.sort_all();

        prop_assert!(ord_like.verify_bijection());
        prop_assert!(sorted.verify_bijection());
        prop_assert_eq!(ord_like.len(), sorted.len());

        for datom in &datoms {
            let eavt = EavtKey::from_datom(datom);
            let aevt = AevtKey::from_datom(datom);
            let vaet = VaetKey::from_datom(datom);
            let avet = AvetKey::from_datom(datom);

            prop_assert_eq!(ord_like.eavt().backend_get(&eavt), sorted.eavt().backend_get(&eavt));
            prop_assert_eq!(ord_like.aevt().backend_get(&aevt), sorted.aevt().backend_get(&aevt));
            prop_assert_eq!(ord_like.vaet().backend_get(&vaet), sorted.vaet().backend_get(&vaet));
            prop_assert_eq!(ord_like.avet().backend_get(&avet), sorted.avet().backend_get(&avet));
        }

        prop_assert_eq!(
            ord_like.eavt_datoms().collect::<Vec<_>>(),
            sorted.eavt_datoms().collect::<Vec<_>>()
        );
        prop_assert_eq!(
            ord_like.aevt_datoms().collect::<Vec<_>>(),
            sorted.aevt_datoms().collect::<Vec<_>>()
        );
        prop_assert_eq!(
            ord_like.vaet_datoms().collect::<Vec<_>>(),
            sorted.vaet_datoms().collect::<Vec<_>>()
        );
        prop_assert_eq!(
            ord_like.avet_datoms().collect::<Vec<_>>(),
            sorted.avet_datoms().collect::<Vec<_>>()
        );
    }
}
```
The repository also keeps `inv_ferr_025_index_backend_roundtrip` as a backend-local
sanity check for `OrdMap<EavtKey, Datom>`, but the direct backend-family parity witness
for this invariant is now `inv_ferr_025_backend_observational_equivalence`.

**Lean theorem**:
```lean
/-- Any pure query over a store depends only on the datom set, not the
    representation that realized it. -/
theorem backend_parametricity (s1 s2 : DatomStore)
    (h : s1 = s2) (f : DatomStore → DatomStore) :
    f s1 = f s2 := by
  rw [h]

/-- The extensional attribute-key index view is determined solely by the
    datom set. -/
theorem index_view_deterministic (s1 s2 : DatomStore)
    (h : s1 = s2) (proj : Datom → Nat) :
    s1.image proj = s2.image proj := by
  rw [h]
```

---

### INV-FERR-026: Write Amplification Bound

**Traces to**: SEED.md §10, INV-FERR-008 (WAL Fsync Ordering),
INV-FERR-013 (Checkpoint Equivalence)
**Verification**: `V:PROP`, `V:KANI`
**Stage**: 0

#### Level 0 (Algebraic Law)
```
Let `wal_bytes(T)` be the byte delta appended to the WAL by transaction trace `T`.
Let `logical_bytes(T)` be the serialized logical payload size of the datoms written
by `T`.

Current Phase 4a certified metric:

  WA_wal(T) = wal_bytes(T) / logical_bytes(T) < 10

for the threshold traces exercised at 10^3, 10^4, and 2×10^5 datoms.

This invariant is intentionally about the measurable WAL-dominant write path, not a
fictional fully-amortized “all durable bytes” number. Checkpoint cost exists, but it
depends on checkpoint cadence and recovery policy; that term is audited separately by
INV-FERR-028 and INV-FERR-075 rather than silently folded into the current WA harness.
```

#### Level 1 (State Invariant)
The current implementation evidence measures physical WAL growth on disk against the
logical datom payload that induced it. This is the part of the write path that occurs
on every committed transaction and therefore dominates per-transaction durable cost.

The current threshold suite verifies two things:
1. The WAL frame format has bounded constant overhead relative to payload size.
2. That bounded overhead remains below the 10x ceiling across larger traces, so there
   is no hidden superlinear framing, metadata duplication, or per-transaction bloat.

Checkpoint bytes are not ignored; they are separated. The reason is methodological:
checkpoint write cost is amortized over many transactions and depends on policy
(frequency, compaction cadence, and snapshot layout). Folding that policy-dependent term
into the same metric would make the spec less precise, not more precise.

#### Level 2 (Implementation Contract)
```rust
fn measure_write_amplification(count: usize) -> f64 {
    let dir = tempfile::TempDir::new().expect("create temp dir");
    let wal_path = dir.path().join("wa_test.wal");
    let db = Database::genesis_with_wal(&wal_path).expect("genesis_with_wal");
    let node = NodeId::from_bytes([2u8; 16]);

    let mut logical_bytes: u64 = 0;
    for i in 0..count {
        let entity = EntityId::from_content(format!("wa-entity-{i}").as_bytes());
        let attr = Attribute::from("db/doc");
        let val = Value::String(format!("wa-value-{i}").into());

        let datom = Datom::new(entity, attr.clone(), val.clone(), TxId::new(0, 0, 0), Op::Assert);
        logical_bytes += serde_json::to_vec(&[&datom]).expect("serialize datom").len() as u64;

        let tx = Transaction::new(agent).assert_datom(entity, attr, val).commit_unchecked();
        db.transact(tx).expect("transact");
    }

    let wal_size = std::fs::metadata(&wal_path).expect("wal file").len();
    wal_size as f64 / logical_bytes as f64
}
```

**Falsification**: Any measured WAL trace with `WA_wal(T) ≥ 10`, or any measurement bug
that makes the physical/logical ratio nonsensical. Specific failure modes:
- **Frame bloat**: WAL metadata grows with trace size or duplicates payload data.
- **Per-transaction serialization overhead**: each commit rewrites unrelated state into
  the WAL append path.
- **Measurement drift**: the “logical bytes” proxy and physical bytes no longer measure
  the same artifact class, producing a false confidence signal.

**proptest strategy**:
```rust
proptest! {
    #[test]
    fn inv_ferr_026_write_amplification(
        txns in prop::collection::vec(arb_transaction(), 1..10),
    ) {
        // See ferratomic-verify/proptest/wal_properties.rs.
        // Measure WAL file growth versus serialized logical payload bytes.
    }
}
```
The threshold suite then scales the same metric to 1K, 10K, and 200K datoms in
`integration/test_thresholds.rs`.

**Lean theorem**:
Not modeled directly in Lean. The Finset Datom model has no notion of byte length,
filesystem growth, or WAL frame encoding, so the quantitative `WA_wal < 10` bound is
verified empirically. The supporting algebraic facts are monotonic growth
(INV-FERR-004) and WAL correctness (INV-FERR-008), not a separate byte-level theorem.

---

### INV-FERR-027: Read P99.99 Latency

**Traces to**: SEED.md §10, INV-FERR-006 (Snapshot Isolation),
ADR-FERR-001 (Persistent Data Structures), ADR-FERR-003 (Concurrency Model)
**Referenced by**: INV-FERR-071 (sorted-array backend — 4.5x lookup improvement), INV-FERR-077 (interpolation search — O(log log N) for BLAKE3 keys)
**Verification**: `V:PROP`, `V:KANI`
**Stage**: 0

#### Level 0 (Algebraic Law)
```
Design target:

  P99.99(latency_eavt_lookup(S_100M, C=10K readers)) < 10 ms

Certified proxy gates in the current repo:

  P99(latency_eavt_lookup(S_10K))   < 1 ms
  P99(latency_eavt_lookup(S_25K))   < 1 ms
  P99(latency_eavt_lookup(S_200K))  < 1 ms
  release_strict_P99(S_200K)        < 100 µs

The invariant is therefore two-layered:
- the product target is the 100M / 10K-reader design requirement,
- the automated certification artifact is the scaled EAVT lookup proxy suite.

The proxy is valid only because the measured path is the same path: an exact EAVT
lookup over an ordered secondary index with no writer lock acquisition on the read path.
```

#### Level 1 (State Invariant)
This invariant is about the engine’s lookup primitive, not end-to-end request latency.
Network transport, Datalog planning, result serialization, and client-side processing
are intentionally excluded.

The current threshold suite measures a single exact EAVT lookup after the store has
already been constructed and promoted. That is the critical primitive behind the
Stage 0 read path. Snapshot acquisition itself is covered by INV-FERR-006 and does not
need to be re-benchmarked here.

The important correctness claim is that the proxy is honest: the same ordered index
lookup path used in certification is the path the engine uses in production for exact
entity-attribute reads. If Ferratomic later introduces a different hot-path lookup
mechanism, this invariant must update its proxy definition instead of silently
reusing the old benchmark.

#### Level 2 (Implementation Contract)
```rust
fn measure_p99_read_latency_ns(
    store: &Store,
    datom_count: usize,
    lookup_count: usize,
) -> (u128, u128, u128) {
    let indexes = store.indexes().unwrap();
    let mut latencies_ns = Vec::with_capacity(lookup_count);

    for i in 0..lookup_count {
        let entity = EntityId::from_content(format!("entity-{}", i % datom_count).as_bytes());
        let key = EavtKey::from_datom(&Datom::new(
            entity,
            Attribute::from("db/doc"),
            Value::String(format!("value-{}", i % datom_count).into()),
            TxId::new(0, 0, 0),
            Op::Assert,
        ));

        let start = Instant::now();
        let _ = indexes.eavt().backend_get(&key);
        latencies_ns.push(start.elapsed().as_nanos());
    }

    latencies_ns.sort_unstable();
    let median = latencies_ns[latencies_ns.len() / 2];
    let p99 = latencies_ns[(latencies_ns.len() * 99) / 100];
    let max = *latencies_ns.last().unwrap();
    (median, p99, max)
}
```

**Falsification**: Either the direct product target is breached in a full-scale benchmark,
or any certified proxy threshold is breached in the current threshold suite. Specific
failure modes:
- **Hot-path regression**: EAVT lookup stops being a single ordered-index probe.
- **Contention regression**: the read path acquires a writer-contended lock.
- **Representation regression**: promoted stores stop exposing query-ready ordered indexes.
- **Measurement dishonesty**: the threshold harness no longer exercises the same lookup
  primitive as the production read path.

**proptest strategy**:
```rust
proptest! {
    #[test]
    fn inv_ferr_027_read_latency_lookup(
        datoms in prop::collection::btree_set(arb_datom(), 0..1000),
    ) {
        // Correctness guard for the exact lookup path.
        // Quantitative latency is enforced by integration thresholds.
    }
}
```

**Lean theorem**:
Not modeled directly in Lean. The Finset Datom model has no latency distribution,
contention scheduler, or cache hierarchy. The mechanized support for this invariant
comes from upstream structural properties: snapshot isolation (INV-FERR-006), ordered
index semantics (INV-FERR-025), and deterministic lookup keys. Quantitative latency is
certified by the threshold harnesses and release benchmarks, not by a standalone theorem.

---

### INV-FERR-028: Cold Start Latency

**Traces to**: SEED.md §10, INV-FERR-013 (Checkpoint Equivalence), INV-FERR-014 (Recovery)
**Referenced by**: INV-FERR-070 (zero-copy cold start — I/O-bound target), INV-FERR-075 (LIVE-first checkpoint — 2-10x reduction)
**Verification**: `V:PROP`, `V:KANI`
**Stage**: 0

#### Level 0 (Algebraic Law)
```
Design target:

  cold_start_to_queryable(S_100M) < 5 s

Certified proxy gates in the current repo:

  cold_start_to_queryable(S_1K)     < 5 s
  cold_start_to_queryable(S_5K)     < 5 s
  cold_start_to_queryable(S_200K)   < 5 s
  release_strict_cold_start(S_200K) < 2 s

where `cold_start_to_queryable` measures the filesystem recovery path ending in a
queryable `Database`, not merely deserialization of bytes.

The proxy suite is intended to falsify superlinear recovery regressions early. It is
not a claim that CI directly executes the 100M-datom product target today.
```

#### Level 1 (State Invariant)
The recovery contract is about time-to-first-queryable database state. A cold start is
not considered successful if it merely parses bytes but leaves the store unable to
serve reads.

Ferratomic’s current automated evidence scales the dataset down to 1K, 5K, and 200K
datoms while keeping the same recovery pipeline: load checkpoint, select recovery
level, rebuild the in-memory store state, and return `ColdStartResult { database, level }`.
This catches algorithmic regressions such as quadratic rebuilds, pathological WAL replay,
or invalid recovery-level fallback, while remaining executable in CI.

The 100M / 5s requirement remains the design target. The spec is explicit that today’s
automated suite certifies a scaled proxy, not the full target directly.

#### Level 2 (Implementation Contract)
```rust
pub fn cold_start(data_dir: &Path) -> Result<ColdStartResult, FerraError> {
    std::fs::create_dir_all(data_dir)?;

    let checkpoint_path = data_dir.join(CHECKPOINT_FILENAME);
    let wal_path = data_dir.join(WAL_FILENAME);

    if checkpoint_path.exists() && wal_path.exists() {
        if let Some(result) = try_checkpoint_plus_wal(&checkpoint_path, &wal_path) {
            return Ok(result);
        }
    }
    if checkpoint_path.exists() && !wal_path.exists() {
        if let Some(result) = try_checkpoint_only(&checkpoint_path, &wal_path) {
            return Ok(result);
        }
    }
    if wal_path.exists() {
        if let Some(result) = try_wal_only(&wal_path) {
            return Ok(result);
        }
    }

    Ok(ColdStartResult {
        database: Database::genesis_with_wal(&wal_path)?,
        level: RecoveryLevel::Genesis,
    })
}

#[cfg(test)]
fn threshold_inv_ferr_028_cold_start() {
    let tmp_dir = prepare_cold_start_dir(1_000);
    let start = Instant::now();
    let loaded = cold_start(tmp_dir.path()).unwrap();
    let elapsed = start.elapsed();

    assert!(loaded.database.epoch() > 0);
    assert!(elapsed.as_secs() < 5);
}
```

**Falsification**: Either the direct 100M benchmark exceeds the design target, or any
proxy threshold breach reveals that the recovery path is not scaling with acceptable
headroom. Specific failure modes:
- **Wrong recovery-level selection**: a valid checkpoint+WAL case falls through to
  slower recovery or to genesis.
- **Superlinear rebuild**: index or store reconstruction grows faster than expected.
- **WAL replay blow-up**: replay cost is proportional to historical file size rather
  than the post-checkpoint delta.
- **False success**: `cold_start()` returns quickly but the resulting `Database` is not
  queryable or has epoch `0` when it should have recovered real data.

**proptest strategy**:
```rust
proptest! {
    #[test]
    fn inv_ferr_028_cold_start_checkpoint_correctness(
        tx_batches in arb_transactions(8),
    ) {
        // See ferratomic-verify/proptest/durability_properties.rs.
        // Correctness is checked by checkpoint round-trip;
        // the timing bound is checked by integration thresholds.
    }
}
```

**Lean theorem**:
Not modeled directly in Lean as a latency claim. The relevant mechanized statement is
the correctness of checkpoint round-trip (INV-FERR-013); the `< 5s` bound is empirical
and is enforced by the threshold harnesses. The spec is explicit that no standalone
Lean theorem certifies the time bound itself.

---

### INV-FERR-029: LIVE View Resolution

**Traces to**: SEED.md §4, C1
**Referenced by**: INV-FERR-075 (LIVE-first checkpoint — idempotent LIVE projection)
**Verification**: `V:PROP`, `V:KANI`, `V:LEAN`
**Stage**: 0

#### Level 0 (Algebraic Law)
```
Let latest_S(e, a, v) be the highest-TxId datom in S whose triple is (e, a, v).

LIVE(S) = { (e, a, v) | latest_S(e, a, v) exists and latest_S(e, a, v).op = Assert }

Equivalently, if datoms are processed in causal TxId order:
  - assert(e, a, v) adds (e, a, v) to LIVE
  - retract(e, a, v) removes (e, a, v) from LIVE

The primary store remains append-only. LIVE is a derived projection:

∀ (e, a, v) ∈ LIVE(S): ∃ d ∈ S, d = (e, a, v, tx, Assert) or d = (e, a, v, tx, Retract)
LIVE(S) ⊆ project_eav(S)
|LIVE(S)| ≤ |S|
```

**Tie-breaking rule (canonical, amended for bd-l64y)**: For same-TxId
different-Op datoms (an assertion and a retraction with identical TxId for
the same triple), the resolution is **Assert wins**:

```
resolve_op : Op × Op → Op
resolve_op Assert _        = Assert
resolve_op _      Assert   = Assert
resolve_op Retract Retract = Retract
```

Equivalently, define the canonical comparison `≤ₗ` over `(TxId, Op)` pairs:

```
(tx₁, op₁) ≤ₗ (tx₂, op₂)  iff  tx₁ < tx₂
                              ∨ (tx₁ = tx₂ ∧ rank(op₁) ≤ rank(op₂))
where rank(Retract) = 0 and rank(Assert) = 1.
```

Note that this is the **OPPOSITE** of Rust's derived `Ord` on `(TxId, Op)`,
which gives `Op::Assert(0) < Op::Retract(1)` and therefore ranks Retract
HIGHER than Assert on tie. The canonical `≤ₗ` rule matches transact
semantics: within a single TxId, EAVT sort places Assert before Retract,
so the Assert is "first" and a derived "keep first" implementation
naturally produces Assert-wins. The merge path must use `≤ₗ` (NOT
`std::cmp::max` on raw `(TxId, Op)` tuples) to converge with the
in-place and batch paths.

This rule is the regression target for **bd-l64y** (three-path tie-breaking
inconsistency: `merge_causal` in `ferratomic-store/src/merge.rs:228-235`,
`build_live_causal` in `ferratomic-store/src/query.rs:96-110`, and
`live_apply` in `ferratomic-store/src/store.rs:460-490` previously diverged
on this case).

#### Level 1 (State Invariant)
The current implementation materializes the LIVE projection in two layers:
- `live_causal : OrdMap<(EntityId, Attribute), OrdMap<Value, (TxId, Op)>>`
- `live_set : OrdMap<(EntityId, Attribute), OrdSet<Value>>`

`live_causal` retains the latest causal event for each `(entity, attribute, value)`
triple. `live_set` is the projection of `live_causal` that keeps only values whose
latest event is `Assert`. This matches the algebra above exactly: a value is LIVE iff
its highest-TxId event for that triple is an assertion.

This invariant certifies the current-store LIVE projection only. The repository does
not currently expose a first-class historical `LIVE(S, epoch)` API, and this spec no
longer claims one. Historical replay and time-travel semantics must be specified
separately when that surface is introduced.

The primary store remains append-only under C1. LIVE never deletes datoms from the
primary store; it only changes which `(e, a, v)` triples are visible in the derived
query surface.

The same invariant is also realized in the positional representation. `PositionalStore`
reconstructs a LIVE bitvector over canonical EAVT datoms by scanning contiguous
`(entity, attribute, value)` runs and marking the last position of each run whose
latest operation is `Assert`. That is not a different invariant; it is the same LIVE
projection at a different representation boundary.

The verification surface is therefore intentionally split rather than collapsed into a
single oversized proof:
- `retraction_removes_from_live_view` checks the set-valued datom-level rule.
- `positional_live_kernel_matches_latest_event_model` checks that the sorted-run kernel
  marks exactly the tails of runs whose latest event is `Assert`.
- `positional_live_kernel_respects_entity_boundaries` checks that adjacent runs sharing
  an attribute/value do not bleed across entity boundaries.
- Runtime tests in `ferratomic-core/src/positional.rs` check that the datom wrapper and
  concrete `BitVec` representation agree with that kernel.

#### Level 2 (Implementation Contract)
```rust
pub(crate) type LiveCausal =
    OrdMap<(EntityId, Attribute), OrdMap<Value, (TxId, Op)>>;

impl Store {
    /// Raw LIVE values for an (entity, attribute) pair.
    /// Values are present iff their latest causal event is Assert.
    pub fn live_values(
        &self,
        entity: EntityId,
        attribute: &Attribute,
    ) -> Option<&OrdSet<Value>> {
        self.live_set.get(&(entity, attribute.clone()))
    }
}

pub(super) fn build_live_causal<'a>(datoms: impl Iterator<Item = &'a Datom>) -> LiveCausal {
    let mut causal: LiveCausal = OrdMap::new();
    for datom in datoms {
        let key = (datom.entity(), datom.attribute().clone());
        let entries = causal.entry(key).or_default();
        let value = datom.value().clone();
        match entries.get(&value) {
            Some(&(existing_tx, _)) if existing_tx >= datom.tx() => {}
            _ => {
                entries.insert(value, (datom.tx(), datom.op()));
            }
        }
    }
    causal
}

pub(super) fn derive_live_set(
    causal: &LiveCausal,
) -> OrdMap<(EntityId, Attribute), OrdSet<Value>> {
    let mut live: OrdMap<(EntityId, Attribute), OrdSet<Value>> = OrdMap::new();
    for (key, entries) in causal {
        let mut values = OrdSet::new();
        for (value, &(_, op)) in entries {
            if op == Op::Assert {
                values.insert(value.clone());
            }
        }
        if !values.is_empty() {
            live.insert(key.clone(), values);
        }
    }
    live
}

// === Three-path equivalence (INV-FERR-029 canonical tie-break, amended for bd-l64y) ===
//
// LIVE state is maintained by THREE code paths in ferratomic-store. All three
// MUST converge on the canonical "Assert wins on same-TxId tie" rule from
// Level 0 above. The shared helper:
//
//   /// Returns true iff `new` should replace `existing` for the same
//   /// (entity, attribute, value) triple, per the canonical tie-break rule.
//   pub(crate) fn should_replace(
//       existing: (TxId, Op),
//       new: (TxId, Op),
//   ) -> bool {
//       match new.0.cmp(&existing.0) {
//           Ordering::Greater => true,
//           Ordering::Less => false,
//           Ordering::Equal => {
//               // Same TxId: Assert outranks Retract (Level 0 tie-break).
//               new.1 == Op::Assert && existing.1 == Op::Retract
//           }
//       }
//   }
//
// The three paths must all use this helper (or an equivalent comparison):
//
// 1. build_live_causal (above, ferratomic-store/src/query.rs:96):
//    Currently uses `existing_tx >= datom.tx()` with the "keep first" pattern,
//    which produces Assert-wins because EAVT sort places Assert first within
//    a TxId. With `should_replace`, the logic becomes explicit:
//        `if should_replace(*existing_event, (datom.tx(), datom.op())) { ... }`
//
// 2. live_apply (ferratomic-store/src/store.rs:460):
//    Currently uses `datom.tx() > existing_tx` with the "keep existing on tie"
//    pattern, which also produces Assert-wins for the same EAVT-sort reason.
//    With `should_replace`, replace `should_update` computation with
//        `let should_update = should_replace((existing_tx, op), (datom.tx(), datom.op()));`
//
// 3. merge_causal (ferratomic-store/src/merge.rs:228):
//    Currently uses `entries.union_with(std::cmp::max)` over raw `(TxId, Op)`,
//    which gives Retract > Assert on tie (because `Op::Retract(1) > Op::Assert(0)`
//    in derived Ord) — INCORRECT per the canonical rule. Replace with:
//        `entries.union_with(|a, b| if should_replace(a, b) { b } else { a })`
//
// Three-path convergence is regression-tested by `test_inv_ferr_029_same_tx_id_assert_wins`
// (proptest below), filed as **bd-l64y**.

fn live_positions_from_sorted_runs<K, FKey, FOp>(len: usize, key_at: FKey, op_at: FOp) -> Vec<u32>
where
    K: PartialEq,
    FKey: Fn(usize) -> K,
    FOp: Fn(usize) -> Op,
{
    let mut live_positions = Vec::new();
    let mut i = 0;
    while i < len {
        let key = key_at(i);
        let mut j = i + 1;
        while j < len && key_at(j) == key {
            j += 1;
        }
        if op_at(j - 1) == Op::Assert {
            live_positions.push((j - 1) as u32);
        }
        i = j;
    }
    live_positions
}

fn live_positions_kernel(canonical: &[Datom]) -> Vec<u32> {
    live_positions_from_sorted_runs(
        canonical.len(),
        |idx| (canonical[idx].entity(), canonical[idx].attribute(), canonical[idx].value()),
        |idx| canonical[idx].op(),
    )
}

#[kani::proof]
#[kani::unwind(10)]
fn retraction_removes_from_live_view() {
    // ferratomic-verify/kani/live_resolution.rs
    // bounded witness: if the latest event for (e, a, v) is Retract,
    // that triple is absent from the computed LIVE view.
}
```

**Falsification**: A triple whose latest event is `Retract` appears in `LIVE(S)`. Or:
a triple whose latest event is `Assert` is absent from `LIVE(S)`. Or: the LIVE
projection mutates the primary store instead of remaining a derived view. Specific
failure modes:
- **Latest-event regression**: an older assert overwrites a newer retract because
  `TxId` ordering is ignored or inverted.
- **Triple-matching bug**: retraction matching ignores `value` and removes the wrong
  `(entity, attribute, value)` entry.
- **Run-boundary collapse**: positional reconstruction merges adjacent canonical runs
  that should remain distinct because entity, attribute, or value changed.
- **Projection leak**: `derive_live_set` retains `Op::Retract` values in the materialized
  LIVE map.
- **Primary mutation**: query code mutates `Store::datoms()` or `datom_set()` while
  resolving LIVE values.

**proptest strategy**:
```rust
proptest! {
    #[test]
    fn test_inv_ferr_029_live_resolution(
        datoms in prop::collection::vec(arb_datom(), 1..50),
    ) {
        let store = Store::from_datoms(datoms.into_iter().collect());

        // Reference model: sort by (TxId, Op-rank Assert-wins), then assert
        // inserts and retract removes. The resulting triple set must equal
        // the native LIVE projection.
    }

    /// bd-l64y regression: same-TxId different-Op edge case.
    /// Verifies build_live_causal, live_apply, and merge_causal all converge
    /// on the canonical "Assert wins on tie" rule from Level 0.
    #[test]
    fn test_inv_ferr_029_same_tx_id_assert_wins(
        e in arb_entity_id(),
        a in arb_attribute(),
        v in arb_value(),
        tx in arb_tx_id(),
    ) {
        // Same (e,a,v) triple, same TxId, opposite Op.
        let assert_datom = Datom::new(e, a.clone(), v.clone(), tx, Op::Assert);
        let retract_datom = Datom::new(e, a.clone(), v.clone(), tx, Op::Retract);

        // Path 1: build_live_causal over the union (single batch).
        let union: BTreeSet<Datom> = [assert_datom.clone(), retract_datom.clone()]
            .into_iter().collect();
        let causal_batch = build_live_causal(union.iter());
        let live_batch = derive_live_set(&causal_batch);
        let assert_present_batch = live_batch
            .get(&(e, a.clone()))
            .map_or(false, |vs| vs.contains(&v));

        // Path 2: live_apply (in-place per-datom transact path).
        let mut store_apply = Store::genesis();
        store_apply.live_apply(&assert_datom);
        store_apply.live_apply(&retract_datom);
        let assert_present_apply = store_apply
            .live_values(e, &a)
            .map_or(false, |vs| vs.contains(&v));

        // Path 3: merge_causal (selective_merge lattice union path).
        let store_a = Store::from_datoms(BTreeSet::from([assert_datom]));
        let store_r = Store::from_datoms(BTreeSet::from([retract_datom]));
        let merged = merge(&store_a, &store_r).unwrap();
        let assert_present_merge = merged
            .live_values(e, &a)
            .map_or(false, |vs| vs.contains(&v));

        // INV-FERR-029 canonical tie-break: Assert wins on same-TxId.
        prop_assert!(assert_present_batch,
            "build_live_causal: Assert must win on same-TxId tie");
        prop_assert!(assert_present_apply,
            "live_apply: Assert must win on same-TxId tie");
        prop_assert!(assert_present_merge,
            "merge_causal: Assert must win on same-TxId tie (bd-l64y regression)");

        // Three-path convergence: all paths must agree on the same result.
        prop_assert_eq!(assert_present_batch, assert_present_apply,
            "INV-FERR-029: build_live_causal and live_apply must converge");
        prop_assert_eq!(assert_present_apply, assert_present_merge,
            "INV-FERR-029: live_apply and merge_causal must converge");
    }
}
```

**Lean theorem**:
```lean
def apply_op (live : Finset (Nat × Nat × Nat)) (d : Datom) : Finset (Nat × Nat × Nat) :=
  let key := (d.e, d.a, d.v)
  if d.op then live ∪ {key} else live \ {key}

def live_view_model (datoms : List Datom) : Finset (Nat × Nat × Nat) :=
  datoms.foldl apply_op ∅

theorem live_bounded (datoms : List Datom) :
    (live_view_model datoms).card ≤ datoms.length := by
  -- ferratomic-verify/lean/Ferratomic/Performance.lean
  -- proven mechanized theorem

theorem retraction_removes (live : Finset (Nat × Nat × Nat)) (e a v : Nat) :
    (e, a, v) ∉ apply_op live ⟨e, a, v, 0, false⟩ := by
  simp [apply_op]

theorem assertion_adds (live : Finset (Nat × Nat × Nat)) (e a v : Nat) :
    (e, a, v) ∈ apply_op live ⟨e, a, v, 0, true⟩ := by
  simp [apply_op]
```

---

### INV-FERR-030: Read Replica Subset

**Traces to**: SEED.md §4, C4
**Verification**: `V:TYPE`, `V:PROP`, `V:KANI`, `V:LEAN`
**Stage**: 0

#### Level 0 (Algebraic Law)
```
Let replica_filter(S, p) = { d ∈ S | p(d) }.

For any store S and any replica predicate p:
  replica_filter(S, p) ⊆ S

For the full-replica predicate p = True:
  replica_filter(S, True) = S

Current Stage 0 certification is about subset selection, not transport.
A replica view may omit leader datoms by policy, but it may never invent
datoms that are absent from the source store.
```

#### Level 1 (State Invariant)
The current codebase does not yet implement a networked `ReadReplica` runtime with WAL
catch-up semantics. What is implemented, and therefore what this Stage 0 invariant
certifies, is the replica-selection boundary:
- `ReplicaFilter` is a predicate over source-store datoms.
- `AcceptAll` is the identity filter for the single-node/full-replica case.
- Any concrete replica projection must be formed by filtering leader datoms, never by
  constructing new datoms.

This keeps the invariant honest. Operational anti-entropy, WAL shipping, and eventual
catch-up belong to the federation/distribution layers and must not be silently claimed
here before a corresponding implementation and proof surface exist.

#### Level 2 (Implementation Contract)
```rust
pub trait ReplicaFilter: Send + Sync {
    /// Returns true iff this replica should store the datom.
    fn accepts(&self, datom: &Datom) -> bool;
}

#[derive(Debug, Default, Clone)]
pub struct AcceptAll;

impl ReplicaFilter for AcceptAll {
    fn accepts(&self, _datom: &Datom) -> bool {
        true
    }
}

/// Conceptual contract:
/// a replica projection is formed only by filtering source-store datoms.
pub fn project_replica(
    leader: impl Iterator<Item = Datom>,
    filter: &dyn ReplicaFilter,
) -> Vec<Datom> {
    leader.filter(|d| filter.accepts(d)).collect()
}
```

**Falsification**: A replica projection contains a datom that was not present in the
source store. Or: `AcceptAll` rejects a source datom, violating the full-replica
identity case. Specific failure modes:
- **Phantom construction**: replica-building code synthesizes a datom instead of
  selecting from the source iterator.
- **Predicate inversion**: a filter returns `false` for a datom that must be kept,
  or `true` for a datom from the wrong source set.
- **Identity breakage**: `AcceptAll` does not preserve full-store membership.

The current spec intentionally does not claim WAL-stream convergence here because no
such runtime is implemented in this layer yet.

**proptest strategy**:
```rust
proptest! {
    #[test]
    fn inv_ferr_030_accept_all_filter(
        datoms in prop::collection::vec(arb_datom(), 1..100),
    ) {
        let filter = AcceptAll;
        for d in &datoms {
            prop_assert!(filter.accepts(d));
        }
    }
}
```

**Lean theorem**:
```lean
def replica_filter (s : DatomStore) (p : Datom → Prop) [DecidablePred p] : DatomStore :=
  s.filter p

theorem accept_all_identity (s : DatomStore) :
    replica_filter s (fun _ => True) = s :=
  Finset.filter_true_of_mem (fun _ _ => trivial)

theorem replica_subset (s : DatomStore) (p : Datom → Prop) [DecidablePred p] :
    replica_filter s p ⊆ s :=
  Finset.filter_subset p s

theorem replica_filter_merge_mono (a b : DatomStore) (p : Datom → Prop) [DecidablePred p] :
    replica_filter a p ⊆ replica_filter (merge a b) p := by
  -- ferratomic-verify/lean/Ferratomic/Performance.lean
```

---

### INV-FERR-031: Genesis Determinism

**Traces to**: SEED.md §4, C7 (Self-bootstrap)
**Referenced by**: INV-FERR-060 (store identity bootstrap), ADR-FERR-027 (store
identity), ADR-FERR-028 (provenance lattice)
**Verification**: `V:TYPE`, `V:PROP`, `V:KANI`, `V:LEAN`
**Stage**: 0

> **Current canonical scope**: `Store::genesis()` builds an empty primary datom set,
> epoch `0`, zero genesis agent, and a deterministic 19-attribute axiomatic schema:
> 9 `db/*`, 5 `lattice/*`, and 5 `tx/*` attributes. The spec no longer claims a
> 23-attribute genesis surface because the current implementation and tests certify 19.
> Additional schema attributes must be installed by transactions until a future
> explicit spec revision changes the axiomatic set.

#### Level 0 (Algebraic Law)
```
Let genesis() be the store bootstrap function.

∀ invocations i, j:
  genesis_i() = genesis_j()

Current certified structure:
  datoms(genesis()) = ∅
  epoch(genesis()) = 0
  genesis_node(genesis()) = [0; 16]
  schema(genesis()) = deterministic 19-attribute axiomatic schema

At the datom-set layer, genesis is the semilattice bottom element.
At the store layer, genesis is a deterministic bootstrap state.
```

#### Level 1 (State Invariant)
Every call to `Store::genesis()` must yield the same bootstrap state. That means:
- the same empty primary store,
- the same epoch,
- the same genesis node,
- the same ordered schema definitions,
- and therefore the same starting point for every test, recovery flow, and replica.

The deterministic schema is assembled from two fixed builders:
- `define_meta_schema`: 9 `db/*` attributes and 5 `lattice/*` attributes
- `define_tx_schema`: 5 transaction metadata attributes

The current transaction metadata surface is exactly:
`tx/time`, `tx/origin`, `tx/provenance`, `tx/rationale`, `tx/validation-override`

This invariant is intentionally narrower than “future genesis may grow”. Axiomatic
schema growth is a spec change, not a silent implementation detail.

#### Level 2 (Implementation Contract)
```rust
/// Deterministic genesis store with the 19 axiomatic meta-schema attributes.
pub fn genesis() -> Store {
    let positional = PositionalStore::from_datoms(std::iter::empty());
    Store {
        repr: StoreRepr::Positional(Arc::new(positional)),
        schema: crate::schema_evolution::genesis_schema(),
        epoch: 0,
        genesis_node: NodeId::from_bytes([0u8; 16]),
        live_causal: OrdMap::new(),
        live_set: OrdMap::new(),
        schema_conflicts: Vec::new(),
    }
}

pub(crate) fn genesis_schema() -> Schema {
    let mut schema = Schema::empty();
    define_meta_schema(&mut schema); // 14 attrs total
    define_tx_schema(&mut schema);   // +5 attrs = 19
    schema
}

#[kani::proof]
fn genesis_determinism() {
    let a = Store::genesis();
    let b = Store::genesis();
    assert_eq!(a.datom_set(), b.datom_set());
    assert_eq!(a.epoch(), b.epoch());
    assert_eq!(a.schema(), b.schema());
    assert_eq!(a.genesis_node(), b.genesis_node());
}
```

**Falsification**: Two calls to `Store::genesis()` produce different datom sets, epochs,
schemas, or genesis agents. Specific failure modes:
- **Schema drift**: `genesis_schema()` depends on nondeterministic iteration order.
- **Bootstrap impurity**: genesis reads wall-clock time, random bytes, host identity,
  or any other external state.
- **Silent scope drift**: implementation adds axiomatic attributes without updating the
  canonical spec and tests.

**proptest strategy**:
```rust
proptest! {
    #[test]
    fn inv_ferr_031_genesis_determinism(_seed in any::<u64>()) {
        let g1 = Store::genesis();
        let g2 = Store::genesis();
        prop_assert_eq!(g1.datom_set(), g2.datom_set());
        prop_assert_eq!(g1.epoch(), g2.epoch());
        prop_assert_eq!(g1.schema(), g2.schema());
        prop_assert_eq!(g1.genesis_node(), g2.genesis_node());
    }
}
```

**Lean theorem**:
```lean
def genesis_model : DatomStore := ∅

theorem genesis_bottom (s : DatomStore) : genesis_model ⊆ s :=
  Finset.empty_subset s

theorem genesis_merge_left (s : DatomStore) : merge genesis_model s = s :=
  Finset.empty_union s

theorem genesis_merge_right (s : DatomStore) : merge s genesis_model = s :=
  Finset.union_empty s

theorem genesis_card : genesis_model.card = 0 :=
  Finset.card_empty
```

---

### INV-FERR-032: LIVE Resolution Correctness

**Traces to**: SEED.md §4, INV-FERR-029, INV-FERR-009
**Referenced by**: INV-FERR-075 (LIVE-first checkpoint)
**Verification**: `V:PROP`, `V:KANI`, `V:LEAN`
**Stage**: 0

#### Level 0 (Algebraic Law)
```
For a fixed entity e and attribute a:

LIVE_values(S, e, a) =
  { v | latest_S(e, a, v) exists and latest_S(e, a, v).op = Assert }

For cardinality-many attributes, the query result is exactly LIVE_values(S, e, a).

For cardinality-one attributes, the query result is:
  LIVE_resolve(S, e, a) =
    arg max_tx { (v, tx) | latest_S(e, a, v) = (Assert, tx) }

If no surviving assert exists, LIVE_resolve(S, e, a) = None.
```

#### Level 1 (State Invariant)
`INV-FERR-029` certifies the raw set-valued LIVE projection. `INV-FERR-032` strengthens
that by certifying the query semantics built on top of it:
- `Store::live_values` returns all non-retracted values for `(entity, attribute)`.
- `Store::live_resolve` returns the surviving value with the highest `TxId`.
- If the most recent value is retracted, resolution falls back to the next-highest
  surviving assert.

This matches the current code exactly. The store maintains the full causal map for all
surviving and retracted values, then resolves card-one reads by selecting the highest
assert `TxId` from that map. The implementation does not hide this behind a generic
schema-driven resolver; callers choose `live_values` or `live_resolve` according to the
attribute semantics they need.

The Lean file proves the assert/retract algebra for the raw LIVE set. The higher-level
“pick the highest surviving `TxId` for card-one” rule is certified by the concrete
runtime Kani harness `select_latest_live_value_lww_semantics`, which targets the exact
selection kernel used by `Store::live_resolve`. The surrounding datom-to-causal-map
wrapper and public query entrypoints are then checked by the Rust tests in
`store/query.rs`, `proptest/schema_properties.rs`, and `integration/test_snapshot.rs`.

#### Level 2 (Implementation Contract)
```rust
impl Store {
    /// Raw LIVE values for an entity-attribute pair.
    pub fn live_values(&self, entity: EntityId, attribute: &Attribute) -> Option<&OrdSet<Value>> {
        self.live_set.get(&(entity, attribute.clone()))
    }

    /// Card-one helper: choose the surviving assert with highest TxId.
    pub fn live_resolve(&self, entity: EntityId, attribute: &Attribute) -> Option<&Value> {
        self.live_causal
            .get(&(entity, attribute.clone()))
            .and_then(select_latest_live_value)
    }
}

fn select_latest_live_value(entries: &OrdMap<Value, (TxId, Op)>) -> Option<&Value> {
    select_latest_live_value_from_iter(entries.iter())
}

fn select_latest_live_value_from_iter<'a>(
    entries: impl Iterator<Item = (&'a Value, &'a (TxId, Op))>,
) -> Option<&'a Value> {
    entries
        .filter(|(_, &(_, op))| op == Op::Assert)
        .max_by(|(lv, (ltx, _)), (rv, (rtx, _))| {
            ltx.cmp(rtx).then_with(|| lv.cmp(rv))
        })
        .map(|(value, _)| value)
}

#[cfg(any(test, feature = "test-utils"))]
fn select_latest_live_value_for_test(entries: &[(Value, (TxId, Op))]) -> Option<&Value> {
    select_latest_live_value_from_iter(entries.iter().map(|(value, meta)| (value, meta)))
}

#[kani::proof]
fn select_latest_live_value_lww_semantics() {
    let older = Value::Long(1);
    let newer = Value::Long(2);

    assert_eq!(
        select_latest_live_value_for_test(&[
            (older.clone(), (TxId::new(1, 0, 0), Assert)),
            (newer.clone(), (TxId::new(2, 0, 0), Assert)),
        ]),
        Some(&newer)
    );

    assert_eq!(
        select_latest_live_value_for_test(&[
            (older.clone(), (TxId::new(1, 0, 0), Assert)),
            (newer.clone(), (TxId::new(2, 0, 0), Retract)),
        ]),
        Some(&older)
    );
}
```

**Falsification**: The query surface returns the wrong current value set. Specific
failure modes:
- **Retracted value present**: `live_values` contains a value whose latest event is
  `Retract`.
- **Surviving value absent**: `live_values` omits a value whose latest event is
  `Assert`.
- **Wrong winner**: `live_resolve` returns a lower-`TxId` value when a higher-`TxId`
  surviving assert exists.
- **Fallback failure**: retracting the current head does not reveal the next-highest
  surviving assert.

**proptest strategy**:
```rust
proptest! {
    #[test]
    fn test_inv_ferr_032_live_semantics(
        entity in arb_entity_id(),
        card_one_values in prop::collection::vec(any::<i64>(), 2..10),
        card_many_values in prop::collection::vec(any::<i64>(), 2..10),
        base_ts in 1000u64..1_000_000u64,
        retract_mask in any::<u16>(),
    ) {
        // Build a reference model from causal TxId order, then verify:
        //   1. Store::live_resolve() matches highest surviving TxId for card-one
        //   2. Store::live_values() matches surviving-value set for card-many
    }
}
```

**Lean theorem**:
```lean
def assertions (datoms : Finset Datom) (e a : Nat) : Finset Nat :=
  (datoms.filter (fun d => d.e = e ∧ d.a = a ∧ d.op = true)).image (fun d => d.v)

def retractions (datoms : Finset Datom) (e a : Nat) : Finset Nat :=
  (datoms.filter (fun d => d.e = e ∧ d.a = a ∧ d.op = false)).image (fun d => d.v)

def live_values (datoms : Finset Datom) (e a : Nat) : Finset Nat :=
  assertions datoms e a \ retractions datoms e a

theorem live_asserted_not_retracted (datoms : Finset Datom) (e a v : Nat)
    (h_in : v ∈ assertions datoms e a)
    (h_not : v ∉ retractions datoms e a) :
    v ∈ live_values datoms e a := by
  exact Finset.mem_sdiff.mpr ⟨h_in, h_not⟩

theorem live_retracted_absent (datoms : Finset Datom) (e a v : Nat)
    (h_retracted : v ∈ retractions datoms e a) :
    v ∉ live_values datoms e a := by
  intro h
  exact absurd h_retracted (Finset.mem_sdiff.mp h).2
```

---

### Capacity Planning (Operational Reference)

Resource estimates by scale tier. Memory figures are for the im::OrdMap representation
(Phase 4a). PositionalStore (INV-FERR-076) reduces memory ~6x. Wavelet matrix
(ADR-FERR-030, Phase 4c+) targets ~5 bytes/datom.

| Scale | Datoms | RAM (OrdMap) | RAM (Positional, est.) | Disk (Checkpoint) |
|-------|--------|-------------|----------------------|-------------------|
| Small | 10K | ~3.5 MB | ~0.6 MB | ~2 MB |
| Medium | 100K | ~35 MB | ~6 MB | ~20 MB |
| Large | 1M | ~350 MB | ~60 MB | ~200 MB |
| XL | 10M | ~3.5 GB | ~600 MB | ~2 GB |
| XXL | 100M | ~35 GB | ~6 GB | ~20 GB |

Per-datom memory breakdown (OrdMap representation):

| Component | Bytes | Notes |
|-----------|-------|-------|
| EntityId | 32 | BLAKE3 hash |
| Attribute | ~40 | Arc<str> with namespace/name |
| Value | ~80 | Enum, average across types |
| TxId | 24 | physical + logical + agent |
| Op | 1 | Assert or Retract |
| im::OrdMap overhead | ~170 | Node pointers, balance, Arc header, 4 index entries |
| **Total (OrdMap)** | **~350** | |
| **Total (Positional)** | **~130** | Contiguous arrays, no tree overhead |
