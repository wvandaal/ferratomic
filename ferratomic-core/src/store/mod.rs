//! `store` -- the G-Set CRDT semilattice: `Store = (P(D), union)`.
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
//! bijection with the primary set via [`Indexes`](crate::indexes::Indexes).
//! INV-FERR-005 is satisfied by updating all indexes on every insert.
//!
//! ## Module layout
//!
//! - [`apply`] -- transaction application, WAL replay, merge construction.
//! - [`checkpoint`] -- byte serialization convenience methods.
//! - [`merge`] -- merge-specific reconstruction helpers.
//! - [`query`] -- snapshot and LIVE-set query helpers.

mod apply;
mod checkpoint;
mod merge;
mod query;

#[cfg(test)]
mod tests;

use std::collections::BTreeSet;

use ferratom::{AgentId, Attribute, AttributeDef, Datom, EntityId, Op, Schema, TxId, Value};
use im::{OrdMap, OrdSet};

pub use self::merge::SchemaConflict;
use crate::indexes::Indexes;

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
/// to the store do not affect it. `im::OrdSet` clone is O(1) via
/// structural sharing (ADR-FERR-001), so snapshot creation is O(1).
#[derive(Debug, Clone)]
pub struct Snapshot {
    /// Structurally-shared copy of the datom set at snapshot time.
    /// O(1) clone via `im::OrdSet` (ADR-FERR-001).
    datoms: OrdSet<Datom>,
    /// Epoch at the time the snapshot was taken.
    epoch: u64,
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
#[derive(Debug, Clone)]
pub struct Store {
    /// Primary datom set. The single source of truth.
    /// `im::OrdSet` provides O(1) clone via structural sharing (ADR-FERR-001).
    pub(crate) datoms: OrdSet<Datom>,
    /// Secondary indexes maintained in bijection with the primary set.
    pub(crate) indexes: Indexes,
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
    /// Accepts `BTreeSet` for generator/test compatibility and converts
    /// to `im::OrdSet` internally (ADR-FERR-001). For merge, use
    /// [`from_merge`] which preserves schema and epoch.
    #[must_use]
    pub fn from_datoms(datoms: BTreeSet<Datom>) -> Self {
        let ord_set: OrdSet<Datom> = datoms.into_iter().collect();
        let indexes = Indexes::from_datoms(ord_set.iter());
        let live_causal = query::build_live_causal(ord_set.iter());
        let live_set = query::derive_live_set(&live_causal);
        Self {
            datoms: ord_set,
            indexes,
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
        let ord_set: OrdSet<Datom> = datoms.into_iter().collect();
        let indexes = Indexes::from_datoms(ord_set.iter());
        let live_causal = query::build_live_causal(ord_set.iter());
        let live_set = query::derive_live_set(&live_causal);
        Self {
            datoms: ord_set,
            indexes,
            schema,
            epoch,
            genesis_agent,
            live_causal,
            live_set,
            schema_conflicts: Vec::new(),
        }
    }

    /// Deterministic genesis store with the 19 axiomatic meta-schema attributes.
    ///
    /// INV-FERR-031: every call to `genesis()` produces an identical store.
    /// The 19 attributes are the ONLY hardcoded elements in the engine.
    /// Every other attribute is defined by transacting datoms that reference
    /// these 19. This is the schema-as-data bootstrap (C3, C7).
    #[must_use]
    pub fn genesis() -> Self {
        Self {
            datoms: OrdSet::new(),
            indexes: Indexes::from_datoms(std::iter::empty()),
            schema: crate::schema_evolution::genesis_schema(),
            epoch: 0,
            genesis_agent: AgentId::from_bytes([0u8; 16]),
            live_causal: OrdMap::new(),
            live_set: OrdMap::new(),
            schema_conflicts: Vec::new(),
        }
    }

    /// Return a reference to the primary datom set.
    ///
    /// INV-FERR-005: this is the authoritative set. All secondary
    /// indexes are bijective with this set.
    #[must_use]
    pub fn datom_set(&self) -> &OrdSet<Datom> {
        &self.datoms
    }

    /// Iterate over all datoms in the store.
    ///
    /// INV-FERR-004: the iterator yields every datom ever inserted.
    /// No datom is skipped or filtered.
    pub fn datoms(&self) -> impl Iterator<Item = &Datom> {
        self.datoms.iter()
    }

    /// Number of datoms in the store.
    ///
    /// INV-FERR-004: this value only increases over the lifetime
    /// of a store (modulo cloning via `from_datoms`).
    #[must_use]
    pub fn len(&self) -> usize {
        self.datoms.len()
    }

    /// Whether the store contains zero datoms.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.datoms.is_empty()
    }

    /// Access the secondary indexes.
    ///
    /// INV-FERR-005: all four indexes are bijective with the primary set.
    #[must_use]
    pub fn indexes(&self) -> &Indexes {
        &self.indexes
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
    /// INV-FERR-029/032: O(log n) per datom. Retains the event with the
    /// highest `TxId` per (entity, attribute, value) triple. Updates the
    /// materialized `live_set` projection to reflect liveness transitions.
    pub(crate) fn live_apply(&mut self, datom: &Datom) {
        let key = (datom.entity(), datom.attribute().clone());
        let value = datom.value().clone();

        let entries = self.live_causal.entry(key.clone()).or_default();
        let was_live = entries.get(&value).is_some_and(|&(_, op)| op == Op::Assert);
        let should_update = entries
            .get(&value)
            .is_none_or(|&(existing_tx, _)| datom.tx() > existing_tx);

        if should_update {
            entries.insert(value.clone(), (datom.tx(), datom.op()));
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

// ---------------------------------------------------------------------------
// Trait implementations
// ---------------------------------------------------------------------------

/// INV-FERR-001..003: Store is a join-semilattice under set union.
/// The merge operation is commutative, associative, and idempotent.
impl ferratom::traits::Semilattice for Store {
    fn merge(&self, other: &Self) -> Result<Self, ferratom::FerraError> {
        Ok(Store::from_merge(self, other))
    }
}

// Note: ContentAddressed for Datom must be impl'd in ferratom crate
// (orphan rule). See ferratom/src/datom.rs -- Datom::content_hash()
// already provides the INV-FERR-012 contract.
