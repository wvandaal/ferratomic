//! `Store` -- the G-Set CRDT semilattice: `Store = (P(D), union)`.
//!
//! The `Store` is the core data structure of Ferratomic. It holds an
//! append-only, content-addressed set of datoms with four secondary
//! indexes (EAVT, AEVT, VAET, AVET) maintained in bijection with
//! the primary set.
//!
//! ## Algebraic properties
//!
//! - **INV-FERR-001**: merge is commutative (set union).
//! - **INV-FERR-002**: merge is associative (set union).
//! - **INV-FERR-003**: merge is idempotent (set union).
//! - **INV-FERR-004**: transact is strictly monotonic -- the store only grows.
//! - **INV-FERR-005**: secondary indexes are in bijection with the primary set.
//! - **INV-FERR-007**: epochs are strictly monotonically increasing.
//! - **INV-FERR-031**: genesis produces a deterministic store.
//!
//! ## Design
//!
//! The primary store uses `im::OrdSet<Datom>` (ADR-FERR-001). Snapshots
//! are O(1) via structural sharing -- `clone()` shares the tree spine.
//! Four secondary indexes (EAVT, AEVT, VAET, AVET) are maintained in
//! bijection with the primary set via [`SortedVecIndexes`].
//! INV-FERR-005 is satisfied by updating all indexes on every insert.

use std::{collections::BTreeSet, sync::Arc};

use ferratom::{AgentId, Attribute, AttributeDef, Datom, EntityId, Op, Schema, TxId, Value};
use ferratomic_index::SortedVecIndexes;
use ferratomic_positional::PositionalStore;
use im::{OrdMap, OrdSet};

use crate::{
    iter::{DatomIter, DatomSetView, SnapshotDatoms},
    merge::SchemaConflict,
    repr::StoreRepr,
};

// ---------------------------------------------------------------------------
// TxReceipt
// ---------------------------------------------------------------------------

/// Receipt returned by a successful [`Store::transact`] call.
///
/// INV-FERR-007: the epoch field is strictly monotonically increasing
/// across successive transactions on the same store.
#[derive(Debug, Clone)]
pub struct TxReceipt {
    /// The epoch at which this transaction was committed.
    pub(crate) epoch: u64,
    /// The datoms inserted by this transaction (stamped with real `TxId`,
    /// including tx metadata datoms). Carried here so callers (`db.rs`)
    /// can write them to WAL and deliver to observers without recomputing
    /// via O(n) set difference.
    pub(crate) datoms: Vec<Datom>,
}

impl TxReceipt {
    /// The epoch at which this transaction was committed.
    ///
    /// INV-FERR-007: each receipt's epoch is strictly greater than
    /// the epoch of the immediately preceding transaction.
    #[must_use]
    pub fn epoch(&self) -> u64 {
        self.epoch
    }

    /// The datoms inserted by this transaction, stamped with the real
    /// `TxId` and including tx metadata datoms (`:tx/time`, `:tx/agent`).
    #[must_use]
    pub fn datoms(&self) -> &[Datom] {
        &self.datoms
    }
}

// ---------------------------------------------------------------------------
// Snapshot
// ---------------------------------------------------------------------------

/// An immutable point-in-time view of the store.
///
/// INV-FERR-006: a snapshot is frozen at creation time. Later writes
/// to the store do not affect it. For `Positional` stores, the `Arc`
/// clone is O(1). For `OrdMap` stores, the `im::OrdSet` clone is O(1)
/// via structural sharing (ADR-FERR-001).
#[derive(Debug, Clone)]
pub struct Snapshot {
    /// Datom set at snapshot time, dispatched by representation.
    pub(crate) datoms: SnapshotDatoms,
    /// Epoch at the time the snapshot was taken.
    pub(crate) epoch: u64,
}

// ---------------------------------------------------------------------------
// Store
// ---------------------------------------------------------------------------

/// The G-Set CRDT semilattice: an append-only set of datoms with
/// secondary indexes and schema.
///
/// `Store = (P(D), union)` where `P(D)` is the powerset of all datoms
/// and `union` is the join (least upper bound) operation. Writes are
/// commutative, associative, and idempotent by construction
/// (INV-FERR-001, INV-FERR-002, INV-FERR-003).
///
/// INV-FERR-004: the store only grows. No datom is ever removed.
/// Retractions are new datoms with `Op::Retract`.
///
/// ## Adaptive representation (bd-h2fz)
///
/// Cold-start-loaded stores use `StoreRepr::Positional` (contiguous arrays,
/// ~6x less memory, cache-optimal reads). On first write, `promote()`
/// converts to `StoreRepr::OrdMap` (persistent tree, O(log n) insert).
/// The promotion is semantics-preserving: callers observe identical behavior
/// regardless of which representation is active.
#[derive(Debug, Clone)]
pub struct Store {
    /// Dual representation: Positional (cold start) or `OrdMap` (write-active).
    pub(crate) repr: StoreRepr,
    /// Attribute definitions governing transact validation.
    pub(crate) schema: Schema,
    /// Monotonically increasing transaction epoch counter.
    /// INV-FERR-007: incremented on every successful transact.
    pub(crate) epoch: u64,
    /// The agent identity used for genesis transactions.
    /// Stored so callers can create transactions against this store.
    pub(crate) genesis_agent: AgentId,
    /// INV-FERR-029/032: Causal OR-Set LIVE lattice.
    ///
    /// Maps `(entity, attribute)` to `value` to `(TxId, Op)` where `TxId` is
    /// the latest causal event for that `(e,a,v)` triple. Values with
    /// `Op::Assert` are LIVE; values with `Op::Retract` are dead but causally
    /// tracked for merge
    /// correctness. This structure is a join-semilattice under per-key
    /// max(TxId), making `merge_causal` a lattice homomorphism.
    pub(crate) live_causal: OrdMap<(EntityId, Attribute), OrdMap<Value, (TxId, Op)>>,
    /// INV-FERR-029: Materialized projection of `live_causal` for the
    /// `live_values()` query API. Contains only values where op == Assert.
    /// Maintained in sync with `live_causal` by `live_apply`.
    pub(crate) live_set: OrdMap<(EntityId, Attribute), OrdSet<Value>>,
    /// INV-FERR-043: Deterministic schema conflicts discovered during merge.
    ///
    /// Non-merge construction paths leave this empty. `from_merge` populates it
    /// so callers can diagnose schema drift without changing merge semantics.
    pub(crate) schema_conflicts: Vec<SchemaConflict>,
}

impl Store {
    /// Construct a store from a `BTreeSet` of datoms.
    ///
    /// INV-FERR-005: indexes are built from the provided datom set,
    /// ensuring bijection by construction. The schema is empty and
    /// epoch starts at 0.
    ///
    /// Accepts `BTreeSet` for generator/test compatibility. Builds a
    /// `PositionalStore` internally (bd-h2fz: cold-start path).
    /// For merge, use `Store::merge` which preserves schema and epoch.
    #[must_use]
    pub fn from_datoms(datoms: BTreeSet<Datom>) -> Self {
        let positional = PositionalStore::from_datoms(datoms.into_iter());
        let live_causal = crate::query::build_live_causal(positional.datoms().iter());
        let live_set = crate::query::derive_live_set(&live_causal);
        Self {
            repr: StoreRepr::Positional(Arc::new(positional)),
            schema: Schema::empty(),
            epoch: 0,
            genesis_agent: AgentId::from_bytes([0u8; 16]),
            live_causal,
            live_set,
            schema_conflicts: Vec::new(),
        }
    }

    /// Reconstruct a store from checkpoint data.
    ///
    /// INV-FERR-013: Used by `load_checkpoint` to rebuild the store from
    /// serialized epoch, genesis agent, schema attributes, and datoms.
    /// INV-FERR-005: indexes are rebuilt from the datom set by construction.
    ///
    /// bd-h2fz: builds `Positional` repr for cache-optimal cold-start reads.
    #[must_use]
    pub fn from_checkpoint(
        epoch: u64,
        genesis_agent: AgentId,
        schema_attrs: Vec<(String, AttributeDef)>,
        datoms: Vec<Datom>,
    ) -> Self {
        let mut schema = Schema::empty();
        for (name, def) in schema_attrs {
            schema.define(Attribute::from(name.as_str()), def);
        }
        let positional = PositionalStore::from_datoms(datoms.into_iter());
        let live_causal = crate::query::build_live_causal(positional.datoms().iter());
        let live_set = crate::query::derive_live_set(&live_causal);
        Self {
            repr: StoreRepr::Positional(Arc::new(positional)),
            schema,
            epoch,
            genesis_agent,
            live_causal,
            live_set,
            schema_conflicts: Vec::new(),
        }
    }

    /// Reconstruct a store from V3 checkpoint data (zero-construction cold start).
    ///
    /// INV-FERR-013: Used by V3 checkpoint deserialization. The datoms are
    /// already sorted and the LIVE bitvector is pre-computed, so this
    /// constructor builds a `PositionalStore` without re-sorting or
    /// recomputing liveness.
    ///
    /// INV-FERR-076: `from_sorted_with_live` is used for the positional store.
    /// Returns an error if preconditions are violated (`live_bits`/canonical
    /// length mismatch, unsorted datoms, or u32 position space overflow).
    ///
    /// # Errors
    ///
    /// Returns `FerraError::InvariantViolation` if the sorted datom or
    /// `live_bits` preconditions are violated (INV-FERR-076).
    pub fn from_checkpoint_v3(
        epoch: u64,
        genesis_agent: AgentId,
        schema_attrs: Vec<(String, AttributeDef)>,
        sorted_datoms: Vec<Datom>,
        live_bits: bitvec::prelude::BitVec<u64, bitvec::prelude::Lsb0>,
    ) -> Result<Self, ferratom::FerraError> {
        let mut schema = Schema::empty();
        for (name, def) in schema_attrs {
            schema.define(Attribute::from(name.as_str()), def);
        }
        let positional = PositionalStore::from_sorted_with_live(sorted_datoms, live_bits)?;
        let live_causal = crate::query::build_live_causal(positional.datoms().iter());
        let live_set = crate::query::derive_live_set(&live_causal);
        Ok(Self {
            repr: StoreRepr::Positional(Arc::new(positional)),
            schema,
            epoch,
            genesis_agent,
            live_causal,
            live_set,
            schema_conflicts: Vec::new(),
        })
    }

    /// Deterministic genesis store with the 19 axiomatic meta-schema attributes.
    ///
    /// INV-FERR-031: every call to `genesis()` produces an identical store.
    /// The 19 attributes are the ONLY hardcoded elements in the engine.
    /// Every other attribute is defined by transacting datoms that reference
    /// these 19. This is the schema-as-data bootstrap (C3, C7).
    ///
    /// bd-h2fz: builds `Positional` repr (empty store, zero-cost).
    #[must_use]
    pub fn genesis() -> Self {
        let positional = PositionalStore::from_datoms(std::iter::empty());
        Self {
            repr: StoreRepr::Positional(Arc::new(positional)),
            schema: crate::schema_evolution::genesis_schema(),
            epoch: 0,
            genesis_agent: AgentId::from_bytes([0u8; 16]),
            live_causal: OrdMap::new(),
            live_set: OrdMap::new(),
            schema_conflicts: Vec::new(),
        }
    }

    /// Return a view of the primary datom set.
    ///
    /// INV-FERR-005: this is the authoritative set. All secondary
    /// indexes are bijective with this set.
    ///
    /// bd-h2fz: returns `DatomSetView` that dispatches to the active
    /// representation. Callers use `contains`, `len`, `iter` uniformly.
    #[must_use]
    pub fn datom_set(&self) -> DatomSetView<'_> {
        match &self.repr {
            StoreRepr::Positional(ps) => DatomSetView::Slice(ps.datoms()),
            StoreRepr::OrdMap { datoms, .. } => DatomSetView::OrdSet(datoms),
        }
    }

    /// Iterate over all datoms in the store.
    ///
    /// INV-FERR-004: the iterator yields every datom ever inserted.
    /// No datom is skipped or filtered.
    #[must_use]
    pub fn datoms(&self) -> DatomIter<'_> {
        match &self.repr {
            StoreRepr::Positional(ps) => DatomIter::Slice(ps.datoms().iter()),
            StoreRepr::OrdMap { datoms, .. } => DatomIter::OrdSet(datoms.iter()),
        }
    }

    /// Number of datoms in the store.
    ///
    /// INV-FERR-004: this value only increases over the lifetime
    /// of a store (modulo cloning via `from_datoms`).
    #[must_use]
    pub fn len(&self) -> usize {
        match &self.repr {
            StoreRepr::Positional(ps) => ps.len(),
            StoreRepr::OrdMap { datoms, .. } => datoms.len(),
        }
    }

    /// Whether the store contains zero datoms.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        match &self.repr {
            StoreRepr::Positional(ps) => ps.is_empty(),
            StoreRepr::OrdMap { datoms, .. } => datoms.is_empty(),
        }
    }

    /// Access the secondary indexes (`OrdMap` variant only).
    ///
    /// INV-FERR-005: all four indexes are bijective with the primary set.
    ///
    /// bd-h2fz: returns `None` for `Positional` stores (indexes are
    /// encoded as permutation arrays, not `SortedVecIndexes`). Returns
    /// `Some` for `OrdMap` stores.
    ///
    /// bd-5zc4: Yoneda index fusion -- returns `SortedVecIndexes` instead
    /// of `Indexes` (`OrdMap` backend).
    #[must_use]
    pub fn indexes(&self) -> Option<&SortedVecIndexes> {
        match &self.repr {
            StoreRepr::Positional(_) => None,
            StoreRepr::OrdMap { indexes, .. } => Some(indexes),
        }
    }

    /// Access the positional store (Positional variant only).
    ///
    /// Returns `Some` for cold-start-loaded stores that have not yet
    /// been promoted, `None` for write-active stores.
    #[must_use]
    pub fn positional(&self) -> Option<&PositionalStore> {
        match &self.repr {
            StoreRepr::Positional(ps) => Some(ps),
            StoreRepr::OrdMap { .. } => None,
        }
    }

    /// Promote from `Positional` to `OrdMap` representation.
    ///
    /// Called automatically on first write (`insert`, `transact`). May also
    /// be called explicitly when the `SortedVecIndexes` API is needed
    /// (e.g., in tests or callers that require `store.indexes()` to return
    /// `Some`). No-op if already `OrdMap`.
    ///
    /// O(n log n) for `OrdSet` construction + O(n log n) for `SortedVecIndexes`.
    /// bd-5zc4: Yoneda index fusion -- uses `SortedVecIndexes` instead of
    /// `Indexes` (`OrdMap` backend). `sort_all()` is called here to ensure
    /// the indexes are query-ready immediately after promotion.
    pub fn promote(&mut self) {
        if let StoreRepr::Positional(ps) = &self.repr {
            let ord_set: OrdSet<Datom> = ps.datoms().iter().cloned().collect();
            let mut indexes = SortedVecIndexes::from_datoms(ord_set.iter());
            indexes.sort_all();
            self.repr = StoreRepr::OrdMap {
                datoms: ord_set,
                indexes,
            };
        }
    }

    /// Demote from `OrdMap` back to `Positional` representation (INV-FERR-072).
    ///
    /// Rebuilds `PositionalStore` from the `OrdMap`'s `OrdSet`. O(n) because:
    /// - `OrdSet` iteration is EAVT-sorted, so `sort_unstable` detects the
    ///   sorted run in O(n).
    /// - `build_live_bitvector` is O(n).
    /// - Permutation arrays are `OnceLock::new()` (lazy, deferred to first access).
    ///
    /// No-op if already `Positional`. Called after `transact` to restore
    /// ns-level read performance via contiguous arrays.
    pub(crate) fn demote(&mut self) {
        if let StoreRepr::OrdMap { datoms, .. } = &self.repr {
            let positional = PositionalStore::from_datoms(datoms.iter().cloned());
            self.repr = StoreRepr::Positional(Arc::new(positional));
        }
    }

    /// Sort the `SortedVecIndexes` after incremental insertions.
    ///
    /// bd-5zc4: `SortedVecBackend` defers sorting until query time.
    /// After calling `insert()` in code that then queries indexes, call
    /// this method to ensure all four backends are in sorted order for
    /// binary-search lookups. No-op for `Positional` stores.
    ///
    /// Note: `transact()` does not need this -- it calls `promote()` (which
    /// sorts) then `demote()`. This is only needed for direct `insert()` callers.
    pub fn ensure_indexes_sorted(&mut self) {
        if let StoreRepr::OrdMap { indexes, .. } = &mut self.repr {
            indexes.sort_all();
        }
    }

    /// Homomorphic store fingerprint (INV-FERR-074).
    ///
    /// Returns `Some` for `Positional` stores (fingerprint computed in
    /// `from_datoms`). Returns `None` for `OrdMap` stores (fingerprint
    /// is not maintained incrementally during transact -- demotion
    /// recomputes it).
    #[must_use]
    pub fn fingerprint(&self) -> Option<&[u8; 32]> {
        match &self.repr {
            StoreRepr::Positional(ps) => Some(ps.fingerprint()),
            StoreRepr::OrdMap { .. } => None,
        }
    }

    /// Access the schema.
    ///
    /// INV-FERR-009: the schema governs transact-time validation.
    /// INV-FERR-031: for a genesis store, this returns the deterministic
    /// meta-schema.
    #[must_use]
    pub fn schema(&self) -> &Schema {
        &self.schema
    }

    /// The agent identity associated with this store's genesis.
    ///
    /// Callers use this to construct `Transaction::new(store.genesis_agent())`
    /// when they need to transact against a genesis store without
    /// manufacturing their own agent identity.
    #[must_use]
    pub fn genesis_agent(&self) -> AgentId {
        self.genesis_agent
    }

    /// The current epoch (transaction counter).
    ///
    /// INV-FERR-007: strictly monotonically increasing. Incremented
    /// by each successful `transact` call.
    #[must_use]
    pub fn epoch(&self) -> u64 {
        self.epoch
    }

    /// Schema conflicts recorded during the most recent merge reconstruction.
    ///
    /// INV-FERR-043: conflicting attribute definitions are resolved
    /// deterministically, but every conflict is also recorded here for
    /// diagnostics. Non-merge stores return an empty slice.
    #[must_use]
    pub fn schema_conflicts(&self) -> &[SchemaConflict] {
        &self.schema_conflicts
    }

    /// Update the causal LIVE lattice for a single datom (incremental).
    ///
    /// INV-FERR-029/032: O(log n) per datom where n = number of distinct
    /// (entity, attribute) keys in `live_causal`. Retains the event with the
    /// highest `TxId` per (entity, attribute, value) triple. Updates the
    /// materialized `live_set` projection to reflect liveness transitions.
    pub(crate) fn live_apply(&mut self, datom: &Datom) {
        let key = (datom.entity(), datom.attribute().clone());
        let value = datom.value().clone();

        // Check existing state without entry() to avoid cloning key on no-op path.
        let (was_live, should_update) = match self
            .live_causal
            .get(&key)
            .and_then(|entries| entries.get(&value))
        {
            Some(&(existing_tx, op)) => (op == Op::Assert, datom.tx() > existing_tx),
            None => (false, true),
        };

        if should_update {
            self.live_causal
                .entry(key.clone())
                .or_default()
                .insert(value.clone(), (datom.tx(), datom.op()));
            let is_live = datom.op() == Op::Assert;

            if is_live && !was_live {
                self.live_set.entry(key).or_default().insert(value);
            } else if !is_live && was_live {
                if let Some(vals) = self.live_set.get_mut(&key) {
                    vals.remove(&value);
                    if vals.is_empty() {
                        self.live_set.remove(&key);
                    }
                }
            }
        }
    }
}

// Trait implementations and test-utils live in lib.rs (re-exports)
// to keep this file under the 500 LOC Gate 8 limit.
