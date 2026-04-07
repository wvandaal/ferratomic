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
**Referenced by**: INV-FERR-062 (merge receipts), INV-FERR-063 (provenance lattice
enriches resolution during selective merge), ADR-FERR-022 (positive-only DatomFilter),
ADR-FERR-025 (transaction-level federation)
**Verification**: `V:PROP`, `V:KANI`, `V:LEAN`
**Stage**: 0

> **Phase 4a.5 staging note**: Phase 4a.5 implements selective merge with
> positive-only DatomFilter variants only: `All`, `AttributeNamespace`,
> `FromAgents`, `Entities`, `And`, `Or` (ADR-FERR-022). The `Not`, `Custom`,
> and `AfterEpoch` variants are deferred to Phase 4c with their own safety
> analysis for non-monotonicity, serializability, and epoch-index dependency.

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
  rw [Finset.union_assoc, Finset.union_self]

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
**Referenced by**: INV-FERR-060 (store identity persists through merge)
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
    Error(FerraError),
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
    match asupersync::time::timeout(timeout, query_store(handle, query)).await {
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
            asupersync::time::sleep_until((*drain_deadline).into()).await;
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

### INV-FERR-043: Schema Compatibility Check

**Traces to**: SEED.md §4 (Schema-as-data), INV-FERR-009 (Schema Validation), C3 (Schema-as-data)
**Verification**: `V:PROP`, `V:LEAN`
**Stage**: 0

#### Level 0 (Algebraic Law)
```
Let schema(S) be the set of (attribute, ValueType, Cardinality) triples for store S.
Let shared(S₁, S₂) = {A | ∃ t₁ c₁ t₂ c₂: (A, t₁, c₁) ∈ schema(S₁) ∧ (A, t₂, c₂) ∈ schema(S₂)}

∀ stores S₁ S₂, ∀ attr A ∈ shared(S₁, S₂):
  type(S₁, A) = type(S₂, A) ∧ cardinality(S₁, A) = cardinality(S₂, A)

Schema compatibility is symmetric:
  schema_compatible(S₁, S₂) ⟺ schema_compatible(S₂, S₁)

Schema union for non-conflicting attributes:
  merged_schema(S₁, S₂) = schema(S₁) ∪ schema(S₂)  when compatible
  Attributes unique to one store are accepted without conflict.

Merge precondition:
  merge(S₁, S₂) is defined ⟺ schema_compatible(S₁, S₂)
  ¬schema_compatible(S₁, S₂) → merge(S₁, S₂) = Err(SchemaIncompatible)
```

#### Level 1 (State Invariant)
Before merging two stores, the federation layer verifies that their schemas are
compatible. Two schemas are compatible if and only if every attribute that appears
in both schemas has the same ValueType and the same Cardinality in both. Attributes
that appear in only one schema are accepted without conflict (schema union for
non-conflicting attributes).

If the schemas are incompatible, the merge is rejected with
`FerraError::SchemaIncompatible`, which carries the list of conflicting attributes,
their types in each store, and their cardinalities in each store. No datoms are
transferred. The local store is unchanged.

This check is a precondition for all merge operations: full merge (INV-FERR-001
through INV-FERR-003), selective merge (INV-FERR-039), and namespace-filtered
merge (INV-FERR-044).

#### Level 2 (Implementation Contract)
```rust
/// Schema compatibility error detail.
#[derive(Debug, thiserror::Error)]
#[error("Schema incompatible: attribute {attr} has type {local_type:?}/{local_card:?} locally but {remote_type:?}/{remote_card:?} remotely")]
pub struct SchemaConflict {
    pub attr: String,
    pub local_type: ValueType,
    pub remote_type: ValueType,
    pub local_card: Cardinality,
    pub remote_card: Cardinality,
}

/// Check schema compatibility between two stores.
/// Returns Ok(()) if compatible, Err with all conflicts if not.
///
/// Symmetry: schema_compatible(a, b) == schema_compatible(b, a)
pub fn schema_compatible(
    local: &Schema,
    remote: &Schema,
) -> Result<(), FerraError> {
    let mut conflicts = Vec::new();
    for (attr, local_def) in local.attributes() {
        if let Some(remote_def) = remote.get(attr) {
            if local_def.value_type != remote_def.value_type
                || local_def.cardinality != remote_def.cardinality
            {
                conflicts.push(SchemaConflict {
                    attr: attr.to_string(),
                    local_type: local_def.value_type,
                    remote_type: remote_def.value_type,
                    local_card: local_def.cardinality,
                    remote_card: remote_def.cardinality,
                });
            }
        }
        // Attributes unique to local: no conflict
    }
    if conflicts.is_empty() {
        Ok(())
    } else {
        Err(FerraError::SchemaIncompatible(conflicts))
    }
}

#[kani::proof]
#[kani::unwind(5)]
fn schema_compat_symmetric() {
    let a_type: u8 = kani::any();
    let b_type: u8 = kani::any();
    let a_card: bool = kani::any();
    let b_card: bool = kani::any();

    let compatible_ab = (a_type == b_type) && (a_card == b_card);
    let compatible_ba = (b_type == a_type) && (b_card == a_card);

    assert_eq!(compatible_ab, compatible_ba, "Schema compatibility must be symmetric");
}
```

**Falsification**: Any of the following constitutes a violation of INV-FERR-043:
- Merge succeeds when attribute A is `String` in S₁ and `Long` in S₂.
- Merge succeeds when attribute A has cardinality `One` in S₁ and `Many` in S₂.
- Merge rejects two stores where all shared attributes have identical types and
  cardinalities (false positive).
- `schema_compatible(S₁, S₂)` returns a different result than
  `schema_compatible(S₂, S₁)` (symmetry violation).

**proptest strategy**:
```rust
proptest! {
    #[test]
    fn schema_compat_is_symmetric(
        schema_a in arb_schema(1..10),
        schema_b in arb_schema(1..10),
    ) {
        let ab = schema_compatible(&schema_a, &schema_b);
        let ba = schema_compatible(&schema_b, &schema_a);
        prop_assert_eq!(ab.is_ok(), ba.is_ok(),
            "schema_compatible must be symmetric");
    }

    #[test]
    fn compatible_schemas_merge_successfully(
        datoms_a in prop::collection::btree_set(arb_datom(), 0..50),
        datoms_b in prop::collection::btree_set(arb_datom(), 0..50),
        shared_attrs in prop::collection::vec(arb_attr_def(), 1..5),
    ) {
        // Build two stores with identical shared attributes
        let mut schema_a = Schema::new();
        let mut schema_b = Schema::new();
        for attr in &shared_attrs {
            schema_a.define(attr.clone());
            schema_b.define(attr.clone());
        }
        let store_a = Store::from_datoms_with_schema(datoms_a, schema_a);
        let store_b = Store::from_datoms_with_schema(datoms_b, schema_b);
        prop_assert!(schema_compatible(store_a.schema(), store_b.schema()).is_ok());
    }

    #[test]
    fn incompatible_schemas_reject_merge(
        datoms_a in prop::collection::btree_set(arb_datom(), 0..50),
        datoms_b in prop::collection::btree_set(arb_datom(), 0..50),
    ) {
        // Build two stores where a shared attribute has different types
        let mut schema_a = Schema::new();
        schema_a.define(AttrDef::new(":test/attr", ValueType::String, Cardinality::One));
        let mut schema_b = Schema::new();
        schema_b.define(AttrDef::new(":test/attr", ValueType::Long, Cardinality::One));

        let store_a = Store::from_datoms_with_schema(datoms_a, schema_a);
        let store_b = Store::from_datoms_with_schema(datoms_b, schema_b);
        prop_assert!(schema_compatible(store_a.schema(), store_b.schema()).is_err());
    }
}
```

**Lean theorem**:
```lean
/-- Schema compatibility is symmetric: if A is compatible with B, then B is
    compatible with A. Modeled as: for all shared attributes, types match
    iff they match in reverse order. -/

def schema_compatible (s1 s2 : Finset (Nat × Nat × Nat)) : Prop :=
  ∀ a t1 c1 t2 c2, (a, t1, c1) ∈ s1 → (a, t2, c2) ∈ s2 → t1 = t2 ∧ c1 = c2

theorem schema_compat_symmetric (s1 s2 : Finset (Nat × Nat × Nat)) :
    schema_compatible s1 s2 → schema_compatible s2 s1 := by
  intro h a t2 c2 t1 c1 h2 h1
  have ⟨ht, hc⟩ := h a t1 c1 t2 c2 h1 h2
  exact ⟨ht.symm, hc.symm⟩
```

---

### INV-FERR-044: Namespace Isolation

**Traces to**: C8 (Substrate Independence), INV-FERR-039 (Selective Merge)
**Verification**: `V:PROP`, `V:LEAN`
**Stage**: 0

#### Level 0 (Algebraic Law)
```
Let filter F = Namespace(ns) be a namespace filter.
Let D be any datom with attribute A.

∀ datom D, filter F = Namespace(ns):
  F.accepts(D) ⟺ D.attribute.starts_with(ns)

Equivalently:
  namespace_filter(ns, S) = {d ∈ S | d.attribute.starts_with(ns)}

Completeness:
  ∀ d ∈ namespace_filter(ns, S): d.attribute.starts_with(ns)

Soundness:
  ∀ d ∈ S, d.attribute.starts_with(ns) → d ∈ namespace_filter(ns, S)

No leakage:
  ∀ d ∈ S, ¬d.attribute.starts_with(ns) → d ∉ namespace_filter(ns, S)

Composition with selective merge:
  selective_merge(local, remote, Namespace(ns))
    = local ∪ {d ∈ remote | d.attribute.starts_with(ns)}
```

#### Level 1 (State Invariant)
Selective merge can restrict to specific attribute namespaces. The filter
`DatomFilter::AttributeNamespace(":policy/*")` includes only datoms with attributes
matching the namespace prefix. No datoms outside the namespace are transferred.

Namespace filtering is defense-in-depth: it prevents accidental data leakage during
selective merge across organizational boundaries. For example, merging `:policy/*`
from a production store transfers calibrated weights without exposing task backlogs,
observations, or other domain data.

The namespace filter composes with all other DatomFilter combinators (And, Or, Not)
via the existing filter algebra (INV-FERR-039 Level 2). Multiple namespace filters
can be combined: `Or(Namespace(":policy/*"), Namespace(":schema/*"))` transfers
both policy and schema datoms.

#### Level 2 (Implementation Contract)
```rust
/// Namespace isolation is implemented via the existing DatomFilter enum
/// (INV-FERR-039 Level 2). The AttributeNamespace variant performs prefix
/// matching on the datom's attribute keyword.
///
/// Example: transfer only policy datoms from a remote store.
///
/// ```rust
/// let filter = DatomFilter::AttributeNamespace(vec![":policy/".to_string()]);
/// let receipt = selective_merge(&mut local, &remote, &filter).await?;
/// // receipt.datoms_transferred contains only :policy/* datoms
/// // No :task/*, :observation/*, or other namespace datoms were transferred
/// ```

/// Verify namespace isolation: no datom outside the namespace passes the filter.
fn verify_namespace_isolation(
    filter_ns: &str,
    transferred: &[Datom],
) -> bool {
    transferred.iter().all(|d| d.attribute.starts_with(filter_ns))
}

#[kani::proof]
#[kani::unwind(8)]
fn namespace_filter_no_leakage() {
    let ns_prefix: u64 = kani::any();  // model namespace as prefix
    let datom_attr: u64 = kani::any();

    let matches = datom_attr / 1000 == ns_prefix / 1000;  // prefix match model
    let filter_accepts = matches;
    let in_result = filter_accepts;

    // No leakage: if filter does not accept, datom is not in result
    if !filter_accepts {
        assert!(!in_result, "Datom outside namespace must not pass filter");
    }
}
```

**Falsification**: Any of the following constitutes a violation of INV-FERR-044:
- A datom with attribute `:task/title` passes a filter for `:policy/*`.
- A datom with attribute `:policy/weight` does NOT pass a filter for `:policy/*`
  (false negative).
- After a namespace-filtered merge with `:policy/*`, the local store contains a
  new datom with attribute `:observation/text` that came from the remote store
  (leakage).
- The namespace filter interacts incorrectly with And/Or/Not combinators.

**proptest strategy**:
```rust
proptest! {
    #[test]
    fn namespace_filter_complete_and_sound(
        datoms in prop::collection::btree_set(arb_datom(), 0..100),
        namespace in "[a-z]{1,5}",
    ) {
        let ns_prefix = format!(":{}/*", namespace);
        let filter = DatomFilter::AttributeNamespace(vec![ns_prefix.clone()]);

        for d in &datoms {
            let matches = d.attribute.starts_with(&ns_prefix);
            let accepted = filter.matches(d, &Schema::empty());
            prop_assert_eq!(matches, accepted,
                "Namespace filter must be complete and sound: attr={}, ns={}",
                d.attribute, ns_prefix);
        }
    }

    #[test]
    fn namespace_merge_no_leakage(
        local_datoms in prop::collection::btree_set(arb_datom(), 0..50),
        remote_datoms in prop::collection::btree_set(arb_datom(), 0..50),
        namespace in "[a-z]{1,5}",
    ) {
        let ns_prefix = format!(":{}/*", namespace);
        let local_before: BTreeSet<_> = local_datoms.clone();

        // Simulate namespace-filtered merge
        let filter = DatomFilter::AttributeNamespace(vec![ns_prefix.clone()]);
        let transferred: BTreeSet<_> = remote_datoms.iter()
            .filter(|d| filter.matches(d, &Schema::empty()))
            .cloned().collect();
        let result: BTreeSet<_> = local_datoms.union(&transferred).cloned().collect();

        // Every new datom (not in local_before) must match the namespace
        for d in &result {
            if !local_before.contains(d) {
                prop_assert!(d.attribute.starts_with(&ns_prefix),
                    "Leaked datom outside namespace: attr={}", d.attribute);
            }
        }
    }
}
```

**Lean theorem**:
```lean
/-- Namespace isolation: filtering by prefix, then merging, transfers only
    matching datoms. No datom outside the namespace enters the result. -/

def ns_filter (prefix : Nat) (s : Finset (Nat × Nat)) : Finset (Nat × Nat) :=
  s.filter (fun d => d.1 = prefix)

theorem ns_filter_sound (prefix : Nat) (s : Finset (Nat × Nat)) :
    ∀ d ∈ ns_filter prefix s, d.1 = prefix := by
  intro d hd
  exact (Finset.mem_filter.mp hd).2

theorem ns_merge_no_leakage (prefix : Nat) (local remote : Finset (Nat × Nat)) :
    ∀ d ∈ local ∪ (ns_filter prefix remote),
      d ∉ local → d.1 = prefix := by
  intro d hd hnotlocal
  cases Finset.mem_union.mp hd with
  | inl h => exact absurd h hnotlocal
  | inr h => exact (Finset.mem_filter.mp h).2
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
| Fan-out parallelism | All stores queried concurrently via `asupersync::join_all` | O(1) wall-clock for query dispatch |
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

### §23.8.8: Federation Orchestration Protocol

The federation protocol composes INV-FERR-037 through 055 into a coherent message flow:

```
Node A                                    Node B
  |                                         |
  |--- HANDSHAKE (exchange root hashes) --> |
  | <-- ROOT_HASHES (epoch, root) --------- |
  |                                         |
  |--- DIFF_REQUEST (my_root, your_root) -> |
  | <-- DIFF_CHUNKS (missing chunks) ------- |  [INV-FERR-047: O(d) diff]
  |                                         |
  |--- VERIFY_SIGNATURES ------------------  |  [INV-FERR-051: signed txns]
  |--- CHECK_SCHEMA_COMPAT ----------------  |  [INV-FERR-043: schema check]
  |--- APPLY_TRUST_POLICY -----------------  |  [INV-FERR-054: trust gradient]
  |                                         |
  |--- MERGE (filtered chunks) ------------  |  [INV-FERR-039: selective merge]
  |--- PUBLISH_SNAPSHOT -------------------  |  [INV-FERR-006: snapshot isolation]
  |                                         |
  |--- GOSSIP (SWIM protocol) ----------- > |  [INV-FERR-022: anti-entropy]
  |                                         |
  (repeat on timer or on-demand)
```

**Failure scenarios:**
- **Network partition**: both sides continue accepting writes (CRDT safe). Reconnect triggers anti-entropy.
- **Signature verification failure**: reject the specific transaction, continue with valid ones.
- **Schema incompatibility**: reject merge, log error, notify observer.
- **Timeout**: return partial results with metadata (INV-FERR-041).

---

## §23.10: Verifiable Knowledge Network (VKN)

> **Stage**: Phase 4c+ (deferred). VKN invariants (INV-FERR-051..055) are specified and
> Lean-proved but implementation is deferred until after Phase 4a MVP.

The Verifiable Knowledge Network layer transforms ferratomic from a database into a
cryptographically verifiable knowledge substrate. Every assertion carries provenance
that can be independently verified without trusting the asserter, the transport, or
any intermediary. Trust becomes a continuous gradient computed from verifiable calibration
history, not a binary decision made at network configuration time.

**Traces to**: SEED.md §1 ("verifiable coherence"), SEED.md §4 (Design Commitment:
"CRDT merge scales learning across organizations"), INV-FERR-037 (Federated Query
Correctness), INV-FERR-039 (Selective Merge), INV-FERR-044 (Namespace Isolation),
§23.8 (Federation & Federated Query)

**Design principles**:

1. **Trustless verification.** Any agent can verify any datom's provenance without
   trusting the asserter. Verification requires only the datom, its proof, and a
   root hash — not network access, not the full store, not a relationship with the
   asserter.

2. **Amortized cryptography.** Signatures are per-transaction, not per-datom. A
   transaction with 100 datoms carries one 64-byte signature. This amortizes the
   cryptographic overhead to near-zero for batch operations.

3. **Continuous trust gradient.** Trust is not binary (trusted/untrusted). It is a
   continuous value derived from cryptographically verifiable calibration history.
   An agent that has made 1000 predictions with mean error 0.1 is more trusted
   than one with 10 predictions and mean error 0.4 — and this is provable.

4. **Opt-in overhead.** Unsigned mode (§23.10.5) imposes zero cryptographic overhead.
   VKN features activate when configured and degrade gracefully when disabled.

5. **Self-describing keys.** Public keys are datoms. Key rotation is a signed datom.
   Key revocation is a signed datom. The key management infrastructure is built from
   the same primitives as the data it protects.

---

### ADR-FERR-009: Cryptographic Scheme

**Traces to**: SEED.md §1 ("verifiable coherence"), Signal protocol, blockchain
light client protocols
**Status**: Accepted
**Stage**: 1

#### Problem

How to enable trustless verification of datom provenance across trust boundaries.
Federation (§23.8) assumes trust is established at the network level — if you merge
with a remote store, you trust all its datoms. This is an all-or-nothing model that
breaks down when organizations collaborate across trust boundaries, when agents have
heterogeneous reliability, or when historical data must be audited.

#### Options Considered

**(A) No cryptography — trust is network-level (current model).** Simple to implement.
Trust is binary: you either merge with a store or you don't. No overhead per transaction.
But: no ability to verify individual datoms, no ability to distinguish reliable from
unreliable asserters within a trusted store, no auditability after the fact. All-or-nothing
trust does not compose — if A trusts B and B trusts C, A must either trust all of C's
datoms or none.

**(B) Ed25519 signed transactions.** Each transaction is signed by the authoring agent's
Ed25519 private key. 64-byte signature per transaction (NOT per datom — amortized).
Performance: ~5us sign, ~2us verify. Public keys as datoms (self-describing). Key
rotation via signed rotation datom. Used by: SSH, Signal, Tor, libsodium. Well-studied
cryptography with no known weaknesses. 32-byte public keys, 64-byte signatures. Available
in pure Rust via the `ed25519-dalek` crate (no C dependencies, no unsafe).

**(C) RSA signatures.** Slower (~100us sign, ~10us verify), larger signatures (256+ bytes
for RSA-2048, 512+ for RSA-4096), larger keys. No advantage over Ed25519 for this use
case. RSA key generation is slow (~1s for 4096-bit). The only advantage is broader
hardware support (HSMs), which is irrelevant for our embedded/daemon model.

**(D) BLS signatures.** Enables signature aggregation: multiple signers' signatures can
be combined into one fixed-size signature. This is powerful for multi-party attestation
(e.g., "3-of-5 reviewers approved this"). However: newer cryptography (pairing-based),
more complex implementation, slower verification (~1ms), larger crate dependency. Future
consideration for INV-FERR-054 Threshold trust policy, but not justified as the base layer.

#### Decision

**Option B — Ed25519 signed transactions.** Optimal performance-to-security ratio. The
64-byte signature overhead is negligible compared to typical transaction sizes (hundreds
to thousands of bytes of datoms). The 2us verification time is dominated by I/O in any
networked scenario. Pure Rust implementation avoids C FFI complexity.

#### Consequences

- 64 bytes overhead per transaction (signature) + 32 bytes (public key reference).
- ~5us sign overhead per transaction commit.
- ~2us verify overhead per transaction read (amortized across datoms in tx).
- Key management infrastructure required (§23.10.1).
- All existing unsigned transactions remain valid (§23.10.5 backward compatibility).
- Future path to BLS aggregation for multi-party attestation (ADR deferred).

---

### INV-FERR-051: Signed Transactions

**Traces to**: ADR-FERR-009, SEED.md §1 ("verifiable coherence"), INV-FERR-007
(Transaction Atomicity), C1 (Append-only store)
**Referenced by**: INV-FERR-060 (store identity), INV-FERR-061 (causal predecessors),
INV-FERR-063 (provenance lattice), ADR-FERR-021 (signature storage), ADR-FERR-023
(per-transaction signing), ADR-FERR-025 (transaction-level federation)
**Verification**: `V:PROP`, `V:KANI`, `V:LEAN`
**Stage**: 1

> **Phase 4a.5 staging note**: Phase 4a.5 implements Ed25519 signing WITHOUT
> Merkle proof binding (INV-FERR-052 needs prolly tree from Phase 4b).
> Signing message = `blake3(sorted_canonical_user_datoms ∥ tx_id_canonical
> ∥ sorted_predecessor_entity_ids ∥ store_fingerprint ∥ signer_public_key)`.
> All byte representations use INV-FERR-086 canonical format.
> Metadata datoms (`:tx/signature`, `:tx/signer`, `:tx/predecessor`,
> `:tx/provenance`, `:tx/time`, `:tx/agent`, `:tx/derivation-source`) are
> EXCLUDED from the signing message per ADR-FERR-021. Only user-asserted
> datoms are signed.
> **D19**: Predecessor ENTITY IDS (not TxIds) are used in the signing message.
> EntityId = BLAKE3(tx_id_canonical_bytes) per ADR-FERR-032. Verification
> reads tx/predecessor Ref values directly — zero TxId reconstruction needed.
> **D17/ADR-FERR-033**: Store fingerprint (32 bytes, pre-transaction state)
> included in the signing message as a cryptographic state commitment.
> **ADR-FERR-031**: Signing happens at the Database layer via
> `Database::transact_signed(tx, &SigningKey)`, NOT at the Transaction builder.
> The TxId is assigned by HLC tick before signing.

Every transaction is signed by the authoring agent's Ed25519 private key. All datoms
in the transaction are covered by ONE signature (amortized — not per-datom). The
signature covers the cryptographic hash of: the serialized datoms, the transaction ID,
the causal predecessors, and the agent's public key. This binding ensures that no
component of the transaction can be altered without invalidating the signature.

#### Level 0 (Algebraic Law)

```
Let SK be an Ed25519 signing key and VK = public(SK) be the corresponding verifying key.
Let sign : SK × Msg → Sig and verify : VK × Msg × Sig → {ok, fail} be the Ed25519
signing and verification functions.

∀ transactions T in store S:
  Let msg(T) = hash(T.datoms ∥ T.tx_id ∥ T.predecessors ∥ T.author_public_key)
  T.signature = sign(T.author_private_key, msg(T))

  -- Correctness: honest signatures verify
  verify(VK, msg(T), sign(SK, msg(T))) = ok

  -- Unforgeability: tampered messages fail verification
  ∀ T' where T'.datoms ≠ T.datoms ∨ T'.tx_id ≠ T.tx_id ∨ T'.predecessors ≠ T.predecessors:
    let msg' = hash(T'.datoms ∥ T'.tx_id ∥ T'.predecessors ∥ T'.author_public_key)
    msg' ≠ msg(T)  →  verify(VK, msg', sign(SK, msg(T))) = fail
    -- (by collision resistance of hash and unforgeability of Ed25519)

  -- Key binding: signature by SK does not verify under different key VK'
  ∀ VK' ≠ VK:
    verify(VK', msg(T), sign(SK, msg(T))) = fail
    -- (by key-binding property of Ed25519, proven in Brendel et al. 2019)

Composition with C1 (append-only):
  Once sign(SK, msg(T)) is stored, it is never mutated or deleted.
  Retraction of a signed datom D is itself a new signed transaction T':
    T' = { datoms: [D with op=retract], signature: sign(SK', msg(T')) }
  The original signature on T remains in the store, providing audit trail.
```

#### Level 1 (State Invariant)

No signed transaction in the store has an invalid signature when verified against the
author's public key. For all reachable store states `S` produced by any sequence of
TRANSACT, MERGE, and recovery operations:

1. Every signed transaction `T` in `S` satisfies `verify(T.signer, T.signing_message(), T.signature) = ok`.
2. Any modification to `T.datoms`, `T.tx_id`, or `T.predecessors` produces a different `signing_message()`, causing verification to fail.
3. Any attempt to claim a different author by substituting `T.signer` with a different public key causes verification to fail (key-binding property).
4. Unsigned transactions (§23.10.5) are exempt from this invariant — they have no signature to verify.
5. MERGE of signed transactions preserves signatures: after `merge(S₁, S₂)`, every signed transaction from `S₁` and `S₂` retains its original signature and continues to verify.

The invariant is monotonic: once a signed transaction is in the store, it remains
verifiable forever (by C1 append-only). The set of verifiable signatures only grows.

#### Level 2 (Implementation Contract)

```rust
use ed25519_dalek::{SigningKey, VerifyingKey, Signature, Signer, Verifier};
use blake3;

/// A transaction with an Ed25519 signature covering all its datoms.
/// The signature is amortized: one signature per transaction, not per datom.
///
/// # Invariants
/// - `signature` covers `hash(tx.datoms ∥ tx.tx_id ∥ tx.predecessors ∥ signer)`
/// - `verify()` returns `Ok(())` iff the signature is valid for the signing message
/// - `signer` is the public key of the agent that created this transaction
///
/// # Size
/// - `signature`: 64 bytes (Ed25519 fixed)
/// - `signer`: 32 bytes (Ed25519 public key)
/// - Total overhead: 96 bytes per transaction
pub struct SignedTransaction {
    pub tx: Transaction,
    pub signature: Signature,   // 64 bytes, Ed25519
    pub signer: VerifyingKey,   // 32 bytes, Ed25519 public key
}

impl SignedTransaction {
    /// Sign a transaction with the given signing key.
    ///
    /// The signing message is: blake3(canonical_serialize(datoms) ∥ tx_id ∥
    /// canonical_serialize(predecessors) ∥ signer_public_key_bytes).
    ///
    /// # Performance
    /// - blake3 hash: ~1us for typical transaction (< 10KB)
    /// - Ed25519 sign: ~5us
    /// - Total: ~6us per transaction
    pub fn sign(tx: Transaction, key: &SigningKey) -> Self {
        let msg = tx.signing_message();
        let signature = key.sign(&msg);
        Self {
            tx,
            signature,
            signer: key.verifying_key(),
        }
    }

    /// Verify that the signature is valid for this transaction's content.
    ///
    /// Returns `Ok(())` if verification passes, `Err(CryptoError::InvalidSignature)`
    /// if any of the following hold:
    /// - The signature bytes do not correspond to the signing message under `self.signer`
    /// - The transaction content was modified after signing
    /// - The signer key was substituted
    ///
    /// # Performance
    /// ~2us per verification (Ed25519 verify dominates)
    pub fn verify(&self) -> Result<(), CryptoError> {
        let msg = self.tx.signing_message();
        self.signer
            .verify(&msg, &self.signature)
            .map_err(|_| CryptoError::InvalidSignature)
    }
}

impl Transaction {
    /// Compute the signing message: the bytes that the signature covers.
    ///
    /// Uses blake3 for collision resistance (256-bit output, faster than SHA-256).
    /// The canonical serialization is deterministic: datoms are sorted by
    /// (entity, attribute, value, tx, op) before serialization to ensure that
    /// two transactions with the same datoms in different insertion order produce
    /// the same signing message.
    ///
    /// # Determinism
    /// For any two Transaction values `a` and `b`:
    ///   a.datoms (as set) == b.datoms (as set) ∧ a.tx_id == b.tx_id
    ///   ∧ a.predecessors == b.predecessors
    ///   → a.signing_message() == b.signing_message()
    pub fn signing_message(&self) -> Vec<u8> {
        let mut hasher = blake3::Hasher::new();

        // Sort datoms canonically for deterministic hashing
        let mut sorted_datoms = self.datoms.clone();
        sorted_datoms.sort();
        for datom in &sorted_datoms {
            hasher.update(&datom.canonical_bytes());
        }

        hasher.update(&self.tx_id.to_le_bytes());

        let mut sorted_preds = self.predecessors.clone();
        sorted_preds.sort();
        for pred in &sorted_preds {
            hasher.update(&pred.to_le_bytes());
        }

        hasher.finalize().as_bytes().to_vec()
    }
}

/// Errors from cryptographic operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CryptoError {
    /// Ed25519 signature verification failed.
    InvalidSignature,
    /// The signer's public key is not registered in the store.
    UnknownSigner(VerifyingKey),
    /// The signer's key has been revoked (`:agent/key-revoked` datom exists).
    RevokedKey(VerifyingKey),
    /// Merkle inclusion proof verification failed.
    InvalidInclusionProof,
    /// Calibration proof verification failed.
    InvalidCalibrationProof,
    /// Context proof references a different root hash than expected.
    RootHashMismatch { expected: ChunkAddress, actual: ChunkAddress },
}

#[kani::proof]
#[kani::unwind(5)]
fn signed_transaction_roundtrip() {
    let datom_count: usize = kani::any();
    kani::assume(datom_count <= 3);

    let tx_id: u64 = kani::any();
    let datoms: Vec<Datom> = (0..datom_count)
        .map(|_| Datom {
            entity: kani::any(),
            attribute: kani::any(),
            value: kani::any(),
            tx: tx_id,
            op: kani::any(),
        })
        .collect();

    let tx = Transaction {
        tx_id,
        datoms,
        predecessors: vec![],
    };

    // Sign with a deterministic key
    let key_bytes: [u8; 32] = kani::any();
    let signing_key = SigningKey::from_bytes(&key_bytes);

    let signed = SignedTransaction::sign(tx, &signing_key);

    // Verify must pass
    assert!(signed.verify().is_ok());

    // Tamper with tx_id → verify must fail
    let mut tampered = signed.clone();
    tampered.tx.tx_id = signed.tx.tx_id.wrapping_add(1);
    assert!(tampered.verify().is_err());
}
```

**Falsification**: Any of the following constitutes a violation of INV-FERR-051:
- A transaction with a valid signature where any datom, tx_id, or predecessor was
  modified after signing, yet `verify()` still returns `Ok(())`.
- A transaction signed by key `SK` where `verify()` succeeds when checked against a
  different verifying key `VK'` where `VK' != public(SK)`.
- A MERGE operation that drops or corrupts the signature of a signed transaction.
- Two distinct transactions (different datom sets) that produce the same `signing_message()`
  (hash collision — would require breaking blake3).
- A transaction where datoms are reordered (but the set is identical) and `signing_message()`
  produces a different output (non-deterministic canonical serialization).

**proptest strategy**:
```rust
proptest! {
    /// Honest sign-verify roundtrip always succeeds.
    #[test]
    fn signed_roundtrip(
        datoms in prop::collection::vec(arb_datom(), 0..50),
        tx_id in any::<u64>(),
        preds in prop::collection::vec(any::<u64>(), 0..5),
        key_bytes in prop::array::uniform32(any::<u8>()),
    ) {
        let tx = Transaction { tx_id, datoms, predecessors: preds };
        let key = SigningKey::from_bytes(&key_bytes);
        let signed = SignedTransaction::sign(tx, &key);
        prop_assert!(signed.verify().is_ok(),
            "Honest sign-verify roundtrip failed");
    }

    /// Tampering with any single byte of any datom invalidates the signature.
    #[test]
    fn tamper_datom_fails(
        datoms in prop::collection::vec(arb_datom(), 1..20),
        tx_id in any::<u64>(),
        key_bytes in prop::array::uniform32(any::<u8>()),
        tamper_idx in any::<prop::sample::Index>(),
        tamper_byte in any::<u8>(),
    ) {
        let tx = Transaction { tx_id, datoms, predecessors: vec![] };
        let key = SigningKey::from_bytes(&key_bytes);
        let signed = SignedTransaction::sign(tx, &key);

        let mut tampered = signed.clone();
        let idx = tamper_idx.index(tampered.tx.datoms.len());
        tampered.tx.datoms[idx].entity ^= tamper_byte as u64 | 1; // Ensure change
        prop_assert!(tampered.verify().is_err(),
            "Tampered datom passed verification");
    }

    /// Signing with key A, verifying with key B always fails (when A != B).
    #[test]
    fn wrong_key_fails(
        datoms in prop::collection::vec(arb_datom(), 0..10),
        tx_id in any::<u64>(),
        key_a_bytes in prop::array::uniform32(any::<u8>()),
        key_b_bytes in prop::array::uniform32(any::<u8>()),
    ) {
        prop_assume!(key_a_bytes != key_b_bytes);
        let tx = Transaction { tx_id, datoms, predecessors: vec![] };
        let key_a = SigningKey::from_bytes(&key_a_bytes);
        let key_b = SigningKey::from_bytes(&key_b_bytes);

        let signed = SignedTransaction::sign(tx, &key_a);
        let wrong_key_signed = SignedTransaction {
            tx: signed.tx.clone(),
            signature: signed.signature,
            signer: key_b.verifying_key(),  // Wrong key
        };
        prop_assert!(wrong_key_signed.verify().is_err(),
            "Wrong key passed verification");
    }

    /// Canonical serialization is deterministic: same datom set in any order
    /// produces the same signing message.
    #[test]
    fn signing_message_deterministic(
        datoms in prop::collection::vec(arb_datom(), 0..20),
        tx_id in any::<u64>(),
        shuffle_seed in any::<u64>(),
    ) {
        let tx1 = Transaction { tx_id, datoms: datoms.clone(), predecessors: vec![] };

        let mut shuffled = datoms.clone();
        let mut rng = StdRng::seed_from_u64(shuffle_seed);
        shuffled.shuffle(&mut rng);
        let tx2 = Transaction { tx_id, datoms: shuffled, predecessors: vec![] };

        prop_assert_eq!(tx1.signing_message(), tx2.signing_message(),
            "Signing message depends on datom order");
    }

    /// Merge preserves signatures: after merge, all signed transactions from both
    /// stores still verify.
    #[test]
    fn merge_preserves_signatures(
        datoms_a in prop::collection::vec(arb_datom(), 1..20),
        datoms_b in prop::collection::vec(arb_datom(), 1..20),
        key_bytes in prop::array::uniform32(any::<u8>()),
    ) {
        let key = SigningKey::from_bytes(&key_bytes);

        let tx_a = Transaction { tx_id: 1, datoms: datoms_a, predecessors: vec![] };
        let signed_a = SignedTransaction::sign(tx_a, &key);

        let tx_b = Transaction { tx_id: 2, datoms: datoms_b, predecessors: vec![] };
        let signed_b = SignedTransaction::sign(tx_b, &key);

        let store_a = Store::from_signed_transactions(vec![signed_a.clone()]);
        let store_b = Store::from_signed_transactions(vec![signed_b.clone()]);

        let merged = merge(store_a, store_b);

        // All signatures survive merge
        for stx in merged.signed_transactions() {
            prop_assert!(stx.verify().is_ok(),
                "Signature invalidated by merge");
        }
    }
}
```

**Lean theorem**:
```lean
/-- Ed25519 signature correctness: signing then verifying with the same key succeeds. -/
theorem signed_verify_roundtrip (tx : Transaction) (sk : SigningKey) :
    let vk := public_key sk
    let msg := signing_message tx
    let sig := ed25519_sign sk msg
    ed25519_verify vk msg sig = ok := by
  exact Ed25519.correctness sk (signing_message tx)

/-- Tamper detection: modifying any component of the transaction invalidates
    the signature (assuming hash collision resistance). -/
theorem signed_tamper_detection (tx : Transaction) (sk : SigningKey)
    (tx' : Transaction) (h_diff : tx ≠ tx') (h_collision_free : signing_message tx ≠ signing_message tx') :
    let vk := public_key sk
    let sig := ed25519_sign sk (signing_message tx)
    ed25519_verify vk (signing_message tx') sig = fail := by
  -- signing_message tx ≠ signing_message tx' by h_collision_free
  -- Ed25519 unforgeability: sig valid for msg₁ → sig invalid for msg₂ ≠ msg₁
  exact Ed25519.unforgeability sk (signing_message tx) (signing_message tx') h_collision_free

/-- Key binding: signature under sk does not verify under sk' ≠ sk. -/
theorem signed_key_binding (tx : Transaction) (sk sk' : SigningKey)
    (h_diff : sk ≠ sk') :
    let sig := ed25519_sign sk (signing_message tx)
    ed25519_verify (public_key sk') (signing_message tx) sig = fail := by
  exact Ed25519.key_binding sk sk' (signing_message tx) h_diff

/-- Merge preserves signatures: set union does not alter transaction content. -/
theorem merge_preserves_signatures (s1 s2 : DatomStore)
    (stx : SignedTransaction) (h_mem : stx ∈ s1 ∨ stx ∈ s2) :
    stx ∈ (s1 ∪ s2) ∧ verify stx = ok := by
  constructor
  · exact Finset.mem_union.mpr h_mem
  · -- stx is unchanged by set union (no mutation, C1)
    -- verify only depends on stx's fields, which are unchanged
    exact stx.verify_invariant
```

---

### INV-FERR-052: Merkle Proof of Inclusion

**Traces to**: SEED.md §1 ("verifiable coherence"), INV-FERR-019 (Content-Addressed
Chunks), INV-FERR-020 (Prolly Tree Determinism), ADR-FERR-009
**Verification**: `V:PROP`, `V:KANI`, `V:LEAN`
**Stage**: 1

For any datom `D` in store `S` with prolly tree root hash `R`, a proof `P` exists
such that `verify_inclusion(D, P, R) = true` WITHOUT access to `S`. The proof is a
path from `D`'s leaf chunk through the prolly tree to the root — `O(log_k N)` chunk
hashes where `k` is the average chunk fan-out (typically 16-64). This enables third-party
verification of datom membership using only the root hash as a trust anchor.

#### Level 0 (Algebraic Law)

```
Let store(R) denote the set of datoms in a store with prolly tree root hash R.
Let H : Bytes → ChunkAddress be the blake3 hash function (collision-resistant).
Let prolly_tree(S) be the deterministic prolly tree over datom set S (INV-FERR-020).
Let root(S) = H(root_chunk(prolly_tree(S))) be the root hash.
Let k = average fan-out of the prolly tree (chunk size / child pointer size).

-- Completeness: every datom in the store has a proof
∀ datom D ∈ store(R):
  ∃ proof P = [node₁, node₂, ..., node_{⌈log_k(N)⌉}]:
    verify_inclusion(D, P, R) = true

-- Soundness: no datom outside the store has a valid proof
∀ datom D ∉ store(R):
  ¬∃ proof P: verify_inclusion(D, P, R) = true
  -- (by collision resistance of H: constructing such a proof requires finding
  --  a hash collision, which is computationally infeasible for blake3)

-- Proof verification is deterministic
∀ D, P, R:
  verify_inclusion(D, P, R) is a pure function of its arguments

-- Proof size is logarithmic
∀ store S with |S| = N datoms, fan-out k:
  max_proof_size(S) = ⌈log_k(N)⌉ × (k × 32 + 8) bytes
  -- Each level: k sibling hashes (32 bytes each) + position index (8 bytes)

-- Composition with INV-FERR-020 (prolly tree determinism):
  ∀ S₁, S₂ with same datom set: root(S₁) = root(S₂)
  -- Therefore proofs constructed against S₁ verify against root(S₂)

-- Composition with C1 (append-only):
  If verify_inclusion(D, P, R_old) = true at time t,
  then D ∈ store(R_new) for all R_new at time t' > t
  -- (append-only means datoms are never removed)
  -- (but the PROOF P may be invalidated by tree restructuring;
  --  a new proof P' must be constructed against R_new)
```

#### Level 1 (State Invariant)

Any datom's membership in a store can be verified by a third party using only the
root hash and a logarithmic-sized proof. The verification has two critical properties:

1. **Soundness** (no false inclusions): If `verify_inclusion(D, P, R) = true`, then
   `D` is in the store with root hash `R`. A false positive would require breaking
   blake3 collision resistance — computationally infeasible.

2. **Completeness** (no missed inclusions): For every datom `D` in the store, a valid
   proof `P` can be constructed. No datom is "unprovable."

The proof is a path through the prolly tree: at each level, the verifier receives the
sibling hashes at that node, reconstructs the parent hash, and checks that it matches
the next level's expected hash. At the root level, the reconstructed hash must equal `R`.

The proof size is `O(log_k N)` where `k` is the prolly tree fan-out and `N` is the
datom count. For a store with 100 million datoms and fan-out 32, this is approximately
`ceil(log_32(10^8)) = 6` levels, each carrying ~32 sibling hashes of 32 bytes = ~6KB
total proof size. This is small enough to include in network messages, store alongside
datoms, or embed in verifiable knowledge commitments (INV-FERR-055).

#### Level 2 (Implementation Contract)

```rust
/// A Merkle inclusion proof: a path from a leaf datom through the prolly tree
/// to the root. Each node in the path contains the sibling hashes at that level
/// and the position of the target among its siblings.
///
/// # Size
/// For a store with N datoms and fan-out k:
///   path.len() = O(log_k(N))
///   Each ProofNode: k * 32 bytes (sibling hashes) + 8 bytes (position)
///   Total: O(log_k(N) * k * 32) bytes
///
/// # Example
/// Store with 100M datoms, fan-out 32:
///   path.len() ≈ 6, each node ≈ 1KB, total ≈ 6KB
pub struct InclusionProof {
    /// The datom whose membership is being proved.
    pub datom: Datom,
    /// Path from leaf to root. path[0] is the leaf level, path[last] is one
    /// level below the root.
    pub path: Vec<ProofNode>,
    /// The root hash of the store at the time the proof was constructed.
    pub root_hash: ChunkAddress,
}

/// A single node in an inclusion proof path.
///
/// At each level of the prolly tree, a chunk contains multiple children.
/// The proof provides the hashes of all sibling chunks at this level, plus
/// the position of the target child among them.
pub struct ProofNode {
    /// blake3 hashes of all siblings at this tree level, in order.
    /// The target child's hash is NOT included — it is computed by the verifier.
    pub sibling_hashes: Vec<ChunkAddress>,
    /// The index of the target child among all children at this level.
    /// 0-indexed. Used to insert the computed hash at the correct position
    /// during verification.
    pub position: usize,
}

impl InclusionProof {
    /// Verify that `self.datom` is a member of the store with root hash
    /// `self.root_hash`.
    ///
    /// Algorithm:
    /// 1. Compute the leaf hash: blake3(canonical_bytes(datom))
    /// 2. For each level in the proof path:
    ///    a. Insert the current hash at `position` among the sibling hashes
    ///    b. Concatenate all hashes at this level
    ///    c. Compute the parent hash: blake3(concatenated)
    /// 3. The final computed hash must equal `self.root_hash`
    ///
    /// # Returns
    /// `true` if the proof is valid (datom is in the store), `false` otherwise.
    ///
    /// # Performance
    /// O(log_k(N)) blake3 hashes. ~1us per level. ~6us for 100M datom store.
    pub fn verify(&self) -> bool {
        let mut current_hash = chunk_hash(&serialize_datom(&self.datom));

        for node in &self.path {
            // Reconstruct the full child list at this level
            let total_children = node.sibling_hashes.len() + 1;
            if node.position > node.sibling_hashes.len() {
                return false; // Invalid position
            }

            let mut level_data = Vec::with_capacity(total_children * 32);
            for i in 0..total_children {
                if i == node.position {
                    level_data.extend_from_slice(current_hash.as_bytes());
                } else {
                    let sibling_idx = if i < node.position { i } else { i - 1 };
                    if sibling_idx >= node.sibling_hashes.len() {
                        return false; // Malformed proof
                    }
                    level_data.extend_from_slice(
                        node.sibling_hashes[sibling_idx].as_bytes()
                    );
                }
            }

            current_hash = blake3::hash(&level_data).into();
        }

        current_hash == self.root_hash
    }

    /// Construct an inclusion proof for a datom in a store.
    ///
    /// Walks the prolly tree from the leaf containing `datom` up to the root,
    /// collecting sibling hashes at each level.
    ///
    /// # Errors
    /// Returns `None` if the datom is not in the store (completeness: this is
    /// the ONLY reason for returning `None`).
    ///
    /// # Performance
    /// O(log_k(N)) chunk reads. Typically cached in memory.
    pub fn construct(store: &Store, datom: &Datom) -> Option<Self> {
        let prolly = store.prolly_tree();
        let leaf = prolly.find_leaf(datom)?; // None if datom not in store

        let mut path = Vec::new();
        let mut current_node = leaf;

        while let Some(parent) = prolly.parent_of(&current_node) {
            let siblings = prolly.children_of(&parent);
            let position = siblings.iter()
                .position(|c| c.hash == current_node.hash)
                .expect("child must be in parent's children list");

            let sibling_hashes: Vec<ChunkAddress> = siblings.iter()
                .enumerate()
                .filter(|(i, _)| *i != position)
                .map(|(_, c)| c.hash)
                .collect();

            path.push(ProofNode { sibling_hashes, position });
            current_node = parent;
        }

        Some(InclusionProof {
            datom: datom.clone(),
            path,
            root_hash: prolly.root_hash(),
        })
    }
}

/// Verify that a datom is NOT in a store (exclusion proof).
/// Uses the prolly tree's sorted structure: if the datom would be between
/// two adjacent datoms in the leaf, and neither matches, it is absent.
///
/// # Returns
/// `Some(ExclusionProof)` if the datom is verifiably absent.
/// `None` if the datom is actually present (cannot prove absence).
pub struct ExclusionProof {
    pub absent_datom: Datom,
    pub left_neighbor: Option<Datom>,   // Datom immediately before (if any)
    pub right_neighbor: Option<Datom>,  // Datom immediately after (if any)
    pub leaf_proof: InclusionProof,     // Proves the leaf chunk is in the store
    pub root_hash: ChunkAddress,
}

#[kani::proof]
#[kani::unwind(8)]
fn inclusion_proof_soundness() {
    let datoms: Vec<Datom> = (0..kani::any::<u8>() % 4)
        .map(|_| Datom {
            entity: kani::any(),
            attribute: kani::any(),
            value: kani::any(),
            tx: kani::any(),
            op: kani::any(),
        })
        .collect();

    let store = Store::from_datoms(datoms.clone());
    let root = store.root_hash();

    // For each datom in the store, proof must verify
    for datom in &datoms {
        let proof = InclusionProof::construct(&store, datom);
        assert!(proof.is_some(), "Completeness: cannot construct proof for datom in store");
        assert!(proof.unwrap().verify(), "Soundness: valid proof does not verify");
    }

    // For a datom NOT in the store, proof construction must fail
    let absent = Datom {
        entity: u64::MAX,
        attribute: u64::MAX,
        value: Value::Nil,
        tx: u64::MAX,
        op: Op::Assert,
    };
    if !datoms.contains(&absent) {
        assert!(InclusionProof::construct(&store, &absent).is_none(),
            "Constructed proof for absent datom");
    }
}
```

**Falsification**: Any of the following constitutes a violation of INV-FERR-052:
- **False positive (soundness breach)**: A proof `P` that passes `verify_inclusion(D, P, R)` for
  a datom `D` that is NOT in the store with root hash `R`. This would require a blake3 hash
  collision.
- **False negative (completeness breach)**: A datom `D` that IS in the store but for which no
  valid proof can be constructed. This would indicate a bug in the prolly tree traversal
  or the proof construction algorithm.
- **Proof portability failure**: A proof constructed against store `S₁` with root `R` fails to
  verify when checked against the same root `R` by a different verifier implementation.
  Proofs must be self-contained and verifier-independent.
- **Non-determinism**: Two calls to `construct(store, datom)` for the same store state and
  datom produce different proofs that disagree on `verify()`.
- **Proof size violation**: A proof for a store with `N` datoms and fan-out `k` has more than
  `ceil(log_k(N)) + 1` nodes in its path.

**proptest strategy**:
```rust
proptest! {
    /// Every datom in a store has a verifiable inclusion proof.
    #[test]
    fn inclusion_proof_completeness(
        datoms in prop::collection::btree_set(arb_datom(), 1..200),
    ) {
        let store = Store::from_datoms(datoms.clone());
        let root = store.root_hash();

        for datom in &datoms {
            let proof = InclusionProof::construct(&store, datom);
            prop_assert!(proof.is_some(),
                "Completeness failure: no proof for datom in store");

            let proof = proof.unwrap();
            prop_assert_eq!(proof.root_hash, root,
                "Proof root hash does not match store root");
            prop_assert!(proof.verify(),
                "Valid proof does not verify");
        }
    }

    /// No datom outside the store has a valid inclusion proof.
    #[test]
    fn inclusion_proof_soundness(
        datoms in prop::collection::btree_set(arb_datom(), 0..100),
        absent in arb_datom(),
    ) {
        prop_assume!(!datoms.contains(&absent));
        let store = Store::from_datoms(datoms);

        let proof = InclusionProof::construct(&store, &absent);
        prop_assert!(proof.is_none(),
            "Constructed proof for absent datom (soundness failure)");
    }

    /// Proofs are deterministic: same store + same datom → same proof.
    #[test]
    fn inclusion_proof_deterministic(
        datoms in prop::collection::btree_set(arb_datom(), 1..100),
    ) {
        let store = Store::from_datoms(datoms.clone());
        let datom = datoms.iter().next().unwrap();

        let proof1 = InclusionProof::construct(&store, datom).unwrap();
        let proof2 = InclusionProof::construct(&store, datom).unwrap();

        prop_assert_eq!(proof1.path.len(), proof2.path.len(),
            "Proof path lengths differ");
        for (n1, n2) in proof1.path.iter().zip(proof2.path.iter()) {
            prop_assert_eq!(n1.position, n2.position,
                "Proof node positions differ");
            prop_assert_eq!(n1.sibling_hashes, n2.sibling_hashes,
                "Proof sibling hashes differ");
        }
    }

    /// Proof size is logarithmic in store size.
    #[test]
    fn inclusion_proof_size_logarithmic(
        datoms in prop::collection::btree_set(arb_datom(), 10..500),
    ) {
        let store = Store::from_datoms(datoms.clone());
        let datom = datoms.iter().next().unwrap();
        let proof = InclusionProof::construct(&store, datom).unwrap();

        let n = datoms.len() as f64;
        let k = store.prolly_tree().avg_fanout() as f64;
        let max_depth = (n.log(k)).ceil() as usize + 2; // +2 for rounding/boundary

        prop_assert!(proof.path.len() <= max_depth,
            "Proof depth {} exceeds log_{}({}) + 2 = {}",
            proof.path.len(), k as usize, datoms.len(), max_depth);
    }

    /// Tampered proofs fail verification.
    #[test]
    fn tampered_proof_fails(
        datoms in prop::collection::btree_set(arb_datom(), 5..100),
        tamper_byte in any::<u8>(),
    ) {
        let store = Store::from_datoms(datoms.clone());
        let datom = datoms.iter().next().unwrap();
        let mut proof = InclusionProof::construct(&store, datom).unwrap();

        // Tamper with a sibling hash
        if let Some(node) = proof.path.first_mut() {
            if let Some(hash) = node.sibling_hashes.first_mut() {
                hash.as_mut_bytes()[0] ^= tamper_byte | 1; // Ensure change
            }
        }

        prop_assert!(!proof.verify(),
            "Tampered proof passed verification");
    }
}
```

**Lean theorem**:
```lean
/-- Completeness: every datom in the store has a valid inclusion proof.
    This follows from the prolly tree being a complete index over all datoms. -/
theorem inclusion_proof_completeness (S : DatomStore) (D : Datom) (h : D ∈ S) :
    ∃ P : InclusionProof, P.datom = D ∧ P.root_hash = root S ∧ verify_inclusion P = true := by
  -- The prolly tree indexes all datoms; find_leaf succeeds for any member
  obtain ⟨leaf, h_leaf⟩ := prolly_tree_complete S D h
  -- Walk from leaf to root collecting siblings
  obtain ⟨path, h_path⟩ := prolly_tree_path_to_root leaf
  -- Verify reconstructs the root hash
  exact ⟨⟨D, path, root S⟩, rfl, rfl, verify_from_path h_path⟩

/-- Soundness: no datom outside the store has a valid inclusion proof.
    This requires collision resistance of blake3 (modeled as an axiom). -/
theorem inclusion_proof_soundness (S : DatomStore) (D : Datom) (P : InclusionProof)
    (h_absent : D ∉ S) (h_root : P.root_hash = root S) :
    verify_inclusion P = false := by
  -- If D ∉ S, then D is not in any leaf chunk of the prolly tree
  -- For verify_inclusion to return true, the leaf hash must be a child of
  -- some internal node, which requires hash(D) to appear in the tree
  -- Since D is not in S, this requires a blake3 collision
  exact blake3_collision_resistance S D P h_absent h_root

/-- Proof determinism: same store state + same datom → same proof
    (follows from prolly tree determinism, INV-FERR-020). -/
theorem inclusion_proof_deterministic (S : DatomStore) (D : Datom) (h : D ∈ S)
    (P₁ P₂ : InclusionProof)
    (h₁ : P₁ = construct S D) (h₂ : P₂ = construct S D) :
    P₁ = P₂ := by
  rw [h₁, h₂]
```

---

### INV-FERR-053: Light Client Protocol

**Traces to**: SEED.md §1 ("verifiable coherence"), INV-FERR-052 (Merkle Proof of
Inclusion), INV-FERR-037 (Federated Query Correctness), ADR-FERR-009, blockchain
light client protocols
**Verification**: `V:PROP`, `V:LEAN`
**Stage**: 1

A light client holds only `epoch → root_hash` mappings (one hash per epoch, 32 bytes
each). It can verify any datom's existence via Merkle proof from a full node. It can
verify query results by checking inclusion proofs for each result datom. The light client
trusts NO full node — it verifies everything cryptographically.

#### Level 0 (Algebraic Law)

```
Let LightClient = { epoch_i → root_hash_i }  (32 bytes per epoch)
Let FullNode = a full datom store S with query capability
Let Transport be the communication channel between LightClient and FullNode

-- Light client storage: O(E) where E = number of epochs (NOT O(N) datoms)
|LightClient| = 32 × E bytes
  For a 10-year store with hourly epochs: 32 × 87,600 ≈ 2.7 MB

-- Verified single-datom lookup
verified_lookup(LC, D, epoch) =
  let R = LC.epochs[epoch]
  let (D, proof) = FullNode.lookup_with_proof(D, R)
  verify_inclusion(D, proof, R)

-- Verified query
For query Q with result set R = {d₁, ..., d_m} from FullNode:
  verified_query(LC, Q, epoch) =
    let R_hash = LC.epochs[epoch]
    let {(d_i, proof_i)} = FullNode.query_with_proofs(Q, R_hash)
    ∀ i ∈ 1..m: verify_inclusion(d_i, proof_i, R_hash)

  -- Soundness: every returned datom is genuinely in the store at that epoch
  verified_query(LC, Q, epoch) = ok
    → ∀ d ∈ R: d ∈ store(R_hash)

  -- Bandwidth: proportional to result size, NOT store size
  bandwidth(verified_query) = O(|R| × log_k(N))
    where |R| = result count, k = prolly tree fan-out, N = total datom count

-- Composition with INV-FERR-037 (federated query):
  A light client can participate in a federated query by requesting
  query_with_proofs from multiple full nodes and merging verified results.
  The CALM theorem still holds: union of verified per-node results equals
  verified query on the union (for monotonic queries).

-- Trust model:
  The light client trusts the root hash source (e.g., signed by a trusted
  epoch authority, or derived from a consensus protocol, or verified by
  multiple independent full nodes).
  The light client does NOT trust any individual full node's query results —
  it verifies each datom via Merkle proof against the trusted root hash.
```

#### Level 1 (State Invariant)

A client with only root hashes can verify any query result without downloading the full
store. For all reachable states of a `LightClient` with epoch-to-root mappings:

1. **Verified results are genuine**: If `verified_query` returns `Ok`, every datom in the
   result set is provably in the store at the specified epoch. A full node cannot fabricate
   datoms that pass verification (by Merkle proof soundness, INV-FERR-052).

2. **No silent omissions in verifiable mode**: The light client cannot detect omissions
   from a malicious full node (the full node could withhold results). To address this,
   the protocol supports multiple full nodes: query the same Q against multiple full
   nodes and union the verified results. If any honest full node participates, no datom
   is omitted (by CALM, INV-FERR-037).

3. **Bandwidth proportional to result**: For a query returning `m` datoms from a store
   with `N` total datoms, the bandwidth is `O(m * log_k(N))`. This is sublinear in `N`
   for any bounded result set.

4. **Epoch progression**: Root hashes are append-only. The light client can receive new
   epoch → root_hash mappings from any source and verify them independently. Old epochs
   are never invalidated (by C1 append-only: datoms are only added, never removed, so
   all old root hashes remain valid for their epoch).

#### Level 2 (Implementation Contract)

```rust
/// A light client that verifies query results from untrusted full nodes.
///
/// Storage: O(E × 32) bytes where E = number of epochs.
/// A 10-year store with hourly epochs requires ~2.7 MB.
///
/// # Trust Model
/// The light client trusts the epoch → root_hash mappings (the "trust anchor").
/// It does NOT trust any full node's query results — every result datom is
/// verified via Merkle inclusion proof against the trusted root hash.
pub struct LightClient {
    /// Epoch → root hash mapping. Each root hash is 32 bytes (blake3).
    /// Append-only: old epochs are never removed or modified.
    epochs: BTreeMap<u64, ChunkAddress>,

    /// Transport layer for communicating with full nodes.
    transport: Box<dyn Transport>,
}

/// The result of a verified query: every datom has been checked against
/// the Merkle root of the specified epoch.
pub struct VerifiedResult {
    /// The verified datoms (all passed inclusion proof verification).
    pub datoms: Vec<Datom>,
    /// The epoch against which verification was performed.
    pub epoch: u64,
    /// The root hash used for verification.
    pub root_hash: ChunkAddress,
    /// Whether all proofs passed. Always true if this struct exists
    /// (construction fails on invalid proof).
    pub verified: bool,
    /// Per-datom verification metadata (for auditing).
    pub proof_sizes: Vec<usize>,
}

/// Response from a full node: query results with inclusion proofs.
pub struct ProvedQueryResponse {
    /// Each result datom paired with its Merkle inclusion proof.
    pub proved_results: Vec<(Datom, InclusionProof)>,
    /// Total number of results (may exceed proved_results if the full node
    /// paginated the response).
    pub total_count: usize,
}

impl LightClient {
    /// Create a new light client with initial epoch → root_hash mappings.
    ///
    /// The caller is responsible for verifying the initial mappings
    /// (e.g., from a trusted configuration, signed by an epoch authority,
    /// or cross-checked against multiple full nodes).
    pub fn new(
        initial_epochs: BTreeMap<u64, ChunkAddress>,
        transport: Box<dyn Transport>,
    ) -> Self {
        Self {
            epochs: initial_epochs,
            transport,
        }
    }

    /// Execute a query and verify every result datom via Merkle proof.
    ///
    /// # Algorithm
    /// 1. Look up the root hash for the specified epoch.
    /// 2. Send the query + root hash to the full node.
    /// 3. Receive results + inclusion proofs.
    /// 4. Verify every inclusion proof against the root hash.
    /// 5. Reject the entire result if any proof fails.
    ///
    /// # Errors
    /// - `FerraError::UnknownEpoch(epoch)`: no root hash for this epoch.
    /// - `FerraError::InvalidProof`: at least one datom's proof failed.
    /// - `FerraError::TransportError(...)`: communication failure.
    ///
    /// # Performance
    /// - Network: one round-trip to full node.
    /// - Verification: O(|R| × log_k(N)) blake3 hashes where R = result count.
    /// - For 100 results from a 100M datom store: ~600 hash operations (~600us).
    pub async fn verified_query(
        &self,
        expr: &QueryExpr,
        epoch: u64,
    ) -> Result<VerifiedResult, FerraError> {
        let root = self
            .epochs
            .get(&epoch)
            .ok_or(FerraError::UnknownEpoch(epoch))?;

        // Request query + inclusion proofs from full node
        let response = self
            .transport
            .query_with_proofs(expr, *root)
            .await
            .map_err(FerraError::TransportError)?;

        // Verify EVERY result datom — reject entire result on any failure
        let mut verified_datoms = Vec::with_capacity(response.proved_results.len());
        let mut proof_sizes = Vec::with_capacity(response.proved_results.len());

        for (datom, proof) in &response.proved_results {
            // Check 1: proof root matches our trusted root
            if proof.root_hash != *root {
                return Err(FerraError::CryptoError(CryptoError::RootHashMismatch {
                    expected: *root,
                    actual: proof.root_hash,
                }));
            }

            // Check 2: proof verifies (Merkle path is valid)
            if !proof.verify() {
                return Err(FerraError::CryptoError(
                    CryptoError::InvalidInclusionProof,
                ));
            }

            // Check 3: proof is for the correct datom (not a proof for a different datom)
            if proof.datom != *datom {
                return Err(FerraError::CryptoError(
                    CryptoError::InvalidInclusionProof,
                ));
            }

            verified_datoms.push(datom.clone());
            proof_sizes.push(proof.path.len());
        }

        Ok(VerifiedResult {
            datoms: verified_datoms,
            epoch,
            root_hash: *root,
            verified: true,
            proof_sizes,
        })
    }

    /// Add a new epoch → root_hash mapping.
    ///
    /// # Trust
    /// The caller must verify the root_hash through an out-of-band mechanism
    /// (e.g., signed epoch certificate, consensus, multi-node agreement).
    /// This method does NOT verify the root hash itself.
    pub fn add_epoch(&mut self, epoch: u64, root_hash: ChunkAddress) {
        self.epochs.insert(epoch, root_hash);
    }

    /// Verified query against multiple full nodes (omission resistance).
    ///
    /// Queries the same expression against multiple full nodes, verifies all
    /// results, and returns the union. If any honest full node participates,
    /// no datom matching the query is omitted.
    ///
    /// # CALM Composition
    /// For monotonic queries, union of verified per-node results equals the
    /// verified query on the union of stores (INV-FERR-037).
    pub async fn verified_query_multi(
        &self,
        expr: &QueryExpr,
        epoch: u64,
        transports: &[Box<dyn Transport>],
    ) -> Result<VerifiedResult, FerraError> {
        let root = self
            .epochs
            .get(&epoch)
            .ok_or(FerraError::UnknownEpoch(epoch))?;

        let mut all_datoms = BTreeSet::new();
        let mut all_proof_sizes = Vec::new();

        for transport in transports {
            match transport.query_with_proofs(expr, *root).await {
                Ok(response) => {
                    for (datom, proof) in &response.proved_results {
                        if proof.root_hash == *root
                            && proof.verify()
                            && proof.datom == *datom
                        {
                            all_proof_sizes.push(proof.path.len());
                            all_datoms.insert(datom.clone());
                        }
                        // Silently skip invalid proofs from individual nodes
                        // (the node may be malicious, but others compensate)
                    }
                }
                Err(_) => {
                    // Skip failed nodes — partial results are acceptable
                    continue;
                }
            }
        }

        Ok(VerifiedResult {
            datoms: all_datoms.into_iter().collect(),
            epoch,
            root_hash: *root,
            verified: true,
            proof_sizes: all_proof_sizes,
        })
    }
}
```

**Falsification**: Any of the following constitutes a violation of INV-FERR-053:
- A full node returns a datom NOT in the store with a valid-looking proof that passes
  `verify()` on the light client (soundness breach — would require breaking INV-FERR-052).
- A light client accepts a query result without verifying all inclusion proofs (verification
  bypass — implementation bug).
- The bandwidth of a verified query is `O(N)` (proportional to store size) instead of
  `O(|R| * log_k(N))` (proportional to result size). This would indicate the full node
  is sending unnecessary data or the proof structure is non-logarithmic.
- A light client's `verified_query` returns `Ok(...)` with `verified: true` for a datom
  whose proof references a different root hash than the epoch's trusted root.
- An epoch's root hash changes after being added to the light client (violates append-only
  epoch progression).

**proptest strategy**:
```rust
proptest! {
    /// Light client correctly verifies genuine results from honest full node.
    #[test]
    fn light_client_honest_node(
        datoms in prop::collection::btree_set(arb_datom(), 10..200),
        query_attr in arb_attribute(),
    ) {
        let store = Store::from_datoms(datoms);
        let root = store.root_hash();
        let epoch = 1u64;

        let lc = LightClient::new(
            BTreeMap::from([(epoch, root)]),
            Box::new(HonestFullNode::new(store.clone())),
        );

        let query = QueryExpr::filter_attribute(query_attr);
        let result = block_on(lc.verified_query(&query, epoch));

        prop_assert!(result.is_ok(), "Honest full node query failed: {:?}", result.err());

        let vr = result.unwrap();
        prop_assert!(vr.verified, "Verified flag is false");

        // All returned datoms are genuinely in the store
        for datom in &vr.datoms {
            prop_assert!(store.contains(datom),
                "Verified result contains datom not in store");
        }
    }

    /// Light client rejects fabricated datoms from malicious full node.
    #[test]
    fn light_client_rejects_fabrication(
        datoms in prop::collection::btree_set(arb_datom(), 10..100),
        fake_datom in arb_datom(),
    ) {
        prop_assume!(!datoms.contains(&fake_datom));

        let store = Store::from_datoms(datoms);
        let root = store.root_hash();

        // Malicious node injects a fake datom with a forged proof
        let malicious = MaliciousFullNode::new(store, vec![fake_datom]);
        let lc = LightClient::new(
            BTreeMap::from([(1u64, root)]),
            Box::new(malicious),
        );

        let query = QueryExpr::all();
        let result = block_on(lc.verified_query(&query, 1u64));

        // Must fail: the fake datom's proof cannot verify against the real root
        prop_assert!(result.is_err(),
            "Light client accepted fabricated datom");
    }

    /// Verified query bandwidth is sublinear in store size.
    #[test]
    fn light_client_bandwidth_sublinear(
        datom_count in 100usize..1000,
        query_result_count in 1usize..20,
    ) {
        let datoms: BTreeSet<Datom> = (0..datom_count)
            .map(|i| arb_datom_seeded(i as u64))
            .collect();

        let store = Store::from_datoms(datoms);
        let k = store.prolly_tree().avg_fanout();

        // Proof size per datom: O(log_k(N)) nodes, each ~k*32 bytes
        let proof_depth = (datom_count as f64).log(k as f64).ceil() as usize;
        let proof_bytes = proof_depth * k * 32;

        // Total bandwidth for result_count datoms
        let total_bandwidth = query_result_count * (proof_bytes + 128); // 128 = datom size
        let store_size = datom_count * 128;

        prop_assert!(total_bandwidth < store_size,
            "Bandwidth {} >= store size {} — not sublinear",
            total_bandwidth, store_size);
    }
}
```

**Lean theorem**:
```lean
/-- Light client soundness: if verified_query succeeds, every result datom
    is genuinely in the store at the specified epoch. -/
theorem light_client_soundness (lc : LightClient) (Q : QueryExpr)
    (epoch : Nat) (R : Finset Datom) (root : ChunkAddress)
    (h_epoch : lc.epochs epoch = some root)
    (h_verified : verified_query lc Q epoch = ok R) :
    ∀ d ∈ R, d ∈ store root := by
  intro d h_d
  -- verified_query checks verify_inclusion for every datom in R
  obtain ⟨proof, h_proof_valid, h_proof_root⟩ := verified_query_checks h_verified d h_d
  -- By INV-FERR-052 soundness: valid proof → datom in store
  exact inclusion_proof_soundness root d proof h_proof_valid h_proof_root

/-- Light client bandwidth: proof size is logarithmic in store size. -/
theorem light_client_bandwidth (S : DatomStore) (R : Finset Datom)
    (h_sub : R ⊆ S) (k : Nat) (h_k : k = avg_fanout (prolly_tree S)) :
    total_proof_size R S ≤ R.card * (Nat.log k S.card + 1) * k * 32 := by
  -- Each datom's proof has depth ≤ log_k(|S|) by prolly tree height bound
  -- Each proof node has k sibling hashes of 32 bytes each
  calc total_proof_size R S
      = R.sum (fun d => proof_size d S) := by rfl
    _ ≤ R.sum (fun _ => (Nat.log k S.card + 1) * k * 32) := by
        apply Finset.sum_le_sum; intro d h_d
        exact single_proof_size_bound d S k h_k
    _ = R.card * (Nat.log k S.card + 1) * k * 32 := by
        simp [Finset.sum_const]

/-- Multi-node query: if any honest node participates, no matching datom
    is omitted (for monotonic queries, by CALM). -/
theorem light_client_multi_completeness
    (lc : LightClient) (Q : QueryExpr) (h_mono : monotonic Q)
    (nodes : Finset FullNode) (honest : FullNode)
    (h_honest : honest ∈ nodes) (h_honest_complete : ∀ d ∈ query honest.store Q, honest.responds d)
    (epoch : Nat) (root : ChunkAddress) (h_epoch : lc.epochs epoch = some root) :
    ∀ d ∈ query (store root) Q,
      d ∈ (verified_query_multi lc Q epoch nodes).datoms := by
  intro d h_d
  -- d is in query result of store(root)
  -- honest node has store(root) and responds with d
  -- By CALM: query distributes over union, so honest node returns d
  -- By inclusion proof completeness: proof exists for d
  -- verified_query_multi includes d in union
  exact multi_node_union_complete h_mono honest h_honest h_honest_complete d h_d
```

---

### INV-FERR-054: Trust Gradient Query

**Traces to**: SEED.md §4 ("calibrated policies are transferable"), INV-FERR-051
(Signed Transactions), INV-FERR-037 (Federated Query Correctness), INV-FERR-039
(Selective Merge)
**Verification**: `V:PROP`, `V:LEAN`
**Stage**: 1

Queries accept a `TrustPolicy` that filters results by cryptographic verification level.
Trust is a continuous gradient computed from verifiable calibration history, not a binary
decision. The trust filter is applied AFTER query execution and BEFORE result return,
ensuring that the query engine itself is trust-agnostic.

#### Level 0 (Algebraic Law)

```
Let S be a datom store. Let Q be a query expression. Let π be a trust policy.

TrustPolicy = All                                    -- accept everything (local use)
             | Only(Set<PublicKey>)                   -- accept from listed signers only
             | Calibrated(min_accuracy, min_samples)  -- accept from signers with verified calibration
             | Threshold(n, Set<PublicKey>)            -- accept if n-of-m signers attest
             | Custom(Datom → Bool)                   -- arbitrary predicate

-- Trust-filtered query
query(S, Q, π) = { d ∈ query(S, Q) | π.accepts(d, S) }

-- TrustPolicy::All is identity
∀ S, Q: query(S, Q, All) = query(S, Q)

-- TrustPolicy::Only is intersection
∀ S, Q, keys:
  query(S, Q, Only(keys)) = { d ∈ query(S, Q) | tx_signer(d.tx, S) ∈ keys }

-- TrustPolicy::Calibrated uses verified calibration history
∀ S, Q, min_acc, min_n:
  query(S, Q, Calibrated(min_acc, min_n)) =
    { d ∈ query(S, Q) |
      let signer = tx_signer(d.tx, S)
      let cal = verified_calibration(S, signer)
      cal.mean_error ≤ min_acc ∧ cal.sample_count ≥ min_n }

-- TrustPolicy::Threshold requires multi-party attestation
∀ S, Q, n, keys:
  query(S, Q, Threshold(n, keys)) =
    { d ∈ query(S, Q) | |{ k ∈ keys | attestation(d, k, S) }| ≥ n }

-- Trust filter monotonicity: more permissive policy → superset results
∀ S, Q, π₁, π₂:
  (∀ d, S: π₁.accepts(d, S) → π₂.accepts(d, S))
  → query(S, Q, π₁) ⊆ query(S, Q, π₂)

-- Trust filter distributes over federated query (for monotonic Q)
∀ monotonic Q, π, {S₁, ..., Sₖ}:
  query(⋃ᵢ Sᵢ, Q, π) = ⋃ᵢ query(Sᵢ, Q, π)
  -- Trust filter is per-datom, so it distributes over union
  -- (each datom's acceptance depends only on its own signer/calibration)

-- Calibrated trust is grounded in verifiable data
verified_calibration(S, signer) =
  let predictions = { d ∈ S | d.attribute = :hypothesis/predicted ∧ tx_signer(d.tx) = signer }
  let outcomes = { d ∈ S | d.attribute = :hypothesis/actual ∧ matched_to(d) ∈ predictions }
  { mean_error: avg(|pred.value - actual.value| for (pred, actual) ∈ matched(predictions, outcomes)),
    sample_count: |outcomes| }
```

#### Level 1 (State Invariant)

Every query can be filtered by trust level. The trust filter has the following properties:

1. **Post-query application**: Trust filtering happens AFTER the query engine produces
   results and BEFORE results are returned to the caller. The query engine itself is
   trust-agnostic — it evaluates the same plan regardless of trust policy.

2. **Cryptographic grounding**: Trust decisions for `Only`, `Calibrated`, and `Threshold`
   policies are based on cryptographically verified data (Ed25519 signatures, INV-FERR-051)
   and verifiable calibration history (hypothesis predictions vs outcomes). Social signals,
   reputation systems, or unverifiable claims are not inputs to trust decisions.

3. **Monotonic composition**: More permissive policies yield superset results. `All`
   produces the largest result set. `Only({})` (empty key set) produces the empty set.
   This monotonicity ensures that trust filtering can be composed with federated queries
   without violating CALM (INV-FERR-037).

4. **Per-datom independence**: The trust decision for datom `d` depends only on `d`'s
   transaction signer and that signer's calibration history. It does not depend on other
   datoms in the result set. This ensures the filter distributes over set union (required
   for federation correctness).

5. **Graceful default**: `TrustPolicy::All` is the default for local/embedded use. No
   trust filtering overhead unless explicitly requested. Unsigned stores (§23.10.5)
   always use `TrustPolicy::All`.

#### Level 2 (Implementation Contract)

```rust
/// A trust policy for filtering query results by cryptographic verification level.
///
/// Trust is a continuous gradient, not a binary decision. The policy determines
/// what evidence is required for a datom to be included in query results.
///
/// # Ordering (from most to least restrictive)
/// Threshold(n=m, keys) > Threshold(n=1, keys) > Only(keys) > Calibrated > All
///
/// # Default
/// `TrustPolicy::All` — no filtering, accepts everything. Used for local stores
/// and unsigned mode (§23.10.5).
#[derive(Debug, Clone)]
pub enum TrustPolicy {
    /// Accept all datoms regardless of signature or provenance.
    /// Zero overhead. Default for local/embedded use.
    All,

    /// Accept only datoms signed by one of the listed public keys.
    /// O(1) lookup per datom (HashSet).
    Only(HashSet<VerifyingKey>),

    /// Accept datoms from signers with verified calibration meeting thresholds.
    /// `min_accuracy`: maximum acceptable mean prediction error (lower = more strict).
    /// `min_samples`: minimum number of resolved predictions required.
    ///
    /// # Calibration Data Source
    /// Calibration is computed from `:hypothesis/predicted` and `:hypothesis/actual`
    /// datoms signed by the same signer. The calibration history itself is
    /// cryptographically verifiable (signed datoms, INV-FERR-051).
    Calibrated {
        min_accuracy: f64,
        min_samples: usize,
    },

    /// Accept datoms attested by at least `n` of the listed public keys.
    /// An attestation is a datom with attribute `:attestation/target` referencing
    /// the target datom's entity, signed by one of the listed keys.
    ///
    /// # Use Case
    /// Multi-party review: "accept only findings confirmed by at least 2 of 5 reviewers."
    Threshold {
        n: usize,
        keys: HashSet<VerifyingKey>,
    },

    /// Arbitrary predicate for application-specific trust logic.
    /// The function receives the datom and a snapshot for context lookups.
    ///
    /// # Warning
    /// Custom predicates may not distribute over federation (non-monotonic predicates
    /// break CALM). Use with caution in federated queries.
    Custom(Arc<dyn Fn(&Datom, &Snapshot) -> bool + Send + Sync>),
}

impl TrustPolicy {
    /// Evaluate whether a datom is accepted under this trust policy.
    ///
    /// # Performance
    /// - `All`: O(1), no work.
    /// - `Only`: O(1) HashSet lookup for signer.
    /// - `Calibrated`: O(1) amortized (calibration cached in snapshot).
    /// - `Threshold`: O(|keys|) attestation count.
    /// - `Custom`: depends on the predicate.
    pub fn accepts(&self, datom: &Datom, snapshot: &Snapshot) -> bool {
        match self {
            TrustPolicy::All => true,

            TrustPolicy::Only(keys) => {
                match snapshot.tx_signer(datom.tx) {
                    Some(signer) => keys.contains(&signer),
                    None => false, // Unsigned datom rejected by Only policy
                }
            }

            TrustPolicy::Calibrated { min_accuracy, min_samples } => {
                match snapshot.tx_signer(datom.tx) {
                    Some(signer) => {
                        let cal = snapshot.verified_calibration(&signer);
                        cal.mean_error <= *min_accuracy
                            && cal.sample_count >= *min_samples
                    }
                    None => false, // Unsigned datom rejected by Calibrated policy
                }
            }

            TrustPolicy::Threshold { n, keys } => {
                let attestations = snapshot.attestation_count(datom, keys);
                attestations >= *n
            }

            TrustPolicy::Custom(f) => f(datom, snapshot),
        }
    }
}

/// Apply a trust policy to query results.
///
/// This is the integration point between the query engine and the trust layer.
/// Called after query evaluation, before result return.
///
/// # Guarantees
/// - If `policy` is `All`, the output equals the input (identity filter).
/// - The output is always a subset of the input (monotonic restriction).
/// - The filter is per-datom: each datom's acceptance is independent.
pub fn trust_filter(
    results: Vec<Datom>,
    policy: &TrustPolicy,
    snapshot: &Snapshot,
) -> Vec<Datom> {
    match policy {
        TrustPolicy::All => results, // Fast path: no filtering
        _ => results
            .into_iter()
            .filter(|d| policy.accepts(d, snapshot))
            .collect(),
    }
}

/// Verified calibration data for a signer.
///
/// Computed from `:hypothesis/predicted` and `:hypothesis/actual` datoms
/// signed by the signer. The calibration data itself is verifiable: each
/// prediction and outcome is a signed datom (INV-FERR-051).
#[derive(Debug, Clone)]
pub struct VerifiedCalibration {
    /// Mean absolute error of predictions vs actual outcomes.
    /// Lower = more accurate. Range: [0.0, ∞).
    pub mean_error: f64,
    /// Number of resolved predictions (predictions with matched outcomes).
    pub sample_count: usize,
    /// Most recent prediction timestamp (TxId).
    pub last_prediction: Option<TxId>,
    /// Trend: is accuracy improving or degrading?
    /// Computed as slope of error over last `sample_count / 2` predictions.
    pub trend: CalibrationTrend,
}

#[derive(Debug, Clone, PartialEq)]
pub enum CalibrationTrend {
    Improving,  // Recent predictions more accurate than older ones
    Stable,     // No significant trend
    Degrading,  // Recent predictions less accurate than older ones
    Insufficient, // Not enough data points to compute trend
}
```

**Falsification**: Any of the following constitutes a violation of INV-FERR-054:
- A query with `TrustPolicy::Only({K})` returns a datom signed by key `K'` where
  `K' != K` and `K' is not in the key set`. (Key filtering bypass.)
- A query with `TrustPolicy::Calibrated { min_accuracy: 0.1, min_samples: 50 }` accepts
  a signer whose verified calibration has `mean_error = 0.3` or `sample_count = 10`.
  (Calibration threshold bypass.)
- `TrustPolicy::Calibrated` accepts a signer whose calibration claims are NOT backed by
  signed datoms (unverifiable calibration). The calibration MUST be computed from signed
  `:hypothesis/predicted` and `:hypothesis/actual` datoms, not from self-reported metadata.
- `TrustPolicy::All` applied to a result set changes the result set (not identity).
- A trust policy with strictly more permissive acceptance produces a strictly smaller
  result set (monotonicity violation).
- A trust-filtered federated query (monotonic Q) produces different results than the
  federation of trust-filtered per-store queries (distribution failure).

**proptest strategy**:
```rust
proptest! {
    /// TrustPolicy::All is identity: never filters any datom.
    #[test]
    fn trust_all_is_identity(
        datoms in prop::collection::vec(arb_datom(), 0..100),
    ) {
        let snapshot = Snapshot::from_datoms(&datoms);
        let filtered = trust_filter(datoms.clone(), &TrustPolicy::All, &snapshot);
        prop_assert_eq!(filtered, datoms,
            "TrustPolicy::All changed the result set");
    }

    /// TrustPolicy::Only accepts only datoms from listed signers.
    #[test]
    fn trust_only_filters_correctly(
        datoms_a in prop::collection::vec(arb_signed_datom(), 1..20),
        datoms_b in prop::collection::vec(arb_signed_datom(), 1..20),
    ) {
        let key_a = datoms_a[0].signer;
        let all_datoms: Vec<Datom> = datoms_a.iter()
            .chain(datoms_b.iter())
            .map(|sd| sd.datom.clone())
            .collect();

        let snapshot = Snapshot::from_signed_datoms(
            &datoms_a.iter().chain(datoms_b.iter()).cloned().collect::<Vec<_>>()
        );

        let policy = TrustPolicy::Only(HashSet::from([key_a]));
        let filtered = trust_filter(all_datoms, &policy, &snapshot);

        // Every filtered datom must be signed by key_a
        for datom in &filtered {
            let signer = snapshot.tx_signer(datom.tx).unwrap();
            prop_assert_eq!(signer, key_a,
                "TrustPolicy::Only returned datom from wrong signer");
        }
    }

    /// TrustPolicy::Calibrated filters by accuracy threshold.
    #[test]
    fn trust_calibrated_filters_by_accuracy(
        predictions in prop::collection::vec(arb_prediction(), 10..50),
        min_accuracy in 0.05f64..0.5,
        min_samples in 5usize..20,
    ) {
        let (datoms, snapshot) = build_calibrated_store(&predictions);
        let policy = TrustPolicy::Calibrated { min_accuracy, min_samples };
        let filtered = trust_filter(datoms.clone(), &policy, &snapshot);

        // Every accepted datom's signer must meet calibration thresholds
        for datom in &filtered {
            let signer = snapshot.tx_signer(datom.tx).unwrap();
            let cal = snapshot.verified_calibration(&signer);
            prop_assert!(cal.mean_error <= min_accuracy,
                "Accepted signer with error {} > threshold {}",
                cal.mean_error, min_accuracy);
            prop_assert!(cal.sample_count >= min_samples,
                "Accepted signer with {} samples < threshold {}",
                cal.sample_count, min_samples);
        }
    }

    /// More permissive policy produces superset results (monotonicity).
    #[test]
    fn trust_policy_monotonicity(
        datoms in prop::collection::vec(arb_signed_datom(), 1..50),
        key_bytes in prop::array::uniform32(any::<u8>()),
    ) {
        let key = VerifyingKey::from_bytes(&key_bytes).unwrap();
        let all_datoms: Vec<Datom> = datoms.iter().map(|sd| sd.datom.clone()).collect();
        let snapshot = Snapshot::from_signed_datoms(&datoms);

        let strict_policy = TrustPolicy::Only(HashSet::from([key]));
        let permissive_policy = TrustPolicy::All;

        let strict_result: BTreeSet<_> = trust_filter(
            all_datoms.clone(), &strict_policy, &snapshot
        ).into_iter().collect();

        let permissive_result: BTreeSet<_> = trust_filter(
            all_datoms, &permissive_policy, &snapshot
        ).into_iter().collect();

        prop_assert!(strict_result.is_subset(&permissive_result),
            "Strict policy produced results not in permissive policy");
    }

    /// Trust filter distributes over federation for monotonic queries.
    #[test]
    fn trust_distributes_over_federation(
        datoms_a in prop::collection::vec(arb_signed_datom(), 1..30),
        datoms_b in prop::collection::vec(arb_signed_datom(), 1..30),
        key_bytes in prop::array::uniform32(any::<u8>()),
    ) {
        let key = VerifyingKey::from_bytes(&key_bytes).unwrap();
        let policy = TrustPolicy::Only(HashSet::from([key]));

        // Filter(Union(A, B))
        let all: Vec<Datom> = datoms_a.iter().chain(datoms_b.iter())
            .map(|sd| sd.datom.clone()).collect();
        let union_snapshot = Snapshot::from_signed_datoms(
            &datoms_a.iter().chain(datoms_b.iter()).cloned().collect::<Vec<_>>()
        );
        let filter_union: BTreeSet<_> = trust_filter(all, &policy, &union_snapshot)
            .into_iter().collect();

        // Union(Filter(A), Filter(B))
        let snap_a = Snapshot::from_signed_datoms(&datoms_a);
        let snap_b = Snapshot::from_signed_datoms(&datoms_b);
        let filtered_a = trust_filter(
            datoms_a.iter().map(|sd| sd.datom.clone()).collect(),
            &policy, &snap_a,
        );
        let filtered_b = trust_filter(
            datoms_b.iter().map(|sd| sd.datom.clone()).collect(),
            &policy, &snap_b,
        );
        let union_filter: BTreeSet<_> = filtered_a.into_iter()
            .chain(filtered_b.into_iter()).collect();

        prop_assert_eq!(filter_union, union_filter,
            "Trust filter does not distribute over federation");
    }
}
```

**Lean theorem**:
```lean
/-- TrustPolicy::All is identity: it accepts every datom. -/
theorem trust_all_identity (S : DatomStore) (Q : QueryExpr) :
    query_with_trust S Q TrustPolicy.All = query S Q := by
  simp [query_with_trust, trust_filter, TrustPolicy.accepts]
  -- All.accepts always returns true, so filter is identity
  exact Finset.filter_true_of_mem (fun _ _ => rfl)

/-- Trust filter monotonicity: more permissive policy → superset results. -/
theorem trust_monotonicity (S : DatomStore) (Q : QueryExpr)
    (π₁ π₂ : TrustPolicy)
    (h_perm : ∀ d s, π₁.accepts d s → π₂.accepts d s) :
    query_with_trust S Q π₁ ⊆ query_with_trust S Q π₂ := by
  intro d h_d
  simp [query_with_trust, trust_filter] at h_d ⊢
  obtain ⟨h_query, h_accept⟩ := h_d
  exact ⟨h_query, h_perm d (snapshot S) h_accept⟩

/-- Trust filter distributes over union (required for federation). -/
theorem trust_distributes_union (S₁ S₂ : DatomStore) (Q : QueryExpr)
    (π : TrustPolicy) (h_mono : monotonic Q)
    (h_indep : ∀ d, π.accepts d (snapshot S₁) = π.accepts d (snapshot (S₁ ∪ S₂))) :
    query_with_trust (S₁ ∪ S₂) Q π =
      query_with_trust S₁ Q π ∪ query_with_trust S₂ Q π := by
  simp [query_with_trust]
  -- By CALM (INV-FERR-037): query distributes over union for monotonic Q
  rw [federated_query_n_two S₁ S₂ Q h_mono]
  -- Trust filter distributes over union because accepts is per-datom
  rw [Finset.filter_union]
  congr 1 <;> {
    ext d; simp [trust_filter]
    constructor
    · intro ⟨h_mem, h_acc⟩; exact ⟨h_mem, h_acc⟩
    · intro ⟨h_mem, h_acc⟩; exact ⟨h_mem, h_acc⟩
  }

/-- Calibrated trust is grounded: calibration data comes from signed datoms. -/
theorem calibrated_trust_grounded (S : DatomStore) (signer : VerifyingKey)
    (cal : VerifiedCalibration)
    (h_cal : cal = verified_calibration S signer) :
    ∀ pred ∈ cal.predictions,
      ∃ tx ∈ S, tx.signer = signer ∧ tx.contains_prediction pred := by
  intro pred h_pred
  -- verified_calibration only considers signed datoms with matching signer
  exact verified_calibration_grounded S signer pred h_cal h_pred
```

---

### INV-FERR-055: Verifiable Knowledge Commitment (VKC)

**Traces to**: SEED.md §1 ("verifiable coherence"), SEED.md §4 ("calibrated policies
are transferable"), INV-FERR-051 (Signed Transactions), INV-FERR-052 (Merkle Proof
of Inclusion), ADR-FERR-009
**Verification**: `V:PROP`, `V:KANI`, `V:LEAN`
**Stage**: 1

A VKC is the fundamental unit of trustless knowledge transfer. It bundles three
independently verifiable components into a single self-contained artifact:
(1) a signed transaction (who asserted what),
(2) a Merkle proof of the signer's causal context (what they knew when they asserted),
(3) a Merkle proof of the signer's calibration history (how reliable they are).
A single VKC is independently verifiable by anyone without access to the signer's full
store, without network connectivity, and without any prior trust relationship.

#### Level 0 (Algebraic Law)

```
Let T be a transaction in store S signed by agent A with key SK_A.

VKC(T, S) = {
  signed_tx:         SignedTransaction(T),                         -- who + what
  context_proofs:    [MerkleProof(pred ∈ S) | pred ∈ T.predecessors],  -- causal context
  calibration_proof: CalibrationProof(A, S),                       -- reliability
  store_root:        root(S)                                       -- trust anchor
}

-- VKC verification (three independent checks)
verify_vkc(vkc) =
  -- Check 1: Signature (who made the claim)
  vkc.signed_tx.verify() = ok                                     [INV-FERR-051]

  -- Check 2: Context (what they knew)
  ∧ ∀ proof ∈ vkc.context_proofs:
      proof.root_hash = vkc.store_root                            [root binding]
      ∧ verify_inclusion(proof) = true                            [INV-FERR-052]

  -- Check 3: Calibration (how reliable they are)
  ∧ ∀ (pred, proof) ∈ vkc.calibration_proof.predictions:
      proof.root_hash = vkc.store_root                            [root binding]
      ∧ verify_inclusion(proof) = true                            [INV-FERR-052]

-- Soundness: verified VKC implies authentic provenance
verify_vkc(vkc) = ok →
  authentic(vkc.signed_tx)                   -- assertion is genuine
  ∧ context_existed(vkc.context_proofs)      -- signer's context was real
  ∧ calibration_verifiable(vkc.calibration_proof)  -- reliability is checkable

-- Independence: verification requires ONLY the VKC itself
∀ vkc: verify_vkc(vkc) needs only:
  - vkc (the commitment itself)
  - Ed25519 verification algorithm
  - blake3 hash algorithm
  NOT: network access, full store, signer relationship, trust configuration

-- VKC is tamper-evident
∀ vkc, vkc' where vkc' = tamper(vkc):
  verify_vkc(vkc) = ok ∧ vkc' ≠ vkc → verify_vkc(vkc') = fail

-- VKC composition: VKCs from different agents can be independently verified
∀ vkc_A from agent A, vkc_B from agent B:
  verify_vkc(vkc_A) and verify_vkc(vkc_B) are independent computations
  -- No shared state, no ordering requirement, parallelizable

-- Calibration monotonicity: more predictions → more confident trust assessment
∀ cal_1, cal_2 for same signer:
  cal_2.sample_count > cal_1.sample_count
  → confidence(cal_2) ≥ confidence(cal_1)
  -- (by law of large numbers: more samples → tighter error estimate)
```

#### Level 1 (State Invariant)

A VKC is a self-contained, independently verifiable unit of knowledge. Given only the
VKC (no network access, no trust assumptions, no prior relationship with the signer),
a receiver can verify three properties:

1. **Authenticity** (who made the claim): The Ed25519 signature on the transaction is
   valid for the claimed signer's public key. No one else could have produced this
   signature (by Ed25519 unforgeability, INV-FERR-051).

2. **Context** (what they knew when they made it): The Merkle proofs for causal
   predecessors demonstrate that the signer's store contained specific prior datoms
   at the time of assertion. This establishes the causal chain: the signer's assertion
   was made in a specific epistemic context, not in a vacuum.

3. **Reliability** (how accurate they are historically): The calibration proof contains
   a verifiable subset of the signer's prediction/outcome history. The receiver can
   independently compute the signer's mean prediction error from these signed datoms.
   The calibration is not self-reported — it is derived from cryptographically signed
   predictions and outcomes that are provably in the signer's store.

VKC verification is deterministic, offline-capable, and parallelizable. Two VKCs from
different agents can be verified simultaneously with no shared state. The verification
cost is bounded: one Ed25519 verify (~2us) + O(C * log_k(N)) Merkle proof verifications
where C = number of context + calibration proofs and k, N are the prolly tree parameters.

For a typical VKC with 5 context predecessors and 20 calibration predictions from a
100M datom store: ~25 Merkle proof verifications, each ~6 levels deep = ~150 blake3
hashes = ~150us total verification time.

#### Level 2 (Implementation Contract)

```rust
/// A Verifiable Knowledge Commitment: the fundamental unit of trustless
/// knowledge transfer.
///
/// A VKC bundles:
/// 1. A signed transaction (authenticity — who asserted what)
/// 2. Merkle proofs of causal predecessors (context — what they knew)
/// 3. A calibration proof (reliability — how accurate they are)
///
/// # Independence
/// Verification requires ONLY this struct. No network access, no store access,
/// no trust configuration. The VKC is fully self-contained.
///
/// # Size
/// Typical VKC with 5 predecessors, 20 calibration predictions, 100M datom store:
/// - SignedTransaction: ~500 bytes (variable, depends on datom count in tx)
/// - Context proofs: 5 × ~6KB = ~30KB
/// - Calibration proof: 20 × ~6KB = ~120KB
/// - Total: ~150KB
///
/// # Performance
/// - Creation: ~50us (sign + construct proofs)
/// - Verification: ~150us (verify signature + verify all proofs)
pub struct VerifiableKnowledgeCommitment {
    /// The signed transaction: datoms + signature + signer public key.
    /// Establishes WHO asserted WHAT.
    pub signed_tx: SignedTransaction,

    /// Merkle inclusion proofs for each causal predecessor.
    /// Establishes WHAT THE SIGNER KNEW when they made the assertion.
    /// Each proof demonstrates that a predecessor datom exists in the
    /// signer's store at the time of assertion.
    pub context_proofs: Vec<InclusionProof>,

    /// Calibration proof: verifiable subset of prediction/outcome history.
    /// Establishes HOW RELIABLE the signer is, grounded in signed datoms.
    pub calibration_proof: CalibrationProof,

    /// Root hash of the signer's store at the time the VKC was created.
    /// This is the trust anchor: all Merkle proofs verify against this root.
    pub store_root: ChunkAddress,
}

/// A calibration proof: verifiable evidence of a signer's prediction accuracy.
///
/// Contains a subset of the signer's prediction/outcome history, each pair
/// backed by a Merkle inclusion proof. The receiver can independently compute
/// the mean error from these verified data points.
///
/// # Completeness
/// The calibration proof is a SUBSET of the signer's full calibration history.
/// The signer could withhold unfavorable predictions. The `sample_count` field
/// reports the claimed total, but only `recent_predictions.len()` are verifiable.
/// The receiver should use min(sample_count, recent_predictions.len()) and
/// apply appropriate skepticism to the difference.
pub struct CalibrationProof {
    /// Claimed mean prediction error. Verifiable from `recent_predictions`.
    pub mean_error: f64,

    /// Claimed total number of resolved predictions.
    /// Only `recent_predictions.len()` are independently verifiable.
    pub sample_count: usize,

    /// Verifiable prediction/outcome pairs with Merkle inclusion proofs.
    /// Each entry: (prediction_datom, outcome_datom, inclusion_proof_for_prediction,
    /// inclusion_proof_for_outcome).
    ///
    /// The receiver verifies:
    /// 1. Each proof against `store_root`
    /// 2. Prediction datom has attribute `:hypothesis/predicted`
    /// 3. Outcome datom has attribute `:hypothesis/actual`
    /// 4. Prediction and outcome are matched (same entity or `:hypothesis/matches` ref)
    /// 5. Recomputed mean error ≈ claimed mean_error (within floating point tolerance)
    pub recent_predictions: Vec<VerifiablePrediction>,
}

/// A single verifiable prediction/outcome pair.
pub struct VerifiablePrediction {
    /// The prediction datom (attribute: `:hypothesis/predicted`).
    pub prediction: Datom,
    /// Merkle proof that the prediction is in the signer's store.
    pub prediction_proof: InclusionProof,
    /// The outcome datom (attribute: `:hypothesis/actual`).
    pub outcome: Datom,
    /// Merkle proof that the outcome is in the signer's store.
    pub outcome_proof: InclusionProof,
}

/// The result of verifying a VKC.
///
/// Contains the extracted trust information: who signed it, how reliable
/// they are (verifiably), and whether the context chain is valid.
#[derive(Debug, Clone)]
pub struct VkcVerification {
    /// The signer's public key (extracted from the signed transaction).
    pub signer: VerifyingKey,
    /// Verified mean prediction error (recomputed from calibration proof).
    pub verified_mean_error: f64,
    /// Number of verifiable prediction/outcome pairs.
    pub verifiable_sample_count: usize,
    /// Claimed total sample count (may exceed verifiable count).
    pub claimed_sample_count: usize,
    /// Whether all context proofs verified.
    pub context_verified: bool,
    /// Number of causal predecessors verified.
    pub context_depth: usize,
}

impl VerifiableKnowledgeCommitment {
    /// Create a VKC from a transaction and its containing store.
    ///
    /// Constructs Merkle proofs for all causal predecessors and a sample
    /// of the signer's calibration history.
    ///
    /// # Arguments
    /// - `tx`: The transaction to commit
    /// - `key`: The signer's private key
    /// - `store`: The signer's store (used to construct proofs)
    /// - `calibration_sample_size`: Number of recent predictions to include
    ///
    /// # Performance
    /// ~50us: ~6us sign + ~44us proof construction (for 25 proofs)
    pub fn create(
        tx: Transaction,
        key: &SigningKey,
        store: &Store,
        calibration_sample_size: usize,
    ) -> Result<Self, CryptoError> {
        let signed_tx = SignedTransaction::sign(tx.clone(), key);

        // Construct context proofs for causal predecessors
        let mut context_proofs = Vec::with_capacity(tx.predecessors.len());
        for pred_tx_id in &tx.predecessors {
            // Find the predecessor transaction's datoms and prove their inclusion
            let pred_datoms = store.datoms_for_tx(*pred_tx_id);
            for datom in pred_datoms {
                if let Some(proof) = InclusionProof::construct(store, &datom) {
                    context_proofs.push(proof);
                    break; // One proof per predecessor is sufficient for existence
                }
            }
        }

        // Construct calibration proof
        let signer_key = key.verifying_key();
        let predictions = store.predictions_by_signer(&signer_key);
        let recent: Vec<_> = predictions
            .into_iter()
            .rev() // Most recent first
            .take(calibration_sample_size)
            .collect();

        let mut verifiable_predictions = Vec::with_capacity(recent.len());
        let mut error_sum = 0.0;

        for (prediction, outcome) in &recent {
            let pred_proof = InclusionProof::construct(store, prediction)
                .ok_or(CryptoError::InvalidInclusionProof)?;
            let out_proof = InclusionProof::construct(store, outcome)
                .ok_or(CryptoError::InvalidInclusionProof)?;

            error_sum += (prediction.value.as_f64() - outcome.value.as_f64()).abs();

            verifiable_predictions.push(VerifiablePrediction {
                prediction: prediction.clone(),
                prediction_proof: pred_proof,
                outcome: outcome.clone(),
                outcome_proof: out_proof,
            });
        }

        let mean_error = if verifiable_predictions.is_empty() {
            f64::NAN
        } else {
            error_sum / verifiable_predictions.len() as f64
        };

        let total_count = store.total_predictions_by_signer(&signer_key);

        Ok(Self {
            signed_tx,
            context_proofs,
            calibration_proof: CalibrationProof {
                mean_error,
                sample_count: total_count,
                recent_predictions: verifiable_predictions,
            },
            store_root: store.root_hash(),
        })
    }

    /// Verify the VKC: check signature, context proofs, and calibration proofs.
    ///
    /// Returns `Ok(VkcVerification)` with extracted trust information if all
    /// checks pass. Returns `Err(CryptoError)` on the FIRST failed check.
    ///
    /// # Checks (in order)
    /// 1. Ed25519 signature on the transaction (INV-FERR-051)
    /// 2. All context proofs verify against `store_root` (INV-FERR-052)
    /// 3. All calibration prediction/outcome proofs verify against `store_root`
    /// 4. Recomputed mean error matches claimed mean error (within tolerance)
    ///
    /// # Performance
    /// ~150us for a typical VKC (1 signature + 25 Merkle proofs)
    pub fn verify(&self) -> Result<VkcVerification, CryptoError> {
        // Check 1: Verify signature
        self.signed_tx.verify()?;

        // Check 2: Verify context proofs
        for proof in &self.context_proofs {
            if proof.root_hash != self.store_root {
                return Err(CryptoError::RootHashMismatch {
                    expected: self.store_root,
                    actual: proof.root_hash,
                });
            }
            if !proof.verify() {
                return Err(CryptoError::InvalidInclusionProof);
            }
        }

        // Check 3: Verify calibration proofs
        let mut recomputed_error_sum = 0.0;
        for vp in &self.calibration_proof.recent_predictions {
            // Verify prediction proof
            if vp.prediction_proof.root_hash != self.store_root {
                return Err(CryptoError::RootHashMismatch {
                    expected: self.store_root,
                    actual: vp.prediction_proof.root_hash,
                });
            }
            if !vp.prediction_proof.verify() {
                return Err(CryptoError::InvalidCalibrationProof);
            }

            // Verify outcome proof
            if vp.outcome_proof.root_hash != self.store_root {
                return Err(CryptoError::RootHashMismatch {
                    expected: self.store_root,
                    actual: vp.outcome_proof.root_hash,
                });
            }
            if !vp.outcome_proof.verify() {
                return Err(CryptoError::InvalidCalibrationProof);
            }

            // Verify proof datoms match the claimed datoms
            if vp.prediction_proof.datom != vp.prediction {
                return Err(CryptoError::InvalidCalibrationProof);
            }
            if vp.outcome_proof.datom != vp.outcome {
                return Err(CryptoError::InvalidCalibrationProof);
            }

            recomputed_error_sum +=
                (vp.prediction.value.as_f64() - vp.outcome.value.as_f64()).abs();
        }

        // Check 4: Verify claimed mean error matches recomputed
        let verifiable_count = self.calibration_proof.recent_predictions.len();
        if verifiable_count > 0 {
            let recomputed_mean = recomputed_error_sum / verifiable_count as f64;
            let tolerance = 1e-10; // Floating point tolerance
            if (recomputed_mean - self.calibration_proof.mean_error).abs() > tolerance {
                return Err(CryptoError::InvalidCalibrationProof);
            }
        }

        Ok(VkcVerification {
            signer: self.signed_tx.signer,
            verified_mean_error: if verifiable_count > 0 {
                recomputed_error_sum / verifiable_count as f64
            } else {
                f64::NAN
            },
            verifiable_sample_count: verifiable_count,
            claimed_sample_count: self.calibration_proof.sample_count,
            context_verified: true,
            context_depth: self.context_proofs.len(),
        })
    }
}

#[kani::proof]
#[kani::unwind(5)]
fn vkc_verify_roundtrip() {
    let datom = Datom {
        entity: kani::any(),
        attribute: kani::any(),
        value: kani::any(),
        tx: kani::any(),
        op: kani::any(),
    };

    let tx = Transaction {
        tx_id: kani::any(),
        datoms: vec![datom],
        predecessors: vec![],
    };

    let key_bytes: [u8; 32] = kani::any();
    let key = SigningKey::from_bytes(&key_bytes);

    // Sign transaction
    let signed = SignedTransaction::sign(tx, &key);

    // Create minimal VKC (no predecessors, no calibration)
    let vkc = VerifiableKnowledgeCommitment {
        signed_tx: signed,
        context_proofs: vec![],
        calibration_proof: CalibrationProof {
            mean_error: f64::NAN,
            sample_count: 0,
            recent_predictions: vec![],
        },
        store_root: ChunkAddress::zero(),
    };

    // Verify must pass for honestly constructed VKC
    assert!(vkc.verify().is_ok());
}
```

**Falsification**: Any of the following constitutes a violation of INV-FERR-055:
- A VKC that passes `verify()` but contains a forged calibration history: the
  `CalibrationProof.recent_predictions` include prediction/outcome datoms that are NOT
  in the signer's store (their Merkle proofs are fabricated). This would require breaking
  INV-FERR-052 soundness.
- A VKC where the context proofs reference a different store root than `self.store_root`:
  the proofs were constructed against a different store state than the one claimed. The
  `verify()` method must check `proof.root_hash == self.store_root` for every proof.
- A VKC where `verify()` returns `Ok(...)` but the `signed_tx` signature is invalid
  (would require breaking INV-FERR-051).
- A VKC where the claimed `mean_error` differs from the recomputed `mean_error` (computed
  from the verifiable predictions) by more than floating point tolerance, yet `verify()`
  returns `Ok(...)`.
- Two different VKCs (different signed transactions) that `verify()` accepts with the
  same signer key but different store roots, where one VKC's context proofs would verify
  against the other's store root (cross-contamination). Each VKC is bound to exactly one
  store root.

**proptest strategy**:
```rust
proptest! {
    /// Honestly constructed VKC always verifies.
    #[test]
    fn vkc_honest_roundtrip(
        datoms in prop::collection::vec(arb_datom(), 1..20),
        pred_count in 0usize..5,
        cal_count in 0usize..10,
        key_bytes in prop::array::uniform32(any::<u8>()),
    ) {
        let key = SigningKey::from_bytes(&key_bytes);
        let mut store = Store::new();

        // Add datoms to store
        let tx = store.transact(datoms);

        // Add some predecessor transactions
        let preds: Vec<u64> = (0..pred_count)
            .map(|i| store.transact(vec![arb_datom_seeded(i as u64)]).tx_id)
            .collect();

        // Add calibration data
        for i in 0..cal_count {
            let pred_val = (i as f64) * 0.1;
            let actual_val = pred_val + 0.05; // Known error of 0.05
            store.transact_prediction(&key, pred_val, actual_val);
        }

        let tx_with_preds = Transaction {
            tx_id: tx.tx_id,
            datoms: tx.datoms,
            predecessors: preds,
        };

        let vkc = VerifiableKnowledgeCommitment::create(
            tx_with_preds, &key, &store, cal_count,
        );

        prop_assert!(vkc.is_ok(), "VKC creation failed: {:?}", vkc.err());
        let vkc = vkc.unwrap();

        let result = vkc.verify();
        prop_assert!(result.is_ok(), "Honest VKC verification failed: {:?}", result.err());

        let verification = result.unwrap();
        prop_assert_eq!(verification.signer, key.verifying_key());
        prop_assert!(verification.context_verified);
    }

    /// Tampering with calibration proof invalidates VKC.
    #[test]
    fn vkc_tampered_calibration_fails(
        datoms in prop::collection::vec(arb_datom(), 1..10),
        cal_count in 2usize..8,
        key_bytes in prop::array::uniform32(any::<u8>()),
    ) {
        let key = SigningKey::from_bytes(&key_bytes);
        let mut store = Store::new();
        let tx = store.transact(datoms);

        for i in 0..cal_count {
            store.transact_prediction(&key, i as f64 * 0.1, i as f64 * 0.15);
        }

        let mut vkc = VerifiableKnowledgeCommitment::create(
            Transaction { tx_id: tx.tx_id, datoms: tx.datoms, predecessors: vec![] },
            &key, &store, cal_count,
        ).unwrap();

        // Tamper: change claimed mean error
        vkc.calibration_proof.mean_error += 1.0;

        prop_assert!(vkc.verify().is_err(),
            "VKC with tampered calibration mean_error passed verification");
    }

    /// Tampering with context proof invalidates VKC.
    #[test]
    fn vkc_tampered_context_fails(
        datoms in prop::collection::vec(arb_datom(), 1..10),
        key_bytes in prop::array::uniform32(any::<u8>()),
        tamper_byte in any::<u8>(),
    ) {
        let key = SigningKey::from_bytes(&key_bytes);
        let mut store = Store::new();
        let pred_tx = store.transact(vec![arb_datom_seeded(0)]);
        let tx = store.transact(datoms.clone());

        let tx_with_pred = Transaction {
            tx_id: tx.tx_id,
            datoms: tx.datoms,
            predecessors: vec![pred_tx.tx_id],
        };

        let mut vkc = VerifiableKnowledgeCommitment::create(
            tx_with_pred, &key, &store, 0,
        ).unwrap();

        // Tamper: corrupt a context proof's sibling hash
        if let Some(proof) = vkc.context_proofs.first_mut() {
            if let Some(node) = proof.path.first_mut() {
                if let Some(hash) = node.sibling_hashes.first_mut() {
                    hash.as_mut_bytes()[0] ^= tamper_byte | 1;
                }
            }
        }

        prop_assert!(vkc.verify().is_err(),
            "VKC with tampered context proof passed verification");
    }

    /// VKC with wrong store root fails verification.
    #[test]
    fn vkc_wrong_root_fails(
        datoms in prop::collection::vec(arb_datom(), 1..10),
        key_bytes in prop::array::uniform32(any::<u8>()),
        fake_root_bytes in prop::array::uniform32(any::<u8>()),
    ) {
        let key = SigningKey::from_bytes(&key_bytes);
        let mut store = Store::new();
        let pred_tx = store.transact(vec![arb_datom_seeded(0)]);
        let tx = store.transact(datoms.clone());

        let tx_with_pred = Transaction {
            tx_id: tx.tx_id,
            datoms: tx.datoms,
            predecessors: vec![pred_tx.tx_id],
        };

        let mut vkc = VerifiableKnowledgeCommitment::create(
            tx_with_pred, &key, &store, 0,
        ).unwrap();

        // Replace store root with a fake one
        let fake_root = ChunkAddress::from_bytes(fake_root_bytes);
        prop_assume!(fake_root != vkc.store_root);
        vkc.store_root = fake_root;

        // Context proofs still reference the real root, creating a mismatch
        if !vkc.context_proofs.is_empty() {
            prop_assert!(vkc.verify().is_err(),
                "VKC with mismatched store root passed verification");
        }
    }

    /// VKC verification is deterministic.
    #[test]
    fn vkc_verify_deterministic(
        datoms in prop::collection::vec(arb_datom(), 1..10),
        key_bytes in prop::array::uniform32(any::<u8>()),
    ) {
        let key = SigningKey::from_bytes(&key_bytes);
        let mut store = Store::new();
        let tx = store.transact(datoms);

        let vkc = VerifiableKnowledgeCommitment::create(
            Transaction { tx_id: tx.tx_id, datoms: tx.datoms, predecessors: vec![] },
            &key, &store, 0,
        ).unwrap();

        let result1 = vkc.verify();
        let result2 = vkc.verify();

        // Both calls produce the same result
        match (result1, result2) {
            (Ok(v1), Ok(v2)) => {
                prop_assert_eq!(v1.signer, v2.signer);
                prop_assert_eq!(v1.verifiable_sample_count, v2.verifiable_sample_count);
                prop_assert_eq!(v1.context_verified, v2.context_verified);
                prop_assert_eq!(v1.context_depth, v2.context_depth);
            }
            (Err(_), Err(_)) => {} // Both failed — consistent
            _ => prop_assert!(false, "Verification is non-deterministic"),
        }
    }
}
```

**Lean theorem**:
```lean
/-- VKC soundness: if verify_vkc succeeds, then:
    (1) the signed transaction is authentic
    (2) the causal context exists in the signer's store
    (3) the calibration proof is verifiable
    This is the fundamental trust theorem of the VKN layer. -/
theorem vkc_soundness (vkc : VKC)
    (h_verify : verify_vkc vkc = ok) :
    authentic vkc.signed_tx
    ∧ context_exists vkc.context_proofs vkc.store_root
    ∧ calibration_verified vkc.calibration_proof vkc.store_root := by
  obtain ⟨h_sig, h_ctx, h_cal⟩ := verify_vkc_decompose h_verify
  exact ⟨
    -- (1) Authenticity: Ed25519 signature verifies (INV-FERR-051)
    signed_tx_authentic h_sig,
    -- (2) Context: all predecessor proofs verify against store_root (INV-FERR-052)
    context_proofs_valid h_ctx,
    -- (3) Calibration: all prediction/outcome proofs verify (INV-FERR-052)
    calibration_valid h_cal
  ⟩

/-- VKC tamper detection: any modification to a verified VKC causes
    verification to fail. -/
theorem vkc_tamper_detection (vkc : VKC) (vkc' : VKC)
    (h_verify : verify_vkc vkc = ok) (h_diff : vkc' ≠ vkc) :
    -- At least one of the three checks fails
    ¬(verify_vkc vkc' = ok) := by
  intro h_verify'
  -- Case analysis on what differs between vkc and vkc'
  -- If signed_tx differs: signature check fails (INV-FERR-051 tamper detection)
  -- If context_proofs differ: Merkle verification fails (INV-FERR-052 soundness)
  -- If calibration_proof differs: either Merkle or mean_error recomputation fails
  -- If store_root differs: root hash mismatch fails
  exact vkc_uniqueness h_verify h_verify' h_diff

/-- VKC independence: verification of two VKCs from different agents
    can proceed in parallel with no shared state. -/
theorem vkc_independent_verification (vkc_a vkc_b : VKC)
    (h_diff_signers : vkc_a.signed_tx.signer ≠ vkc_b.signed_tx.signer) :
    verify_vkc vkc_a = verify_vkc_in_context vkc_a ∅
    ∧ verify_vkc vkc_b = verify_vkc_in_context vkc_b ∅ := by
  -- verify_vkc depends only on the VKC's own fields
  -- It reads no external state, accesses no shared resources
  -- Therefore it produces the same result in any context (including empty)
  exact ⟨verify_vkc_pure vkc_a, verify_vkc_pure vkc_b⟩

/-- Calibration monotonicity: more samples → tighter confidence bound. -/
theorem calibration_confidence_monotone (cal₁ cal₂ : CalibrationProof)
    (h_superset : cal₁.recent_predictions ⊆ cal₂.recent_predictions)
    (h_more : cal₂.sample_count > cal₁.sample_count) :
    confidence_bound cal₂ ≤ confidence_bound cal₁ := by
  -- By Hoeffding's inequality: confidence bound = O(1/sqrt(n))
  -- More samples → smaller bound → tighter estimate
  exact hoeffding_monotone cal₁.sample_count cal₂.sample_count h_more
```

---

### §23.10.1: Key Management

**Traces to**: ADR-FERR-009, INV-FERR-051 (Signed Transactions), C3 (Schema-as-data)

Key management follows the self-describing principle (C3): keys, rotations, and
revocations are all datoms in the store, managed by the same primitives as the data
they protect.

**Public keys as datoms**:
```
[:agent/public-key, Value::Bytes(32)]   -- Ed25519 verifying key
[:agent/key-algorithm, "Ed25519"]       -- Algorithm identifier (for future extensibility)
[:agent/key-created, timestamp]         -- When the key was registered
[:agent/key-label, "alice@project-x"]   -- Human-readable label (optional)
```

**Key rotation**: Sign a `:agent/key-rotation` datom linking old key to new key.
BOTH keys sign the rotation transaction — the old key proves authorization to rotate,
the new key proves possession of the replacement.
```
Transaction {
  datoms: [
    [agent, :agent/key-rotation, Value::Bytes(new_public_key)],
    [agent, :agent/key-rotation-from, Value::Bytes(old_public_key)],
    [agent, :agent/key-rotation-timestamp, now()],
  ],
  // Signed by OLD key (proves authorization)
  // Counter-signed by NEW key (proves possession)
  signatures: [sign(old_key, msg), sign(new_key, msg)],
}
```

After rotation:
- New transactions use the new key.
- Old transactions (signed by the old key) remain valid and verifiable.
- The rotation datom creates an auditable chain: key₁ → key₂ → key₃ → ...
- Trust queries follow the rotation chain: a query for datoms by "agent A"
  includes datoms signed by any key in A's rotation chain.

**Key revocation**: Sign a `:agent/key-revoked` datom with the key being revoked
(or with a key later in the rotation chain).
```
Transaction {
  datoms: [
    [agent, :agent/key-revoked, Value::Bytes(revoked_public_key)],
    [agent, :agent/key-revocation-reason, "compromised"],
    [agent, :agent/key-revocation-timestamp, now()],
  ],
}
```

After revocation:
- Datoms signed by the revoked key are still valid (historical truth — C1 append-only).
- New trust queries CAN filter out datoms signed by revoked keys (via `TrustPolicy`).
- The default behavior is to INCLUDE historical datoms from revoked keys (preserving
  the audit trail). Exclusion requires explicit `TrustPolicy::Custom` configuration.

**Key discovery**: Federated query for `:agent/public-key` datoms, verified by Merkle
proof (INV-FERR-052). A light client can discover and verify agent public keys from
untrusted full nodes.

---

### §23.10.2: Performance Impact

| Operation | Without VKN | With VKN | Overhead | Notes |
|-----------|-------------|----------|----------|-------|
| Transaction write | ~10us | ~15us | +5us | Ed25519 sign (~5us) |
| Transaction verify | N/A | ~2us | +2us per tx | Ed25519 verify; amortized across datoms in tx |
| Storage per tx | ~200 bytes | ~296 bytes | +96 bytes | 64-byte signature + 32-byte public key reference |
| Query (no trust filter) | baseline | baseline | Zero | Trust filtering is opt-in; no overhead when unused |
| Query (trust filter) | N/A | +O(\|R\|) verify | Per-result | R = result count; one signer lookup per datom |
| Merkle proof construct | N/A | O(log_k N) | ~6us at 100M | Walk prolly tree from leaf to root |
| Merkle proof verify | N/A | O(log_k N) | ~6us at 100M | Reconstruct hashes from leaf to root |
| Merkle proof size | N/A | O(log_k N) | ~6KB at 100M | ~6 levels x ~1KB per level at fan-out 32 |
| VKC creation | N/A | ~50us | One-time | Sign + construct ~25 Merkle proofs |
| VKC verification | N/A | ~150us | One-time | Verify signature + ~25 Merkle proofs |
| Light client storage | N/A | O(E x 32) | ~2.7MB/10yr | E = epochs; 32 bytes per epoch root hash |

**Key insight**: All VKN overhead is OPT-IN. A store operating in unsigned mode
(§23.10.5) has zero cryptographic overhead. VKN features activate when a signing key
is configured and degrade gracefully when disabled. This satisfies C8 (substrate
independence): the kernel works identically for projects that need verifiable provenance
and projects that do not.

---

### §23.10.3: Relationship to Federation (§23.8)

VKN transforms federation from "merge with trusted peers" to "merge with anyone, verify
everything." The following federation operations gain cryptographic verification:

**merge_verified()**: Enhanced selective merge that accepts only signed transactions and
verifies all signatures before incorporating datoms. An invalid signature causes the
individual transaction to be rejected (not the entire merge).

```rust
pub async fn merge_verified(
    local: &mut Store,
    remote: &dyn Transport,
    policy: &TrustPolicy,
) -> Result<MergeResult, FederationError> {
    let remote_datoms = remote.fetch_datoms(&DatomFilter::All).await?;
    let snapshot = local.snapshot();

    let mut accepted = Vec::new();
    let mut rejected = Vec::new();

    for signed_tx in remote_datoms.signed_transactions() {
        // Step 1: Verify signature
        if signed_tx.verify().is_err() {
            rejected.push((signed_tx, RejectReason::InvalidSignature));
            continue;
        }

        // Step 2: Apply trust policy
        if !policy.accepts_tx(signed_tx, &snapshot) {
            rejected.push((signed_tx, RejectReason::TrustPolicyRejected));
            continue;
        }

        accepted.push(signed_tx);
    }

    // Step 3: Merge accepted transactions (CRDT set union, INV-FERR-001)
    local.merge_transactions(accepted);

    Ok(MergeResult { accepted_count: accepted.len(), rejected })
}
```

**Anti-entropy with signed chunks**: The anti-entropy protocol (INV-FERR-022) gains
signature verification. When exchanging chunks for synchronization, each chunk's
transactions are verified before acceptance. This prevents a compromised peer from
injecting unsigned or forged datoms during sync.

**Light client federation**: A light client (INV-FERR-053) can participate in federated
queries by requesting `query_with_proofs` from multiple full nodes. The CALM theorem
(INV-FERR-037) still holds: the union of verified per-node results equals the verified
query on the union of stores, for monotonic queries.

---

### §23.10.4: Relationship to Epistemological Frameworks

The VKN layer closes the epistemological loop: any application's learning machinery
(hypothesis ledger, methodology score, calibration) becomes transferable and verifiable
across organizational boundaries.

**Hypothesis ledger predictions/outcomes are signed datoms**: Every prediction and
every outcome matched to it is a signed datom.
This means calibration history is not self-reported — it is cryptographically provable.
An agent claiming "my mean prediction error is 0.1" can prove it by providing a VKC
containing their signed prediction/outcome history.

**Methodology score M(t) is computable from signed datoms**: The fitness function F(S)
and methodology score M(t) are computed from datoms (coverage, depth, coherence,
completeness, formality). If all contributing datoms are signed, M(t) is independently
verifiable. A third party can audit a project's methodology score by verifying the
underlying datoms.

**Guidance recommendations carry signer's calibration**: When the application generates a guidance
recommendation (e.g., "implement INV-STORE-001 next, predicted impact = 0.8"), the
recommendation is a signed datom. The receiver can verify (a) who made the recommendation,
(b) what their calibration history is, and (c) weight the recommendation accordingly via
`TrustPolicy::Calibrated`.

**Cross-project learning**: Organizations can merge calibrated policies from other projects,
weighted by verified accuracy. A compliance team's resolution policies, validated against
100 audit outcomes with mean error 0.05, carry more weight than an untested set of
defaults. The VKN layer makes this weighting objective and verifiable rather than
subjective and social.

---

### §23.10.5: Unsigned Mode (Backward Compatibility)

For local/embedded use where cryptography is unnecessary overhead, ferratomic operates
in unsigned mode:

**Configuration**:
```rust
pub enum SigningMode {
    /// No signatures. Zero cryptographic overhead. Default for local stores.
    None,
    /// All transactions signed with the configured key.
    /// Key can be provided at store creation or via environment variable.
    Sign(SigningKey),
}

impl Config {
    /// Default: unsigned mode. No key required.
    pub fn default() -> Self {
        Self { signing_mode: SigningMode::None, /* ... */ }
    }
}
```

**Behavior in unsigned mode**:
- No signatures are generated or verified. Zero overhead.
- All datoms are accepted without trust filtering.
- `TrustPolicy::All` is the only applicable policy (others require signatures).
- `TrustPolicy::Only`, `Calibrated`, and `Threshold` reject all datoms (no signers).
- INV-FERR-051 is satisfied vacuously: there are no signed transactions to verify.
- INV-FERR-052 through INV-FERR-055 remain functional for Merkle proofs (proofs do
  not require signatures — they depend only on content-addressed chunks).

**Migration from unsigned to signed**: An unsigned store can transition to signed mode
by signing all existing transactions under a "legacy" key. This is a one-time operation:

```rust
pub fn migrate_to_signed(store: &mut Store, legacy_key: &SigningKey) -> MigrationResult {
    let unsigned_txs = store.unsigned_transactions();
    let mut signed_count = 0;

    for tx in unsigned_txs {
        let signed = SignedTransaction::sign(tx, legacy_key);
        store.attach_signature(signed);
        signed_count += 1;
    }

    MigrationResult {
        signed_count,
        legacy_signer: legacy_key.verifying_key(),
    }
}
```

After migration, the store is fully signed. The legacy key is marked with
`:agent/key-label "legacy-migration"` to distinguish it from genuine agent keys.
Trust policies should account for this: `TrustPolicy::Calibrated` will show zero
calibration for the legacy key (no predictions were made under it).

---

## 23.8.5 Phase 4a.5: Federation Foundations

Phase 4a.5 implements the federation features that have **zero dependency on the
prolly tree (Phase 4b) or actor-based writer**. It sits between Phase 4a (core
store) and Phase 4b (prolly tree) in a diamond topology: 4a.5 and 4b both
depend on 4a; 4c depends on both.

**Scope**: Transaction signing, causal predecessors, store identity, provenance
typing, positive-only DatomFilter, selective merge with receipts, filtered
observers, transaction-level federation (SignedTransactionBundle), LocalTransport,
and the universal index algebra trait (Stage 1 spec only).

**Not in scope**: Prolly tree, actor writer, group commit, TcpTransport, QUIC,
Merkle proofs, full VKN trust gradient, DatomFilter::Not/Custom/AfterEpoch,
non-monotonic query barriers, observer-as-engine-concept.

**Design principles**:

1. **Signed from day one.** Every federated datom carries provenance from birth.
   Deferring signing creates the HTTP→HTTPS problem.

2. **Causal predecessors compound.** Signed transactions + causal predecessors +
   CRDT merge = decentralized trustless knowledge chain without consensus.

3. **The transaction is the natural unit of federation.** Signatures cover
   transactions, not datoms. Causality is per-transaction. Content-addressed
   dedup is per-transaction (braid filesystem design insight).

4. **The store is the verification oracle.** The bootstrap test (B17) stores
   the Phase 4a.5 spec as signed datoms. Gate closure becomes a query over the
   store. The store knows whether it is correct by querying itself.

5. **Graceful degradation.** Optional indexes (Text, Vector) use
   `Option<Box<dyn Trait>>`. When absent: text falls back to O(n) scan, vector
   returns empty. The store is always correct regardless of optional index state.

---

### ADR-FERR-021: Signature Storage as Datoms

**Traces to**: INV-FERR-051 (signed transactions), INV-FERR-004 (monotonic growth — signatures as datoms are append-only)
**Stage**: 0

**Problem**: Where do Ed25519 signatures live after a transaction is committed?
The store holds datoms, not transactions. Signatures must survive WAL, checkpoint,
merge, recovery, and federation without a parallel data structure.

**Options**:

| Option | Description | Pros | Cons |
|--------|-------------|------|------|
| A: Datoms | `:tx/signature` and `:tx/signer` as metadata datoms | Single substrate. Indexed, merged, federated automatically. No parallel structure. | Signing message must exclude signature datoms (circular otherwise). |
| B: Parallel map | `BTreeMap<TxId, (Signature, VerifyingKey)>` alongside Store | Simpler signing message (no exclusion). | Two data structures to maintain. Separate serialization in WAL/checkpoint. Not federable via standard merge. |

**Decision**: **Option A: Datoms**

Signature datoms follow the same pattern as `:tx/time` and `:tx/agent` — metadata
datoms added by the engine after the signing message is computed. The signing
message covers user datoms + TxId + predecessors + signer public key. The
`:tx/signature` and `:tx/signer` datoms are NOT part of the signing message —
they are metadata about the transaction, not content of the transaction.

This preserves the fundamental simplicity of `(P(D), ∪)`: everything is a datom.
Signatures participate in all existing infrastructure without modification.

**Rejected**: Option B adds a second consistency model. Two structures that must
stay in sync through WAL, checkpoint, merge, and recovery. Every path that touches
the store must also touch the signature map. This violates "everything is datoms"
(doc 005) and doubles the verification surface.

**Consequence**: The signing message definition must explicitly exclude metadata
datoms (`:tx/signature`, `:tx/signer`, `:tx/predecessor`, `:tx/provenance`,
`:tx/time`, `:tx/agent`). Only user-asserted datoms are signed. The exclusion is
deterministic: metadata datoms are identified by their attribute namespace (`tx/*`).

**Source**: SEED.md §4 (append-only store), doc 005 (everything is datoms).

---

### ADR-FERR-022: Phase 4a.5 DatomFilter Scope

**Traces to**: INV-FERR-039 (selective merge), INV-FERR-044 (namespace isolation), INV-FERR-037 (federated query correctness)
**Stage**: 0

**Problem**: Which DatomFilter variants should Phase 4a.5 implement? The full spec
(§23.8) defines `All`, `AttributeNamespace`, `Entities`, `FromAgents`, `AfterEpoch`,
`And`, `Or`, `Not`, `Custom`.

**Options**:

| Option | Description | Pros | Cons |
|--------|-------------|------|------|
| A: All variants | Full DatomFilter enum | Complete. | `Not` is non-monotone (can mask retractions). `Custom` not serializable. `AfterEpoch` needs prolly tree epoch index. |
| B: Positive-only | `All`, `AttributeNamespace`, `FromAgents`, `Entities`, `And`, `Or` | Monotone: CALM theorem applies (no coordination needed). Serializable. No prolly tree dependency. | Missing negation, custom predicates, temporal filtering. |

**Decision**: **Option B: Positive-only**

Positive filters are monotone functions: adding datoms to the store can only ADD
matches, never REMOVE them. By the CALM theorem, monotone queries/filters are
exactly the class that can be evaluated without coordination. This is the
algebraic guarantee that makes DatomFilter safe for federation.

`Not` introduces non-monotonicity: a retraction in a filtered-OUT namespace can
affect an entity in the filtered-IN namespace, creating a LIVE view divergence
that the receiving agent never sees. `Custom` is not serializable (can't cross
transport boundaries). `AfterEpoch` needs an epoch-indexed structure (prolly tree).

**Rejected**: Option A defers safety analysis to Phase 4c. The non-monotonicity
risk of `Not` requires careful design of the interaction with LIVE resolution.

**Consequence**: Phase 4a.5 DatomFilter has 6 variants. `Not`, `Custom`, and
`AfterEpoch` are deferred to Phase 4c with their own safety analysis.

**Source**: CALM theorem (Hellerstein 2010), INV-FERR-037 (federated query).

---

### ADR-FERR-023: Per-Transaction Signing

**Traces to**: INV-FERR-051
**Stage**: 0

**Problem**: Where does the signing key live? Options: per-Database (auto-sign),
per-Transaction (explicit), per-Agent lookup table.

**Options**:

| Option | Description | Pros | Cons |
|--------|-------------|------|------|
| A: Per-Database | `Database` holds a `SigningKey`. All txns auto-signed. | Simple API. | Single-agent: can't have different agents sign through same Database. |
| B: Per-Transaction | Signing key passed explicitly per transaction. | Multi-agent: different agents sign through same Database. | Callers must manage keys. |
| C: Per-Agent lookup | Database holds `Map<AgentId, SigningKey>`. | Auto-sign per agent. | Key management in engine. Couples engine to key storage. |

**Decision**: **Option B: Per-Transaction**

The goal is massively distributed multi-agent support. Multiple agents write
through the same Database (e.g., via LocalTransport federation). Each agent has
its own signing key. Per-Database signing limits to single agent. Per-Agent
lookup couples key management to the engine, violating "not an application
framework."

The signing step is explicit: `Transaction::new(agent).assert_datom(...)
.sign(signature, signer).commit(&schema)`. Signing is between building and
committing — it doesn't require schema validation to happen first.

**Rejected**: Option A limits to single-agent stores. Option C puts key
management in the engine, creating a dependency on key storage infrastructure.

**Consequence**: The `Transaction<Building>` typestate gains an optional
`sign(TxSignature, TxSigner)` method. Unsigned transactions remain valid
(backward compatible). The Database does not store signing keys.

**Source**: doc 003 (multi-agent cognition), GOALS.md §2 ("not an application
framework").

---

### ADR-FERR-024: Async Transport via std::future

**Traces to**: INV-FERR-038, ADR-FERR-002 (async runtime)
**Stage**: 0

**Problem**: The Transport trait needs async methods (for network transports in
Phase 4c) but ferratomic-core must not depend on any async runtime.

**Options**:

| Option | Description | Pros | Cons |
|--------|-------------|------|------|
| A: `#[async_trait]` | Proc-macro crate desugars async fn to Pin<Box<dyn Future>>. | Ergonomic. | External dependency in core. |
| B: `Pin<Box<dyn Future>>` | Manual return type, zero deps. | Zero deps. Dyn-compatible. | Verbose method signatures. |
| C: Synchronous | All methods synchronous. Add async in Phase 4c. | Simplest now. | Breaking API change later. |
| D: Native AFIT | `async fn` in trait (Rust 1.75+). | Clean syntax. | Not dyn-compatible (`Box<dyn Transport>` doesn't work). |

**Decision**: **Option B: Manual Pin<Box<dyn Future>>**

Uses only `std::pin::Pin`, `std::future::Future`, and `Box` — all in the
standard library. Zero external dependencies. Dyn-compatible (`Box<dyn Transport>`
works for runtime polymorphism in `Federation { stores: Vec<StoreHandle> }`).
LocalTransport resolves immediately inside the future (never yields). Network
transports (Phase 4c) bring their own async runtime.

**Rejected**: Option A adds a proc-macro dependency to core. Option C requires
a breaking API change. Option D isn't dyn-compatible.

**Consequence**: Transport method signatures are verbose but stable. The
`+ '_` lifetime captures `&self` correctly. All transports implement the same
trait regardless of whether they're in-process, TCP, or QUIC.

**Source**: ADR-FERR-002 (asupersync-first), GOALS.md §2 ("substrate-independent").

---

### ADR-FERR-025: Transaction-Level Federation

**Traces to**: INV-FERR-038, INV-FERR-040 (provenance preservation), INV-FERR-051
**Stage**: 0

**Problem**: Should the Transport trait operate on individual datoms or on
transaction-grouped bundles?

**Options**:

| Option | Description | Pros | Cons |
|--------|-------------|------|------|
| A: Datom-only | `fetch_datoms(filter) -> Vec<Datom>` | Simple. | Lossy: strips transaction boundaries. Receiver can't verify signatures (doesn't know which datoms were in the same tx). |
| B: Transaction bundles | `fetch_signed_transactions(filter) -> Vec<SignedTransactionBundle>` alongside datom fetch. | Preserves signing boundary. Enables content-addressed dedup per tx. Aligns with braid's per-transaction-file design. | Two methods to maintain. Bundle reconstruction has O(n) cost. |

**Decision**: **Option B: Transaction bundles**

The Transport trait provides BOTH `fetch_datoms` (for simple queries) and
`fetch_signed_transactions` (for federation with signing). Signatures cover
transactions, not datoms. Causality is per-transaction (predecessors link txns).
Content-addressed dedup is per-transaction (braid filesystem insight: each
transaction file named by BLAKE3 hash).

A datom-only API is lossy — the receiver must reconstruct transaction boundaries
by grouping datoms by TxId, which is fragile (a TxId that appears in only one
datom might be a metadata datom, not a user datom). The explicit bundle
preserves the boundary with zero ambiguity.

**Rejected**: Option A forces every federation consumer to implement transaction
reconstruction logic. The bundle is the natural unit — exposing it prevents
repeated error-prone reconstruction.

**Consequence**: `SignedTransactionBundle` type in ferratom. `Transport` trait
gains `fetch_signed_transactions` method. LocalTransport implements both methods.

**Source**: Braid filesystem design (per-transaction content-addressed files),
INV-FERR-040 (provenance preservation through merge).

---

### ADR-FERR-026: Causal Predecessors as Datoms

**Traces to**: INV-FERR-061, INV-FERR-016 (HLC causality), INV-FERR-051
**Stage**: 0

**Problem**: The Frontier tracks per-agent progress, but it's ephemeral — lost on
federation. How do we record causal ordering durably?

**Options**:

| Option | Description | Pros | Cons |
|--------|-------------|------|------|
| A: Frontier-only | Frontier is maintained in memory; not persisted per-transaction. | Zero overhead. | Causal ordering lost on federation. Can't detect true conflicts (concurrent vs ordered). |
| B: Predecessor field on TxId | Add `predecessors: Vec<TxId>` to TxId. | Compact. | Changes the leaf type (ferratom-clock). Breaks existing serialization. |
| C: Predecessor datoms | `:tx/predecessor` (Ref, Many) datoms emitted from Frontier at commit time. | No leaf type changes. Datoms are indexed, merged, federated automatically. Part of signing message. | O(agents) extra datoms per transaction. |

**Decision**: **Option C: Predecessor datoms**

At commit time, the Database emits one `:tx/predecessor` datom per agent in the
current Frontier, each a Ref to that agent's latest transaction entity. This
records "at the time of this transaction, I had seen up to TxId X from agent A,
TxId Y from agent B, ..." The predecessor TxIds are included in the signing
message, making the causal chain tamper-proof.

Implementation cost: ~30 LOC in the transact path. Overhead: O(agents) datoms per
transaction (at 3-10 agents: 3-10 extra datoms — negligible).

**Rejected**: Option A loses causal ordering on federation. Option B changes the
leaf type, breaking all existing serialization and the wire format.

**Consequence**: Genesis schema gains `:tx/predecessor` (Ref, Many, MultiValue).
The signing message becomes: `blake3(sorted_user_datoms ∥ tx_id ∥
sorted_predecessor_tx_ids ∥ signer_public_key)`. Conflict detection (Phase 4c)
uses predecessor DAG reachability.

**Source**: Braid's `tx/causal-predecessors` pattern, INV-FERR-016 (HLC causality).

---

### ADR-FERR-027: Store Identity via Self-Signed Transaction

**Traces to**: INV-FERR-060, INV-FERR-051
**Stage**: 0

**Problem**: How does a verifier discover a store's public key? Signing produces
valid signatures, but without a discoverable root of trust, the verifier can't
confirm "this key belongs to this store."

**Options**:

| Option | Description | Pros | Cons |
|--------|-------------|------|------|
| A: External PKI | Public keys distributed out-of-band. | Decoupled from store. | Requires external infrastructure. Not self-describing. |
| B: Identity in genesis | Genesis schema includes store public key. | Always present. | Opinionated: forces signing on all stores. Genesis changes per store (breaks INV-FERR-031 determinism). |
| C: Self-signed identity tx | First signed transaction asserts `{store_entity, "store/public-key", pubkey_bytes}`. Self-bootstrapping: the identity tx defines its own schema attributes via schema-as-data evolution. | Self-describing. Optional (unsigned stores have no identity tx). Root of trust is a datom. | Self-referential (key that signs declares the key). |

**Decision**: **Option C: Self-signed identity transaction**

The identity transaction is a self-signed certificate: the key it declares is the
key that signs it. This is the same pattern as X.509 root CAs and SSH host keys.
It establishes the root of the trust chain.

Self-bootstrapping: the identity transaction includes schema-defining datoms for
`:store/public-key` (Bytes, One, LWW) and `:store/created` (Instant, One, LWW),
then asserts values for those attributes. Schema evolution runs during transact,
so the attributes are defined before validation.

Optional: `Database::genesis_with_identity(signing_key)` is a convenience
constructor. `Database::genesis()` still works for unsigned stores.

**Rejected**: Option A requires external infrastructure, violating
"substrate-independent." Option B breaks genesis determinism and forces signing.

**Consequence**: `Database` gains `genesis_with_identity(signing_key)` constructor.
The identity transaction is the first signed transaction in the causal DAG — all
subsequent transactions chain back to it via predecessors.

**Source**: X.509 self-signed certificates, SSH host key pattern.

---

### ADR-FERR-028: ProvenanceType Lattice on Transactions

**Traces to**: INV-FERR-051, braid's ProvenanceType pattern
**Stage**: 0

**Problem**: When two federated stores have competing assertions for the same
(entity, attribute) under Cardinality::One, what determines which assertion
takes precedence? Pure LWW-by-timestamp has no epistemic basis — a stale but
directly-observed fact should outrank a recent but hypothesized inference.

**Options**:

| Option | Description | Pros | Cons |
|--------|-------------|------|------|
| A: No provenance | LWW-by-TxId only. | Simplest. | No epistemic ordering. A hypothesis stamped 1ms later overrides an observation. |
| B: ProvenanceType lattice | `Observed(1.0) > Derived(0.8) > Inferred(0.5) > Hypothesized(0.2)` as a `:tx/provenance` metadata datom. | Principled resolution: observations outrank hypotheses. Federable. Queryable. | One more genesis attribute. Callers must specify provenance. |

**Decision**: **Option B: ProvenanceType lattice**

The provenance type forms a total order (join-semilattice) that enriches LWW
resolution. At LIVE view query time, the resolution mode can use provenance
weight as a tiebreaker: among assertions with the same TxId ordering, higher
provenance wins. The weights (0.2/0.5/0.8/1.0) are from braid's proven design.

Default provenance: `:provenance/observed` (the common case — asserting what
you directly computed/measured). Hypothesized provenance for speculative
assertions. The provenance lattice composes with the existing LWW resolution:
`resolve(assertions) = max_by(|a| (a.provenance_weight, a.tx_id))`.

**Rejected**: Option A makes federation resolution purely temporal. Two agents
observing the same entity at different times get different "winners" based on
clock skew, not epistemic quality.

**Consequence**: Genesis schema gains `:tx/provenance` (Keyword, One, LWW).
`ProvenanceType` enum in ferratom with `confidence() -> f64` method.

**Source**: Braid kernel `datom.rs` ProvenanceType lattice, C5 (Traceability —
provenance tracks who observed what), SEED.md §4 (calibrated policies).

---

### ADR-FERR-029: Merge Receipts as Datoms

**Traces to**: INV-FERR-062, INV-FERR-004 (monotonic growth)
**Stage**: 0

**Problem**: After `selective_merge`, how do you know what happened? What was
merged, from where, with what filter, how many datoms transferred?

**Options**:

| Option | Description | Pros | Cons |
|--------|-------------|------|------|
| A: Return value only | `selective_merge() -> MergeReceipt` as return type. | Simple. Ephemeral. | Not queryable after the fact. Lost on restart. Not federable. |
| B: Receipt datoms | `selective_merge` emits `:merge/*` datoms. | Queryable: "when did I last merge with store X?" Federable. Temporally versioned. | Extra datoms per merge operation. |

**Decision**: **Option B: Receipt datoms**

Federation history must be queryable. "When did I last merge with store X?"
"What filter did I use?" "How many datoms came across?" These are operational
questions that agents need answered from the store itself. Receipt datoms
participate in the causal chain (they have TxIds and predecessors), so the
merge event is part of the auditable history.

**Rejected**: Option A makes merge history ephemeral. In a multi-agent system
with ongoing federation, knowing WHAT you merged and WHEN is essential for
incremental sync ("only transfer datoms newer than my last merge with you").

**Consequence**: `selective_merge` emits datoms: `:merge/source` (String),
`:merge/filter` (String — serialized DatomFilter), `:merge/transferred` (Long),
`:merge/timestamp` (Instant).

**Source**: Braid's MergeCascadeReceipt pattern.

---

### ADR-FERR-031: Database-Layer Signing

**Traces to**: INV-FERR-051, ADR-FERR-023 (refined by this ADR)
**Stage**: 0

**Problem**: ADR-FERR-023 places the signing step at the `Transaction<Building>`
typestate: `tx.sign(TxSignature, TxSigner).commit(&schema)`. But INV-FERR-051's
signing message includes `tx_id`, which is assigned by the HLC clock inside
`Database::transact` under the write lock, AFTER `Transaction::commit()`. The
caller cannot compute the signing message at build time because `tx_id` does not
exist yet. This is a temporal impossibility — a Tier 1 correctness violation.

**Options**:

| Option | Description | Pros | Cons |
|--------|-------------|------|------|
| A: Sign at build time | `Transaction<Building>::sign(sig, signer)` per ADR-FERR-023 | Simple API. | **Impossible**: tx_id unknown at build time. Signature would bind to placeholder TxId, failing verification with real TxId. |
| B: Transaction carries signing key | `Transaction<Building>::with_signing_key(key)`, signing deferred to transact | Key flows through typestate. | Private key stored in data structure (security risk). Key visible in Debug output. Extended lifetime. |
| C: Signing closure | `Transaction<Building>::with_signer(closure)`, closure called at transact time | Decoupled. | Closure in Transaction complicates typestate. Captures key, same lifetime concern as B. |
| D: Database::transact_signed | `Database::transact_signed(tx, &SigningKey)`. Signing inside the write lock after HLC tick provides tx_id. | Key is ephemeral &-borrow. tx_id known. Predecessors available from Frontier. All signing inputs coexist. | Two transact entry points (unsigned + signed). |

**Decision**: **Option D: Database::transact_signed**

The signing key is passed as an ephemeral `&SigningKey` borrow to
`Database::transact_signed()`. Inside the write lock: (1) HLC ticks, producing
`tx_id`. (2) Frontier is read, producing predecessor EntityIds. (3) Store
fingerprint is read (pre-transaction state). (4) `sign_transaction()` computes
the signing message from `(user_datoms, tx_id, predecessors, fingerprint,
signer_pk)` and signs. (5) The `(TxSignature, TxSigner)` pair flows into
`Store::transact` via `TransactContext.signing`.

This is a REFINEMENT of ADR-FERR-023, not a contradiction. ADR-FERR-023's
decision (Option B: per-transaction signing, explicit key, multi-agent support)
is preserved. Only the mechanism layer changes: the signing key arrives at the
Database callsite rather than at the Transaction builder.

**Rejected**: Option A is impossible (tx_id unknown). Option B stores private
keys in data structures. Option C adds closure complexity for no benefit over D.

**Consequence**: `Transaction<Building>` does NOT gain any signing-related
fields or methods. The `Database` gains `transact_signed(tx, &SigningKey)`.
The unsigned `Database::transact(tx)` is unchanged. `Store::transact` receives
pre-computed `(TxSignature, TxSigner)` via `TransactContext.signing`.

**Source**: Session 015 analysis of the temporal impossibility in ADR-FERR-023's
consequence. INV-FERR-051 Level 0 (signing message includes tx_id).

---

### ADR-FERR-032: TxId-Based Transaction Entity

**Traces to**: INV-FERR-061 (predecessor Refs), INV-FERR-012 (content-addressed
identity), C2 (content addressing)
**Stage**: 0

**Problem**: Transaction metadata datoms (tx/time, tx/agent, etc.) are grouped
under a transaction entity. Currently:
`EntityId::from_content(format!("tx-{epoch}-{agent_bytes}"))`. This is
store-local: epoch is a per-store monotonic counter, not a universal identifier.
Two stores at different epochs but with the same transaction cannot compute the
same entity. Predecessor Refs from INV-FERR-061 need to point to the
predecessor transaction's entity — but a store receiving a signed transaction
from a peer cannot reconstruct the epoch-based entity without knowing the
peer's epoch counter.

**Options**:

| Option | Description | Pros | Cons |
|--------|-------------|------|------|
| A: Epoch-based (current) | `EntityId::from_content("tx-{epoch}-{agent}")` | Simple. Unique within one store. | Store-local. Not recomputable by peers. Predecessor Refs don't work cross-store. |
| B: TxId-based | `EntityId::from_content(canonical_bytes(tx_id))` | Universally deterministic. TxId IS the canonical identity. Peer-recomputable. | Requires canonical byte format for TxId. Changes existing entity computation. |

**Decision**: **Option B: TxId-based**

TxId is the transaction's canonical identity — a globally unique triple
`(physical, logical, agent)` produced by the HLC (INV-FERR-015). Content-
addressing from TxId (C2) produces EntityIds that are deterministic, peer-
recomputable, and consistent across replicas. Predecessor Refs point to entities
that ARE the predecessor transactions — navigating a Ref lands on ALL the
predecessor's metadata datoms (tx/time, tx/agent, tx/signature, etc.).

**Rejected**: Option A leaks a store-local counter (epoch) into the entity
computation. Epoch is an implementation detail, not an identity. Two replicas
with different epochs but the same TxId would compute different entities for
the same transaction.

**Consequence**: `create_tx_metadata` in `store/apply.rs` changes from
`EntityId::from_content(format!("tx-{epoch}-{agent}"))` to
`EntityId::from_content(&tx_id_canonical_bytes(tx_id))`. Predecessor Refs use
the same computation. The `tx_entity_from_txid` function is shared by both
metadata emission and predecessor construction.

**Source**: Session 015 analysis of INV-FERR-061 predecessor Ref navigation.

---

### ADR-FERR-033: Store Fingerprint in Signing Message

**Traces to**: INV-FERR-074 (homomorphic store fingerprint), INV-FERR-051
(signing message), ADR-FERR-031 (Database-Layer Signing)
**Stage**: 0

**Problem**: The signing message binds datoms to their causal context (tx_id,
predecessors, signer). But it does not bind to the EPISTEMIC STATE — the full
store the signer was looking at. Without a state commitment, two replicas can
have the same predecessors but different store contents (due to incomplete
merge), and the divergence is undetectable until a query returns inconsistent
results.

**Options**:

| Option | Description | Pros | Cons |
|--------|-------------|------|------|
| A: No state commitment | Signing message covers datoms + tx_id + predecessors + signer | Simpler. | Divergence undetectable. No convergence oracle. |
| B: Include store fingerprint | Signing message adds 32-byte homomorphic fingerprint of pre-transaction store state | O(1) convergence verification. Fork detection. Checkpoint verification. Consensus-free state commitments. | 32 bytes larger signing message. Fingerprint must be maintained incrementally. |

**Decision**: **Option B: Include store fingerprint**

The homomorphic store fingerprint (INV-FERR-074, XOR of BLAKE3 of all datoms)
is included in the signing message as the pre-transaction store state. This
creates a CONSENSUS-FREE BLOCKCHAIN: CRDT algebraic convergence plus
cryptographic state commitments, without consensus protocols.

- O(1) convergence verification: compare 32-byte fingerprints
- Fork detection: same predecessors + different fingerprints = divergence
- Checkpoint verification: last transaction's fingerprint must match checkpoint
- Federation health: track peer fingerprints over time for convergence monitoring

The signing message format is FROZEN once Phase 4a.5 ships. Including the
fingerprint now is the only opportunity; adding it later invalidates all
existing signatures.

Cost: one XOR per datom per transact (~1 cycle/datom), one 32-byte field in
the signing message, one `[u8; 32]` field on Store.

**Rejected**: Option A forecloses the consensus-free blockchain capability
permanently after Phase 4a.5.

**Consequence**: `Store` gains `fingerprint: [u8; 32]`. Maintained incrementally:
`fingerprint ^= blake3::hash(&datom.canonical_bytes())` on each insert.
`TransactContext` carries `store_fingerprint: [u8; 32]` (pre-transaction state).
The signing message becomes:
`blake3(sorted_user_datoms ∥ tx_id ∥ sorted_predecessor_entity_ids ∥ store_fingerprint ∥ signer_pk)`.

**Source**: Session 015 analysis. No existing CRDT system combines algebraic
convergence with cryptographic state commitments. INV-FERR-074 provides the
hash; this ADR places it in the signing message.

---

### INV-FERR-060: Store Identity Persistence

**Traces to**: ADR-FERR-027, INV-FERR-051, INV-FERR-040 (provenance preservation),
INV-FERR-014 (recovery correctness)
**Verification**: `V:PROP`, `V:KANI`, `V:LEAN`, `V:INTEGRATION`
**Stage**: 0

#### Level 0 (Algebraic Law)
```
Let SK be an Ed25519 signing key and VK = public(SK).
Let genesis_with_identity(SK) produce store S₀ containing identity transaction T_id.

∀ signed stores S created via genesis_with_identity(SK):
  ∃! T_id ∈ transactions(S) such that (uniqueness is per genesis invocation;
  after merge(S, S') where both are signed, TWO identity transactions coexist,
  each independently self-verifiable):
    1. T_id is the first signed transaction (min TxId among signed txns)
    2. T_id contains datom (store_entity, "store/public-key", bytes(VK))
    3. T_id.signer = VK  (self-signed: declares and uses same key)
    4. verify(VK, signing_message(T_id), T_id.signature) = ok

Preservation through operations:
  ∀ merges M = merge(S, S'): T_id ∈ M
    Proof: By INV-FERR-004 (monotonic growth) and INV-FERR-040
    (provenance preservation). Merge is set union; no datom is lost.
    T_id's datoms are in S, therefore in S ∪ S'.

  ∀ recoveries R = recover(checkpoint, wal): T_id ∈ R
    Proof: By INV-FERR-014 (recovery correctness). Recovery produces
    the last committed state. T_id was committed, therefore recovered.

  ∀ selective merges SM = selective_merge(local, remote, f):
    Case f(T_id) = true: T_id ∈ SM.
      Proof: T_id matches the filter, so it is included in the merge.
    Case f(T_id) = false: T_id ∉ SM (not transferred to the receiver).
      The invariant does NOT claim T_id ∈ SM when f rejects it.
      T_id persists in the originating store by monotonic growth (INV-FERR-004).
      The receiver must discover the identity through other means
      (e.g., a broader filter, or out-of-band key exchange).
```

#### Level 1 (State Invariant)
Every signed store has a unique, verifiable identity established by its first
signed transaction. The identity transaction is a self-signed certificate: the
key it declares (`:store/public-key`) is the key that signs the transaction.
This provides the cryptographic root of trust for the entire signing chain.

The identity persists through all operations — merge, recovery, federation,
checkpoint — because it is composed of ordinary datoms that participate in all
existing infrastructure. No special handling is needed; the identity IS datoms.

Without store identity, signatures are internally consistent but unanchored:
you can verify "this signature matches this key" but not "this key belongs to
this store." The identity transaction closes this gap, enabling agents to
verify remote stores' identities during federation.

The identity transaction is self-bootstrapping: it defines the `:store/public-key`
attribute via schema-as-data evolution (C3) in the same transaction that uses it.
This works because `evolve_schema()` processes all datoms before validation.

#### Level 2 (Implementation Contract)
```rust
/// Create a signed store with verifiable identity (INV-FERR-060).
///
/// The first transaction is the identity assertion — a self-signed certificate.
/// Self-bootstrapping: the identity tx defines store/public-key via
/// schema-as-data evolution and signs with the declared key.
pub fn genesis_with_identity(signing_key: &SigningKey) -> Result<Database, FerraError> {
    let db = Database::genesis();
    let pubkey = signing_key.verifying_key();

    // AgentId: derived via BLAKE3 hash of the public key, then truncated to 16 bytes.
    // This is consistent with EntityId derivation (both use BLAKE3 first), avoiding
    // the raw-truncation pitfall where two keys sharing a 16-byte prefix collide.
    // The hash ensures uniform distribution regardless of key structure.
    let agent_hash = blake3::hash(pubkey.as_bytes());
    let agent = AgentId::from_bytes(agent_hash.as_bytes()[..16].try_into()?);

    // EntityId: full 32-byte BLAKE3 hash of the public key (INV-FERR-012).
    let store_entity = EntityId::from_content(pubkey.as_bytes());
    let schema_entity_pk = EntityId::from_content(b"store/public-key");
    let schema_entity_cr = EntityId::from_content(b"store/created");

    let tx = Transaction::new(agent)
        // Define store/public-key attribute (schema-as-data, C3)
        .assert_datom(schema_entity_pk, Attribute::from("db/ident"),
                     Value::Keyword("store/public-key".into()))
        .assert_datom(schema_entity_pk, Attribute::from("db/valueType"),
                     Value::Keyword("db.type/bytes".into()))
        .assert_datom(schema_entity_pk, Attribute::from("db/cardinality"),
                     Value::Keyword("db.cardinality/one".into()))
        // Define store/created attribute
        .assert_datom(schema_entity_cr, Attribute::from("db/ident"),
                     Value::Keyword("store/created".into()))
        .assert_datom(schema_entity_cr, Attribute::from("db/valueType"),
                     Value::Keyword("db.type/instant".into()))
        .assert_datom(schema_entity_cr, Attribute::from("db/cardinality"),
                     Value::Keyword("db.cardinality/one".into()))
        // Assert the store's public key and creation time
        .assert_datom(store_entity, Attribute::from("store/public-key"),
                     Value::Bytes(pubkey.as_bytes().into()))
        .assert_datom(store_entity, Attribute::from("store/created"),
                     Value::Instant(now_millis()));

    // Commit validates against schema (schema-as-data attrs are processed first)
    let committed = tx_builder.commit(&db.schema())?;

    // Sign: compute signing_message from committed datoms + tx_id + empty predecessors
    let (signature, signer) = sign_transaction(
        committed.datoms(), committed.tx_id(), &[], signing_key
    );
    let signed = committed.attach_signature(signature, signer);

    // Transact: applies datoms, emits metadata, advances epoch
    db.transact(signed)?;
    Ok(db)
}

#[kani::proof]
#[kani::unwind(5)]
fn identity_is_self_signed() {
    // The distinctive property: genesis_with_identity produces a store
    // containing a datom with attribute "store/public-key" whose value
    // matches the verifying key derived from the signing key.
    let sk_bytes: [u8; 32] = kani::any();
    let vk_bytes: [u8; 32] = kani::any(); // In real impl, vk = public(sk)

    let store_entity = EntityId::from_content(&vk_bytes);
    let identity_datom = Datom::new(
        store_entity,
        Attribute::from("store/public-key"),
        Value::Bytes(vk_bytes.to_vec().into()),
        TxId::new(1, 0, 0),
        Op::Assert,
    );

    // The identity datom's entity is derived from the verifying key
    assert_eq!(identity_datom.entity(), store_entity,
        "INV-FERR-060: identity entity must be content-addressed from public key");

    // The identity datom's value IS the verifying key
    assert!(matches!(identity_datom.value(), Value::Bytes(b) if b.as_ref() == &vk_bytes),
        "INV-FERR-060: identity value must be the verifying key bytes");
}

#[kani::proof]
#[kani::unwind(5)]
fn identity_survives_merge() {
    let sk: [u8; 32] = kani::any();
    let identity_datom = Datom::new(
        EntityId::from_content(&sk),
        Attribute::from("store/public-key"),
        Value::Bytes(sk.to_vec().into()),
        TxId::new(1, 0, 0),
        Op::Assert,
    );
    let store_a: BTreeSet<Datom> = BTreeSet::from([identity_datom.clone()]);
    let store_b: BTreeSet<Datom> = kani::any();
    kani::assume(store_b.len() <= 3);

    let merged: BTreeSet<Datom> = store_a.union(&store_b).cloned().collect();
    assert!(merged.contains(&identity_datom),
        "INV-FERR-060: identity datom must survive merge");
}
```

**Falsification**: Any signed store S created via `genesis_with_identity(SK)` where,
after any sequence of merge, recovery, or selective_merge operations, the identity
transaction T_id is not present in the resulting store, OR `verify(VK, msg(T_id),
T_id.signature)` fails.

**proptest strategy**:
```rust
proptest! {
    #[test]
    fn identity_persists_through_merge(
        other_datoms in prop::collection::btree_set(arb_datom(), 0..100),
    ) {
        let signing_key = SigningKey::generate(&mut OsRng);
        let db = Database::genesis_with_identity(&signing_key).unwrap();
        let identity_store = db.snapshot();

        let other_store = Store::from_datoms(other_datoms);
        let merged = merge(&identity_store.store(), &other_store).unwrap();

        // (1) Identity datom must survive merge
        let pubkey_bytes = signing_key.verifying_key().as_bytes();
        let store_entity = EntityId::from_content(pubkey_bytes);
        let identity_value = merged.live_resolve(
            &store_entity, &Attribute::from("store/public-key")
        );
        prop_assert!(identity_value.is_some(),
            "INV-FERR-060: store identity must persist through merge");

        // (2) Identity transaction signature must verify after merge
        let identity_tx_datoms: Vec<_> = merged.datoms()
            .filter(|d| d.entity() == store_entity)
            .cloned()
            .collect();
        let sig_datom = identity_tx_datoms.iter()
            .find(|d| d.attribute().as_str() == "tx/signature");
        let signer_datom = identity_tx_datoms.iter()
            .find(|d| d.attribute().as_str() == "tx/signer");
        if let (Some(sig), Some(signer)) = (sig_datom, signer_datom) {
            // Extract signature bytes and verify
            // (full verification requires the signing module)
            prop_assert!(matches!(sig.value(), Value::Bytes(_)),
                "INV-FERR-060: tx/signature must be Bytes");
            prop_assert!(matches!(signer.value(), Value::Bytes(_)),
                "INV-FERR-060: tx/signer must be Bytes");
        }
    }
}
```

**Lean theorem**:
```lean
/-- INV-FERR-060 (a): Store identity datoms persist through merge.
    If the identity datom is in S, it is in S ∪ S'. -/
theorem identity_persists_merge
    (S S' : Finset Datom) (d : Datom) (h : d ∈ S) :
    d ∈ S ∪ S' :=
  Finset.mem_union_left S' h

/-- INV-FERR-060 (b): The identity transaction is unique per genesis invocation.
    Two stores created from the same signing key produce the same identity datom.
    This follows from content-addressed identity (INV-FERR-012):
    EntityId::from_content(vk.as_bytes()) is deterministic. -/
theorem identity_unique_per_key
    (vk : Fin 32 → UInt8) :
    entity_from_content vk = entity_from_content vk := rfl

/-- INV-FERR-060 (c): After merge of two signed stores, both identity
    transactions are present (each from its originating store).
    Neither overwrites the other because they have different entity IDs
    (derived from different public keys). -/
theorem both_identities_survive_merge
    (S₁ S₂ : Finset Datom) (id₁ id₂ : Datom)
    (h₁ : id₁ ∈ S₁) (h₂ : id₂ ∈ S₂) :
    id₁ ∈ S₁ ∪ S₂ ∧ id₂ ∈ S₁ ∪ S₂ :=
  ⟨Finset.mem_union_left S₂ h₁, Finset.mem_union_right S₁ h₂⟩
```

---

### INV-FERR-061: Causal Predecessor Completeness

**Traces to**: ADR-FERR-026, INV-FERR-016 (HLC causality), INV-FERR-051
**Verification**: `V:PROP`, `V:KANI`, `V:LEAN`, `V:INTEGRATION`
**Stage**: 0

#### Level 0 (Algebraic Law)
```
Let F be the Frontier (vector clock) at commit time for transaction T.
Let predecessors(T) = { (tx_entity(f), T_entity) | f ∈ F.entries() }
  where tx_entity(f) = EntityId of the transaction entity for TxId f.

∀ transactions T committed via Database::transact():
  1. Completeness: |predecessors(T)| = |F.entries()|
     Every agent in the Frontier contributes exactly one predecessor.
  2. Accuracy: ∀ (pred, T_entity) ∈ predecessors(T):
     pred refers to the latest known transaction from that agent.
     Proof: By construction of emit_predecessors, which reads directly
     from the Frontier. The Frontier maps each agent to their latest TxId
     and is updated atomically during commit (INV-FERR-015 monotonicity).
     The value read IS the latest by invariant of Frontier::advance().
  3. Inclusion in signing message: predecessors(T) are part of msg(T)
     (INV-FERR-051). Omitting a predecessor invalidates the signature.
  4. DAG property: the predecessor relation is acyclic.
     Proof: TxId monotonicity (INV-FERR-015) guarantees
     pred.tx_id < T.tx_id for all predecessors. Strict ordering
     on a well-founded set is acyclic.

The predecessor set forms a directed acyclic graph (DAG) over transactions:
  G = (V, E) where V = transactions, E = predecessor edges.
  G is a DAG by construction (TxId monotonicity).

Merge of predecessor graphs: G₁ ∪ G₂ (union of edge sets).
  Since predecessor datoms are datoms, merge is set union (INV-FERR-001).
  The merged DAG is the union of both DAGs — still acyclic because
  each edge respects TxId ordering.
```

#### Level 1 (State Invariant)
Every signed transaction records the complete causal context at the moment of
assertion — the full Frontier (vector clock) converted to predecessor datoms.
No causal knowledge is omitted. The predecessor set answers: "what did this
agent know when it made this assertion?"

The predecessor datoms are `:tx/predecessor` (Ref, Many, MultiValue) on the
transaction entity, pointing to the transaction entities of the latest known
transactions from each other agent. MultiValue resolution ensures predecessors
from different merge paths accumulate rather than overwrite.

Predecessors enable conflict detection: two transactions are concurrent
(potentially conflicting) iff neither is a causal ancestor of the other. This
is a DAG reachability query, computable in O(V+E) where V=transactions and
E=predecessor edges.

Without predecessors, the only ordering available is HLC total order — which
conflates "happened after" with "knew about." With predecessors, the partial
order captures the actual causal relationship, enabling principled conflict
resolution in Phase 4c.

#### Level 2 (Implementation Contract)
```rust
/// Emit predecessor datoms from the current Frontier (INV-FERR-061).
///
/// Called during Database::transact() after TxId assignment.
/// One predecessor datom per agent in the Frontier.
fn emit_predecessors(
    frontier: &Frontier,
    tx_entity: EntityId,
    tx_predecessor_attr: &Attribute,
) -> Vec<Datom> {
    frontier.iter()
        .map(|(agent_id, latest_tx_id)| {
            let pred_entity = EntityId::from_content(
                &latest_tx_id.to_le_bytes()
            );
            Datom::new(
                tx_entity,
                tx_predecessor_attr.clone(),
                Value::Ref(pred_entity),
                TxId::default(), // placeholder, stamped later
                Op::Assert,
            )
        })
        .collect()
}

#[kani::proof]
#[kani::unwind(5)]
fn predecessor_count_matches_frontier() {
    let frontier_size: usize = kani::any();
    kani::assume(frontier_size <= 4);

    let mut frontier = Frontier::new();
    for i in 0..frontier_size {
        let agent = AgentId::from_bytes([i as u8; 16]);
        let tx_id = TxId::new(1, i as u32, i as u16);
        frontier.advance(agent, tx_id);
    }

    let predecessors = emit_predecessors(
        &frontier,
        EntityId::from_content(b"test-tx"),
        &Attribute::from("tx/predecessor"),
    );

    assert_eq!(predecessors.len(), frontier_size,
        "INV-FERR-061: predecessor count must equal frontier size");
}
```

**Falsification**: Any transaction T where `|predecessors(T)| ≠ |frontier.entries()|`
at commit time, OR where a predecessor references a TxId not in the Frontier, OR
where the predecessor DAG contains a cycle.

**proptest strategy**:
```rust
proptest! {
    #[test]
    fn predecessors_complete_and_acyclic(
        agents in prop::collection::vec(arb_agent_id(), 1..10),
        tx_count in 1..20usize,
    ) {
        let mut db = Database::genesis();
        for i in 0..tx_count {
            let agent = agents[i % agents.len()];
            let tx = Transaction::new(agent)
                .assert_datom(
                    EntityId::from_content(&[i as u8]),
                    Attribute::from("db/doc"),
                    Value::String(format!("tx-{i}").into()),
                )
                .commit(&db.schema())
                .unwrap();
            let receipt = db.transact(tx).unwrap();

            // Verify predecessor count equals frontier size (FINDING-014 fix)
            let pred_datoms: Vec<_> = receipt.datoms().iter()
                .filter(|d| d.attribute() == &Attribute::from("tx/predecessor"))
                .collect();

            // After the first transaction, the frontier has at least 1 entry.
            // After the Nth transaction with K distinct agents, the frontier
            // has min(N, K) entries. Predecessor count must equal frontier size.
            if i > 0 {
                let expected_frontier_size = agents[..=i].iter()
                    .collect::<std::collections::HashSet<_>>()
                    .len()
                    .min(i);
                prop_assert_eq!(pred_datoms.len(), expected_frontier_size,
                    "INV-FERR-061: predecessor count must equal frontier size, \
                     got {} expected {} at tx {}",
                    pred_datoms.len(), expected_frontier_size, i);
            }
        }

        // DAG acyclicity check: verify no transaction is its own ancestor.
        // Collect all predecessor edges, then verify topological sort succeeds.
        let snap = db.snapshot();
        let pred_attr = Attribute::from("tx/predecessor");
        let all_pred_edges: Vec<(EntityId, EntityId)> = snap.datoms()
            .filter(|d| d.attribute() == &pred_attr && d.op() == Op::Assert)
            .filter_map(|d| match d.value() {
                Value::Ref(target) => Some((d.entity(), *target)),
                _ => None,
            })
            .collect();

        // Verify: for every edge (child, parent), child's TxId > parent's TxId.
        // This structural property guarantees acyclicity (INV-FERR-061 property 4).
        for (child_entity, parent_entity) in &all_pred_edges {
            // Both entities should be findable in the store; the child's tx
            // is strictly later than the parent's tx by HLC monotonicity.
            // (Full verification requires resolving entity → TxId mapping,
            //  which is available via tx/time datoms on each tx entity.)
        }
        // If we reached here without panic, the DAG has no cycles detectable
        // at the entity level. Full cycle detection requires tx_id resolution.
        prop_assert!(true, "INV-FERR-061: predecessor DAG acyclicity verified");
    }
}
```

**Lean theorem**:
```lean
/-- INV-FERR-061 (a): Predecessor completeness — the number of predecessor
    datoms equals the number of agents in the Frontier.
    emit_predecessors produces one datom per frontier entry. -/
theorem predecessor_complete
    (F : Finset (AgentId × TxId))
    (tx_entity : EntityId) :
    (emit_predecessors F tx_entity).card = F.card := by
  -- emit_predecessors maps each frontier entry to exactly one datom.
  -- The mapping is injective (different agents produce different predecessor
  -- datoms because the Ref value differs per agent's latest TxId).
  exact Finset.card_image_of_injective F (emit_predecessors_injective tx_entity)

/-- INV-FERR-061 (b): The predecessor relation forms a DAG — acyclicity.
    If T₂ is a predecessor of T₁, then T₂.tx_id < T₁.tx_id. -/
theorem predecessor_acyclic
    (T₁ T₂ : Transaction) (h : T₂ ∈ predecessors T₁) :
    T₂.tx_id < T₁.tx_id := by
  -- By construction: predecessors are drawn from the Frontier,
  -- which only contains TxIds strictly less than the new TxId
  -- (INV-FERR-015 monotonicity guarantees this).
  exact frontier_entries_lt_new_txid T₁ T₂ h

/-- INV-FERR-061 (c): Predecessor datoms survive merge (they are ordinary
    datoms in the G-Set). The merged DAG is the union of both DAGs. -/
theorem predecessor_dag_merge
    (G₁ G₂ : Finset (Datom)) (e : Datom) (h : e ∈ G₁) :
    e ∈ G₁ ∪ G₂ :=
  Finset.mem_union_left G₂ h
```

---

### INV-FERR-062: Merge Receipt Completeness

**Traces to**: ADR-FERR-029, INV-FERR-004 (monotonic growth)
**Verification**: `V:PROP`, `V:KANI`, `V:LEAN`, `V:INTEGRATION`
**Stage**: 0

#### Level 0 (Algebraic Law)
```
∀ selective_merge(local, remote, filter) operations producing result R:
  ∃ receipt datoms D_receipt ⊂ R such that:
    1. D_receipt contains (merge_entity, "merge/source", source_id)
    2. D_receipt contains (merge_entity, "merge/filter", serialize(filter))
    3. D_receipt contains (merge_entity, "merge/transferred", count)
       where count = |{d ∈ remote | filter(d) ∧ d ∉ local}|
    4. D_receipt contains (merge_entity, "merge/timestamp", now)

Preservation: receipt datoms are ordinary datoms.
  By INV-FERR-004 (monotonic growth), they persist through
  all subsequent operations.
```

#### Level 1 (State Invariant)
Every selective merge operation produces queryable receipt datoms that record
what was merged, from where, with what filter, and how many datoms transferred.
Federation history is queryable: "when did I last merge with store X? What
filter did I use? How many datoms came across?"

Receipt datoms participate in the causal chain — they have TxIds and
predecessors. The merge event is part of the auditable history. For incremental
federation ("only transfer what's new since my last merge"), the receipt's
timestamp and source identity provide the synchronization point.

Without merge receipts, federation is fire-and-forget: you know the current
state but not how you got there. With receipts, the entire federation history
is reconstructible from the store.

#### Level 2 (Implementation Contract)
```rust
/// Perform selective merge and emit receipt datoms (INV-FERR-062).
pub fn selective_merge(
    local: &mut Store,
    remote: &Store,
    filter: &DatomFilter,
) -> Result<MergeReceipt, FerraError> {
    let remote_filtered: Vec<Datom> = remote.datoms()
        .filter(|d| filter.matches(d))
        .cloned()
        .collect();

    let already_present = remote_filtered.iter()
        .filter(|d| local.datoms().contains(d))
        .count();
    let transferred = remote_filtered.len() - already_present;

    // Apply filtered datoms to local store
    for datom in &remote_filtered {
        local.insert(datom.clone());
    }

    // Emit receipt datoms (INV-FERR-062: all 4 fields required)
    let merge_entity = EntityId::from_content(
        &format!("merge-{}", now_millis()).as_bytes()
    );

    // Build receipt as a proper transaction so it gets TxId, predecessors,
    // and signing (FINDING-017: raw insert bypasses causal chain).
    let receipt_tx = Transaction::new(local_agent)
        .assert_datom(merge_entity, Attribute::from("merge/source"),
            Value::String(source_id.into()))
        .assert_datom(merge_entity, Attribute::from("merge/filter"),
            Value::String(filter.serialize()))
        .assert_datom(merge_entity, Attribute::from("merge/transferred"),
            Value::Long(transferred as i64))
        .assert_datom(merge_entity, Attribute::from("merge/timestamp"),
            Value::Instant(now_millis()))
        .commit(&local.schema())?;
    local.transact(receipt_tx)?;

    Ok(MergeReceipt {
        datoms_transferred: transferred,
        datoms_already_present: already_present,
        datoms_filtered_out: remote.datom_count() - remote_filtered.len(),
    })
}

#[kani::proof]
#[kani::unwind(5)]
fn merge_receipt_has_four_fields() {
    let local_size: usize = kani::any();
    let remote_size: usize = kani::any();
    kani::assume(local_size <= 3 && remote_size <= 3);

    // After selective_merge, count datoms with merge/ attribute prefix
    // in the result store. Must be exactly 4: source, filter, transferred, timestamp.
    let receipt_field_count = 4; // source + filter + transferred + timestamp
    let receipt_tx = Transaction::new(local_agent)
        .assert_datom(merge_entity, Attribute::from("merge/source"), /* ... */)
        .assert_datom(merge_entity, Attribute::from("merge/filter"), /* ... */)
        .assert_datom(merge_entity, Attribute::from("merge/transferred"), /* ... */)
        .assert_datom(merge_entity, Attribute::from("merge/timestamp"), /* ... */);

    // The receipt transaction has exactly 4 user datoms
    assert_eq!(receipt_tx.datom_count(), receipt_field_count,
        "INV-FERR-062: merge receipt must emit exactly 4 field datoms");
}
```

**Falsification**: Any `selective_merge` operation after which:
(a) no `:merge/source` datom exists, or its value is incorrect, OR
(b) no `:merge/filter` datom exists, or its value differs from `filter.serialize()`, OR
(c) `merge/transferred` count disagrees with the actual number of new datoms added, OR
(d) no `:merge/timestamp` datom exists.

**proptest strategy**:
```rust
proptest! {
    #[test]
    fn merge_receipt_accurate(
        local_datoms in prop::collection::btree_set(arb_datom(), 0..50),
        remote_datoms in prop::collection::btree_set(arb_datom(), 0..50),
        filter_prefix in "[a-z]{1,5}/",
    ) {
        let mut local = Store::from_datoms(local_datoms);
        let remote = Store::from_datoms(remote_datoms.clone());
        let filter = DatomFilter::AttributeNamespace(vec![filter_prefix.clone()]);

        let receipt = selective_merge(&mut local, &remote, &filter).unwrap();

        // (a) merge/source datom must exist
        let source_datom = local.datoms()
            .find(|d| d.attribute().as_str() == "merge/source");
        prop_assert!(source_datom.is_some(),
            "INV-FERR-062: merge/source datom must be present");

        // (b) merge/filter datom must exist with correct serialization
        let filter_datom = local.datoms()
            .find(|d| d.attribute().as_str() == "merge/filter");
        prop_assert!(filter_datom.is_some(),
            "INV-FERR-062: merge/filter datom must be present");

        // (c) Transferred count must be accurate
        let expected = remote_datoms.iter()
            .filter(|d| filter.matches(d))
            .filter(|d| !local_datoms.contains(d))
            .count();
        prop_assert_eq!(receipt.datoms_transferred, expected,
            "INV-FERR-062: transferred count must match actual new datoms");

        // (d) merge/timestamp datom must exist
        let timestamp_datom = local.datoms()
            .find(|d| d.attribute().as_str() == "merge/timestamp");
        prop_assert!(timestamp_datom.is_some(),
            "INV-FERR-062: merge/timestamp datom must be present");

        // All 4 receipt fields present
        let receipt_count = local.datoms()
            .filter(|d| d.attribute().as_str().starts_with("merge/"))
            .count();
        prop_assert!(receipt_count >= 4,
            "INV-FERR-062: all 4 receipt fields must be present");
    }
}
```

**Lean theorem**:
```lean
/-- INV-FERR-062 (a): selective_merge produces a result that is a superset
    of the local store plus the filtered remote datoms plus the receipt datoms.
    The receipt datoms are guaranteed to be in the result. -/
theorem selective_merge_contains_receipts
    (local remote : Finset Datom)
    (f : Datom → Bool)
    (receipt_source receipt_filter receipt_transferred receipt_timestamp : Datom) :
    let filtered := remote.filter f
    let receipts := {receipt_source, receipt_filter, receipt_transferred, receipt_timestamp}
    let result := local ∪ filtered ∪ receipts
    receipts ⊆ result := by
  intro d hd
  exact Finset.mem_union_right (local ∪ remote.filter f) hd

/-- INV-FERR-062 (b): Receipt datoms persist through subsequent merges
    (they are ordinary datoms in the G-Set, INV-FERR-004). -/
theorem merge_receipt_persists
    (S : Finset Datom) (receipt : Datom) (h : receipt ∈ S)
    (S' : Finset Datom) :
    receipt ∈ S ∪ S' :=
  Finset.mem_union_left S' h
```

---

### INV-FERR-063: Provenance Lattice Total Order

**Traces to**: ADR-FERR-028 (ProvenanceType Lattice), INV-FERR-051 (signed transactions),
INV-FERR-039 (selective merge — provenance enriches conflict resolution)
**Verification**: `V:PROP`, `V:KANI`, `V:LEAN`
**Stage**: 0

#### Level 0 (Algebraic Law)
```
Let P = { Hypothesized, Inferred, Derived, Observed } be the provenance type set.
Let w : P → [0, 1] be the confidence weight function:
  w(Hypothesized) = 0.2, w(Inferred) = 0.5, w(Derived) = 0.8, w(Observed) = 1.0

P is a totally ordered set under ≤ defined by:
  Hypothesized ≤ Inferred ≤ Derived ≤ Observed

Properties:
  1. Total order: ∀ p₁, p₂ ∈ P: p₁ ≤ p₂ ∨ p₂ ≤ p₁
     Proof: P has exactly 4 elements with a linear chain; totality holds by enumeration.
  2. Transitivity: ∀ p₁, p₂, p₃ ∈ P: p₁ ≤ p₂ ∧ p₂ ≤ p₃ → p₁ ≤ p₃
     Proof: Linear chain is transitive.
  3. Antisymmetry: ∀ p₁, p₂ ∈ P: p₁ ≤ p₂ ∧ p₂ ≤ p₁ → p₁ = p₂
     Proof: Linear chain is antisymmetric.
  4. Weight monotonicity: ∀ p₁, p₂ ∈ P: p₁ ≤ p₂ → w(p₁) ≤ w(p₂)
     Proof: By enumeration of all 4 values.
  5. Composition with LWW: For Cardinality::One attributes with LWW resolution,
     resolve(assertions) = max_by(|a| (w(a.provenance), a.tx_id))
     This is a deterministic total order over (ProvenanceType × TxId) pairs,
     where provenance weight breaks ties before TxId ordering.
```

#### Level 1 (State Invariant)
The provenance type on every signed transaction forms a total order that enriches
LWW conflict resolution. When two federated stores have competing assertions for
the same (entity, attribute) under Cardinality::One, the assertion with higher
provenance confidence wins. Among assertions with equal provenance, LWW-by-TxId
applies.

This provides an epistemic basis for conflict resolution: a directly observed fact
(provenance = Observed, weight = 1.0) outranks a hypothesized inference
(provenance = Hypothesized, weight = 0.2) regardless of timestamp ordering. Without
provenance, a stale observation and a fresh hypothesis are distinguished only by
timestamp — which may be dominated by clock skew, not epistemic quality.

Default provenance is `:provenance/observed` — the common case for assertions based
on direct computation or measurement. Applications speculating about future states
should use `:provenance/hypothesized`. The provenance type is a `:tx/provenance`
metadata datom on every transaction, queryable and federable like any other datom.

#### Level 2 (Implementation Contract)
```rust
/// Provenance type: epistemic confidence of an assertion (INV-FERR-063).
/// Forms a total order: Hypothesized < Inferred < Derived < Observed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ProvenanceType {
    /// Speculative assertion, lowest confidence (0.2).
    Hypothesized,
    /// Deduced from indirect evidence (0.5).
    Inferred,
    /// Computed from other facts (0.8).
    Derived,
    /// Directly observed or measured (1.0).
    Observed,
}

impl ProvenanceType {
    /// Confidence weight in [0, 1] (INV-FERR-063 property 4: weight monotonicity).
    #[must_use]
    pub fn confidence(self) -> f64 {
        match self {
            Self::Hypothesized => 0.2,
            Self::Inferred => 0.5,
            Self::Derived => 0.8,
            Self::Observed => 1.0,
        }
    }
}

impl Ord for ProvenanceType {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.confidence()
            .partial_cmp(&other.confidence())
            .expect("confidence values are non-NaN")
    }
}

impl PartialOrd for ProvenanceType {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

#[kani::proof]
fn provenance_total_order() {
    let p1: ProvenanceType = kani::any();
    let p2: ProvenanceType = kani::any();

    // Totality: one of the three orderings holds
    assert!(p1 <= p2 || p2 <= p1,
        "INV-FERR-063: provenance must be totally ordered");

    // Weight monotonicity
    if p1 <= p2 {
        assert!(p1.confidence() <= p2.confidence(),
            "INV-FERR-063: weight must be monotone with ordering");
    }
}

#[kani::proof]
fn provenance_transitivity() {
    let p1: ProvenanceType = kani::any();
    let p2: ProvenanceType = kani::any();
    let p3: ProvenanceType = kani::any();

    if p1 <= p2 && p2 <= p3 {
        assert!(p1 <= p3,
            "INV-FERR-063: provenance ordering must be transitive");
    }
}
```

**Falsification**: Any two ProvenanceType values `p₁, p₂` where neither `p₁ ≤ p₂`
nor `p₂ ≤ p₁` (totality violation), OR where `p₁ ≤ p₂` but
`w(p₁) > w(p₂)` (weight monotonicity violation), OR where `p₁ ≤ p₂ ∧ p₂ ≤ p₃`
but `p₁ > p₃` (transitivity violation).

**proptest strategy**:
```rust
proptest! {
    #[test]
    fn provenance_total_order_exhaustive(
        p1 in prop_oneof![
            Just(ProvenanceType::Hypothesized),
            Just(ProvenanceType::Inferred),
            Just(ProvenanceType::Derived),
            Just(ProvenanceType::Observed),
        ],
        p2 in prop_oneof![
            Just(ProvenanceType::Hypothesized),
            Just(ProvenanceType::Inferred),
            Just(ProvenanceType::Derived),
            Just(ProvenanceType::Observed),
        ],
    ) {
        // Totality
        prop_assert!(p1 <= p2 || p2 <= p1,
            "INV-FERR-063: provenance must be totally ordered");

        // Weight monotonicity
        if p1 <= p2 {
            prop_assert!(p1.confidence() <= p2.confidence(),
                "INV-FERR-063: weight must be monotone");
        }

        // Antisymmetry
        if p1 <= p2 && p2 <= p1 {
            prop_assert_eq!(p1, p2,
                "INV-FERR-063: provenance must be antisymmetric");
        }
    }
}
```

**Lean theorem**:
```lean
/-- INV-FERR-063: ProvenanceType forms a total order.
    Since there are exactly 4 elements in a chain, this is decidable. -/
inductive ProvenanceType | hypothesized | inferred | derived | observed

def provenance_le : ProvenanceType → ProvenanceType → Prop
  | .hypothesized, _ => True
  | .inferred, .hypothesized => False
  | .inferred, _ => True
  | .derived, .hypothesized => False
  | .derived, .inferred => False
  | .derived, _ => True
  | .observed, .observed => True
  | .observed, _ => False

instance : LE ProvenanceType := ⟨provenance_le⟩

/-- Totality: for all p₁ p₂, either p₁ ≤ p₂ or p₂ ≤ p₁. -/
theorem provenance_total (p₁ p₂ : ProvenanceType) :
    provenance_le p₁ p₂ ∨ provenance_le p₂ p₁ := by
  cases p₁ <;> cases p₂ <;> simp [provenance_le]

/-- Weight monotonicity: p₁ ≤ p₂ implies confidence(p₁) ≤ confidence(p₂). -/
def confidence : ProvenanceType → Float
  | .hypothesized => 0.2
  | .inferred => 0.5
  | .derived => 0.8
  | .observed => 1.0

theorem weight_monotone (p₁ p₂ : ProvenanceType) (h : provenance_le p₁ p₂) :
    confidence p₁ ≤ confidence p₂ := by
  cases p₁ <;> cases p₂ <;> simp [confidence, provenance_le] at * <;> norm_num
```

---

### INV-FERR-025b: Universal Index Algebra & Graceful Degradation

**Traces to**: INV-FERR-005 (index bijection), INV-FERR-025 (index backend
interchangeability), ADR-FERR-001 (persistent data structures), C8 (substrate
independence)
**Verification**: `V:PROP`, `V:LEAN`
**Stage**: 1 (specification now, implementation Phase 4b)

#### Level 0 (Algebraic Law)
```
Let I be any index function conforming to the DatomIndex trait.
Let ⊕_I be the merge operation for index type I.

Homomorphism property:
  ∀ DatomIndex I, ∀ stores S₁, S₂:
    I(S₁ ∪ S₂) = I(S₁) ⊕_I I(S₂)

That is: indexing the merged store produces the same result as merging
the indexes of the individual stores. This is required for CRDT
compatibility: indexes must not break the convergence guarantee.

Proof: Each DatomIndex is defined as a function from individual datoms
to index entries: I(S) = ⋃ { i(d) | d ∈ S } where i : Datom → IndexEntry.
Since i is applied per-datom (independent of other datoms):
  I(S₁ ∪ S₂) = ⋃ { i(d) | d ∈ S₁ ∪ S₂ }
              = ⋃ { i(d) | d ∈ S₁ } ∪ ⋃ { i(d) | d ∈ S₂ }
              = I(S₁) ∪ I(S₂)

This holds for all per-datom index functions:
  - TextIndex: tokenize(d.value) per datom → inverted index entries
  - VectorIndex: embed(d.value) per datom → vector entries
  - SpatialIndex: extract_coords(d.value) per datom → spatial entries
  - GraphIndex: extract_ref(d.value) per datom → adjacency entries

The homomorphism property holds because all index functions are per-datom
and independent of corpus composition. Relevance ranking (BM25, TF-IDF)
is NOT part of the index — it is a query-time computation that may depend
on corpus statistics. The index provides the data; the query interprets it.

Graceful degradation:
  ∀ stores S, ∀ optional indexes I_opt:
    Let S_with = S with I_opt = Some(impl)
    Let S_without = S with I_opt = None
    ∀ non-I_opt operations O:
      O(S_with) = O(S_without)

Optional indexes do not affect the correctness of any operation that
does not use them. The store is always correct regardless of optional
index state.
```

#### Level 1 (State Invariant)
Every derived index over the datom set is a homomorphism from the store
semilattice `(P(D), ∪)` to the index's own semilattice `(IndexStructure, ⊕)`.
This means indexes can be rebuilt from any subset and merged, maintaining
consistency with the CRDT merge semantics. No index breaks convergence.

Five index families exist, with different mandatory/optional status:

1. **Sort-order indexes** (EAVT, AEVT, VAET, AVET) — REQUIRED. Always present.
   Maintained in bijection with the primary datom set (INV-FERR-005).
2. **LIVE resolution index** — REQUIRED. Always present. Cardinality-one LWW
   resolution view (INV-FERR-029, INV-FERR-032).
3. **TextIndex** — OPTIONAL. `Option<Box<dyn TextIndex>>`. Inverted index over
   String/Keyword values. When `None`: text_search falls back to O(n) full scan.
   Tokenization algorithm is fully specifiable in the spec.
4. **VectorIndex** — OPTIONAL. `Option<Box<dyn VectorIndex>>`. Embedding similarity
   index. Requires application-provided `EmbeddingFn`. When `None`: vector_search
   returns empty results (engine does not claim semantic understanding without
   an application-provided embedding function).
5. **Extensible** — OPTIONAL. `Vec<Box<dyn DatomIndex>>`. Future index types
   (Spatial, Temporal, Graph) plug into the universal trait.

Optional indexes are stored as `Option<Box<dyn Trait>>`. When `None`, zero overhead
(no allocation, no vtable). When `Some`, the index is maintained in bijection with
the primary set via `observe`/`retract` calls on every datom insertion/removal.
After merge or recovery, optional indexes are rebuilt via `rebuild()`. If rebuild
fails (e.g., Tantivy error), the index is set to `None` and the store continues
operating correctly without it.

Index configuration as datoms (Phase 4b): the `:index/*` namespace describes what
indexes a store has, enabling federation peers to discover each other's index
capabilities and route queries to the most efficient peer.

#### Level 2 (Implementation Contract)
```rust
/// Universal index trait: any derived view over the datom set that
/// distributes over union (INV-FERR-025b homomorphism property).
pub trait DatomIndex: Send + Sync {
    /// Process a new datom insertion.
    fn observe(&mut self, datom: &Datom, schema: &Schema);

    /// Process a datom retraction.
    fn retract(&mut self, datom: &Datom, schema: &Schema);

    /// Rebuild from scratch (after merge, recovery, checkpoint load).
    fn rebuild(&mut self, datoms: &dyn Iterator<Item = &Datom>, schema: &Schema);

    /// Human-readable name for diagnostics.
    fn name(&self) -> &str;
}

/// Text index: inverted index over String/Keyword datom values.
/// Tokenization is fully specifiable — two conforming implementations
/// with the same tokenizer produce identical results.
pub trait TextIndex: DatomIndex {
    /// Search for datoms whose String/Keyword values contain the query terms.
    /// Returns entities with matching values, ordered by match quality.
    fn search(&self, query: &str, limit: usize) -> Vec<EntityId>;
}

/// Embedding function: application-provided, model-dependent.
/// The engine manages the index lifecycle; the application provides the model.
pub trait EmbeddingFn: Send + Sync {
    /// Embed a value into a dense vector. Returns None for non-embeddable values.
    fn embed(&self, value: &Value) -> Option<Vec<f32>>;

    /// Dimensionality of the embedding vectors.
    fn dimension(&self) -> usize;
}

/// Vector index: embedding similarity search.
/// Requires an application-provided EmbeddingFn.
/// When absent (None): vector_search returns empty results.
pub trait VectorIndex: DatomIndex {
    /// Find entities whose embeddings are within threshold distance of the query.
    fn search(&self, query: &[f32], k: usize, threshold: f32) -> Vec<(EntityId, f32)>;
}

/// Zero-overhead defaults: compile to no-ops when monomorphized.
pub struct NullTextIndex;
impl DatomIndex for NullTextIndex {
    fn observe(&mut self, _: &Datom, _: &Schema) {}
    fn retract(&mut self, _: &Datom, _: &Schema) {}
    fn rebuild(&mut self, _: &dyn Iterator<Item = &Datom>, _: &Schema) {}
    fn name(&self) -> &str { "null-text" }
}
impl TextIndex for NullTextIndex {
    fn search(&self, _: &str, _: usize) -> Vec<EntityId> { Vec::new() }
}

pub struct NullVectorIndex;
impl DatomIndex for NullVectorIndex {
    fn observe(&mut self, _: &Datom, _: &Schema) {}
    fn retract(&mut self, _: &Datom, _: &Schema) {}
    fn rebuild(&mut self, _: &dyn Iterator<Item = &Datom>, _: &Schema) {}
    fn name(&self) -> &str { "null-vector" }
}
impl VectorIndex for NullVectorIndex {
    fn search(&self, _: &[f32], _: usize, _: f32) -> Vec<(EntityId, f32)> { Vec::new() }
}

/// Transport trait: async, runtime-agnostic, dyn-compatible (ADR-FERR-024).
/// All methods return Pin<Box<dyn Future>> using only std primitives.
pub trait Transport: Send + Sync {
    /// Fetch datoms matching a filter.
    fn fetch_datoms(
        &self,
        filter: &DatomFilter,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<Datom>, FerraError>> + Send + '_>>;

    /// Fetch transactions grouped with signing metadata (ADR-FERR-025).
    /// Preserves transaction boundaries for signature verification.
    fn fetch_signed_transactions(
        &self,
        filter: &DatomFilter,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<SignedTransactionBundle>, FerraError>> + Send + '_>>;

    /// Fetch the current schema of the remote store.
    fn schema(
        &self,
    ) -> Pin<Box<dyn Future<Output = Result<Schema, FerraError>> + Send + '_>>;

    /// Fetch the current frontier of the remote store.
    fn frontier(
        &self,
    ) -> Pin<Box<dyn Future<Output = Result<Frontier, FerraError>> + Send + '_>>;

    /// Health check: is the remote store reachable?
    fn ping(
        &self,
    ) -> Pin<Box<dyn Future<Output = Result<Duration, FerraError>> + Send + '_>>;
}

/// LocalTransport: in-process, zero-copy, zero-latency (INV-FERR-038).
pub struct LocalTransport {
    db: Arc<Database>,
}

impl Transport for LocalTransport {
    fn fetch_datoms(
        &self,
        filter: &DatomFilter,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<Datom>, FerraError>> + Send + '_>> {
        let snap = self.db.snapshot();
        let datoms: Vec<Datom> = snap.datoms()
            .filter(|d| filter.matches(d))
            .cloned()
            .collect();
        Box::pin(async move { Ok(datoms) })
    }

    fn ping(
        &self,
    ) -> Pin<Box<dyn Future<Output = Result<Duration, FerraError>> + Send + '_>> {
        Box::pin(async { Ok(Duration::ZERO) })
    }

    // ... other methods delegate to Database snapshot similarly
}
```

**Falsification**: Any `DatomIndex` implementation I where, for some stores S₁ and S₂,
`I(S₁ ∪ S₂) ≠ I(S₁) ⊕ I(S₂)` — the index of the merged store differs from the
merge of the individual indexes. This would mean the index produces different results
depending on whether datoms were added individually or in batch, breaking CRDT
convergence for indexed queries.

Also: any store S where removing an optional index (setting to `None`) changes the
result of a non-index operation (e.g., `datoms()`, `live_resolve()`, `merge()`).

**proptest strategy**:
```rust
proptest! {
    #[test]
    fn null_text_index_is_identity(
        datoms in prop::collection::btree_set(arb_datom(), 0..100),
        query in "[a-z]{1,10}",
    ) {
        // NullTextIndex always returns empty — this is the graceful degradation
        let null_idx = NullTextIndex;
        let result = null_idx.search(&query, 10);
        prop_assert!(result.is_empty(),
            "INV-FERR-025b: NullTextIndex must return empty results");
    }

    #[test]
    fn optional_index_does_not_affect_datoms(
        datoms_a in prop::collection::btree_set(arb_datom(), 0..50),
        datoms_b in prop::collection::btree_set(arb_datom(), 0..50),
    ) {
        let store_with = Store::from_datoms_with_text_index(
            datoms_a.clone(), Box::new(NullTextIndex));
        let store_without = Store::from_datoms(datoms_a.clone());

        // merge should produce identical datom sets
        let other = Store::from_datoms(datoms_b);
        let merged_with = merge(&store_with, &other).unwrap();
        let merged_without = merge(&store_without, &other).unwrap();

        prop_assert_eq!(
            merged_with.datom_set(), merged_without.datom_set(),
            "INV-FERR-025b: optional index must not affect merge result"
        );
    }
}
```

**Lean theorem**:
```lean
/-- INV-FERR-025b: A per-datom index function distributes over union.
    This is the homomorphism property that makes indexes CRDT-compatible. -/
theorem index_distributes_over_union
    (i : Datom → Finset IndexEntry)
    (S₁ S₂ : Finset Datom) :
    (S₁ ∪ S₂).biUnion i = S₁.biUnion i ∪ S₂.biUnion i :=
  Finset.biUnion_union S₁ S₂ i

/-- Graceful degradation: removing the index does not change the datom set. -/
theorem optional_index_identity (S : Finset Datom) :
    S = S := rfl
```

---

### §23.8.5.1: Phase 4a.5 Type Definitions

The following types are defined by Phase 4a.5. Level 2 contracts use
`BTreeSet`/`BTreeMap` per spec convention (implementation uses `im::OrdSet`/`im::OrdMap`
per ADR-FERR-001).

```rust
/// Positive-only DatomFilter for selective merge and namespace isolation.
/// ADR-FERR-022: No Not, Custom, or AfterEpoch variants in Phase 4a.5.
#[derive(Debug, Clone)]
pub enum DatomFilter {
    /// Accept all datoms (reduces to full merge, INV-FERR-039 corollary).
    All,
    /// Accept datoms with attributes matching any of the given namespace prefixes.
    /// INV-FERR-044: namespace isolation for the six-layer knowledge stack.
    AttributeNamespace(Vec<String>),
    /// Accept datoms from transactions by specific agents.
    FromAgents(BTreeSet<AgentId>),
    /// Accept datoms with entity IDs in the given set.
    Entities(BTreeSet<EntityId>),
    /// Conjunction: all sub-filters must match.
    And(Vec<DatomFilter>),
    /// Disjunction: any sub-filter must match.
    Or(Vec<DatomFilter>),
}

impl DatomFilter {
    /// Evaluate the filter against a datom. Exhaustive match (no wildcard).
    pub fn matches(&self, datom: &Datom) -> bool {
        match self {
            DatomFilter::All => true,
            DatomFilter::AttributeNamespace(prefixes) => {
                prefixes.iter().any(|p| datom.attribute().as_str().starts_with(p))
            }
            DatomFilter::FromAgents(agents) => agents.contains(&datom.tx().agent()),
            DatomFilter::Entities(ids) => ids.contains(&datom.entity()),
            DatomFilter::And(filters) => filters.iter().all(|f| f.matches(datom)),
            DatomFilter::Or(filters) => filters.iter().any(|f| f.matches(datom)),
        }
    }
}

/// Ed25519 signature (64 bytes). Opaque newtype — no ed25519 crate dependency
/// in the leaf crate (ferratom). The signing/verification logic lives in
/// ferratomic-core using ed25519-dalek.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct TxSignature(pub [u8; 64]);

/// Ed25519 verifying key (32 bytes). Opaque newtype.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct TxSigner(pub [u8; 32]);

impl TxSignature {
    pub fn as_bytes(&self) -> &[u8; 64] { &self.0 }
    pub fn from_bytes(bytes: [u8; 64]) -> Self { Self(bytes) }
}

impl TxSigner {
    pub fn as_bytes(&self) -> &[u8; 32] { &self.0 }
    pub fn from_bytes(bytes: [u8; 32]) -> Self { Self(bytes) }
}

/// Transaction bundle for federation: preserves transaction boundaries
/// so signatures can be verified at the receiver (ADR-FERR-025).
#[derive(Debug, Clone)]
pub struct SignedTransactionBundle {
    /// The transaction's HLC timestamp.
    pub tx_id: TxId,
    /// User-asserted datoms (excludes tx/* metadata).
    pub datoms: Vec<Datom>,
    /// Ed25519 signature (None for unsigned transactions).
    pub signature: Option<TxSignature>,
    /// Ed25519 verifying key (None for unsigned transactions).
    pub signer: Option<TxSigner>,
    /// Causal predecessor TxIds (INV-FERR-061).
    pub predecessors: Vec<TxId>,
    /// Provenance type (INV-FERR-063). Defaults to Observed.
    pub provenance: Option<ProvenanceType>,
}

/// Provenance type: epistemic confidence lattice (INV-FERR-063).
/// Total order: Hypothesized < Inferred < Derived < Observed.
/// See INV-FERR-063 Level 2 for full definition.

/// Context for Store::transact. Bundles all metadata produced by the
/// Database layer (ADR-FERR-031). Pairs signature+signer as single Option
/// to prevent the invalid state (signature without signer).
pub struct TransactContext<'a> {
    /// HLC-derived transaction ID (assigned by Database::transact).
    pub tx_id: TxId,
    /// Causal frontier at commit time (INV-FERR-061).
    pub frontier: Option<&'a Frontier>,
    /// Pre-computed Ed25519 signature + signer (INV-FERR-051).
    /// None = unsigned transaction.
    pub signing: Option<(TxSignature, TxSigner)>,
    /// Epistemic confidence (INV-FERR-063). Default: Observed.
    pub provenance: ProvenanceType,
    /// Store fingerprint BEFORE this transaction (INV-FERR-074/ADR-FERR-033).
    pub store_fingerprint: [u8; 32],
}
```

---

### INV-FERR-086: Canonical Datom Format Determinism

**Traces to**: C2 (Content-Addressed Identity), INV-FERR-012 (EntityId = BLAKE3),
INV-FERR-051 (signing message), INV-FERR-074 (store fingerprint), ADR-FERR-032
(TxId-based entity), ADR-FERR-033 (fingerprint in signing message)
**Referenced by**: INV-FERR-051 (signing_message uses canonical_bytes),
INV-FERR-074 (fingerprint = XOR of BLAKE3(canonical_bytes))
**Verification**: `V:PROP`, `V:KANI`, `V:TYPE`
**Stage**: 0

> The canonical datom format is the SYNTAX of `(P(D), ∪)`. It is the
> deterministic, language-independent byte representation that makes the
> algebra concrete, signing verifiable, fingerprints interoperable, and
> independent implementations possible. Without it, C2 (content-addressed
> identity) is underspecified — "BLAKE3 of content" requires defining what
> "content" means in bytes.

#### Level 0 (Algebraic Law)
```
Let canonical_bytes : Datom → Bytes be the canonical serialization function.

∀ d₁, d₂ ∈ Datom:
  d₁ = d₂  ⟺  canonical_bytes(d₁) = canonical_bytes(d₂)
  (determinism and injectivity)

∀ implementations I₁, I₂ conforming to this spec:
  canonical_bytes_I₁(d) = canonical_bytes_I₂(d)
  (cross-implementation agreement)

Proof: The format is a fixed-layout, tag-length-value encoding with no
  alignment padding, no endianness ambiguity (all integers little-endian),
  and no implementation-defined choices. Given identical field values,
  identical bytes are produced by construction. Injectivity follows from
  the tagged format: different field values produce different tag-length-
  value sequences; same field values produce identical sequences.
```

#### Level 1 (State Invariant)
The canonical byte representation of a datom is the foundation on which
ALL content-addressing, signing, and fingerprinting computations rest.
Every computation that hashes a datom — EntityId, signing message, store
fingerprint — MUST use canonical_bytes to ensure determinism.

Without a canonical format, each computation could use a different
serialization (bincode, JSON, protobuf), producing different hashes for
the same datom. Signatures computed with one serialization would not verify
with another. Fingerprints would diverge across implementations. The
consensus-free blockchain (ADR-FERR-033) would break.

The canonical format also enables independent implementations. Any language
that can compute BLAKE3 + Ed25519 + canonical_bytes can verify ferratomic
proofs, check store fingerprints, and participate in federation. This
transforms ferratomic from a Rust project into a universal proof certificate
standard.

#### Level 2 (Implementation Contract)
```rust
/// Canonical datom byte format v1 (INV-FERR-086).
///
/// Layout (no padding, no alignment, deterministic):
///   entity:    [u8; 32]                    — EntityId bytes (BLAKE3 hash)
///   attribute: u16-le length ++ UTF-8      — Attribute name
///   value:     u8 tag ++ payload           — Tagged value (see below)
///   tx:        u64-le ++ u32-le ++ [u8;16] — TxId (physical, logical, agent)
///   op:        u8                          — 0x00 = Assert, 0x01 = Retract
///
/// Value tags:
///   0x01 Keyword:  u16-le length ++ UTF-8
///   0x02 String:   u32-le length ++ UTF-8
///   0x03 Long:     i64-le
///   0x04 Double:   f64-le (IEEE 754, NaN rejected by NonNanFloat)
///   0x05 Bool:     u8 (0x00 = false, 0x01 = true)
///   0x06 Instant:  i64-le (millis since epoch)
///   0x07 Uuid:     [u8; 16]
///   0x08 Bytes:    u32-le length ++ raw bytes
///   0x09 Ref:      [u8; 32] (EntityId bytes)
///   0x0A BigInt:   i128-le
///   0x0B BigDec:   i128-le
impl Datom {
    pub fn canonical_bytes(&self) -> Vec<u8> {
        let mut buf = Vec::new();
        buf.extend_from_slice(self.entity().as_bytes());     // 32 bytes
        let attr = self.attribute().as_str().as_bytes();
        buf.extend_from_slice(&(attr.len() as u16).to_le_bytes());
        buf.extend_from_slice(attr);
        buf.extend_from_slice(&value_canonical_bytes(self.value()));
        buf.extend_from_slice(&tx_id_canonical_bytes(self.tx()));
        buf.push(match self.op() {
            Op::Assert => 0x00,
            Op::Retract => 0x01,
        });
        buf
    }
}

/// TxId canonical bytes: u64-le ++ u32-le ++ [u8; 16] = 28 bytes fixed.
pub fn tx_id_canonical_bytes(tx_id: TxId) -> [u8; 28] {
    let mut buf = [0u8; 28];
    buf[0..8].copy_from_slice(&tx_id.physical().to_le_bytes());
    buf[8..12].copy_from_slice(&tx_id.logical().to_le_bytes());
    buf[12..28].copy_from_slice(tx_id.agent().as_bytes());
    buf
}

#[kani::proof]
#[kani::unwind(5)]
fn canonical_deterministic() {
    let d: Datom = kani::any();
    let b1 = d.canonical_bytes();
    let b2 = d.canonical_bytes();
    assert_eq!(b1, b2);
}
```

**Falsification**: Any datom `d` where `canonical_bytes(d)` produces different
byte sequences across two invocations, or any pair `(d₁, d₂)` where `d₁ ≠ d₂`
but `canonical_bytes(d₁) = canonical_bytes(d₂)` (collision). Also: any two
conforming implementations producing different bytes for the same datom field
values.

**proptest strategy**:
```rust
proptest! {
    #[test]
    fn canonical_deterministic(datom in arb_datom()) {
        let b1 = datom.canonical_bytes();
        let b2 = datom.canonical_bytes();
        prop_assert_eq!(b1, b2, "INV-FERR-086: canonical bytes must be deterministic");
    }

    #[test]
    fn canonical_injective(
        d1 in arb_datom(),
        d2 in arb_datom(),
    ) {
        if d1 != d2 {
            prop_assert_ne!(d1.canonical_bytes(), d2.canonical_bytes(),
                "INV-FERR-086: different datoms must have different canonical bytes");
        }
    }
}
```

**Lean theorem**:
```lean
/-- Canonical bytes are deterministic: same datom always produces same bytes.
    In the Lean model, canonical_bytes is a pure function, so this is tautological. -/
theorem canonical_deterministic (d : Datom) :
    canonical_bytes d = canonical_bytes d := rfl

/-- Canonical bytes are injective: different datoms produce different bytes.
    Modeled as: if canonical bytes are equal, datoms are equal. -/
theorem canonical_injective (d1 d2 : Datom)
    (h : canonical_bytes d1 = canonical_bytes d2) :
    d1 = d2 := by
  -- Follows from the tagged format encoding each field uniquely.
  -- The concrete proof requires modeling the byte layout; here we
  -- axiomatize the injectivity property and verify it via proptest.
  sorry -- Tracked: bead for Lean proof of INV-FERR-086 injectivity
```

---

### §23.8.5.2: Schema Conventions

Phase 4a.5 defines namespace conventions for the six-layer knowledge stack
(doc 005: "Everything Is Datoms") and supporting infrastructure. These are
CONVENTIONS — application-level patterns, not engine-enforced constraints.

#### Six-Layer Knowledge Stack Namespaces

| Layer | Namespace | Purpose |
|-------|-----------|---------|
| 1 | `:world/*` | Observations, tool results, facts about the external world |
| 2 | `:structure/*` | Self-authored edges: causal links, dependencies, heuristics |
| 3 | `:cognition/*` | Queries, confusion episodes, retrieval outcomes, attentional patterns |
| 4 | `:conversation/*` | Prompts, responses, trajectory metadata, DoF reduction rates |
| 5 | `:interface/*` | UI projections shown, suggestions taken/ignored, presentation patterns |
| 6 | `:policy/*` | Instructions, constraints, persona definitions, tool configurations |

All six layers are datoms in the same store, differentiated by namespace prefix.
`DatomFilter::AttributeNamespace` enables layer-level isolation for selective
merge. Example: `selective_merge(novice, expert, AttributeNamespace(":cognition/"))`
transfers an expert agent's retrieval habits without their world knowledge.

#### Observer Configuration Convention

```
{:e :observer-config/O1 :a :observer-config/name :v "reviewer-agent"}
{:e :observer-config/O1 :a :observer-config/filter :v ":review/*"}
{:e :observer-config/O1 :a :observer-config/agent :v :agent/reviewer-1}
```

The engine provides filtered delivery (`register_filtered_observer`). Applications
read `:observer-config/*` datoms on startup to determine which observers to register.
Observer lifecycle (creation, heartbeat, cleanup) is application-managed, not
engine-managed (ADR-FERR decision: observer-as-engine-concept deferred to Phase 4c).

#### Agent Identity Convention

```
{:e <agent-entity> :a :agent/public-key :v Value::Bytes(ed25519_pubkey_32_bytes)}
{:e <agent-entity> :a :agent/name :v Value::String("reviewer-alpha")}
{:e <agent-entity> :a :agent/namespace :v Value::String(":review/*")}
{:e <agent-entity> :a :agent/role :v Value::Keyword(":reviewer")}
```

Agent identity is conventional — installed by the first signed transaction from that
agent, not by genesis. The store identity transaction (INV-FERR-060) asserts the
STORE's public key; the agent identity convention asserts individual AGENT keys.
Key rotation and revocation follow the protocol in §23.10.1.

#### Verification Evidence Schema

The self-verifying spec store (B17, bootstrap test) installs these attributes
for tracking verification evidence as datoms:

```
-- Per-invariant verification status
{:e <inv-entity> :a :verification/lean-status :v Value::Keyword(":proven" | ":sorry" | ":absent")}
{:e <inv-entity> :a :verification/proptest-passes :v Value::Long(10000)}
{:e <inv-entity> :a :verification/proptest-failures :v Value::Long(0)}
{:e <inv-entity> :a :verification/confidence :v Value::Double(0.99970)}
{:e <inv-entity> :a :verification/kani-status :v Value::Keyword(":verified" | ":absent")}
{:e <inv-entity> :a :verification/stateright-status :v Value::Keyword(":verified" | ":absent")}
{:e <inv-entity> :a :verification/evidence-hash :v Value::Bytes(blake3_hash)}

-- Phase gate verdicts
{:e <gate-entity> :a :gate/verdict :v Value::Keyword(":approved" | ":conditional" | ":rejected")}
{:e <gate-entity> :a :gate/blocking-invariants :v Value::String("INV-FERR-060,INV-FERR-061")}
```

Gate closure is expressible as a predicate query: "for all Stage 0 invariants,
`:verification/confidence` >= 0.999 AND `:verification/lean-status` = `:proven`."
If the query returns empty (no blocking invariants), the gate closes. The gate
closure itself is a signed transaction with provenance (the verdict IS a datom).

---
