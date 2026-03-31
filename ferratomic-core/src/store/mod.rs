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

mod apply;
mod checkpoint;

use std::collections::BTreeSet;

use ferratom::{AgentId, Attribute, AttributeDef, Datom, EntityId, Op, Schema, Value};
use im::{OrdMap, OrdSet};

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

impl Snapshot {
    /// Iterate over all datoms visible in this snapshot.
    ///
    /// INV-FERR-006: the iterator yields exactly the datoms that
    /// were present when the snapshot was created -- no more, no fewer.
    pub fn datoms(&self) -> impl Iterator<Item = &Datom> {
        self.datoms.iter()
    }

    /// The epoch at which this snapshot was taken.
    ///
    /// INV-FERR-011: observer epochs derived from snapshots are
    /// monotonically non-decreasing.
    #[must_use]
    pub fn epoch(&self) -> u64 {
        self.epoch
    }
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
    /// INV-FERR-029/032: Incrementally maintained LIVE view.
    /// Maps (entity, attribute) → set of non-retracted values.
    /// Card-one: `live_resolve` returns the LWW (highest-epoch) value.
    /// Card-many: `live_resolve` returns all non-retracted values.
    /// Updated O(k) per transaction, O(1) read.
    pub(crate) live_set: OrdMap<(EntityId, Attribute), OrdSet<Value>>,
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
        let live_set = build_live_set(ord_set.iter());
        Self {
            datoms: ord_set,
            indexes,
            schema: Schema::empty(),
            epoch: 0,
            genesis_agent: AgentId::from_bytes([0u8; 16]),
            live_set,
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
        let live_set = build_live_set(ord_set.iter());
        Self {
            datoms: ord_set,
            indexes,
            schema,
            epoch,
            genesis_agent,
            live_set,
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
            live_set: OrdMap::new(),
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

    /// Resolve the LIVE value(s) for an entity-attribute pair.
    ///
    /// INV-FERR-029: Returns the set of non-retracted values.
    /// INV-FERR-032: For card-one attributes, the caller should take the
    /// value with the highest `TxId` epoch (LWW). For card-many, all values
    /// are current. This method returns the raw set; callers apply
    /// cardinality resolution.
    #[must_use]
    pub fn live_values(&self, entity: EntityId, attribute: &Attribute) -> Option<&OrdSet<Value>> {
        self.live_set.get(&(entity, attribute.clone()))
    }

    /// Resolve a single LIVE value for a card-one entity-attribute pair.
    ///
    /// INV-FERR-032: For cardinality-one, returns the latest (highest in
    /// `OrdSet` iteration order) non-retracted value, or `None` if fully
    /// retracted or never asserted.
    #[must_use]
    pub fn live_resolve(&self, entity: EntityId, attribute: &Attribute) -> Option<&Value> {
        self.live_set
            .get(&(entity, attribute.clone()))
            .and_then(|vals| vals.get_max())
    }

    /// Update the LIVE set for a single datom (incremental maintenance).
    ///
    /// INV-FERR-029: O(1) per datom. Assertions insert into the value set;
    /// retractions remove from it. Empty sets are pruned.
    pub(crate) fn live_apply(&mut self, datom: &Datom) {
        let key = (datom.entity(), datom.attribute().clone());
        match datom.op() {
            Op::Assert => {
                let vals = self.live_set.entry(key).or_default();
                vals.insert(datom.value().clone());
            }
            Op::Retract => {
                if let Some(vals) = self.live_set.get_mut(&key) {
                    vals.remove(datom.value());
                    if vals.is_empty() {
                        self.live_set.remove(&key);
                    }
                }
            }
        }
    }

    /// Take an immutable point-in-time snapshot of the store.
    ///
    /// INV-FERR-006: the returned snapshot is frozen. Subsequent
    /// calls to `transact` or `insert` do not affect it.
    #[must_use]
    pub fn snapshot(&self) -> Snapshot {
        // O(1) via im::OrdSet structural sharing (ADR-FERR-001).
        // No Arc wrapper needed -- im::OrdSet clone shares the tree spine.
        Snapshot {
            datoms: self.datoms.clone(),
            epoch: self.epoch,
        }
    }
}

/// Build a LIVE set from an iterator of datoms (full rebuild).
///
/// INV-FERR-029: Used during cold start, checkpoint load, and merge.
/// Processes datoms in iteration order (EAVT by `Datom::Ord`).
fn build_live_set<'a>(
    datoms: impl Iterator<Item = &'a Datom>,
) -> OrdMap<(EntityId, Attribute), OrdSet<Value>> {
    let mut live: OrdMap<(EntityId, Attribute), OrdSet<Value>> = OrdMap::new();
    for datom in datoms {
        let key = (datom.entity(), datom.attribute().clone());
        match datom.op() {
            Op::Assert => {
                let vals = live.entry(key).or_default();
                vals.insert(datom.value().clone());
            }
            Op::Retract => {
                if let Some(vals) = live.get_mut(&key) {
                    vals.remove(datom.value());
                    if vals.is_empty() {
                        live.remove(&key);
                    }
                }
            }
        }
    }
    live
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

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use ferratom::{Attribute, Cardinality, EntityId, Op, TxId, Value, ValueType};

    use super::*;
    use crate::schema_evolution::{parse_cardinality, parse_value_type};

    /// Helper: build a sample datom for testing.
    fn sample_datom(seed: &str) -> Datom {
        Datom::new(
            EntityId::from_content(seed.as_bytes()),
            Attribute::from("test/name"),
            Value::String(Arc::from(seed)),
            TxId::new(1, 0, 0),
            Op::Assert,
        )
    }

    #[test]
    fn test_from_datoms_preserves_set() {
        let mut set = BTreeSet::new();
        set.insert(sample_datom("a"));
        set.insert(sample_datom("b"));

        let store = Store::from_datoms(set.clone());
        // Compare by converting BTreeSet to OrdSet for type compatibility.
        let expected: im::OrdSet<Datom> = set.into_iter().collect();
        assert_eq!(*store.datom_set(), expected);
        assert_eq!(store.len(), 2);
    }

    #[test]
    fn test_from_datoms_empty() {
        let store = Store::from_datoms(BTreeSet::new());
        assert!(store.is_empty());
        assert_eq!(store.len(), 0);
    }

    #[test]
    fn test_inv_ferr_031_genesis_determinism() {
        let a = Store::genesis();
        let b = Store::genesis();
        assert_eq!(
            a.schema(),
            b.schema(),
            "INV-FERR-031: genesis() must produce identical schemas"
        );
        assert_eq!(a.datom_set(), b.datom_set());
        assert_eq!(a.epoch(), b.epoch());
    }

    #[test]
    fn test_inv_ferr_031_genesis_schema_has_19_attributes() {
        let store = Store::genesis();
        assert_eq!(
            store.schema().len(),
            19,
            "INV-FERR-031: genesis schema must have exactly 19 axiomatic attributes"
        );
        // Core meta-schema (1-9)
        assert!(store.schema().get(&Attribute::from("db/ident")).is_some());
        assert!(store
            .schema()
            .get(&Attribute::from("db/valueType"))
            .is_some());
        assert!(store
            .schema()
            .get(&Attribute::from("db/cardinality"))
            .is_some());
        assert!(store.schema().get(&Attribute::from("db/doc")).is_some());
        assert!(store.schema().get(&Attribute::from("db/unique")).is_some());
        assert!(store
            .schema()
            .get(&Attribute::from("db/isComponent"))
            .is_some());
        assert!(store
            .schema()
            .get(&Attribute::from("db/resolutionMode"))
            .is_some());
        assert!(store
            .schema()
            .get(&Attribute::from("db/latticeOrder"))
            .is_some());
        assert!(store
            .schema()
            .get(&Attribute::from("db/lwwClock"))
            .is_some());
        // Lattice (10-14)
        assert!(store
            .schema()
            .get(&Attribute::from("lattice/ident"))
            .is_some());
        assert!(store
            .schema()
            .get(&Attribute::from("lattice/elements"))
            .is_some());
        assert!(store
            .schema()
            .get(&Attribute::from("lattice/comparator"))
            .is_some());
        assert!(store
            .schema()
            .get(&Attribute::from("lattice/bottom"))
            .is_some());
        assert!(store
            .schema()
            .get(&Attribute::from("lattice/top"))
            .is_some());
        // Transaction metadata (15-19)
        assert!(store.schema().get(&Attribute::from("tx/time")).is_some());
        assert!(store.schema().get(&Attribute::from("tx/agent")).is_some());
        assert!(store
            .schema()
            .get(&Attribute::from("tx/provenance"))
            .is_some());
        assert!(store
            .schema()
            .get(&Attribute::from("tx/rationale"))
            .is_some());
        assert!(store
            .schema()
            .get(&Attribute::from("tx/coherence-override"))
            .is_some());
    }

    #[test]
    fn test_inv_ferr_005_index_bijection_from_datoms() {
        let mut set = BTreeSet::new();
        set.insert(sample_datom("x"));
        set.insert(sample_datom("y"));
        set.insert(sample_datom("z"));

        let store = Store::from_datoms(set);
        let primary: BTreeSet<&Datom> = store.datoms().collect();
        let eavt: BTreeSet<&Datom> = store.indexes().eavt_datoms().collect();
        let aevt: BTreeSet<&Datom> = store.indexes().aevt_datoms().collect();
        let vaet: BTreeSet<&Datom> = store.indexes().vaet_datoms().collect();
        let avet: BTreeSet<&Datom> = store.indexes().avet_datoms().collect();

        assert_eq!(primary, eavt, "INV-FERR-005: EAVT must match primary");
        assert_eq!(primary, aevt, "INV-FERR-005: AEVT must match primary");
        assert_eq!(primary, vaet, "INV-FERR-005: VAET must match primary");
        assert_eq!(primary, avet, "INV-FERR-005: AVET must match primary");
    }

    #[test]
    fn test_genesis_is_empty_of_datoms() {
        let store = Store::genesis();
        assert!(store.is_empty(), "genesis store must have zero datoms");
    }

    #[test]
    fn test_snapshot_is_frozen() {
        let mut store = Store::from_datoms(BTreeSet::new());
        store.insert(&sample_datom("before"));

        let snap = store.snapshot();
        let snap_set_before: BTreeSet<&Datom> = snap.datoms().collect();

        store.insert(&sample_datom("after"));

        // bd-3bg regression: compare datom SETS, not counts.
        let snap_set_after: BTreeSet<&Datom> = snap.datoms().collect();
        assert_eq!(
            snap_set_before, snap_set_after,
            "INV-FERR-006: snapshot datom set must not change after later inserts"
        );
        assert_eq!(
            snap_set_before.len(),
            1,
            "snapshot should have exactly 1 datom"
        );
    }

    #[test]
    fn test_parse_value_type_all_variants() {
        assert_eq!(
            parse_value_type("db.type/keyword"),
            Some(ValueType::Keyword)
        );
        assert_eq!(parse_value_type("db.type/string"), Some(ValueType::String));
        assert_eq!(parse_value_type("db.type/long"), Some(ValueType::Long));
        assert_eq!(parse_value_type("db.type/double"), Some(ValueType::Double));
        assert_eq!(
            parse_value_type("db.type/boolean"),
            Some(ValueType::Boolean)
        );
        assert_eq!(
            parse_value_type("db.type/instant"),
            Some(ValueType::Instant)
        );
        assert_eq!(parse_value_type("db.type/uuid"), Some(ValueType::Uuid));
        assert_eq!(parse_value_type("db.type/bytes"), Some(ValueType::Bytes));
        assert_eq!(parse_value_type("db.type/ref"), Some(ValueType::Ref));
        assert_eq!(parse_value_type("db.type/bigint"), Some(ValueType::BigInt));
        assert_eq!(parse_value_type("db.type/bigdec"), Some(ValueType::BigDec));
        assert_eq!(parse_value_type("db.type/unknown"), None);
    }

    #[test]
    fn test_parse_cardinality_variants() {
        assert_eq!(
            parse_cardinality("db.cardinality/one"),
            Some(Cardinality::One)
        );
        assert_eq!(
            parse_cardinality("db.cardinality/many"),
            Some(Cardinality::Many)
        );
        assert_eq!(parse_cardinality("db.cardinality/unknown"), None);
    }

    /// bd-20j: Semilattice trait is usable via generic bounds.
    #[test]
    fn test_semilattice_trait_bound() {
        use ferratom::traits::Semilattice;

        fn requires_semilattice<T: Semilattice>(a: &T, b: &T) -> Result<T, ferratom::FerraError> {
            a.merge(b)
        }

        let a = Store::genesis();
        let b = Store::genesis();
        let merged = requires_semilattice(&a, &b).expect("merge should succeed");
        assert_eq!(
            merged.epoch(),
            0,
            "bd-20j: Semilattice merge of genesis stores"
        );
    }

    /// bd-20j: ContentAddressed trait is usable via generic bounds.
    #[test]
    fn test_content_addressed_trait_bound() {
        use ferratom::traits::ContentAddressed;

        fn requires_content_addressed<T: ContentAddressed>(x: &T) -> [u8; 32] {
            x.content_hash()
        }

        let datom = sample_datom("trait-test");
        let hash = requires_content_addressed(&datom);
        assert_ne!(
            hash, [0u8; 32],
            "bd-20j: ContentAddressed must produce non-zero hash"
        );
    }
}
