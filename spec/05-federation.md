## 23.8 Federation & Federated Query

Federation extends the single-store model to multi-store environments where independent
datom stores — potentially on different machines, different networks, or different
continents — participate in a unified query and merge fabric. The CRDT foundation
(INV-FERR-001 through INV-FERR-003) guarantees that merge remains correct regardless
of topology. Federation adds the operational machinery: transport abstraction, fan-out
query, selective merge, provenance preservation, latency tolerance, and live migration.

**Traces to**: SEED.md §4 (Design Commitment: "CRDT merge scales learning across
organizations"), SEED.md §10 (The Bootstrap), INV-FERR-010 (Merge Convergence),
INV-FERR-022 (Anti-Entropy Convergence), INV-FERR-033 (Cross-Shard Query Correctness),
INV-FERR-034 through INV-FERR-036 (Partition Tolerance)

**Design principles**:

1. **Transport transparency.** Application code never knows or cares whether a store
   is local (in-process), same-machine (Unix socket), LAN (TCP), or WAN (QUIC/gRPC).
   The `Transport` trait abstracts all of these behind the same async interface.

2. **CALM-correct fan-out.** For monotonic queries, fan-out + merge equals query on
   merged store. This is not a heuristic — it is a theorem (INV-FERR-037). Non-monotonic
   queries are explicitly classified and handled via materialization.

3. **Selective knowledge transfer.** Agents do not need to import entire remote stores.
   Selective merge with attribute-namespace filters enables precise knowledge transfer
   (e.g., "learn calibrated policies from project X without importing its task history").

4. **Provenance is never lost.** Every datom retains its original TxId through any
   number of merges across any number of stores. The agent field of TxId answers
   "who observed this?" across organizational boundaries.

5. **Graceful degradation.** Federation operates under partial failure. Timed-out stores
   produce partial results with explicit metadata, not silent data loss.

---

### INV-FERR-037: Federated Query Correctness

**Traces to**: SEED.md §4, INV-FERR-033 (Cross-Shard Query Correctness), CALM theorem
**Verification**: `V:PROP`, `V:KANI`, `V:LEAN`, `V:MODEL`
**Stage**: 0

#### Level 0 (Algebraic Law)
```
Let {S₁, S₂, ..., Sₖ} be a set of datom stores.
Let Q be a monotonic query (no negation, no aggregation, no set difference).
Let query : DatomStore → Result be the Datalog evaluation function.

∀ monotonic Q, ∀ {S₁, ..., Sₖ}:
  query(⋃ᵢ Sᵢ) = ⋃ᵢ query(Sᵢ)

Proof:
  By structural induction on Q:
  - Base case (attribute filter): Finset.filter_biUnion (proven in INV-FERR-033).
  - Join case: monotonic functions distribute over union by definition.
  - Union case: union distributes over union (trivially).
  - Projection case: image distributes over union.

  The CALM theorem (Hellerstein 2010, Ameloot et al. 2011) establishes that
  monotonic queries are exactly the class of queries that can be evaluated
  without coordination. Fan-out + merge is the coordination-free evaluation
  strategy.

For non-monotonic Q' (negation, aggregation, set difference):
  query(⋃ᵢ Sᵢ) ≠ ⋃ᵢ query(Sᵢ)    (in general)

Non-monotonic queries require full materialization:
  materialize({S₁, ..., Sₖ}) → S_full = ⋃ᵢ Sᵢ
  then query(S_full)
```

#### Level 1 (State Invariant)
For all reachable federation states `F = {S₁, ..., Sₖ}` where each `Sᵢ` is produced
by any sequence of TRANSACT, MERGE, and recovery operations: a monotonic federated
query returns exactly the same result set as querying the union of all stores. This
holds regardless of:
- The number of stores (k ≥ 1).
- The size distribution (some stores may have millions of datoms, others may be empty).
- The physical location of stores (in-process, same machine, different continent).
- The transport used to reach each store (local, TCP, QUIC, gRPC, Unix socket).
- The latency characteristics (some fast, some slow, as long as all respond).
- The overlap between stores (stores may share datoms from prior merges).

The federation query evaluator MUST classify every query as monotonic or non-monotonic
before execution, using the same classification logic as INV-FERR-033. For monotonic
queries, the evaluator fans out to all stores concurrently, collects per-store results,
and merges via set union. For non-monotonic queries, the evaluator materializes all
stores into a single in-memory store, then evaluates locally.

#### Level 2 (Implementation Contract)
```rust
/// A federation of datom stores, potentially heterogeneous in transport.
pub struct Federation {
    stores: Vec<StoreHandle>,
}

/// A handle to a store: local (in-process) or remote (over transport).
pub enum StoreHandle {
    Local(Database),
    Remote(RemoteStore),
}

/// A remote store accessed via a transport layer.
pub struct RemoteStore {
    id: StoreId,
    transport: Box<dyn Transport>,
    addr: SocketAddr,
    timeout: Duration,
}

/// Fan-out a monotonic query to all stores, merge results.
/// For non-monotonic queries, materializes first.
///
/// # Errors
/// Returns `FederationError::AllStoresTimedOut` if every store times out.
/// Returns partial results if some stores time out (with `partial: true`).
///
/// # Panics
/// Never panics. All errors are captured in FederatedResult::store_responses.
pub async fn federated_query(
    federation: &Federation,
    query: &QueryExpr,
) -> Result<FederatedResult, FederationError> {
    let monotonicity = classify_query(query);

    match monotonicity {
        QueryMonotonicity::Monotonic => {
            // Fan-out to all stores concurrently
            let futures: Vec<_> = federation.stores.iter()
                .map(|handle| query_store(handle, query))
                .collect();
            let responses = join_all(futures).await;

            // Merge results via set union (correct by CALM)
            let mut merged = QueryResult::empty();
            let mut store_responses = Vec::with_capacity(responses.len());
            let mut any_ok = false;

            for (i, response) in responses.into_iter().enumerate() {
                match response {
                    Ok(result) => {
                        any_ok = true;
                        store_responses.push(StoreResponse {
                            store_id: federation.stores[i].id(),
                            latency: result.latency,
                            datom_count: result.datom_count,
                            status: ResponseStatus::Ok,
                        });
                        merged = merged.union(result.data);
                    }
                    Err(StoreError::Timeout(elapsed)) => {
                        store_responses.push(StoreResponse {
                            store_id: federation.stores[i].id(),
                            latency: elapsed,
                            datom_count: 0,
                            status: ResponseStatus::Timeout,
                        });
                    }
                    Err(e) => {
                        store_responses.push(StoreResponse {
                            store_id: federation.stores[i].id(),
                            latency: Duration::ZERO,
                            datom_count: 0,
                            status: ResponseStatus::Error(e.to_string()),
                        });
                    }
                }
            }

            if !any_ok {
                return Err(FederationError::AllStoresTimedOut);
            }

            let partial = store_responses.iter()
                .any(|r| r.status != ResponseStatus::Ok);

            Ok(FederatedResult {
                results: merged,
                store_responses,
                partial,
            })
        }
        QueryMonotonicity::NonMonotonic => {
            // Materialize all stores, then query locally
            let full_store = federation.materialize().await?;
            let result = eval_query(&full_store, query)?;
            Ok(FederatedResult {
                results: result,
                store_responses: federation.stores.iter()
                    .map(|h| StoreResponse {
                        store_id: h.id(),
                        latency: Duration::ZERO, // measured during materialize
                        datom_count: 0,
                        status: ResponseStatus::Ok,
                    })
                    .collect(),
                partial: false,
            })
        }
    }
}

#[kani::proof]
#[kani::unwind(10)]
fn federated_query_monotonic_correct() {
    let store_a: BTreeSet<Datom> = kani::any();
    let store_b: BTreeSet<Datom> = kani::any();
    kani::assume(store_a.len() <= 3 && store_b.len() <= 3);

    let attr: u64 = kani::any();

    // Union of stores, then query
    let union: BTreeSet<_> = store_a.union(&store_b).cloned().collect();
    let full_result: BTreeSet<_> = union.iter()
        .filter(|d| d.a == attr)
        .cloned().collect();

    // Query each, then union of results
    let result_a: BTreeSet<_> = store_a.iter()
        .filter(|d| d.a == attr)
        .cloned().collect();
    let result_b: BTreeSet<_> = store_b.iter()
        .filter(|d| d.a == attr)
        .cloned().collect();
    let federated_result: BTreeSet<_> = result_a.union(&result_b)
        .cloned().collect();

    assert_eq!(full_result, federated_result);
}
```

**Falsification**: A monotonic query `Q` and store set `{S₁, ..., Sₖ}` where
`query(⋃ᵢ Sᵢ) ≠ ⋃ᵢ query(Sᵢ)`. Specific failure modes:
- **Result loss**: a datom satisfying the query predicate in some `Sᵢ` is absent from the
  federated result (fan-out failed to reach that store, or result merge dropped it).
- **Result gain**: a datom NOT satisfying the query predicate in any `Sᵢ` appears in the
  federated result (spurious results from merge interaction).
- **Monotonicity misclassification**: a non-monotonic query is classified as monotonic,
  causing fan-out to produce incorrect results (e.g., `COUNT(*)` across stores gives
  sum of per-store counts instead of count of union).
- **Transport-dependent results**: the same query returns different results depending on
  whether a store is `StoreHandle::Local` vs `StoreHandle::Remote` (transport leak).
- **Order-dependent merge**: the order in which per-store results arrive affects the
  final result (violates commutativity of result union).

**proptest strategy**:
```rust
proptest! {
    #[test]
    fn federated_query_correct(
        stores in prop::collection::vec(
            prop::collection::btree_set(arb_datom(), 0..100),
            1..5,
        ),
        query_attr in arb_attribute(),
    ) {
        // Build federation
        let store_objs: Vec<Store> = stores.iter()
            .map(|datoms| Store::from_datoms(datoms.clone()))
            .collect();

        // Full union, then query
        let mut all_datoms = BTreeSet::new();
        for s in &stores {
            all_datoms.extend(s.iter().cloned());
        }
        let full_result: BTreeSet<_> = all_datoms.iter()
            .filter(|d| d.attribute == query_attr)
            .cloned().collect();

        // Per-store query, then union
        let federated_result: BTreeSet<_> = store_objs.iter()
            .flat_map(|s| s.datoms.iter()
                .filter(|d| d.attribute == query_attr)
                .cloned())
            .collect();

        prop_assert_eq!(full_result, federated_result,
            "Federated query violated CALM: query(union) != union(query)");
    }

    #[test]
    fn federated_query_result_order_independent(
        stores in prop::collection::vec(
            prop::collection::btree_set(arb_datom(), 0..50),
            2..4,
        ),
        query_attr in arb_attribute(),
        permutation_seed in any::<u64>(),
    ) {
        let store_objs: Vec<Store> = stores.iter()
            .map(|datoms| Store::from_datoms(datoms.clone()))
            .collect();

        // Query in original order
        let result_original: BTreeSet<_> = store_objs.iter()
            .flat_map(|s| s.datoms.iter()
                .filter(|d| d.attribute == query_attr)
                .cloned())
            .collect();

        // Query in permuted order
        let mut permuted = store_objs.clone();
        let mut rng = StdRng::seed_from_u64(permutation_seed);
        permuted.shuffle(&mut rng);

        let result_permuted: BTreeSet<_> = permuted.iter()
            .flat_map(|s| s.datoms.iter()
                .filter(|d| d.attribute == query_attr)
                .cloned())
            .collect();

        prop_assert_eq!(result_original, result_permuted,
            "Federated query depends on store order");
    }
}
```

**Lean theorem**:
```lean
/-- Federated query correctness: for monotonic queries (modeled as filter
    predicates), querying the union of stores equals the union of per-store
    queries. This is a direct generalization of INV-FERR-033 from shards
    to federated stores. -/

-- Two-store case (base for induction)
theorem federated_query_two (s1 s2 : DatomStore) (p : Datom → Prop)
    [DecidablePred p] :
    (s1 ∪ s2).filter p = s1.filter p ∪ s2.filter p := by
  exact Finset.filter_union s1 s2 p

-- N-store case (generalized)
theorem federated_query_n (stores : Finset (Fin k)) (f : Fin k → DatomStore)
    (p : Datom → Prop) [DecidablePred p] :
    (stores.biUnion f).filter p = stores.biUnion (fun i => (f i).filter p) := by
  induction stores using Finset.induction with
  | empty => simp
  | insert ha ih =>
    simp [Finset.biUnion_insert]
    rw [Finset.filter_union]
    congr 1
    exact ih

-- Commutativity of result merge (order-independence)
theorem federated_result_comm (r1 r2 : Finset Result) :
    r1 ∪ r2 = r2 ∪ r1 := by
  exact Finset.union_comm r1 r2

-- Associativity of result merge (grouping-independence)
theorem federated_result_assoc (r1 r2 r3 : Finset Result) :
    (r1 ∪ r2) ∪ r3 = r1 ∪ (r2 ∪ r3) := by
  exact Finset.union_assoc r1 r2 r3
```

---

### INV-FERR-038: Federation Substrate Transparency

**Traces to**: SEED.md §4 (Substrate Independence — C8), INV-FERR-037, ADR-FERR-007
**Verification**: `V:PROP`, `V:MODEL`
**Stage**: 0

#### Level 0 (Algebraic Law)
```
Let Transport be the trait abstracting store access.
Let Local : Transport and Remote : Transport be two implementations.
Let query : Transport → QueryExpr → Result be the query function.

∀ Q ∈ QueryExpr, ∀ S ∈ DatomStore:
  query(Local(S), Q) = query(Remote(S), Q)

More generally, for any federation F = {H₁, ..., Hₖ} where each Hᵢ is
either Local(Sᵢ) or Remote(Sᵢ):

  federated_query(F, Q) = federated_query(F', Q)

where F' is any re-labeling of handles (swapping Local ↔ Remote) as long
as each Hᵢ and H'ᵢ refer to the same underlying store Sᵢ.

The Transport layer is a faithful functor: it preserves the algebraic
structure of the store. Transport ∘ query = query ∘ Transport = query.
```

#### Level 1 (State Invariant)
For all reachable stores `S` and all queries `Q`: the query result is identical
regardless of whether `S` is accessed via `StoreHandle::Local`, `StoreHandle::Remote`
with TCP transport, `StoreHandle::Remote` with QUIC transport, `StoreHandle::Remote`
with Unix socket transport, or any other `Transport` implementation. The only observable
differences are:
- **Latency**: Remote transports add network round-trip time.
- **StoreResponse metadata**: The `latency` field and `status` field reflect transport
  characteristics. But the `results` field is identical.

Application code that depends only on `FederatedResult::results` (and not on
`FederatedResult::store_responses`) produces identical behavior regardless of
deployment topology. This is the substrate transparency guarantee.

#### Level 2 (Implementation Contract)
```rust
/// The Transport trait: all store access goes through this.
/// Implementations: LocalTransport, TcpTransport, QuicTransport,
/// GrpcTransport, UnixSocketTransport.
///
/// The trait contract: for any store S and query Q,
/// transport.query(Q) returns the same result as S.query(Q).
///
/// # Errors
/// Transport errors (network, timeout, protocol) are distinct from
/// query errors (invalid query, schema mismatch). The caller can
/// distinguish them via FerraError variants.
#[async_trait]
pub trait Transport: Send + Sync + 'static {
    /// Execute a query against the remote store.
    async fn query(&self, expr: &QueryExpr) -> Result<TransportResult, TransportError>;

    /// Fetch datoms matching a filter (for selective merge).
    async fn fetch_datoms(&self, filter: &DatomFilter) -> Result<Vec<Datom>, TransportError>;

    /// Fetch the current schema of the remote store.
    async fn schema(&self) -> Result<Schema, TransportError>;

    /// Fetch the current epoch/frontier of the remote store.
    async fn frontier(&self) -> Result<Frontier, TransportError>;

    /// Stream WAL entries from a given epoch (for live migration).
    async fn stream_wal(&self, from_epoch: Epoch) -> Result<WalStream, TransportError>;

    /// Health check: is the remote store reachable?
    async fn ping(&self) -> Result<Duration, TransportError>;
}

/// Local transport: in-process, zero-copy, zero-latency.
pub struct LocalTransport {
    db: Arc<Database>,
}

#[async_trait]
impl Transport for LocalTransport {
    async fn query(&self, expr: &QueryExpr) -> Result<TransportResult, TransportError> {
        let snapshot = self.db.snapshot();
        let result = eval_query(&snapshot, expr)
            .map_err(TransportError::QueryFailed)?;
        Ok(TransportResult {
            data: result,
            latency: Duration::ZERO,
            datom_count: snapshot.datom_count(),
        })
    }
    // ... other methods delegate to Database directly
}

/// TCP transport: LAN/datacenter, persistent connections, reconnect.
pub struct TcpTransport {
    addr: SocketAddr,
    pool: ConnectionPool,
    timeout: Duration,
}

/// Verify transport transparency: same query, same store, different transports.
#[cfg(test)]
fn verify_transport_transparency(
    store: &Store,
    query: &QueryExpr,
    transport_a: &dyn Transport,
    transport_b: &dyn Transport,
) -> bool {
    let result_a = block_on(transport_a.query(query)).unwrap();
    let result_b = block_on(transport_b.query(query)).unwrap();
    result_a.data == result_b.data
}
```

**Falsification**: A query `Q` and store `S` where `query(Local(S), Q)` produces a
different result set than `query(Remote(S), Q)`. Specific failure modes:
- **Serialization loss**: a Value variant is not correctly serialized/deserialized over
  the wire (e.g., `Value::Bytes(Arc<[u8]>)` loses trailing zeros, or `Value::Keyword`
  case is altered).
- **Encoding divergence**: local and remote paths use different Datom serialization
  formats, producing different hash-based comparisons.
- **Query plan divergence**: the remote side uses a different query plan that produces
  different results for edge cases (e.g., different join ordering affects deduplication).
- **Schema version mismatch**: the remote side has a schema evolution that the local
  side hasn't received, causing different attribute resolution.

**proptest strategy**:
```rust
proptest! {
    #[test]
    fn transport_transparency(
        datoms in prop::collection::btree_set(arb_datom(), 0..100),
        query_attr in arb_attribute(),
    ) {
        let store = Store::from_datoms(datoms);
        let db = Database::from_store(store.clone());

        let local = LocalTransport::new(Arc::new(db.clone()));

        // Simulate remote: serialize query, send to store, deserialize result
        let remote = LoopbackTransport::new(Arc::new(db));

        let query = QueryExpr::attribute_filter(query_attr);
        let result_local = block_on(local.query(&query)).unwrap();
        let result_remote = block_on(remote.query(&query)).unwrap();

        prop_assert_eq!(result_local.data, result_remote.data,
            "Transport transparency violated: local != remote for same store and query");
    }

    #[test]
    fn value_roundtrip_over_transport(
        value in arb_value(),
    ) {
        let bytes = value.serialize_transport();
        let roundtripped = Value::deserialize_transport(&bytes).unwrap();
        prop_assert_eq!(value, roundtripped,
            "Value lost fidelity through transport serialization");
    }
}
```

**Lean theorem**:
```lean
/-- Transport transparency: Local and Remote are faithful functors.
    We model this as: any function f applied to a store S produces the
    same result regardless of the transport wrapper. -/

-- Transport is modeled as the identity morphism on DatomStore.
-- The algebraic content passes through unchanged.
def local_transport (s : DatomStore) : DatomStore := s
def remote_transport (s : DatomStore) : DatomStore := s

theorem transport_transparency (s : DatomStore) (f : DatomStore → α) :
    f (local_transport s) = f (remote_transport s) := by
  unfold local_transport remote_transport

-- Applied to query (filter)
theorem transport_query_equiv (s : DatomStore) (p : Datom → Prop)
    [DecidablePred p] :
    (local_transport s).filter p = (remote_transport s).filter p := by
  unfold local_transport remote_transport
```

---

### INV-FERR-039: Selective Merge (Knowledge Transfer)

**Traces to**: SEED.md §4 (CRDT merge = set union), INV-FERR-001 through INV-FERR-003,
SEED.md §10 (calibrated policies are transferable)
**Verification**: `V:PROP`, `V:KANI`, `V:LEAN`
**Stage**: 0

#### Level 0 (Algebraic Law)
```
Let filter : Datom → Bool be a predicate selecting datoms.
Let selective_merge(local, remote, filter) = local ∪ {d ∈ remote | filter(d)}

Theorem: selective_merge preserves CRDT properties.

Proof:
  Let R_f = {d ∈ remote | filter(d)} ⊆ remote.
  selective_merge(local, remote, filter) = local ∪ R_f.

  Since R_f ⊆ remote and R_f is a set:
  1. Commutativity of the merge component:
     local ∪ R_f is a union of two sets, which is commutative.
  2. Associativity: (local ∪ R_f₁) ∪ R_f₂ = local ∪ (R_f₁ ∪ R_f₂)
     by associativity of set union.
  3. Idempotency: local ∪ R_f ∪ R_f = local ∪ R_f
     by idempotency of set union.
  4. Monotonicity: local ⊆ selective_merge(local, remote, filter)
     since A ⊆ A ∪ B for any B.

The key insight: filtering before union does not violate any CRDT property
because the filter is applied to the SOURCE, not to the RESULT. The
operation is still "add some datoms" — just fewer of them.

Corollary: selective_merge with filter = (λd. true) reduces to full merge.
Corollary: selective_merge with filter = (λd. false) is the identity on local.
```

#### Level 1 (State Invariant)
For all reachable stores `(local, remote)` and all filters `f`:
- `local ⊆ selective_merge(local, remote, f)` (monotonicity — no datoms lost from local).
- `selective_merge(local, remote, f) ⊆ local ∪ remote` (no datoms invented).
- `{d ∈ selective_merge(local, remote, f) | d ∉ local} ⊆ {d ∈ remote | f(d)}`
  (only filtered datoms from remote are added).
- Repeated selective_merge with the same filter is idempotent.
- The order of multiple selective_merges from different remotes does not affect the
  final state (commutativity of union applies to filtered subsets too).

Selective merge is the mechanism for knowledge transfer across organizational boundaries.
It enables scenarios like: "Import the calibrated policy weights from the production
team's store without importing their task backlog."

#### Level 2 (Implementation Contract)
```rust
/// A filter predicate for selective merge.
/// Filters operate on datom metadata (entity, attribute, value, tx, op).
#[derive(Debug, Clone)]
pub enum DatomFilter {
    /// Accept all datoms (equivalent to full merge)
    All,
    /// Accept datoms with attributes in the given namespace prefixes
    AttributeNamespace(Vec<String>),
    /// Accept datoms with entity IDs in the given set
    Entities(BTreeSet<EntityId>),
    /// Accept datoms from transactions by specific agents
    FromAgents(BTreeSet<AgentId>),
    /// Accept datoms from transactions after a given epoch
    AfterEpoch(Epoch),
    /// Conjunction: all sub-filters must match
    And(Vec<DatomFilter>),
    /// Disjunction: any sub-filter must match
    Or(Vec<DatomFilter>),
    /// Negation: invert the filter
    Not(Box<DatomFilter>),
    /// Custom predicate (for application-specific filtering)
    Custom(Arc<dyn Fn(&Datom) -> bool + Send + Sync>),
}

impl DatomFilter {
    /// Evaluate the filter against a datom.
    pub fn matches(&self, datom: &Datom, schema: &Schema) -> bool {
        match self {
            DatomFilter::All => true,
            DatomFilter::AttributeNamespace(prefixes) => {
                prefixes.iter().any(|p| datom.attribute.starts_with(p))
            }
            DatomFilter::Entities(ids) => ids.contains(&datom.entity),
            DatomFilter::FromAgents(agents) => agents.contains(&datom.tx.agent),
            DatomFilter::AfterEpoch(epoch) => datom.tx.wall_time > epoch.0,
            DatomFilter::And(filters) => {
                filters.iter().all(|f| f.matches(datom, schema))
            }
            DatomFilter::Or(filters) => {
                filters.iter().any(|f| f.matches(datom, schema))
            }
            DatomFilter::Not(inner) => !inner.matches(datom, schema),
            DatomFilter::Custom(pred) => pred(datom),
        }
    }
}

/// Merge receipt: documents what was transferred.
pub struct MergeReceipt {
    pub source_store: StoreId,
    pub target_store: StoreId,
    pub datoms_transferred: usize,
    pub datoms_filtered_out: usize,
    pub datoms_already_present: usize,
    pub filter_applied: DatomFilter,
    pub duration: Duration,
}

/// Perform selective merge: import filtered datoms from remote into local.
///
/// # Guarantees
/// - local is monotonically non-decreasing (no datoms removed) (INV-FERR-004).
/// - Only datoms matching the filter are transferred.
/// - Transferred datoms retain their original TxId (INV-FERR-040).
/// - The operation is idempotent: repeating it is a no-op.
///
/// # Errors
/// Returns error if the remote store's schema is incompatible (INV-FERR-043).
pub async fn selective_merge(
    local: &mut Database,
    remote: &dyn Transport,
    filter: &DatomFilter,
) -> Result<MergeReceipt, FederationError> {
    // Step 1: Verify schema compatibility
    let remote_schema = remote.schema().await?;
    verify_schema_compatibility(local.schema(), &remote_schema)?;

    // Step 2: Fetch matching datoms from remote
    let remote_datoms = remote.fetch_datoms(filter).await?;

    // Step 3: Compute delta (datoms not already in local)
    let local_snapshot = local.snapshot();
    let mut to_add = Vec::new();
    let mut already_present = 0;

    for datom in &remote_datoms {
        if local_snapshot.contains(datom) {
            already_present += 1;
        } else {
            to_add.push(datom.clone());
        }
    }

    let filtered_out = remote_datoms.len() - to_add.len() - already_present;

    // Step 4: Apply datoms to local store
    if !to_add.is_empty() {
        local.apply_datoms(to_add.clone())?;
    }

    Ok(MergeReceipt {
        source_store: remote.id(),
        target_store: local.id(),
        datoms_transferred: to_add.len(),
        datoms_filtered_out: filtered_out,
        datoms_already_present: already_present,
        filter_applied: filter.clone(),
        duration: Duration::ZERO, // filled by caller
    })
}

#[kani::proof]
#[kani::unwind(10)]
fn selective_merge_monotonic() {
    let local: BTreeSet<Datom> = kani::any();
    let remote: BTreeSet<Datom> = kani::any();
    kani::assume(local.len() <= 3 && remote.len() <= 3);

    let filter_attr: u64 = kani::any();
    let filtered_remote: BTreeSet<_> = remote.iter()
        .filter(|d| d.a == filter_attr)
        .cloned().collect();

    let result: BTreeSet<_> = local.union(&filtered_remote).cloned().collect();

    // Monotonicity: local is a subset of result
    for d in &local {
        assert!(result.contains(d));
    }

    // No invention: result is a subset of local ∪ remote
    let full_union: BTreeSet<_> = local.union(&remote).cloned().collect();
    for d in &result {
        assert!(full_union.contains(d));
    }
}
```

**Falsification**: A selective_merge operation where:
- A datom in `local` before the merge is absent after (monotonicity violation).
- A datom in the result is not in `local ∪ remote` (datom invention).
- A datom in the result is not in `local` and does not match the filter but is from
  `remote` (filter bypass).
- Repeating the same selective_merge changes the store (idempotency violation).
- The same selective_merge with different argument order produces different results
  when the filter selects the same datoms (commutativity violation).

**proptest strategy**:
```rust
proptest! {
    #[test]
    fn selective_merge_preserves_local(
        local_datoms in prop::collection::btree_set(arb_datom(), 0..100),
        remote_datoms in prop::collection::btree_set(arb_datom(), 0..100),
        filter_prefix in "[a-z]{1,3}",
    ) {
        let local = Store::from_datoms(local_datoms.clone());
        let remote = Store::from_datoms(remote_datoms.clone());
        let filter = DatomFilter::AttributeNamespace(vec![filter_prefix]);

        let result = selective_merge_sync(&local, &remote, &filter);

        // Every local datom is preserved
        for d in &local_datoms {
            prop_assert!(result.datom_set().contains(d),
                "Local datom lost during selective merge");
        }
    }

    #[test]
    fn selective_merge_only_filtered(
        local_datoms in prop::collection::btree_set(arb_datom(), 0..100),
        remote_datoms in prop::collection::btree_set(arb_datom(), 0..100),
        filter_prefix in "[a-z]{1,3}",
    ) {
        let local = Store::from_datoms(local_datoms.clone());
        let remote = Store::from_datoms(remote_datoms.clone());
        let filter = DatomFilter::AttributeNamespace(vec![filter_prefix.clone()]);

        let result = selective_merge_sync(&local, &remote, &filter);

        // Every datom in result that's not in local must match the filter
        for d in result.datom_set() {
            if !local_datoms.contains(d) {
                prop_assert!(d.attribute.starts_with(&filter_prefix),
                    "Non-filtered datom {} imported from remote", d);
                prop_assert!(remote_datoms.contains(d),
                    "Datom not from local or remote");
            }
        }
    }

    #[test]
    fn selective_merge_idempotent(
        local_datoms in prop::collection::btree_set(arb_datom(), 0..50),
        remote_datoms in prop::collection::btree_set(arb_datom(), 0..50),
        filter_prefix in "[a-z]{1,3}",
    ) {
        let local = Store::from_datoms(local_datoms);
        let remote = Store::from_datoms(remote_datoms);
        let filter = DatomFilter::AttributeNamespace(vec![filter_prefix]);

        let once = selective_merge_sync(&local, &remote, &filter);
        let twice = selective_merge_sync(&once, &remote, &filter);

        prop_assert_eq!(once.datom_set(), twice.datom_set(),
            "Selective merge is not idempotent");
    }

    #[test]
    fn selective_merge_all_equals_full_merge(
        local_datoms in prop::collection::btree_set(arb_datom(), 0..50),
        remote_datoms in prop::collection::btree_set(arb_datom(), 0..50),
    ) {
        let local = Store::from_datoms(local_datoms.clone());
        let remote = Store::from_datoms(remote_datoms.clone());

        let selective = selective_merge_sync(&local, &remote, &DatomFilter::All);
        let full = merge(&local, &remote);

        prop_assert_eq!(selective.datom_set(), full.datom_set(),
            "Selective merge with All filter != full merge");
    }
}
```

**Lean theorem**:
```lean
/-- Selective merge: local ∪ filter(remote) preserves CRDT properties. -/

-- Selective merge is union with a filtered subset
def selective_merge (local remote : DatomStore) (filter : Datom → Prop)
    [DecidablePred filter] : DatomStore :=
  local ∪ remote.filter filter

-- Monotonicity: local is always a subset of the result
theorem selective_merge_mono (local remote : DatomStore) (filter : Datom → Prop)
    [DecidablePred filter] :
    local ⊆ selective_merge local remote filter := by
  unfold selective_merge
  exact Finset.subset_union_left

-- No invention: result is a subset of local ∪ remote
theorem selective_merge_bounded (local remote : DatomStore) (filter : Datom → Prop)
    [DecidablePred filter] :
    selective_merge local remote filter ⊆ local ∪ remote := by
  unfold selective_merge
  apply Finset.union_subset_union_right
  exact Finset.filter_subset filter remote

-- Idempotency: repeating selective merge is a no-op
theorem selective_merge_idemp (local remote : DatomStore) (filter : Datom → Prop)
    [DecidablePred filter] :
    selective_merge (selective_merge local remote filter) remote filter
    = selective_merge local remote filter := by
  unfold selective_merge
  rw [Finset.union_assoc]
  rw [Finset.union_idempotent (remote.filter filter)]
  sorry -- may need Finset.union_self for the filtered part

-- filter = true reduces to full merge
theorem selective_merge_all (local remote : DatomStore) :
    selective_merge local remote (fun _ => True) = local ∪ remote := by
  unfold selective_merge
  simp [Finset.filter_true_of_mem]

-- filter = false is identity on local
theorem selective_merge_none (local remote : DatomStore) :
    selective_merge local remote (fun _ => False) = local := by
  unfold selective_merge
  simp [Finset.filter_false]
  exact Finset.union_empty local
```

---

### INV-FERR-040: Merge Provenance Preservation

**Traces to**: SEED.md §4 (Traceability — C5), INV-FERR-001 through INV-FERR-003,
INV-FERR-012 (Content-Addressed Identity)
**Verification**: `V:PROP`, `V:KANI`, `V:LEAN`
**Stage**: 0

#### Level 0 (Algebraic Law)
```
Let D = [e, a, v, tx, op] be a datom with tx = (wall_time, logical, agent).
Let merge(A, B) = A ∪ B.

∀ d ∈ merge(A, B):
  d.tx = d_original.tx   where d_original is the datom as created by its originating agent

Proof:
  merge = set union. Set union does not modify elements.
  Therefore tx (including the agent field) is preserved exactly.

Corollary: For any datom d in a federated query result, d.tx.agent identifies
the agent that originally created d, regardless of how many merges and
selective merges d has passed through.
```

#### Level 1 (State Invariant)
For all reachable stores resulting from any sequence of TRANSACT, MERGE,
selective_merge, and federated query operations: every datom retains its original
`TxId` unchanged. The `TxId` is part of the datom's identity (INV-FERR-012:
content-addressed identity includes `tx`), so any modification would create a
different datom, violating content-addressed identity.

Provenance preservation enables cross-organizational auditing: "which agent, at
which time, on which machine, first observed this fact?" The answer is always
available via `d.tx.agent` and `d.tx.wall_time`, even after the datom has been
merged through dozens of intermediate stores.

#### Level 2 (Implementation Contract)
```rust
/// Merge two stores. Every datom retains its original TxId.
/// No TxId is modified, rewritten, or re-stamped during merge.
///
/// # Invariant
/// For every datom d in the result:
///   d.tx == (the TxId from d's originating transaction)
///
/// This is structural: merge = set union, and union does not modify elements.
/// Any merge implementation that rewrites TxIds is INCORRECT.
pub fn merge(a: &Store, b: &Store) -> Store {
    // BTreeSet::union preserves elements without modification
    let merged: BTreeSet<Datom> = a.datoms.union(&b.datoms).cloned().collect();
    Store::from_datoms(merged)
}

/// Query: given a datom, return the agent that originally created it.
/// This works across any number of merges because TxId is immutable.
pub fn provenance_agent(datom: &Datom) -> AgentId {
    datom.tx.agent
}

/// Query: given a datom, return the wall-clock time of original creation.
pub fn provenance_time(datom: &Datom) -> u64 {
    datom.tx.wall_time
}

/// Query: which store(s) contributed a given datom?
/// Uses the agent field of TxId to trace origin.
pub fn provenance_trace(
    datom: &Datom,
    federation: &Federation,
) -> Vec<StoreId> {
    federation.stores.iter()
        .filter(|h| {
            let snapshot = h.snapshot();
            snapshot.contains(datom)
        })
        .map(|h| h.id())
        .collect()
}

#[kani::proof]
#[kani::unwind(10)]
fn merge_preserves_provenance() {
    let a: BTreeSet<Datom> = kani::any();
    let b: BTreeSet<Datom> = kani::any();
    kani::assume(a.len() <= 3 && b.len() <= 3);

    let merged: BTreeSet<_> = a.union(&b).cloned().collect();

    // Every datom in merged has the same tx as in its source
    for d in &merged {
        if a.contains(d) {
            let orig = a.iter().find(|x| *x == d).unwrap();
            assert_eq!(d.tx, orig.tx);
        }
        if b.contains(d) {
            let orig = b.iter().find(|x| *x == d).unwrap();
            assert_eq!(d.tx, orig.tx);
        }
    }
}
```

**Falsification**: A datom `d` produced by agent `A` at time `t` that, after passing
through one or more merge/selective_merge operations, has `d.tx.agent != A` or
`d.tx.wall_time != t`. Specific failure modes:
- **Re-stamping**: the merge implementation creates a new TxId for merged datoms
  (e.g., to record "when the merge happened" rather than "when the datom was created").
- **Agent rewriting**: the selective_merge implementation replaces the remote agent ID
  with the local agent ID (claiming ownership of remote knowledge).
- **TxId normalization**: a serialization/deserialization round-trip through a transport
  layer normalizes TxId fields (e.g., truncating agent to 8 bytes instead of 16).
- **Content hash collision**: two different datoms produce the same content hash
  (INV-FERR-012 violation), causing the merge to drop one and keep the other with
  a different TxId.

**proptest strategy**:
```rust
proptest! {
    #[test]
    fn merge_preserves_all_txids(
        a_datoms in prop::collection::btree_set(arb_datom(), 0..100),
        b_datoms in prop::collection::btree_set(arb_datom(), 0..100),
    ) {
        let a = Store::from_datoms(a_datoms.clone());
        let b = Store::from_datoms(b_datoms.clone());
        let merged = merge(&a, &b);

        for d in merged.datom_set() {
            // Every datom in merged must have the exact tx from its source
            let in_a = a_datoms.iter().find(|x| x.entity == d.entity
                && x.attribute == d.attribute
                && x.value == d.value
                && x.op == d.op);
            let in_b = b_datoms.iter().find(|x| x.entity == d.entity
                && x.attribute == d.attribute
                && x.value == d.value
                && x.op == d.op);

            let source_tx = in_a.or(in_b)
                .expect("Datom in merged not found in either source");
            prop_assert_eq!(d.tx, source_tx.tx,
                "TxId changed during merge: {:?} -> {:?}", source_tx.tx, d.tx);
        }
    }

    #[test]
    fn selective_merge_preserves_txids(
        local_datoms in prop::collection::btree_set(arb_datom(), 0..50),
        remote_datoms in prop::collection::btree_set(arb_datom(), 0..50),
        filter_prefix in "[a-z]{1,3}",
    ) {
        let local = Store::from_datoms(local_datoms.clone());
        let remote = Store::from_datoms(remote_datoms.clone());
        let filter = DatomFilter::AttributeNamespace(vec![filter_prefix]);

        let result = selective_merge_sync(&local, &remote, &filter);

        for d in result.datom_set() {
            if !local_datoms.contains(d) {
                // Came from remote — tx must be the remote's original tx
                let remote_orig = remote_datoms.iter().find(|x| x == &d)
                    .expect("Datom not from local or remote");
                prop_assert_eq!(d.tx, remote_orig.tx,
                    "TxId changed during selective merge");
            }
        }
    }
}
```

**Lean theorem**:
```lean
/-- Merge provenance preservation: set union does not modify elements,
    so TxId (and all other datom fields) are preserved exactly. -/

-- Model: a datom's tx field is a projection
def tx_of (d : Datom) : TxId := d.tx

-- Union preserves membership and identity
theorem merge_preserves_tx (a b : DatomStore) (d : Datom) (h : d ∈ a ∪ b) :
    ∃ s ∈ ({a, b} : Finset DatomStore), d ∈ s := by
  rw [Finset.mem_union] at h
  cases h with
  | inl ha => exact ⟨a, Finset.mem_insert_self a {b}, ha⟩
  | inr hb => exact ⟨b, Finset.mem_insert.mpr (Or.inr (Finset.mem_singleton_iff.mpr rfl)), hb⟩

-- Key insight: union does not create new elements
theorem union_no_invention (a b : DatomStore) (d : Datom) (h : d ∈ a ∪ b) :
    d ∈ a ∨ d ∈ b := by
  exact Finset.mem_union.mp h

-- Therefore: d.tx is unchanged (it's the same element, not a copy with modified fields)
-- This is structural: Finset.union returns elements from a or b, not new constructions.
```

---

### INV-FERR-041: Transport Latency Tolerance

**Traces to**: SEED.md §4, INV-FERR-034 through INV-FERR-036 (Partition Tolerance),
INV-FERR-037 (Federated Query Correctness)
**Verification**: `V:PROP`, `V:MODEL`
**Stage**: 0

#### Level 0 (Algebraic Law)
```
Let timeout : StoreHandle → Duration be the per-store timeout configuration.
Let respond(Sᵢ, Q, t) be true iff store Sᵢ responds to query Q within time t.

∀ federation F = {S₁, ..., Sₖ}, ∀ monotonic Q:
  Let R = {Sᵢ | respond(Sᵢ, Q, timeout(Sᵢ))} be the responding stores.
  Let T = {Sᵢ | ¬respond(Sᵢ, Q, timeout(Sᵢ))} be the timed-out stores.

  If R ≠ ∅:
    federated_query(F, Q).results = ⋃ᵢ∈R query(Sᵢ)
    federated_query(F, Q).partial = (T ≠ ∅)
    federated_query(F, Q).store_responses contains per-store status

  If R = ∅:
    federated_query(F, Q) = Err(AllStoresTimedOut)

The partial result is a VALID SUBSET of the full federated result:
  federated_query(F, Q).results ⊆ ⋃ᵢ query(Sᵢ)  (for all i, not just R)

The caller decides whether partial results are acceptable for their use case.
```

#### Level 1 (State Invariant)
For all reachable federation states and all queries: the federation layer NEVER
blocks indefinitely waiting for a slow or unreachable store. Each store has a
configurable timeout (default: 30 seconds). Stores that do not respond within
their timeout are marked as `ResponseStatus::Timeout` in the `store_responses`
vector. The overall result is still returned (with `partial: true`) as long as
at least one store responded.

The partial result is always a valid subset: it contains exactly the datoms
matching the query from the stores that responded. It never contains datoms
from stores that timed out (no stale cache, no speculative results). The
`store_responses` vector provides full transparency: the caller can see
exactly which stores contributed and which did not.

For non-monotonic queries, partial results are NOT returned (since the query
requires the full datom set). If any store times out during materialization
for a non-monotonic query, the entire query fails with `MaterializationIncomplete`.

#### Level 2 (Implementation Contract)
```rust
/// Per-store response metadata.
pub struct StoreResponse {
    pub store_id: StoreId,
    pub latency: Duration,
    pub datom_count: usize,
    pub status: ResponseStatus,
}

/// Response status for a single store in a federated query.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResponseStatus {
    Ok,
    Timeout,
    Error(String),
    /// Store was skipped (e.g., query didn't need this shard)
    Skipped,
}

/// Federated query result with per-store metadata.
pub struct FederatedResult {
    /// Merged results from all responding stores.
    pub results: QueryResult,
    /// Per-store metadata: latency, datom count, status.
    pub store_responses: Vec<StoreResponse>,
    /// True if any store timed out or errored.
    /// The caller must check this and decide if partial results suffice.
    pub partial: bool,
    /// Timestamp of the federation snapshot (max TxId across responding stores).
    pub snapshot_timestamp: TxId,
}

/// Query a single store with timeout.
async fn query_store_with_timeout(
    handle: &StoreHandle,
    query: &QueryExpr,
    timeout: Duration,
) -> Result<TransportResult, StoreError> {
    match tokio::time::timeout(timeout, query_store(handle, query)).await {
        Ok(result) => result,
        Err(_elapsed) => Err(StoreError::Timeout(timeout)),
    }
}

/// Configuration for federation behavior.
pub struct FederationConfig {
    /// Default per-store timeout.
    pub default_timeout: Duration,
    /// Per-store timeout overrides.
    pub store_timeouts: HashMap<StoreId, Duration>,
    /// Whether to return partial results on timeout (monotonic queries only).
    pub allow_partial: bool,
    /// Maximum concurrent store queries (backpressure).
    pub max_concurrent: usize,
}

impl Default for FederationConfig {
    fn default() -> Self {
        FederationConfig {
            default_timeout: Duration::from_secs(30),
            store_timeouts: HashMap::new(),
            allow_partial: true,
            max_concurrent: 64,
        }
    }
}
```

**Falsification**: A federated query that blocks indefinitely when a store is
unreachable (timeout not enforced). Or: a partial result that contains datoms
from a timed-out store (stale cache served as fresh). Or: `partial` is `false`
when a store actually timed out (silent data loss). Specific failure modes:
- **Infinite hang**: the transport layer does not respect the timeout (e.g.,
  TCP keepalive holds the connection open indefinitely).
- **Phantom results**: a store times out mid-response, and the partially received
  datoms are included in the result (incomplete data masquerading as complete).
- **Silent timeout**: a store times out but `partial` is set to `false` (caller
  believes the result is complete).
- **Non-monotonic partial**: a non-monotonic query returns partial results from
  responding stores (aggregation on subset gives wrong answer).

**proptest strategy**:
```rust
proptest! {
    #[test]
    fn partial_result_is_subset_of_full(
        stores in prop::collection::vec(
            prop::collection::btree_set(arb_datom(), 0..50),
            2..5,
        ),
        responding_mask in prop::collection::vec(any::<bool>(), 2..5),
        query_attr in arb_attribute(),
    ) {
        let responding_mask = &responding_mask[..stores.len()];

        // Full result (all stores respond)
        let all_datoms: BTreeSet<_> = stores.iter()
            .flat_map(|s| s.iter().cloned())
            .collect();
        let full_result: BTreeSet<_> = all_datoms.iter()
            .filter(|d| d.attribute == query_attr)
            .cloned().collect();

        // Partial result (only responding stores)
        let partial_datoms: BTreeSet<_> = stores.iter()
            .zip(responding_mask.iter())
            .filter(|(_, &responds)| responds)
            .flat_map(|(s, _)| s.iter().cloned())
            .collect();
        let partial_result: BTreeSet<_> = partial_datoms.iter()
            .filter(|d| d.attribute == query_attr)
            .cloned().collect();

        // Partial is always a subset of full
        prop_assert!(partial_result.is_subset(&full_result),
            "Partial result is not a subset of full result");

        // If all respond, partial == full
        if responding_mask.iter().all(|&r| r) {
            prop_assert_eq!(partial_result, full_result,
                "All stores responded but results differ");
        }
    }

    #[test]
    fn timeout_metadata_accurate(
        store_count in 2..5usize,
        timeout_indices in prop::collection::hash_set(0..4usize, 0..3),
    ) {
        let timeout_indices: Vec<_> = timeout_indices.into_iter()
            .filter(|&i| i < store_count)
            .collect();

        // Simulate: some stores time out
        let responses: Vec<ResponseStatus> = (0..store_count)
            .map(|i| {
                if timeout_indices.contains(&i) {
                    ResponseStatus::Timeout
                } else {
                    ResponseStatus::Ok
                }
            })
            .collect();

        let partial = responses.iter().any(|r| *r != ResponseStatus::Ok);

        // partial must be true iff any store is not Ok
        prop_assert_eq!(partial, !timeout_indices.is_empty(),
            "partial flag inconsistent with store responses");
    }
}
```

**Lean theorem**:
```lean
/-- Latency tolerance: partial results are valid subsets.
    We model responding stores as a subset of all stores. -/

-- The result from responding stores is a subset of the full result
theorem partial_subset_full (stores : Finset (Fin k))
    (responding : Finset (Fin k))
    (h_sub : responding ⊆ stores)
    (f : Fin k → DatomStore)
    (p : Datom → Prop) [DecidablePred p] :
    (responding.biUnion f).filter p ⊆ (stores.biUnion f).filter p := by
  apply Finset.filter_subset_filter
  exact Finset.biUnion_subset_biUnion_of_subset_left f h_sub

-- When all stores respond, partial == full
theorem all_respond_equals_full (stores : Finset (Fin k))
    (f : Fin k → DatomStore)
    (p : Datom → Prop) [DecidablePred p] :
    (stores.biUnion f).filter p = (stores.biUnion f).filter p := by
  rfl
```

---

### INV-FERR-042: Live Migration (Substrate Transition)

**Traces to**: SEED.md §4 (Substrate Independence — C8), INV-FERR-038 (Transport Transparency),
INV-FERR-006 (Snapshot Isolation), INV-FERR-008 (WAL Ordering)
**Verification**: `V:PROP`, `V:MODEL`
**Stage**: 1

#### Level 0 (Algebraic Law)
```
Let S(t) be the store state at time t.
Let T_old and T_new be the old and new transport handles.
Let swap(t_s) be the atomic swap of transport handle at time t_s.

∀ query Q issued at time t:
  If t < t_s:  query uses T_old, sees S(t) via T_old
  If t ≥ t_s:  query uses T_new, sees S(t) via T_new

Correctness condition:
  At the moment of swap, S_new(t_s) = S_old(t_s)
  i.e., the new location has caught up to the old location.

Process:
  1. t_start: begin streaming WAL from T_old to T_new
  2. t_catchup: T_new has replayed all WAL entries up to T_old's current epoch
  3. t_s: atomic swap — all new queries go to T_new
  4. t_s + drain: T_old remains read-only for in-flight queries
  5. t_decommission: T_old is shut down

Between t_start and t_s: new writes go to T_old, are streamed to T_new.
At t_s: T_new is at most 1 WAL frame behind T_old (bounded by stream latency).
The atomic swap is: `ArcSwap::store(new_handle)` — wait-free, lock-free.
```

#### Level 1 (State Invariant)
A store can be migrated from one transport to another (e.g., local to remote, TCP to
QUIC, machine A to machine B) without stopping queries. During migration:
- Existing queries that started before the swap continue to completion using the old
  transport (they hold a reference via `Arc`).
- New queries after the swap use the new transport.
- No query sees a "gap" (missing datoms) or a "split" (different results from old vs new).

The migration process is observable: the federation emits events for each phase
(streaming started, catchup complete, swap executed, drain complete, decommissioned).
The operator can monitor progress and abort if needed.

The key correctness condition is catchup completeness: at the moment of swap, the new
location must have all datoms that the old location has. Since the store is append-only
(C1), the new location only needs to process new WAL entries since streaming started.
WAL ordering (INV-FERR-008) guarantees that replay is deterministic.

#### Level 2 (Implementation Contract)
```rust
/// Live migration: move a store from one transport to another.
pub struct Migration {
    /// The store being migrated.
    store_id: StoreId,
    /// Old transport handle (source).
    old_handle: Arc<ArcSwap<StoreHandle>>,
    /// New transport handle (destination).
    new_transport: Box<dyn Transport>,
    /// Migration state machine.
    state: MigrationState,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MigrationState {
    /// Not started.
    Idle,
    /// WAL is being streamed from old to new.
    Streaming { from_epoch: Epoch, entries_sent: u64 },
    /// New location has caught up to old location.
    CaughtUp { epoch: Epoch },
    /// Transport handle has been swapped. Old is draining in-flight queries.
    Swapped { drain_deadline: Instant },
    /// Old transport decommissioned. Migration complete.
    Complete,
    /// Migration aborted. Old transport still active.
    Aborted { reason: String },
}

impl Migration {
    /// Start streaming WAL entries from old to new location.
    pub async fn start_streaming(&mut self) -> Result<(), MigrationError> {
        let current_epoch = self.old_handle.load().epoch().await?;
        let wal_stream = self.old_handle.load().stream_wal(Epoch(0)).await?;

        // Stream all WAL entries to new transport
        let mut entries_sent = 0u64;
        while let Some(entry) = wal_stream.next().await {
            self.new_transport.apply_wal_entry(entry?).await?;
            entries_sent += 1;
        }

        self.state = MigrationState::Streaming {
            from_epoch: current_epoch,
            entries_sent,
        };
        Ok(())
    }

    /// Catch up: stream any WAL entries written since streaming started.
    pub async fn catchup(&mut self) -> Result<(), MigrationError> {
        let MigrationState::Streaming { from_epoch, .. } = &self.state else {
            return Err(MigrationError::InvalidState);
        };

        let current_epoch = self.old_handle.load().epoch().await?;
        let delta_stream = self.old_handle.load()
            .stream_wal(*from_epoch).await?;

        while let Some(entry) = delta_stream.next().await {
            self.new_transport.apply_wal_entry(entry?).await?;
        }

        self.state = MigrationState::CaughtUp { epoch: current_epoch };
        Ok(())
    }

    /// Atomic swap: redirect all new queries to the new transport.
    /// In-flight queries on the old transport continue to completion.
    pub fn swap(&mut self, drain_timeout: Duration) -> Result<(), MigrationError> {
        let MigrationState::CaughtUp { .. } = &self.state else {
            return Err(MigrationError::InvalidState);
        };

        let new_handle = StoreHandle::Remote(RemoteStore {
            id: self.store_id,
            transport: self.new_transport.clone_boxed(),
            addr: self.new_transport.addr(),
            timeout: Duration::from_secs(30),
        });

        // ArcSwap::store is wait-free, lock-free, atomic
        self.old_handle.store(Arc::new(new_handle));

        self.state = MigrationState::Swapped {
            drain_deadline: Instant::now() + drain_timeout,
        };
        Ok(())
    }

    /// Decommission the old transport after drain period.
    pub async fn decommission(&mut self) -> Result<(), MigrationError> {
        let MigrationState::Swapped { drain_deadline } = &self.state else {
            return Err(MigrationError::InvalidState);
        };

        // Wait for drain period (in-flight queries complete)
        if Instant::now() < *drain_deadline {
            tokio::time::sleep_until((*drain_deadline).into()).await;
        }

        self.state = MigrationState::Complete;
        Ok(())
    }

    /// Abort migration. Old transport remains active. No data loss.
    pub fn abort(&mut self, reason: String) {
        self.state = MigrationState::Aborted { reason };
    }
}
```

**Falsification**: A query `Q` issued during migration that returns different results
than it would have without migration. Specific failure modes:
- **Gap**: a query issued immediately after swap sees fewer datoms than the old transport
  had (catchup incomplete).
- **Duplication**: a query sees the same datom twice (once from old transport, once from
  new) with different metadata.
- **Hang**: in-flight queries on the old transport never complete because the old
  transport is shut down before they finish.
- **Data loss**: WAL entries written between catchup and swap are lost (the new
  transport never receives them).
- **Ordering violation**: WAL entries are replayed out of order on the new transport,
  producing a different store state (violates INV-FERR-008).

**proptest strategy**:
```rust
proptest! {
    #[test]
    fn migration_preserves_datom_set(
        initial_datoms in prop::collection::btree_set(arb_datom(), 0..100),
        post_stream_datoms in prop::collection::btree_set(arb_datom(), 0..20),
    ) {
        let mut old_store = Store::from_datoms(initial_datoms.clone());

        // Simulate: stream initial state to new store
        let mut new_store = Store::from_datoms(initial_datoms.clone());

        // Simulate: new writes arrive on old store after streaming starts
        for d in &post_stream_datoms {
            old_store.insert(d.clone());
        }

        // Simulate: catchup streams the delta
        for d in &post_stream_datoms {
            new_store.insert(d.clone());
        }

        // After catchup: stores must be identical
        prop_assert_eq!(old_store.datom_set(), new_store.datom_set(),
            "New store diverged from old after catchup");
    }

    #[test]
    fn migration_abort_preserves_old(
        datoms in prop::collection::btree_set(arb_datom(), 0..100),
    ) {
        let store = Store::from_datoms(datoms.clone());

        // Simulate: start migration, then abort
        // Old store must be completely unchanged
        prop_assert_eq!(store.datom_set(), &datoms,
            "Abort modified the old store");
    }
}
```

**Lean theorem**:
```lean
/-- Live migration correctness: after catchup, old and new stores are equal.
    Since the store is append-only and WAL replay is deterministic,
    streaming + catchup produces an identical store. -/

-- Model: streaming is union of initial + delta
def stream_and_catchup (initial delta : DatomStore) : DatomStore :=
  initial ∪ delta

-- The old store after writes is also initial + delta
def old_after_writes (initial delta : DatomStore) : DatomStore :=
  initial ∪ delta

-- They are equal (by reflexivity of union)
theorem migration_correct (initial delta : DatomStore) :
    stream_and_catchup initial delta = old_after_writes initial delta := by
  unfold stream_and_catchup old_after_writes

-- Abort preserves the old store (no-op on old)
theorem migration_abort_safe (old_store : DatomStore) :
    old_store = old_store := by
  rfl
```

---

### §23.8.1: Federation API

The full Federation API surface, with types and method signatures.

```rust
/// Unique identifier for a store within a federation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Ord, PartialOrd)]
pub struct StoreId(pub [u8; 16]);

/// A federation of datom stores.
pub struct Federation {
    /// The stores in this federation, indexed by StoreId.
    stores: Vec<StoreHandle>,
    /// Configuration for federation behavior.
    config: FederationConfig,
}

/// A handle to a store: local (in-process) or remote (over transport).
pub enum StoreHandle {
    Local(Database),
    Remote(RemoteStore),
}

impl StoreHandle {
    pub fn id(&self) -> StoreId;
    pub async fn snapshot(&self) -> Result<Snapshot, TransportError>;
    pub async fn epoch(&self) -> Result<Epoch, TransportError>;
}

/// A remote store accessed via a transport layer.
pub struct RemoteStore {
    pub id: StoreId,
    pub transport: Box<dyn Transport>,
    pub addr: SocketAddr,
    pub timeout: Duration,
}

impl Federation {
    /// Create a new federation with no stores.
    pub fn new(config: FederationConfig) -> Self;

    /// Execute a federated query across all stores.
    /// Monotonic queries: fan-out + merge (INV-FERR-037).
    /// Non-monotonic queries: materialize + evaluate.
    pub async fn query(&self, expr: &QueryExpr) -> Result<FederatedResult, FederationError>;

    /// Selective merge: import filtered datoms from source into target (INV-FERR-039).
    pub async fn selective_merge(
        &self,
        target: &mut Database,
        source: StoreHandle,
        filter: DatomFilter,
    ) -> Result<MergeReceipt, FederationError>;

    /// Full materialization: merge all stores into a new local Database.
    /// Use for non-monotonic queries or when a complete local copy is needed.
    pub async fn materialize(&self) -> Result<Database, FederationError>;

    /// Add a store to the federation.
    pub fn add_store(&mut self, handle: StoreHandle);

    /// Remove a store from the federation.
    /// In-flight queries to this store will complete (they hold Arc references).
    pub fn remove_store(&mut self, id: StoreId);

    /// List all stores with their current status.
    pub async fn store_status(&self) -> Vec<(StoreId, StoreStatus)>;

    /// Live migration: move a store from one transport to another (INV-FERR-042).
    pub async fn migrate(
        &mut self,
        store_id: StoreId,
        new_transport: Box<dyn Transport>,
        drain_timeout: Duration,
    ) -> Result<(), MigrationError>;
}

/// Federated query result.
pub struct FederatedResult {
    /// Merged results from all responding stores.
    pub results: QueryResult,
    /// Per-store metadata: latency, datom count, status.
    pub store_responses: Vec<StoreResponse>,
    /// True if any store timed out or errored (INV-FERR-041).
    pub partial: bool,
}

/// Per-store response metadata.
pub struct StoreResponse {
    pub store_id: StoreId,
    pub latency: Duration,
    pub datom_count: usize,
    pub status: ResponseStatus,
}

/// Response status.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResponseStatus {
    Ok,
    Timeout,
    Error(String),
    Skipped,
}

/// Merge receipt from selective_merge.
pub struct MergeReceipt {
    pub source_store: StoreId,
    pub target_store: StoreId,
    pub datoms_transferred: usize,
    pub datoms_filtered_out: usize,
    pub datoms_already_present: usize,
    pub filter_applied: DatomFilter,
    pub duration: Duration,
}

/// Federation errors.
#[derive(Debug)]
pub enum FederationError {
    AllStoresTimedOut,
    MaterializationIncomplete { responding: usize, total: usize },
    SchemaIncompatible { local: Schema, remote: Schema, conflict: String },
    TransportError(TransportError),
    StoreNotFound(StoreId),
    MigrationFailed(MigrationError),
}
```

### §23.8.2: Performance Considerations

| Aspect | Characteristic | Bound |
|--------|---------------|-------|
| Fan-out parallelism | All stores queried concurrently via `tokio::join_all` | O(1) wall-clock for query dispatch |
| Result merge | Union of per-store result sets | O(sum of |R_i|) — linear in total result size |
| Network bandwidth | Only QUERY RESULTS cross the network, not full stores | O(|result|) per store, not O(|store|) |
| Federated query latency | P99 = max(per-store P99) + merge overhead | Bounded by slowest responding store + O(|result|) merge |
| Selective merge bandwidth | Only matching datoms transferred | O(|filter matches|), not O(|remote store|) |
| Materialization | Full merge of all stores into local Database | O(sum of |S_i|) — one-time cost |
| Connection pooling | Persistent connections to remote stores | Amortized 0 connection setup cost after first query |
| Incremental federation | Stores can join/leave without restarting | O(1) add/remove via `Arc` reference counting |
| Migration overhead | WAL streaming + catchup + atomic swap | O(|WAL delta|) for catchup, O(1) for swap |
| Backpressure | `max_concurrent` limits parallel store queries | Prevents thundering herd on large federations |

**Key insight**: The CRDT foundation means that federation has NO coordination cost
for monotonic queries. The CALM theorem guarantees that fan-out + merge is correct
without any locking, consensus, or distributed transaction protocol. The only
coordination point is non-monotonic queries, which require materialization.

### §23.8.3: Transport Layer Heterogeneity

All transports implement the same `Transport` trait (INV-FERR-038). The Federation
never inspects which concrete transport a `StoreHandle` uses.

| Transport | Use case | Characteristics |
|-----------|----------|-----------------|
| `LocalTransport` | Same-process federation | Zero-copy, zero-latency, no serialization. Direct `Arc<Database>` access. |
| `UnixSocketTransport` | Same-machine federation | Low-latency (~50us), no TLS needed, uses filesystem permissions for auth. Ideal for multi-process on same host. |
| `TcpTransport` | LAN/datacenter | Persistent connections, TCP keepalive, reconnect on failure. TLS optional. Connection pooling per remote. |
| `QuicTransport` | WAN/internet | Multiplexed streams, 0-RTT reconnect, built-in TLS. Handles NAT traversal. Best for cross-region federation. |
| `GrpcTransport` | Cloud services | Load balancing, service discovery, TLS, auth headers, health checking. Integrates with Kubernetes service mesh. |

**Transport selection guideline**: Use the simplest transport that meets latency and
security requirements. Start with `LocalTransport` for testing, `UnixSocketTransport`
for production on single machine, `TcpTransport` for LAN, `QuicTransport` for WAN.

**Wire format**: All transports use the same serialization format for `QueryExpr`,
`QueryResult`, `Datom`, and `Schema`. The format is a length-prefixed, BLAKE3-checksummed
binary encoding (same as checkpoint format, INV-FERR-013). This ensures that
transport transparency (INV-FERR-038) holds by construction — the serialization
layer is shared, not per-transport.

### §23.8.4: Dependency Injection & Substrate Migration

`StoreHandle` is a **runtime value**, not a compile-time type parameter. This enables:

1. **Runtime topology changes**: Add or remove stores without recompilation.
2. **Live migration** (INV-FERR-042): Swap a store's transport without stopping queries.
3. **Testing**: Inject `LoopbackTransport` (serializes and deserializes in-process) to
   test the full transport path without network infrastructure.
4. **Gradual rollout**: Migrate stores one-by-one from local to remote as the system scales.

**Migration process**:
1. Start new transport (e.g., provision remote machine, start ferratomic server).
2. Stream WAL from old location to new location.
3. Catch up: stream delta WAL entries written since step 2 started.
4. Atomic swap: `ArcSwap::store(new_handle)` — wait-free, lock-free.
5. Drain: old transport remains alive for in-flight queries (configurable drain period).
6. Decommission: shut down old transport after drain.

**Rollback**: If errors spike after swap, the migration can be aborted by swapping
back to the old handle. The old transport is kept alive during the drain period
specifically to enable rollback.

**Zero-downtime guarantee**: The `ArcSwap` pattern ensures that the swap itself is
a single atomic pointer write. No mutex, no condition variable, no query queue.
Queries in progress continue with their existing `Arc` reference; new queries
pick up the new handle immediately.

### §23.8.5: Knowledge Transfer Use Cases (Application-Level)

These scenarios demonstrate selective merge (INV-FERR-039) in practice:

| Scenario | Filter | Effect |
|----------|--------|--------|
| Learn calibrated policies from another project | `AttributeNamespace(vec!["policy/".into(), "calibration/".into()])` | Import policy weights and calibration data. Ignore tasks, observations, session history. |
| Import spec elements from a team | `AttributeNamespace(vec!["spec/".into(), "intent/".into()])` | Import INV/ADR/NEG definitions. Ignore implementation artifacts. |
| Federated search across all local projects | `Federation` over `LocalTransport` stores | Query all projects simultaneously. No data movement — queries fan out and results merge. |
| Cloud-scale agent coordination | `Federation` over `TcpTransport`/`QuicTransport` | Agents on different machines share knowledge through federated queries. Selective merge for deliberate knowledge transfer. |
| Offline work + sync | Local store accumulates datoms offline | On reconnect: `selective_merge(remote, local, All)` pushes local knowledge to shared store. `selective_merge(local, remote, filter)` pulls relevant updates. |
| Cross-organization knowledge exchange | `And(vec![AttributeNamespace(vec!["policy/"]), FromAgents(trusted_agents)])` | Import only policies from trusted agents. Defense in depth: namespace filter + agent filter. |

### §23.8.6: Security & Trust

#### INV-FERR-043: Schema Compatibility Check

**Traces to**: INV-FERR-009 (Schema Validation), INV-FERR-039 (Selective Merge)
**Verification**: `V:PROP`
**Stage**: 1

Before any merge (full or selective) between two stores, the schema compatibility
MUST be verified. Compatibility means:
- For every attribute present in BOTH schemas: the `ValueType`, `Cardinality`, and
  resolution mode must be identical.
- Attributes present in only one schema are always compatible (they will be added to
  the other schema upon merge).
- Schema evolution (adding new attributes) is always safe. Schema mutation (changing
  existing attribute types) is a compatibility failure.

```rust
/// Verify that two schemas are compatible for merge.
/// Returns Ok(()) if compatible, Err with conflict details if not.
pub fn verify_schema_compatibility(
    local: &Schema,
    remote: &Schema,
) -> Result<(), SchemaConflict> {
    for (attr, local_def) in local.attributes() {
        if let Some(remote_def) = remote.get(attr) {
            if local_def.value_type != remote_def.value_type {
                return Err(SchemaConflict::TypeMismatch {
                    attribute: attr.clone(),
                    local_type: local_def.value_type,
                    remote_type: remote_def.value_type,
                });
            }
            if local_def.cardinality != remote_def.cardinality {
                return Err(SchemaConflict::CardinalityMismatch {
                    attribute: attr.clone(),
                    local_card: local_def.cardinality,
                    remote_card: remote_def.cardinality,
                });
            }
        }
    }
    Ok(())
}
```

**Falsification**: A merge proceeds between two stores with incompatible schemas
(e.g., attribute `:task/priority` is `Long` in one store and `String` in another),
producing a store with conflicting attribute definitions.

#### INV-FERR-044: Namespace Isolation

**Traces to**: INV-FERR-039 (Selective Merge), C8 (Substrate Independence)
**Verification**: `V:PROP`
**Stage**: 1

Selective merge can restrict to specific attribute namespaces, providing defense
in depth against unintended knowledge transfer. The `AttributeNamespace` filter
uses prefix matching on attribute names (e.g., `"policy/"` matches `:policy/weight`,
`:policy/threshold`, etc.).

Namespace isolation is enforced at the filter level, not the transport level. The
transport transfers whatever datoms the filter selects. The caller is responsible
for constructing appropriate filters.

```rust
/// Restrict selective merge to specific namespaces.
/// Example: import only policy datoms from a remote store.
pub fn namespace_filter(namespaces: &[&str]) -> DatomFilter {
    DatomFilter::AttributeNamespace(
        namespaces.iter().map(|s| s.to_string()).collect()
    )
}
```

**Falsification**: A selective_merge with `AttributeNamespace(vec!["policy/"])` filter
that imports datoms with attributes outside the `policy/` namespace (e.g., `:task/title`).

**Future extensions** (not specified in this stage):
- Cryptographic provenance: TxIds signed by the originating agent's key pair. Receivers
  can verify that a datom was actually created by the claimed agent.
- Access control lists: per-namespace read/write permissions enforced at the transport
  layer. A remote store can refuse to serve datoms from restricted namespaces.
- Audit trail: every selective_merge operation is itself recorded as datoms in the
  receiving store (`:merge/source`, `:merge/filter`, `:merge/timestamp`, `:merge/count`).

### §23.8.7: Consistency Model

| Scope | Model | Guarantee |
|-------|-------|-----------|
| Single store | Snapshot isolation (INV-FERR-006) | Reads see a consistent point-in-time snapshot. Writes are linearizable (INV-FERR-007). |
| Within a federation (monotonic queries) | Linearizable by CALM | Fan-out + merge produces the same result as querying the merged store. No coordination needed. |
| Within a federation (non-monotonic queries) | Point-in-time | Full materialization creates a local snapshot. The result reflects the state of all stores at approximately the same time (bounded by materialization latency). |
| Across federations | Strong eventual consistency | CRDT guarantee (INV-FERR-001 through INV-FERR-003): any two stores that have received the same set of datoms (in any order) converge to the same state. |
| During live migration | Linearizable reads | The ArcSwap pattern ensures that every query sees a consistent store state. No query sees a "torn" state (half old, half new). |

**FederationSnapshot**: For non-monotonic federated queries, the system records a
`FederationSnapshot` timestamp: the maximum `TxId` across all responding stores at
the time of materialization. This timestamp enables reproducible queries: "what was
the federation-wide answer to Q at time T?"

```rust
/// A timestamp representing the state of the entire federation at a point in time.
#[derive(Debug, Clone)]
pub struct FederationSnapshot {
    /// Per-store TxId at the time of snapshot.
    pub store_epochs: BTreeMap<StoreId, TxId>,
    /// The maximum TxId across all stores (the federation "now").
    pub max_tx: TxId,
    /// Which stores contributed to this snapshot.
    pub participating_stores: BTreeSet<StoreId>,
}
```

---

